//! Whether a newer Unluminate has been released.
//!
//! `task-1804` §6: *"Nothing in the binary asks whether a newer version exists. A person who installs
//! 0.34.2 stays on 0.34.2 until they happen to visit the releases page. For a product that releases
//! on every finished task this is the largest single gap between 'we shipped a fix' and 'someone has
//! the fix'."*
//!
//! The machinery was already most of the way there: `tools/release.ps1` and `tools/release.sh`
//! publish a GitHub release with the installer attached on every finished task, and `releases/` keeps
//! the file. What was missing was a check and a prompt, and this is the check.
//!
//! ## Nothing is fetched that was not asked for, and that is not softened here
//!
//! `task-1692` drew this line and the chat pane kept it: *"There is no discovery, no model list, no
//! telemetry and nothing at startup."* An editor that phones home the moment it opens is exactly
//! what that rule exists to prevent, and "but it is only checking for updates" is what every program
//! that does it says.
//!
//! So **`update.check` is `off` until somebody says otherwise**, and with it off nothing is ever
//! sent. What is always there is `Unluminate -> Check for Updates`, which is a person asking, and
//! `unluminate-cli update check`, which is an agent asking. Turning the setting to `start` is a
//! person saying *ask every time I open it*, once, in a Settings page.
//!
//! ## What it asks, and what it does not
//!
//! One `GET` to `https://api.github.com/repos/jasonmcaffee/unluminate/releases/latest`, unauthenticated,
//! with no query, no header identifying this machine beyond the `user-agent` GitHub requires, and no
//! body. It sends **nothing about the person or the project**: not which files are open, not which
//! version is installed — the comparison is made here, on what came back, so the server is never told
//! what to compare against.
//!
//! ## The transport is the chat pane's
//!
//! `unluminate_chat::client::tls_config`, verbatim, rather than a second one. That function is where
//! two measured facts live — `ureq`'s default TLS provider is Rustls whether or not the feature is
//! on, so an `https` request that does not name the provider **panics inside the transport** on the
//! worker thread; and `RootCerts::WebPki` switches the machine's own certificate store off, which
//! fails behind an employer's private chain. A second copy of that reasoning would be a second place
//! to get it wrong.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Where the releases are.
const RELEASES: &str = "https://api.github.com/repos/jasonmcaffee/unluminate/releases/latest";
/// Where a person goes to get one.
pub const RELEASES_PAGE: &str = "https://github.com/jasonmcaffee/unluminate/releases";
/// GitHub refuses a request with no `user-agent` outright, so it names the program and nothing else.
const AGENT: &str = "Unluminate";
/// How long the whole thing is given. It is a background question and a slow answer is no answer.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What the releases page says the newest one is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// `0.35.0` — the tag with its `v` taken off.
    pub version: String,
    /// The page a person downloads it from.
    pub url: String,
    /// What the release said about itself, cut to something a status bar can hold.
    pub notes: String,
}

/// What a check came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// There is a newer one.
    Newer(Release),
    /// This is the newest one, and this is what it is.
    Current(String),
    /// It could not be asked. The message is what went wrong, in the server's own words where there
    /// are any -- `unluminate-git`'s rule about never inventing one.
    Failed(String),
}

impl Answer {
    /// One sentence for the status bar and for the command line's `message`.
    pub fn sentence(&self) -> String {
        match self {
            Answer::Newer(release) => format!(
                "Unluminate {} is out. This is {}. {}",
                release.version,
                crate::build_info::VERSION,
                RELEASES_PAGE
            ),
            Answer::Current(version) => format!("Unluminate {version} is the newest there is."),
            Answer::Failed(problem) => format!("Could not check for a newer Unluminate: {problem}"),
        }
    }
}

/// A check running on a thread, and its answer when it arrives.
///
/// A thread and a waker, arranged as `unluminate_git::Worker`, the text search and the chat client
/// already are — because a request over the network on the drawing thread would stop the window
/// drawing for as long as it took, which on a slow connection looks exactly like a crash.
pub struct Check {
    answers: Receiver<Answer>,
    /// True until the answer has been taken, so the About box can say it is asking.
    asking: bool,
}

