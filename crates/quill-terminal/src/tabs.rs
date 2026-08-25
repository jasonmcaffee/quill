//! Several terminals, with one of them showing.
//!
//! A tab is a session and nothing else: there is no state a tab carries that the session does not, so
//! closing a tab is dropping a session, which shuts its shell down.

use crate::session::{Session, SessionSettings, Size, Waker};

/// The terminals in the tile, and which one is showing.
pub struct Tabs {
    sessions: Vec<Session>,
    active: usize,
    /// What a new tab runs, and where.
    pub settings: SessionSettings,
    /// The last error from starting a shell, so the tile can say why there is no terminal.
    pub last_error: Option<String>,
}

impl Tabs {
    pub fn new(settings: SessionSettings) -> Self {
        Self { sessions: Vec::new(), active: 0, settings, last_error: None }
    }

    /// Start another terminal and show it. Returns false when the shell could not be started, in which case
    /// [`Self::last_error`] says why.
    pub fn open(&mut self, size: Size, waker: Waker) -> bool {
        match Session::spawn(&self.settings, size, waker) {
            Ok(session) => {
                self.sessions.push(session);
                self.active = self.sessions.len() - 1;
                self.last_error = None;
                true
            }
            Err(problem) => {
                self.last_error = Some(format!(
                    "Quill could not start {}: {problem}",
                    self.settings.shell.clone().unwrap_or_else(|| "a shell".to_owned())
                ));
                false
            }
        }
    }

    /// Add a terminal with no shell behind it, which is what the tests and the screenshot tests use.
    pub fn open_detached(&mut self, size: Size) {
        self.sessions.push(Session::detached(size));
        self.active = self.sessions.len() - 1;
    }

    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Show tab `index`. A number past the end is ignored rather than clamped, because clamping would show
    /// a tab nobody asked for.
    pub fn show(&mut self, index: usize) {
        if index < self.sessions.len() {
            self.active = index;
        }
    }

    pub fn active(&self) -> Option<&Session> {
        self.sessions.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active)
    }

    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    /// The name on each tab, in order.
    ///
    /// Two tabs running the same program would otherwise be told apart only by where they are, so a number
    /// is put in front when a name is used more than once.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for session in &self.sessions {
            let name = session.name().to_owned();
            let repeats = names.iter().filter(|seen| seen.ends_with(&name)).count();
            if repeats > 0 {
                names.push(format!("{name} {}", repeats + 1));
            } else {
                names.push(name);
            }
        }
        names
    }

    /// Close tab `index`, which stops its shell. The tab to its left is shown next.
    pub fn close(&mut self, index: usize) {
        if index >= self.sessions.len() {
            return;
        }
        self.sessions.remove(index);
        if self.active >= self.sessions.len() {
            self.active = self.sessions.len().saturating_sub(1);
        }
    }

    /// Deal with what every session's program asked for, and forget the ones whose shell has stopped.
    ///
    /// A tab whose shell has stopped is closed, because a terminal showing a shell that has gone is a tab
    /// that can only be closed by hand for no reason. This is what happens when `exit` is typed.
    pub fn pump(&mut self) {
        for session in self.sessions.iter_mut() {
            session.pump();
        }
        let before = self.sessions.len();
        // A detached session never stops, so this only affects tabs with a shell behind them.
        self.sessions.retain(|session| session.is_running());
        if self.sessions.len() != before && self.active >= self.sessions.len() {
            self.active = self.sessions.len().saturating_sub(1);
        }
    }

    /// Tell every terminal the new size, not only the one showing, so that a tab switched to later is
    /// already the right shape.
    pub fn resize(&mut self, size: Size) {
        for session in self.sessions.iter_mut() {
            session.resize(size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs() -> Tabs {
        Tabs::new(SessionSettings::default())
    }

    #[test]
    fn a_new_tile_has_no_terminals_in_it() {
        let tabs = tabs();
        assert!(tabs.is_empty());
        assert_eq!(tabs.count(), 0);
        assert!(tabs.active().is_none());
    }

    #[test]
    fn opening_a_tab_shows_it() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        tabs.open_detached(Size::new(8, 40));
        assert_eq!(tabs.count(), 2);
        assert_eq!(tabs.active_index(), 1, "the new one is the one showing");
        tabs.show(0);
        assert_eq!(tabs.active_index(), 0);
        tabs.show(9);
        assert_eq!(tabs.active_index(), 0, "a tab that is not there is not shown");
    }

    #[test]
    fn closing_a_tab_leaves_the_one_to_its_left_showing() {
        let mut tabs = tabs();
        for _ in 0..3 {
            tabs.open_detached(Size::new(8, 40));
        }
        assert_eq!(tabs.active_index(), 2);
        tabs.close(2);
        assert_eq!(tabs.count(), 2);
        assert_eq!(tabs.active_index(), 1);
        tabs.close(0);
        assert_eq!(tabs.count(), 1);
        assert_eq!(tabs.active_index(), 0);
        tabs.close(0);
        assert!(tabs.is_empty(), "closing the last tab leaves nothing, and the tile hides itself");
    }

    #[test]
    fn two_tabs_running_the_same_thing_are_told_apart_by_a_number() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        tabs.open_detached(Size::new(8, 40));
        assert_eq!(tabs.names(), vec!["detached", "detached 2"]);
    }

    #[test]
    fn a_tab_is_named_after_the_title_its_program_set() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        tabs.active_mut().expect("a tab").feed(b"\x1b]0;claude\x07");
        assert_eq!(tabs.names(), vec!["claude"]);
    }

    #[test]
    fn resizing_reaches_every_tab_and_not_only_the_one_showing() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        tabs.open_detached(Size::new(8, 40));
        tabs.resize(Size::new(20, 100));
        for session in tabs.sessions() {
            assert_eq!(session.size().rows, 20);
            assert_eq!(session.size().columns, 100);
        }
    }
}
