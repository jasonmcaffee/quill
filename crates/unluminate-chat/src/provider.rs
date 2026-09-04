//! What an endpoint is: a name, a wire shape, somewhere to send to, a model, and — for the two
//! shapes that need one — where its key comes from.
//!
//! The ticket asks for "a config in settings to allow configure url for Claude, codex etc.", so a
//! provider is a **row in a list** rather than a constant in the binary, and the three that ship are
//! only the three rows that are there the first time the page is opened.
//!
//! ## Somewhere to send to is a program as often as it is a URL
//!
//! **The two rows the ticket names are command-line agents, not APIs.** `claude` and `codex` are
//! both installed on this machine, both already signed in, and both will stream a whole agentic turn
//! as JSON on their standard output. Talking to them is therefore *better* than talking to
//! `api.anthropic.com` in every way that matters here: there is **no key for Unluminate to hold at all**,
//! the agent brings its own tools and its own permission model rather than being handed Unluminate's, it
//! reads the `CLAUDE.md` and `AGENTS.md` of the project the pane is open on, and a conversation
//! carries on through the agent's own session rather than by re-sending the transcript every turn.
//!
//! So a row is one of two things and `Provider::is_a_program` is the question: a **program**, whose
//! `command` names it and whose `url` is unused, or an **endpoint**, whose `url` is an address and
//! whose `command` is empty. Everything downstream is unchanged, because all five shapes are read
//! into the same `Reply` values.
//! ## Where a key lives, which is nowhere Unluminate writes
//!
//! `services::agent_tasks::keychain` says it plainly: a secret is never written into a settings
//! file, because a settings file is copied between machines, readable by anything that can read the
//! folder, and pasted into a bug report. It also says, honestly, that there is **no Windows
//! keychain** in Unluminate — on Windows it answers `None` and refuses to write.
//!
//! So a provider names an **environment variable** and Unluminate reads it at the moment a request is
//! sent. `ANTHROPIC_API_KEY` is the name every tool on this machine already uses and the one
//! `claude` itself reads, so on the platform Unluminate is developed on there is a way to have a key that
//! does not involve Unluminate storing one. What is written down is the *name of the place the key is*,
//! exactly as Agent-Tasks writes down the name of a keychain entry. A local endpoint names neither,
//! because llama.cpp wants no key.

/// Which protocol something speaks.
///
/// Five, and each is here because something the ticket names speaks it.
///
/// **`claude-cli` and `codex-cli` are programs rather than addresses**, and they are the two the
/// ticket names: *"connection to Claude and codex etc through cli"*. Claude Code's
/// `--output-format stream-json` is the **Anthropic wire verbatim**, nested one level down inside a
/// `stream_event` envelope, so the decoder that already reads `/v1/messages` reads it with a wrapper
/// and nothing more — measured against real runs, recorded in `tests/streams/claude-cli.jsonl`
/// and `claude-cli-tool.jsonl` and replayed by `tests/agent_streams.rs`.
/// Codex's `--json` is a different model again: a thread of **items** that are started, updated and
/// completed, where a shell command the agent ran is an item beside the words it said.
///
/// **OpenAI's `/v1/chat/completions`** is what llama.cpp, LM Studio, Ollama, vLLM, every gateway on
/// this machine and most of OpenAI speak, so one shape reaches nearly everything.
///
/// **Anthropic's `/v1/messages`** is genuinely a different protocol — named events, indexed content
/// blocks, a system prompt that is a field rather than a message, tool results gathered into one
/// `user` message — and it is the one the ticket names first.
///
/// **OpenAI's `/v1/responses`** is here because the ticket names `codex` and **the Codex models are
/// served on that endpoint alone**: `gpt-5-codex` and everything after it are Responses-API-only, so
/// a `codex` row pointed at `/v1/chat/completions` is a row that fails on its first message. The TDD
/// opened by saying two shapes were enough and naming a third as deliberately left out; that was
/// wrong about the very row the ticket asks for, and §8.1 now says so rather than quietly shipping a
/// default that cannot work. It is a flat item list rather than messages, its events are named, and
/// a tool call is an item of its own rather than a field on a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Wire {
    OpenAi,
    Anthropic,
    /// OpenAI's `/v1/responses`, which is the only endpoint the Codex models are served on.
    Responses,
    /// The `claude` command line, in `--print --output-format stream-json` mode.
    ClaudeCli,
    /// The `codex exec --json` command line.
    CodexCli,
}