impl Check {
    /// Start asking. `wake` asks the window to draw again when the answer lands.
    pub fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (sender, answers) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("unluminate-update-check".to_owned())
            .spawn(move || {
                let answer = ask();
                // The window may have gone; a send to a closed channel is the ordinary end of this
                // thread rather than something to report.
                let _ = sender.send(answer);
                wake();
            })
            .ok();
        Self { answers, asking: true }
    }

    /// The same, against a scripted server, which is what the tests drive.
    pub fn start_from(url: String, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (sender, answers) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("unluminate-update-check".to_owned())
            .spawn(move || {
                let _ = sender.send(ask_at(&url));
                wake();
            })
            .ok();
        Self { answers, asking: true }
    }

    /// The answer, once. `None` while it is still being asked.
    pub fn poll(&mut self) -> Option<Answer> {
        match self.answers.try_recv() {
            Ok(answer) => {
                self.asking = false;
                Some(answer)
            }
            Err(_) => None,
        }
    }

    /// True while nothing has come back yet.
    pub fn is_asking(&self) -> bool {
        self.asking
    }
}

/// Ask the real releases endpoint. Runs on the worker thread.
pub fn ask() -> Answer {
    ask_at(RELEASES)
}

/// The same against any address, so a test can point it at a server on loopback.
pub fn ask_at(url: &str) -> Answer {
    let config = ureq::Agent::config_builder()
        .tls_config(unluminate_chat::client::tls_config())
        // The body of a 403 is where GitHub says *why* -- a rate limit, usually -- and that is the
        // whole of what there is to tell somebody. `unluminate-chat` makes the same argument.
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        // A redirect goes somewhere nobody named. There is nothing secret in this request, so this is
        // not the security decision it is in the chat client; it is the same decision about honesty.
        .max_redirects(0)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let sent = agent
        .get(url)
        .header("user-agent", AGENT)
        .header("accept", "application/vnd.github+json")
        .call();
    let mut reply = match sent {
        Ok(reply) => reply,
        Err(problem) => return Answer::Failed(problem.to_string()),
    };
    let status = reply.status().as_u16();
    let body = reply.body_mut().read_to_string().unwrap_or_default();
    if status != 200 {
        // GitHub's own words, cut short: its `message` is a sentence and the rest of the object is
        // documentation links nobody reads out of a status bar.
        let said = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("message")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("the releases page answered {status}"));
        return Answer::Failed(said);
    }
    match read(&body) {
        Some(release) => match is_newer(crate::build_info::VERSION, &release.version) {
            true => Answer::Newer(release),
            false => Answer::Current(release.version),
        },
        None => Answer::Failed("the releases page answered something this version cannot read".to_owned()),
    }
}

/// One release out of what GitHub sent.
///
/// Pure, so every shape it has to survive is a test with no socket behind it.
pub fn read(body: &str) -> Option<Release> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = value.get("tag_name")?.as_str()?.trim();
    let version = tag.trim_start_matches('v').trim().to_owned();
    if version.is_empty() {
        return None;
    }
    let url = value
        .get("html_url")
        .and_then(|url| url.as_str())
        .unwrap_or(RELEASES_PAGE)
        .to_owned();
    // The first line of the notes, which is what `release.ps1` writes as the summary. The rest is the
    // download instructions, which somebody reading a status bar does not need.
    let notes = value
        .get("body")
        .and_then(|body| body.as_str())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned();
    Some(Release { version, url, notes })
}

/// Whether `found` is a later version than `current`.
///
/// Compared as numbers a part at a time, because `0.9.0` is **older** than `0.34.2` and every
/// comparison of the two as text says otherwise. A part that is not a number stops the comparison
/// and answers no: a tag like `v1.0.0-rc1` is not something this version knows how to be sure about,
/// and telling somebody there is an update when there may not be is worse than saying nothing.
pub fn is_newer(current: &str, found: &str) -> bool {
    let parts = |version: &str| -> Option<Vec<u64>> {
        version.split('.').map(|part| part.trim().parse::<u64>().ok()).collect()
    };
    let (Some(current), Some(found)) = (parts(current), parts(found)) else {
        return false;
    };
    for index in 0..current.len().max(found.len()) {
        let (here, there) = (current.get(index).copied().unwrap_or(0), found.get(index).copied().unwrap_or(0));
        if there != here {
            return there > here;
        }
    }
    false
}

