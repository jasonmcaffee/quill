//! What a data source is, and reading one out of a URL.
//!
//! **No password is on this value**, and that is the point of it rather than an omission. A data
//! source records the *place* a password is — an environment variable, or an entry in the machine's
//! own keychain — and the secret is fetched at the moment a connection is opened and dropped as soon
//! as it has been sent. `services::agent_tasks::keychain` sets that rule for the board's four secrets
//! and Agent-Chat keeps it for an API key; this is the same rule for a database.
//!
//! A password somebody types into the connect dialog is the third case, and it is carried separately
//! in [`Secret`] so that it cannot accidentally be written down with the rest: where IntelliJ's
//! dialog offers `Save: Forever`, this one offers `until this window closes`.

use crate::rows::{Answer, Failure};

/// Which engine a data source speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Engine {
    #[default]
    Postgres,
    Sqlite,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Postgres => "postgres",
            Engine::Sqlite => "sqlite",
        }
    }

    /// The engines this version speaks, which is what a refusal names.
    pub const ALL: &'static [&'static str] = &["postgres", "sqlite"];

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "postgres" | "postgresql" | "pgsql" | "pg" => Some(Engine::Postgres),
            "sqlite" | "sqlite3" => Some(Engine::Sqlite),
            _ => None,
        }
    }
}

/// Whether the connection is encrypted, and whether it has to be.
///
/// Three values rather than PostgreSQL's six. `verify-full` and a certificate file are deliberately
/// absent: `Require` means encrypted and verified against **the machine's own certificate store**,
/// which is where every other program on this machine looks, and a data source needing a private
/// certificate authority is a data source whose authority belongs in that store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SslMode {
    /// Never encrypted.
    Disable,
    /// Ask; carry on in the clear if the server says no. PostgreSQL's own default.
    #[default]
    Prefer,
    /// Ask, and refuse to continue if the server says no.
    Require,
}

impl SslMode {
    pub fn name(self) -> &'static str {
        match self {
            SslMode::Disable => "disable",
            SslMode::Prefer => "prefer",
            SslMode::Require => "require",
        }
    }

    pub const ALL: &'static [&'static str] = &["disable", "prefer", "require"];

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "disable" => Some(SslMode::Disable),
            "prefer" => Some(SslMode::Prefer),
            "require" => Some(SslMode::Require),
            _ => None,
        }
    }
}

/// Where a password is, which is all that is ever written down.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Secret {
    /// There is none: trust authentication, or a SQLite file.
    #[default]
    None,
    /// The name of an environment variable, read when a connection is opened and never held.
    ///
    /// The only route on Windows, which has no keychain Quill can write to — see
    /// `services::agent_tasks::keychain`, which says so plainly rather than claiming one.
    Environment(String),
    /// The name of an entry in the machine's own keychain. macOS and Linux.
    Keychain(String),
    /// Typed into the dialog, held in this process and written nowhere.
    Typed(String),
}

impl Secret {
    /// What the settings page shows: where the password is, never what it is.
    pub fn describe(&self) -> String {
        match self {
            Secret::None => "none".to_owned(),
            Secret::Environment(name) => format!("environment {name}"),
            Secret::Keychain(name) => format!("keychain {name}"),
            Secret::Typed(_) => "typed, until this window closes".to_owned(),
        }
    }
}

/// One data source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// What it is called in the tree, on the command line and in the settings file.
    pub name: String,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    /// The database on the server, or — for SQLite — the file.
    pub database: String,
    pub user: String,
    pub ssl: SslMode,
    pub secret: Secret,
    /// The server enforces this, and Quill hides the editing controls as well. See
    /// `tasks/task-1777-database-plugin-tdd.md` §7.
    pub read_only: bool,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            name: String::new(),
            engine: Engine::Postgres,
            host: "localhost".to_owned(),
            port: 5432,
            database: String::new(),
            user: String::new(),
            ssl: SslMode::default(),
            secret: Secret::default(),
            read_only: false,
        }
    }
}