/// Every wire shape this version speaks, which is what a settings page offers and what a
/// configuration naming an unknown one is refused with.
///
/// The sixth registry of this shape in Unluminate, after the renderers, the project detectors, the
/// debuggers, the UI providers and the chrome. A configuration naming a shape this version has not
/// got is refused with the list rather than loading as an endpoint whose every request fails
/// obscurely — the rule `language.renders` set and every one since has kept.
pub const WIRES: &[&str] = &[
    "claude-cli",
    "codex-cli",
    "openai",
    "anthropic",
    "responses",
];

impl Wire {
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Responses => "responses",
            Self::ClaudeCli => "claude-cli",
            Self::CodexCli => "codex-cli",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim() {
            "openai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            "responses" => Some(Self::Responses),
            "claude-cli" => Some(Self::ClaudeCli),
            "codex-cli" => Some(Self::CodexCli),
            _ => None,
        }
    }

    /// Whether this shape is a program on this machine rather than an address on the network.
    ///
    /// The one question everything else branches on: a program needs no key, no URL and no tool
    /// catalogue from Unluminate, and its conversation is continued by its own session id rather than by
    /// sending the transcript again.
    pub fn is_a_program(self) -> bool {
        matches!(self, Self::ClaudeCli | Self::CodexCli)
    }

    /// Whether this shape carries a whole agent — its own tools, its own permissions, its own files.
    ///
    /// The same two today, and a separate question on purpose: `is_a_program` is about the
    /// transport and this is about what is on the other end of it.
    pub fn is_an_agent(self) -> bool {
        self.is_a_program()
    }
}

/// One endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    /// What it is called in the header chip, in `plugins run agent-chat use` and in the settings
    /// file. Lower case, one word.
    pub name: String,
    pub wire: Wire,
    /// The program to run, for a wire shape that is a command line. Empty for one that is an address.
    ///
    /// A name looked up on `PATH`, or an absolute path for an agent installed somewhere that is not
    /// on it. Only the program: the arguments are the shape's own and are built in `agent.rs`, so a
    /// row cannot be configured into a command line that means something else entirely.
    pub command: String,
    /// The whole URL, including the path — because that is the part that differs between a hosted
    /// API and the llama.cpp on this machine, and a base URL that Unluminate appended a path to would be
    /// an Unluminate that decided what somebody's gateway looks like.
    pub url: String,
    pub model: String,
    /// The environment variable the key is read from. Empty means the endpoint wants none.
    pub key_env: String,
    /// A keychain entry to read the key from instead, on a platform that has one.
    ///
    /// Empty on Windows however it is configured, because `keychain::read` answers `None` there and
    /// a field that pretended otherwise would be a field claiming a secret is somewhere it is not.
    pub key_entry: String,
    /// The most tokens to ask for in one answer.
    pub max_tokens: u32,
}

/// What `max_tokens` is when nothing says.
///
/// Anthropic's API **requires** it, so there is no "unset" to pass through; OpenAI's treats it as
/// optional and it is sent anyway so the two behave the same. Four thousand is long enough for an
/// answer with code in it and short enough that a runaway is not expensive.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

