//! What an endpoint is: a name, a wire shape, a URL, a model, and where its key comes from.
//!
//! The ticket asks for "a config in settings to allow configure url for Claude, codex etc.", so a
//! provider is a **row in a list** rather than a constant in the binary, and the three that ship are
//! only the three rows that are there the first time the page is opened.
//!
//! ## Where a key lives, which is nowhere Quill writes
//!
//! `services::agent_tasks::keychain` says it plainly: a secret is never written into a settings
//! file, because a settings file is copied between machines, readable by anything that can read the
//! folder, and pasted into a bug report. It also says, honestly, that there is **no Windows
//! keychain** in Quill — on Windows it answers `None` and refuses to write.
//!
//! So a provider names an **environment variable** and Quill reads it at the moment a request is
//! sent. `ANTHROPIC_API_KEY` is the name every tool on this machine already uses and the one
//! `claude` itself reads, so on the platform Quill is developed on there is a way to have a key that
//! does not involve Quill storing one. What is written down is the *name of the place the key is*,
//! exactly as Agent-Tasks writes down the name of a keychain entry. A local endpoint names neither,
//! because llama.cpp wants no key.

/// Which protocol an endpoint speaks.
///
/// Two, and there are exactly two for a reason. **OpenAI's `/v1/chat/completions`** is what
/// llama.cpp, LM Studio, Ollama, vLLM, every gateway on this machine and OpenAI itself speak, so one
/// shape reaches nearly everything. **Anthropic's `/v1/messages`** is genuinely a different
/// protocol — named events, indexed content blocks, a system prompt that is a field rather than a
/// message — and it is the one the ticket names first.
///
/// A third was weighed and left out; see `tasks/task-1767-agent-chat-tdd.md` §8.1. Adding one is an
/// arm here, an arm in `wire.rs` and a test, rather than a redesign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Wire {
    OpenAi,
    Anthropic,
}

/// Every wire shape this version speaks, which is what a settings page offers and what a
/// configuration naming an unknown one is refused with.
///
/// The sixth registry of this shape in Quill, after the renderers, the project detectors, the
/// debuggers, the UI providers and the chrome. A configuration naming a shape this version has not
/// got is refused with the list rather than loading as an endpoint whose every request fails
/// obscurely — the rule `language.renders` set and every one since has kept.
pub const WIRES: &[&str] = &["openai", "anthropic"];

impl Wire {
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim() {
            "openai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }
}

/// One endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    /// What it is called in the header chip, in `plugins run agent-chat use` and in the settings
    /// file. Lower case, one word.
    pub name: String,
    pub wire: Wire,
    /// The whole URL, including the path — because that is the part that differs between a hosted
    /// API and the llama.cpp on this machine, and a base URL that Quill appended a path to would be
    /// a Quill that decided what somebody's gateway looks like.
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
                wire: Wire::Anthropic,
                url: "https://api.anthropic.com/v1/messages".to_owned(),
                model: "claude-opus-5".to_owned(),
                key_env: "ANTHROPIC_API_KEY".to_owned(),
                key_entry: String::new(),
                max_tokens: DEFAULT_MAX_TOKENS,
            },
            Provider {
                name: "codex".to_owned(),
                wire: Wire::OpenAi,
                url: "https://api.openai.com/v1/chat/completions".to_owned(),
                model: "gpt-5-codex".to_owned(),
                key_env: "OPENAI_API_KEY".to_owned(),
                key_entry: String::new(),
                max_tokens: DEFAULT_MAX_TOKENS,
            },
            Provider {
                name: "local".to_owned(),
                wire: Wire::OpenAi,
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
                    "`{}` reads its key from ${}, and this window has no such variable. Set it and start Quill again.",
                    self.name, self.key_env
                ),
            });
        }
        None
    }

    /// Whether this endpoint is configured to need a key at all.
    pub fn wants_a_key(&self) -> bool {
        !self.key_env.trim().is_empty() || !self.key_entry.trim().is_empty()
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
            Wire::OpenAi => {
                if let Some(key) = self.key() {
                    headers.push(("authorization".to_owned(), format!("Bearer {key}")));
                }
            }
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

/// The keychain entry called `name`, on a platform that has a keychain.
///
/// `security` on macOS and `secret-tool` on Linux, driven the way `services::agent_tasks::keychain`
/// drives them and for the same reason — the machine's own keychain has the machine's own unlock
/// rules. This crate cannot call that module (it is in `quill-app`, which depends on this one), so
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
            "quill-agent-chat",
            "-a",
            name,
            "-w",
        ]);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = std::process::Command::new("secret-tool");
        command.args(["lookup", "service", "quill-agent-chat", "account", name]);
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
    fn the_three_that_ship_are_the_two_the_ticket_names_and_one_that_needs_no_account() {
        let defaults = Provider::defaults();
        let names: Vec<&str> = defaults.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, vec!["claude", "codex", "local"]);
        assert_eq!(defaults[0].wire, Wire::Anthropic);
        assert_eq!(defaults[1].wire, Wire::OpenAi);
        // The local one asks for no key at all, so the pane answers on a machine with no account
        // configured.
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

    #[test]
    fn a_missing_key_names_the_variable_it_would_have_come_from() {
        // The refusal has to say what to do about it, which is `task-1692`'s rule for a missing
        // debug adapter applied to a missing key.
        let mut provider = Provider::defaults()[0].clone();
        provider.key_env = "QUILL_A_VARIABLE_NOTHING_SETS".to_owned();
        let why = provider.why_not().expect("a refusal");
        assert!(why.contains("QUILL_A_VARIABLE_NOTHING_SETS"), "{why}");
        assert!(!provider.has_a_key());
        // And with the variable set it goes away, and the key reaches the headers.
        std::env::set_var("QUILL_A_VARIABLE_NOTHING_SETS", "secret-value");
        assert_eq!(provider.why_not(), None);
        assert!(provider.has_a_key());
        let headers = provider.headers();
        assert!(headers
            .iter()
            .any(|(name, value)| name == "x-api-key" && value == "secret-value"));
        assert!(headers.iter().any(|(name, _)| name == "anthropic-version"));
        std::env::remove_var("QUILL_A_VARIABLE_NOTHING_SETS");
    }

    #[test]
    fn the_two_shapes_authenticate_the_way_their_own_apis_do() {
        std::env::set_var("QUILL_TEST_OPENAI_KEY", "sk-test");
        let mut provider = Provider::defaults()[1].clone();
        provider.key_env = "QUILL_TEST_OPENAI_KEY".to_owned();
        let headers = provider.headers();
        assert!(headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer sk-test"));
        assert!(!headers.iter().any(|(name, _)| name == "x-api-key"));
        std::env::remove_var("QUILL_TEST_OPENAI_KEY");
    }

    #[test]
    fn an_empty_variable_is_the_same_as_no_variable() {
        // Because that is what an unset key looks like in a shell that exported it and then cleared
        // it, and a request sent with an empty bearer token fails with a message about the token
        // rather than about the configuration.
        std::env::set_var("QUILL_TEST_EMPTY_KEY", "   ");
        let mut provider = Provider::defaults()[0].clone();
        provider.key_env = "QUILL_TEST_EMPTY_KEY".to_owned();
        assert!(!provider.has_a_key());
        assert!(provider.why_not().is_some());
        std::env::remove_var("QUILL_TEST_EMPTY_KEY");
    }
}