impl Source {
    /// A SQLite data source on a file.
    pub fn sqlite(name: impl Into<String>, file: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            engine: Engine::Sqlite,
            database: file.into(),
            host: String::new(),
            port: 0,
            ..Self::default()
        }
    }

    /// What the tree shows under the name: where this source points.
    pub fn where_it_points(&self) -> String {
        match self.engine {
            Engine::Postgres => format!("PostgreSQL · {}:{}/{}", self.host, self.port, self.database),
            Engine::Sqlite => format!("SQLite · {}", self.database),
        }
    }

    /// Read a data source out of what somebody typed.
    ///
    /// Two shapes, because those are the two things a person has to hand: a PostgreSQL URL, which is
    /// what every tool on this machine already prints, and a path, which is what a SQLite database
    /// is. Anything else is refused with a sentence naming both, rather than being loaded as a source
    /// that fails obscurely the first time it is opened.
    pub fn parse(name: &str, text: &str) -> Answer<Source> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Failure::said("a data source needs a URL or the path of a SQLite file."));
        }
        if let Some(rest) = text
            .strip_prefix("postgres://")
            .or_else(|| text.strip_prefix("postgresql://"))
        {
            return parse_postgres(name, rest);
        }
        if let Some(rest) = text.strip_prefix("sqlite://") {
            return Ok(Source::sqlite(name, rest.trim_start_matches('/')));
        }
        if text.contains("://") {
            let scheme = text.split("://").next().unwrap_or_default();
            return Err(Failure::said(format!(
                "`{scheme}` is not an engine this version of Quill speaks, and it speaks {}.",
                Engine::ALL.join(", ")
            )));
        }
        // Anything that is not a URL is a file, which is what a SQLite data source is. A path that
        // does not exist is not refused here: it is refused when it is opened, by the engine, in its
        // own words.
        Ok(Source::sqlite(name, text))
    }

    /// The URL this source would be written as, with no secret in it.
    ///
    /// Used by the settings page and by `plugins run database sources`, so what an agent reads back is
    /// what a person could type in — and never a password, even one that was typed.
    pub fn url(&self) -> String {
        match self.engine {
            Engine::Sqlite => self.database.clone(),
            Engine::Postgres => {
                let user = match self.user.is_empty() {
                    true => String::new(),
                    false => format!("{}@", encode(&self.user)),
                };
                format!(
                    "postgres://{user}{}:{}/{}?sslmode={}",
                    self.host,
                    self.port,
                    encode(&self.database),
                    self.ssl.name()
                )
            }
        }
    }
}

/// `postgres://user@host:port/database?sslmode=…`, with the scheme already taken off.
fn parse_postgres(name: &str, rest: &str) -> Answer<Source> {
    let mut source = Source { name: name.to_owned(), engine: Engine::Postgres, ..Source::default() };
    let (authority, query) = match rest.split_once('?') {
        Some((authority, query)) => (authority, Some(query)),
        None => (rest, None),
    };
    let (authority, database) = match authority.split_once('/') {
        Some((authority, database)) => (authority, database),
        None => (authority, ""),
    };
    let (user, host) = match authority.rsplit_once('@') {
        Some((user, host)) => (user, host),
        None => ("", authority),
    };
    // A password in the URL is **read and thrown away**, with the refusal saying where one goes
    // instead. Accepting it silently would put a secret in a settings file through the one door this
    // design closes everywhere else.
    if let Some((user_only, _)) = user.split_once(':') {
        source.user = decode(user_only);
        return Err(Failure::said(format!(
            "that URL has a password in it, and Quill does not write one down. Set it as an \
             environment variable and name the variable on the data source instead — the user \
             `{}` and the rest of the URL are fine.",
            source.user
        )));
    }
    source.user = decode(user);
    if !host.is_empty() {
        match host.rsplit_once(':') {
            // An IPv6 literal is `[::1]:5432`, so a colon inside brackets is not the port's.
            Some((address, port)) if !port.contains(']') => {
                source.host = address.trim_matches(['[', ']']).to_owned();
                source.port = port
                    .parse()
                    .map_err(|_| Failure::said(format!("`{port}` is not a port number.")))?;
            }
            _ => source.host = host.trim_matches(['[', ']']).to_owned(),
        }
    }
    source.database = decode(database);
    if let Some(query) = query {
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "sslmode" => {
                    source.ssl = SslMode::from_name(value).ok_or_else(|| {
                        Failure::said(format!(
                            "sslmode is `{value}`, and Quill has {}.",
                            SslMode::ALL.join(", ")
                        ))
                    })?
                }
                "user" => source.user = decode(value),
                "dbname" | "database" => source.database = decode(value),
                "password" => {
                    return Err(Failure::said(
                        "that URL has a password in it, and Quill does not write one down. Name an \
                         environment variable on the data source instead.",
                    ))
                }
                // An option this version has no answer for is said rather than ignored, which is the
                // rule every other named-value key in Quill keeps.
                other => {
                    return Err(Failure::said(format!(
                        "`{other}` is not something Quill's PostgreSQL connections take, and they \
                         take sslmode, user and dbname."
                    )))
                }
            }
        }
    }
    if source.database.is_empty() {
        source.database = source.user.clone();
    }
    if source.user.is_empty() {
        return Err(Failure::said("that URL names no user, and PostgreSQL needs one."));
    }
    Ok(source)
}