impl Provider {
    /// The three rows a configuration that has never been written starts with.
    ///
    /// `claude` and `codex` are the two the ticket names. `local` is there because the OpenAI shape
    /// is what everything on this machine already speaks, so "etc." is answered by a row rather than
    /// by a third protocol.
    pub fn defaults() -> Vec<Provider> {
        vec![
            Provider {
                name: "claude".to_owned(),
                // **The command line, not the API.** It is already signed in, it brings its own
                // tools, it reads the project's own `CLAUDE.md`, and Unluminate holds no key for it.
                wire: Wire::ClaudeCli,
                command: "claude".to_owned(),
                url: String::new(),
                // Empty means the agent's own default, which is the model the person chose in
                // `claude` itself. Naming one here would quietly override a choice made elsewhere.
                model: String::new(),
                key_env: String::new(),
                key_entry: String::new(),
                max_tokens: DEFAULT_MAX_TOKENS,
            },
            Provider {
                name: "codex".to_owned(),
                wire: Wire::CodexCli,
                command: "codex".to_owned(),
                url: String::new(),
                model: String::new(),
                key_env: String::new(),
                key_entry: String::new(),
                max_tokens: DEFAULT_MAX_TOKENS,
            },
            Provider {
                name: "local".to_owned(),
                wire: Wire::OpenAi,
                command: String::new(),
                // The address llama.cpp, LM Studio and every OpenAI-compatible server on this
                // machine listen on, with no key, so the pane answers on a machine with no account
                // configured at all.
                url: "http://127.0.0.1:8080/v1/chat/completions".to_owned(),
                model: "local".to_owned(),
                key_env: String::new(),
                key_entry: String::new(),
                max_tokens: DEFAULT_MAX_TOKENS,
            },
        ]
    }

    /// Whether this endpoint can be reached at all, as a sentence naming what is missing.
    ///
    /// Checked before a request rather than after one, so a URL with a typo in it is a sentence in
    /// the pane rather than a connection refused thirty seconds later.
    pub fn why_not(&self) -> Option<String> {
        if self.wire.is_a_program() {
            return self.why_the_program_will_not_run();
        }
        if self.url.trim().is_empty() {
            return Some(format!(
                "`{}` has no URL. Settings -> Agent-Chat is where it goes.",
                self.name
            ));
        }
        if !self.url.starts_with("http://") && !self.url.starts_with("https://") {
            return Some(format!(
                "`{}` has `{}` for a URL, which is not an address: it starts with http:// or https://.",
                self.name, self.url
            ));
        }
        if self.model.trim().is_empty() {
            return Some(format!("`{}` names no model.", self.name));
        }
        if self.wants_a_key() && self.key().is_none() {
            return Some(match self.key_env.is_empty() {
                true => format!(
                    "`{}` reads its key from the keychain entry `{}`, and there is nothing in it.",
                    self.name, self.key_entry
                ),
                false => format!(
                    "`{}` reads its key from ${}, and this window has no such variable. Set it and start Unluminate again.",
                    self.name, self.key_env
                ),
            });
        }
        None
    }

    /// Why the program this row names cannot be run, as a sentence naming it.
    ///
    /// Checked before the child is spawned, so a machine with no `codex` on it says so in the pane
    /// rather than answering with the operating system's own words about a file not being found —
    /// which is `task-1692`'s rule for a missing debug adapter, kept for a missing agent.
    fn why_the_program_will_not_run(&self) -> Option<String> {
        let named = self.command.trim();
        if named.is_empty() {
            return Some(format!(
                "`{}` names no program. Settings -> Agent-Chat is where it goes.",
                self.name
            ));
        }
        match program(named) {
            Some(_) => None,
            None => Some(format!(
                "`{named}` is not installed, or is not on this window's PATH. Install it and start Unluminate again."
            )),
        }
    }

    /// Whether this row runs a program rather than sending to an address.
    pub fn is_a_program(&self) -> bool {
        self.wire.is_a_program()
    }

    /// Where the program really is, or `None` when nothing of that name can be found.
    pub fn program_path(&self) -> Option<std::path::PathBuf> {
        match self.wire.is_a_program() {
            true => program(self.command.trim()),
            false => None,
        }
    }

