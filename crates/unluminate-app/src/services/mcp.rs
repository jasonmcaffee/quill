//! The window's half of the MCP server.
//!
//! The protocol, the tools and both transports live in `unluminate_cli::mcp`, where the client can reach
//! them and where nothing knows what a window is. What is here is the small part that only a window
//! can do: start and stop the HTTP endpoint as the settings change, and say what it is doing so the
//! settings page can show it.
//!
//! ## The window hosts a server; it is not a second way in
//!
//! The endpoint this starts drives Unluminate exactly as `unluminate-cli` does — down the control channel, with
//! the token out of the instance file. It happens to be running inside one of the windows it can
//! drive, and that is all. Two consequences fall out of it and both are wanted:
//!
//! - **One server drives every window.** It finds Unluminates by reading the instances folder, so the
//!   endpoint hosted by this window answers for the one next to it too, with `instance` saying
//!   which. That turns the obvious collision — two Unluminates, one `mcp.port` — from a bug into the
//!   behaviour: the second window sees the port is held, does not start a second listener, and says
//!   so on the page.
//! - **`--control off` means there is nothing to serve.** A window with no command channel has no
//!   instance file, so an endpoint would listen on a port and be able to do nothing. It says that
//!   instead of pretending.

use std::path::{Path, PathBuf};

use unluminate_cli::mcp::{self, http::Endpoint, tools::Areas, Unluminates, Server, Shape};

/// What the window is doing about MCP, which is what the page and `mcp status` both read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The setting is off. Nothing is listening, which is the default.
    Off,
    /// Listening on this port.
    Listening(u16),
    /// The setting is on, but the port is held by something else — nearly always the Unluminate in the
    /// next window, whose endpoint serves this one too.
    PortTaken(u16, String),
    /// The setting is on and there is no command channel to drive, because Unluminate was started with
    /// `--control off`.
    NoChannel,
}

impl State {
    /// The sentence the settings page and the status bar show.
    pub fn message(&self) -> String {
        match self {
            State::Off => {
                "Not listening. Agents that launch Unluminate's server themselves still work.".to_owned()
            }
            State::Listening(port) => format!("Listening at {}", mcp::endpoint(*port)),
            State::PortTaken(port, problem) => format!(
                "Port {port} is already in use ({problem}). If that is another Unluminate, its server \
                 drives this window too."
            ),
            State::NoChannel => {
                "Unluminate was started with --control off, so there is no command channel for an MCP \
                 server to drive."
                    .to_owned()
            }
        }
    }

    pub fn port(&self) -> Option<u16> {
        match self {
            State::Listening(port) => Some(*port),
            _ => None,
        }
    }

    /// The word `mcp status --json` reports, so a script can branch on it.
    pub fn name(&self) -> &'static str {
        match self {
            State::Off => "off",
            State::Listening(_) => "listening",
            State::PortTaken(_, _) => "port-taken",
            State::NoChannel => "no-channel",
        }
    }
}

/// The endpoint this window is hosting, if it is hosting one.
pub struct Hosted {
    endpoint: Option<Endpoint>,
    /// What was asked for last, so a frame that changed nothing does nothing. The folder is in it
    /// because `project open` changes which project this window is showing, and the endpoint's
    /// preference for which Unluminate to drive is that folder.
    wanted: Option<(u16, Shape, Areas, PathBuf)>,
    state: State,
}