/// A `Sender` that nothing reads, for a caller that wants the shape and not the answer.
pub type Answers = Sender<Answer>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_compared_as_numbers_rather_than_as_text() {
        assert!(is_newer("0.34.2", "0.35.0"));
        assert!(is_newer("0.34.2", "0.34.3"));
        assert!(is_newer("0.34.2", "1.0.0"));
        assert!(!is_newer("0.34.2", "0.34.2"));
        assert!(!is_newer("0.34.2", "0.34.1"));
        // The comparison this exists for: as text, "0.9.0" sorts after "0.34.2".
        assert!(!is_newer("0.34.2", "0.9.0"));
        assert!(is_newer("0.9.0", "0.34.2"));
        // Shorter is not smaller: 1.0 and 1.0.0 are the same version.
        assert!(!is_newer("1.0.0", "1.0"));
        assert!(!is_newer("1.0", "1.0.0"));
    }

    #[test]
    fn a_tag_this_version_cannot_be_sure_about_is_not_an_update() {
        assert!(!is_newer("0.34.2", "1.0.0-rc1"));
        assert!(!is_newer("0.34.2", "nightly"));
        assert!(!is_newer("0.34.2", ""));
    }

    #[test]
    fn a_release_is_read_out_of_what_github_sends() {
        let body = r#"{
            "tag_name": "v0.35.0",
            "html_url": "https://github.com/jasonmcaffee/unluminate/releases/tag/v0.35.0",
            "body": "\n\nFind and Replace, and the keystroke at 2 MB\n\nWindows: download the setup below."
        }"#;
        let release = read(body).expect("it reads");
        assert_eq!(release.version, "0.35.0", "the v comes off");
        assert_eq!(release.url, "https://github.com/jasonmcaffee/unluminate/releases/tag/v0.35.0");
        assert_eq!(
            release.notes, "Find and Replace, and the keystroke at 2 MB",
            "the first line that says something, not the download instructions"
        );
    }

    #[test]
    fn a_reply_with_no_tag_in_it_is_not_a_release() {
        assert_eq!(read("{}"), None);
        assert_eq!(read(r#"{"tag_name": ""}"#), None);
        assert_eq!(read("not json at all"), None);
        assert_eq!(read(r#"{"message": "Not Found"}"#), None);
    }

    #[test]
    fn a_release_with_no_notes_and_no_url_still_answers() {
        let release = read(r#"{"tag_name": "v1.2.3"}"#).expect("it reads");
        assert_eq!(release.version, "1.2.3");
        assert_eq!(release.url, RELEASES_PAGE, "the releases page, when the release names none");
        assert_eq!(release.notes, "");
    }

    /// The whole of it, end to end, against a scripted server on loopback.
    ///
    /// `unluminate-chat`'s arrangement: a `TcpListener` on `127.0.0.1:0` replaying fixed bytes, which
    /// is what makes "the client, end to end" a unit test with no network and nothing to be flaky
    /// about.
    #[test]
    fn a_newer_release_on_a_scripted_server_is_read_as_an_update() {
        let body = format!(
            r#"{{"tag_name": "v999.0.0", "html_url": "https://example.invalid/999", "body": "A much later one"}}"#
        );
        let url = scripted(200, &body);
        match ask_at(&url) {
            Answer::Newer(release) => {
                assert_eq!(release.version, "999.0.0");
                assert_eq!(release.notes, "A much later one");
            }
            other => panic!("expected an update, got {other:?}"),
        }
    }

    #[test]
    fn the_version_this_is_reads_as_the_newest_there_is() {
        let body = format!(r#"{{"tag_name": "v{}"}}"#, crate::build_info::VERSION);
        match ask_at(&scripted(200, &body)) {
            Answer::Current(version) => assert_eq!(version, crate::build_info::VERSION),
            other => panic!("expected current, got {other:?}"),
        }
    }

    /// A refusal is quoted in the server's own words rather than replaced by one made up here.
    #[test]
    fn a_rate_limit_is_reported_in_githubs_own_words() {
        let body = r#"{"message": "API rate limit exceeded for 203.0.113.1."}"#;
        match ask_at(&scripted(403, body)) {
            Answer::Failed(said) => assert!(said.contains("rate limit"), "{said}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_server_that_is_not_there_is_a_failure_rather_than_a_panic() {
        // Port 1 on loopback, which nothing listens on.
        match ask_at("http://127.0.0.1:1/releases") {
            Answer::Failed(_) => {}
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// One request, answered with `status` and `body`, and the address to ask it at.
    fn scripted(status: u16, body: &str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("the address");
        let body = body.to_owned();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            // Enough of the request to know it arrived. The client is not reading yet, so this must
            // not block on a body that is never sent.
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
            let _ = stream.flush();
        });
        format!("http://{address}/releases/latest")
    }
}