    /// Whether this endpoint is configured to need a key at all.
    pub fn wants_a_key(&self) -> bool {
        // **A program never does.** `claude` and `codex` hold their own credentials, which is the
        // whole reason talking to them is better than talking to the APIs behind them.
        !self.wire.is_a_program()
            && (!self.key_env.trim().is_empty() || !self.key_entry.trim().is_empty())
    }

    /// The key, read now.
    ///
    /// Read at the moment of use and never held, which is `keychain::read`'s own rule: the value is
    /// in this process for as long as it takes to put it in a header. The environment is asked first
    /// because it is the one that answers on every platform.
    pub fn key(&self) -> Option<String> {
        let named = self.key_env.trim();
        if !named.is_empty() {
            if let Ok(value) = std::env::var(named) {
                let value = value.trim().to_owned();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
        let entry = self.key_entry.trim();
        match entry.is_empty() {
            true => None,
            false => read_a_keychain_entry(entry),
        }
    }

    /// Whether a key is there, for a settings page that says `set` or `not set` and never the value.
    pub fn has_a_key(&self) -> bool {
        self.key().is_some()
    }

    /// The headers this endpoint's requests carry, key included.
    ///
    /// Built here rather than in `client.rs` so that the difference between the two APIs'
    /// authentication — a bearer token against an `x-api-key` and a version — is in the file that
    /// knows what an endpoint is.
    pub fn headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
        match self.wire {
            // The two OpenAI shapes authenticate the same way; only their bodies differ.
            Wire::OpenAi | Wire::Responses => {
                if let Some(key) = self.key() {
                    headers.push(("authorization".to_owned(), format!("Bearer {key}")));
                }
            }
            // A program is not sent headers at all, and asking for them is a caller's mistake
            // rather than something to answer with a guess.
            Wire::ClaudeCli | Wire::CodexCli => {}
            Wire::Anthropic => {
                if let Some(key) = self.key() {
                    headers.push(("x-api-key".to_owned(), key));
                }
                // Required, and it is a date rather than a number. Pinned rather than tracked: a
                // client that sent whatever was newest would change behaviour when somebody else
                // published something.
                headers.push(("anthropic-version".to_owned(), "2023-06-01".to_owned()));
            }
        }
        headers
    }
}

/// Where the program called `name` really is, looking on `PATH` the way a shell would.
///
/// A name with a separator in it is a path and is taken as one, which is `login_shell::find`'s rule.
/// The **file that was found** is what comes back rather than the bare name, so a refusal can say
/// where it looked and `Command::new` cannot pick a different one a moment later.
///
/// **On Windows the extensions in `PATHEXT` are tried first and the bare file is not tried at all**,
/// which is what `cmd.exe` itself does: a file with no extension is not a program there. That is not
/// a nicety — npm installs three files for `codex`, and one of them is an extension-less shell script
/// for Git Bash. Found first, it was handed to `CreateProcess`, which answered *"%1 is not a valid
/// Win32 application. (os error 193)"*, and the pane reported that a perfectly working agent could
/// not be started. Measured against the real `codex` on this machine.
pub fn program(name: &str) -> Option<std::path::PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let looks_like_a_path = name.contains('/') || name.contains(std::path::MAIN_SEPARATOR);
    if looks_like_a_path {
        let path = std::path::PathBuf::from(name);
        return path.is_file().then(|| absolute(path));
    }
    let named_with_an_extension = std::path::Path::new(name).extension().is_some();
    let extensions: Vec<String> = match cfg!(windows) && !named_with_an_extension {
        true => std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
            .split(';')
            .map(|one| one.trim().to_owned())
            .filter(|one| !one.is_empty())
            .collect(),
        false => Vec::new(),
    };
    // A bare name on Windows is only ever the extensions; everywhere else, and for a name that
    // carries its own extension, it is the file itself.
    let bare_is_a_program = !cfg!(windows) || named_with_an_extension;
    let path = std::env::var_os("PATH")?;
    for folder in std::env::split_paths(&path) {
        for extension in &extensions {
            let with = folder.join(format!("{name}{extension}"));
            if with.is_file() {
                return Some(absolute(with));
            }
        }
        let bare = folder.join(name);
        if bare_is_a_program && bare.is_file() {
            return Some(absolute(bare));
        }
    }
    None
}