impl Hosted {
    pub fn new() -> Self {
        Self { endpoint: None, wanted: None, state: State::Off }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Bring what is running into line with what the settings say.
    ///
    /// Called after any settings change and after the command channel opens. It is cheap when
    /// nothing moved — two comparisons — because it is called from the same places every other
    /// settings change is applied from and none of those is a rare event.
    ///
    /// `has_channel` is false when Unluminate was started with `--control off`.
    pub fn reconcile(
        &mut self,
        enabled: bool,
        port: u16,
        shape: Shape,
        areas: &Areas,
        has_channel: bool,
        folder: &Path,
    ) {
        if !enabled {
            self.endpoint = None;
            self.wanted = None;
            self.state = State::Off;
            return;
        }
        if !has_channel {
            self.endpoint = None;
            self.wanted = None;
            self.state = State::NoChannel;
            return;
        }
        let wanted = (port, shape, areas.clone(), folder.to_path_buf());
        if self.wanted.as_ref() == Some(&wanted) && self.endpoint.is_some() {
            return;
        }
        // Dropped **before** the new one is started, or a change of tool shape on the same port
        // would be a listener trying to bind a port the old one still holds.
        self.endpoint = None;
        let server =
            Server::equipped(shape, areas.clone(), Unluminates::for_window(folder.to_path_buf()));
        match Endpoint::start(port, server) {
            Ok(endpoint) => {
                self.state = State::Listening(endpoint.port());
                self.endpoint = Some(endpoint);
            }
            Err(problem) => self.state = State::PortTaken(port, problem.to_string()),
        }
        self.wanted = Some(wanted);
    }
}

impl Default for Hosted {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_host() -> Hosted {
        Hosted::new()
    }

    /// A folder that need not exist: nothing here reads it, it is only the preference passed to the
    /// driver when several Unluminates are running.
    fn a_project() -> PathBuf {
        PathBuf::from("/a/project")
    }

    #[test]
    fn nothing_listens_until_the_setting_is_on() {
        let mut hosted = a_host();
        hosted.reconcile(false, 0, Shape::Grouped, &Areas::all(), true, &a_project());
        assert_eq!(hosted.state(), &State::Off);
        assert!(hosted.endpoint.is_none());
    }

    #[test]
    fn turning_it_on_listens_and_turning_it_off_gives_the_port_back() {
        let mut hosted = a_host();
        // Port zero, so the operating system finds a free one and two tests can run at once. What
        // is being checked is the reconciling rather than the number.
        hosted.reconcile(true, 0, Shape::Grouped, &Areas::all(), true, &a_project());
        let port = match hosted.state() {
            State::Listening(port) => *port,
            other => panic!("it should be listening, not {other:?}"),
        };
        assert!(port > 0, "the operating system should have chosen one");

        hosted.reconcile(false, 0, Shape::Grouped, &Areas::all(), true, &a_project());
        assert_eq!(hosted.state(), &State::Off);
    }

    #[test]
    fn a_frame_that_changed_nothing_does_not_restart_the_listener() {
        let mut hosted = a_host();
        hosted.reconcile(true, 0, Shape::Grouped, &Areas::all(), true, &a_project());
        let first = hosted.state().port().expect("a port");
        for _ in 0..5 {
            hosted.reconcile(true, first, Shape::Grouped, &Areas::all(), true, &a_project());
        }
        assert_eq!(hosted.state().port(), Some(first), "it should still be the same listener");
    }

    #[test]
    fn a_window_with_no_command_channel_says_so_rather_than_listening_uselessly() {
        let mut hosted = a_host();
        hosted.reconcile(true, 0, Shape::Grouped, &Areas::all(), false, &a_project());
        assert_eq!(hosted.state(), &State::NoChannel);
        assert!(hosted.endpoint.is_none());
        assert!(hosted.state().message().contains("--control off"));
    }

    #[test]
    fn a_port_somebody_else_holds_is_reported_rather_than_swallowed() {
        // The two-windows-one-port case, which is the ordinary one and must read as a fact rather
        // than as a failure.
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = held.local_addr().expect("an address").port();
        let mut hosted = a_host();
        hosted.reconcile(true, port, Shape::Grouped, &Areas::all(), true, &a_project());
        match hosted.state() {
            State::PortTaken(taken, _) => assert_eq!(*taken, port),
            other => panic!("it should have reported the port as taken, not {other:?}"),
        }
        assert!(hosted.state().message().contains("another Unluminate"));
    }
}