/// Percent-decoding, for the user and database parts of a URL.
///
/// Written here rather than taken from `percent-encoding`: this crate has no window and no HTTP in
/// it, and the two places a URL is read need eight lines rather than a dependency.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&text[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The other direction, for the characters that would otherwise change what a URL means.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_postgres_url_is_read_into_its_parts() {
        let source = Source::parse("ai", "postgres://postgres@localhost:5432/ai").expect("read");
        assert_eq!(source.engine, Engine::Postgres);
        assert_eq!(source.user, "postgres");
        assert_eq!(source.host, "localhost");
        assert_eq!(source.port, 5432);
        assert_eq!(source.database, "ai");
        assert_eq!(source.ssl, SslMode::Prefer, "PostgreSQL's own default");
    }

    #[test]
    fn a_url_with_no_port_takes_the_default_and_a_missing_database_takes_the_user() {
        let source = Source::parse("x", "postgres://jason@db.example.com").expect("read");
        assert_eq!(source.port, 5432);
        assert_eq!(source.database, "jason");
    }

    #[test]
    fn an_ipv6_literal_is_not_split_at_its_own_colons() {
        let source = Source::parse("x", "postgres://me@[::1]:5433/thing").expect("read");
        assert_eq!(source.host, "::1");
        assert_eq!(source.port, 5433);
        let no_port = Source::parse("x", "postgres://me@[2001:db8::1]/thing").expect("read");
        assert_eq!(no_port.host, "2001:db8::1");
        assert_eq!(no_port.port, 5432);
    }

    #[test]
    fn a_password_in_the_url_is_refused_with_where_one_goes_instead() {
        // The one door this design closes everywhere else: accepting a password here would write a
        // secret into a settings file through a URL somebody pasted.
        let refused = Source::parse("x", "postgres://me:hunter2@localhost/db").expect_err("refused");
        assert!(refused.message.contains("does not write one down"), "{refused}");
        assert!(!refused.message.contains("hunter2"), "the refusal must not quote the secret");
        let query = Source::parse("x", "postgres://me@localhost/db?password=hunter2").expect_err("refused");
        assert!(!query.message.contains("hunter2"));
    }

    #[test]
    fn an_unknown_option_is_said_rather_than_ignored() {
        let refused =
            Source::parse("x", "postgres://me@localhost/db?connect_timeout=5").expect_err("refused");
        assert!(refused.message.contains("connect_timeout"), "{refused}");
        assert!(refused.message.contains("sslmode"), "and it names what is taken: {refused}");
        let mode = Source::parse("x", "postgres://me@localhost/db?sslmode=verify-full").expect_err("refused");
        assert!(mode.message.contains("disable, prefer, require"), "{mode}");
    }

    #[test]
    fn anything_that_is_not_a_url_is_a_sqlite_file() {
        let source = Source::parse("tasks", r"C:\jason\dev\quill\tasks.db").expect("read");
        assert_eq!(source.engine, Engine::Sqlite);
        assert_eq!(source.database, r"C:\jason\dev\quill\tasks.db");
        let scheme = Source::parse("x", "mysql://root@localhost/db").expect_err("refused");
        assert!(scheme.message.contains("postgres, sqlite"), "{scheme}");
    }

    #[test]
    fn the_url_it_writes_back_has_no_secret_in_it_even_when_one_was_typed() {
        let mut source = Source::parse("ai", "postgres://postgres@localhost:5432/ai").expect("read");
        source.secret = Secret::Typed("hunter2".to_owned());
        assert_eq!(source.url(), "postgres://postgres@localhost:5432/ai?sslmode=prefer");
        assert!(!source.url().contains("hunter2"));
        assert_eq!(source.secret.describe(), "typed, until this window closes");
        assert!(!source.secret.describe().contains("hunter2"));
    }

    #[test]
    fn percent_encoding_survives_a_round_trip() {
        let source = Source::parse("x", "postgres://a%40b@localhost/my%20db").expect("read");
        assert_eq!(source.user, "a@b");
        assert_eq!(source.database, "my db");
        let again = Source::parse("x", &source.url()).expect("read back");
        assert_eq!(again.user, "a@b");
        assert_eq!(again.database, "my db");
    }
}