/// `path` made absolute against the folder this process is in, if it is not already.
///
/// **Because the child is started somewhere else.** A relative program — one typed into the settings
/// page as `.\agent.exe`, or found through a relative entry in `PATH` — is checked against Unluminate's
/// own current directory and then handed to a `Command` whose working directory has been set to the
/// project. It would resolve against the project instead, which is a different file, and on a folder
/// somebody else can write is a different file somebody else chose. Not `canonicalize`, which on
/// Windows answers with a verbatim `\\?\` path — the exact thing `unluminate_terminal::paths::plain`
/// exists to strip back off again.
fn absolute(path: std::path::PathBuf) -> std::path::PathBuf {
    if path.is_absolute() {
        return path;
    }
    match std::env::current_dir() {
        Ok(here) => here.join(path),
        Err(_) => path,
    }
}

/// The keychain entry called `name`, on a platform that has a keychain.
///
/// `security` on macOS and `secret-tool` on Linux, driven the way `services::agent_tasks::keychain`
/// drives them and for the same reason — the machine's own keychain has the machine's own unlock
/// rules. This crate cannot call that module (it is in `unluminate-app`, which depends on this one), so
/// the two lines are here; the doctrine, and the note that Windows has no keychain, live there.
///
/// Nothing is printed or logged. A read that fails answers `None` and says nothing about why.
fn read_a_keychain_entry(name: &str) -> Option<String> {
    if !name
        .chars()
        .all(|one| one.is_ascii_alphanumeric() || matches!(one, '-' | '.' | '_'))
    {
        return None;
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("security");
        command.args([
            "find-generic-password",
            "-s",
            "unluminate-agent-chat",
            "-a",
            name,
            "-w",
        ]);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = std::process::Command::new("secret-tool");
        command.args(["lookup", "service", "unluminate-agent-chat", "account", name]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = name;
        None
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let output = command.output().ok()?;
        if !output.status.success() {
            return None;
        }
        let secret = String::from_utf8(output.stdout).ok()?;
        let secret = secret.trim_end_matches(['\n', '\r']).to_owned();
        match secret.is_empty() {
            true => None,
            false => Some(secret),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_rows_the_ticket_names_are_command_lines_and_neither_wants_a_key() {
        // *"connection to Claude and codex etc through cli"* — so the two rows that ship for them
        // run the agent already installed on this machine rather than sending to the API behind it.
        // **Neither holds a key, whatever a settings file says**, which is the whole reason this is
        // the better half of the feature: there is nothing for Unluminate to store, read or redact.
        let defaults = Provider::defaults();
        let names: Vec<&str> = defaults.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, vec!["claude", "codex", "local"]);
        assert_eq!(defaults[0].wire, Wire::ClaudeCli);
        assert_eq!(defaults[0].command, "claude");
        assert!(defaults[0].url.is_empty(), "a program has no address");
        assert_eq!(defaults[1].wire, Wire::CodexCli);
        assert_eq!(defaults[1].command, "codex");
        for row in &defaults[..2] {
            assert!(row.is_a_program());
            assert!(!row.wants_a_key());
            // And no model is named, so the agent answers with the one its own person chose.
            assert!(row.model.is_empty(), "{}", row.name);
        }
        // The third is an address, so the ticket's "configure url" has something to configure and a
        // machine with neither agent installed can still be pointed at an llama.cpp.
        assert_eq!(defaults[2].wire, Wire::OpenAi);
        assert!(!defaults[2].is_a_program());
        assert!(!defaults[2].wants_a_key());
        assert_eq!(defaults[2].why_not(), None);
    }

    #[test]
    fn every_shipped_wire_name_is_one_this_version_speaks() {
        // The registry and the enum cannot disagree, which is what the five registries before this
        // one each have a test for.
        for name in WIRES {
            assert!(
                Wire::from_name(name).is_some(),
                "{name} is registered with no code"
            );
            assert_eq!(Wire::from_name(name).expect("just checked").name(), *name);
        }
        assert!(Wire::from_name("gemini").is_none());
        for provider in Provider::defaults() {
            assert!(WIRES.contains(&provider.wire.name()));
        }
    }

    #[test]
    fn a_url_with_a_typo_in_it_is_a_sentence_rather_than_a_connection_refused() {
        let mut provider = Provider::defaults()[2].clone();
        provider.url = "127.0.0.1:8080/v1/chat/completions".to_owned();
        let why = provider.why_not().expect("a refusal");
        assert!(why.contains("http://"), "{why}");
        provider.url = String::new();
        assert!(provider.why_not().expect("a refusal").contains("no URL"));
        provider.url = "http://127.0.0.1:8080/v1/chat/completions".to_owned();
        provider.model = String::new();
        assert!(provider.why_not().expect("a refusal").contains("no model"));
    }

    /// An endpoint of `wire`, which is what a person configuring one gets.
    fn endpoint(wire: Wire, url: &str) -> Provider {
        Provider {
            name: wire.name().to_owned(),
            wire,
            command: String::new(),
            url: url.to_owned(),
            model: "a-model".to_owned(),
            key_env: String::new(),
            key_entry: String::new(),
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    #[test]
    fn a_missing_key_names_the_variable_it_would_have_come_from() {
        // The refusal has to say what to do about it, which is `task-1692`'s rule for a missing
        // debug adapter applied to a missing key.
        let mut provider = endpoint(Wire::Anthropic, "https://api.anthropic.com/v1/messages");
        provider.key_env = "UNLUMINATE_A_VARIABLE_NOTHING_SETS".to_owned();
        let why = provider.why_not().expect("a refusal");
        assert!(why.contains("UNLUMINATE_A_VARIABLE_NOTHING_SETS"), "{why}");
        assert!(!provider.has_a_key());
        // And with the variable set it goes away, and the key reaches the headers.
        std::env::set_var("UNLUMINATE_A_VARIABLE_NOTHING_SETS", "secret-value");
        assert_eq!(provider.why_not(), None);
        assert!(provider.has_a_key());
        let headers = provider.headers();
        assert!(headers
            .iter()
            .any(|(name, value)| name == "x-api-key" && value == "secret-value"));
        assert!(headers.iter().any(|(name, _)| name == "anthropic-version"));
        std::env::remove_var("UNLUMINATE_A_VARIABLE_NOTHING_SETS");
    }

    #[test]
    fn the_two_shapes_authenticate_the_way_their_own_apis_do() {
        std::env::set_var("UNLUMINATE_TEST_OPENAI_KEY", "sk-test");
        let mut provider = endpoint(Wire::Responses, "https://api.openai.com/v1/responses");
        provider.key_env = "UNLUMINATE_TEST_OPENAI_KEY".to_owned();
        let headers = provider.headers();
        assert!(headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer sk-test"));
        assert!(!headers.iter().any(|(name, _)| name == "x-api-key"));
        std::env::remove_var("UNLUMINATE_TEST_OPENAI_KEY");
    }

    #[test]
    fn an_empty_variable_is_the_same_as_no_variable() {
        // Because that is what an unset key looks like in a shell that exported it and then cleared
        // it, and a request sent with an empty bearer token fails with a message about the token
        // rather than about the configuration.
        std::env::set_var("UNLUMINATE_TEST_EMPTY_KEY", "   ");
        let mut provider = endpoint(Wire::Anthropic, "https://api.anthropic.com/v1/messages");
        provider.key_env = "UNLUMINATE_TEST_EMPTY_KEY".to_owned();
        assert!(!provider.has_a_key());
        assert!(provider.why_not().is_some());
        std::env::remove_var("UNLUMINATE_TEST_EMPTY_KEY");
    }
}
