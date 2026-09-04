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
                    "Unluminous could not start {}: {problem}",
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

    /// The tab at `index`, whatever is showing. `None` when the number is past the end, which the
    /// caller answers with the not-found refusal rather than reaching for the tab that is showing.
    pub fn at(&self, index: usize) -> Option<&Session> {
        self.sessions.get(index)
    }

    /// The tab at `index`, mutable, for the verbs that change a tab by number.
    pub fn at_mut(&mut self, index: usize) -> Option<&mut Session> {
        self.sessions.get_mut(index)
    }

    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    /// The name on each tab, in order.
    ///
    /// Two tabs running the same program would otherwise be told apart only by where they are, so a number
    /// is put in front when a name is used more than once. A name somebody typed is left exactly as they
    /// typed it: the number is there to tell two tabs apart, and a person who has called two tabs the same
    /// thing has already said what they want them called.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        // How many tabs before this one are running the same thing. Counted against the name the
        // session gives rather than against the names already worked out: comparing against those
        // made the third `powershell.exe` a second `powershell.exe 2`, because `powershell.exe 2`
        // is not a name any later tab is ever compared equal to.
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for session in &self.sessions {
            if let Some(given) = session.given_name() {
                names.push(given.to_owned());
                continue;
            }
            let name = session.name().to_owned();
            let count = seen.entry(name.clone()).or_insert(0);
            *count += 1;
            names.push(match *count {
                1 => name,
                repeat => format!("{name} {repeat}"),
            });
        }
        names
    }

    /// Call tab `index` something else, which is what a right click on it offers.
    ///
    /// An empty name puts the tab back to being named after the program in it, so there is one way to undo
    /// a rename rather than a second command meaning "forget the name I gave".
    pub fn rename(&mut self, index: usize, name: &str) -> bool {
        match self.sessions.get_mut(index) {
            Some(session) => {
                session.rename(name);
                true
            }
            None => false,
        }
    }

    /// Move the tab at `index` so that it sits `position` tabs along, which is what dragging one does and
    /// what `unluminous-cli terminal move` asks for.
    ///
    /// `position` counts the tabs **as they are on the screen now**, including the one being moved, because
    /// that is what a person dragging it is looking at. Taking it out first shifts everything after it up
    /// by one, so a move to a place further along has one subtracted from it here rather than at every
    /// call. A position past the end means the end.
    ///
    /// The tab that was moved is the one showing afterwards, which is what dragging something somewhere
    /// means; `OpenFiles::drag_tab` leaves a file tab the same way.
    pub fn move_tab(&mut self, index: usize, position: usize) -> bool {
        if index >= self.sessions.len() {
            return false;
        }
        let mut position = position;
        if index < position {
            position -= 1;
        }
        let position = position.min(self.sessions.len() - 1);
        if position == index {
            // Picked up and put back where it was. Nothing moves, and showing it is still right.
            self.active = index;
            return true;
        }
        let session = self.sessions.remove(index);
        self.sessions.insert(position, session);
        self.active = position;
        true
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
    fn a_tab_is_reached_by_its_number_whatever_is_showing() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        tabs.open_detached(Size::new(8, 40));
        tabs.show(0);
        assert_eq!(tabs.active_index(), 0);
        // The second tab, which is not the one showing, is reached by its number and written to by
        // its number, and neither of those touches the tab that is showing.
        tabs.at_mut(1).expect("the second tab").feed(b"the second one\r\n");
        assert!(tabs.at(1).expect("the second tab").snapshot().contains("the second one"));
        assert!(!tabs.at(0).expect("the first tab").snapshot().contains("the second one"));
        assert!(tabs.at(9).is_none(), "a number past the end is nothing, not the tab showing");
        assert!(tabs.at_mut(9).is_none());
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
    fn the_third_tab_running_the_same_thing_is_the_third_and_not_a_second_second() {
        let mut tabs = tabs();
        for _ in 0..3 {
            tabs.open_detached(Size::new(8, 40));
        }
        assert_eq!(tabs.names(), vec!["detached", "detached 2", "detached 3"]);
    }

    #[test]
    fn a_name_a_person_typed_beats_the_one_the_program_set() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        tabs.rename(0, "build");
        assert_eq!(tabs.names(), vec!["build"]);
        // The program setting a title afterwards does not take the name away again, which is the
        // whole point: `claude` sets one on every prompt.
        tabs.active_mut().expect("a tab").feed(b"]0;claude");
        assert_eq!(tabs.names(), vec!["build"]);
    }

    #[test]
    fn an_empty_name_puts_a_tab_back_to_being_named_after_its_program() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        tabs.rename(0, "build");
        tabs.rename(0, "   ");
        assert_eq!(tabs.names(), vec!["detached"]);
    }

    #[test]
    fn a_name_a_person_typed_is_never_numbered() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        tabs.open_detached(Size::new(8, 40));
        tabs.rename(0, "build");
        tabs.rename(1, "build");
        assert_eq!(tabs.names(), vec!["build", "build"]);
    }

    #[test]
    fn renaming_a_tab_that_is_not_there_does_nothing() {
        let mut tabs = tabs();
        tabs.open_detached(Size::new(8, 40));
        assert!(!tabs.rename(9, "build"));
        assert_eq!(tabs.names(), vec!["detached"]);
    }

    #[test]
    fn a_tab_dragged_along_the_strip_lands_where_the_pointer_left_it() {
        let mut tabs = tabs();
        for name in ["one", "two", "three"] {
            tabs.open_detached(Size::new(8, 40));
            tabs.rename(tabs.count() - 1, name);
        }
        // The first tab dropped after the last: position counts the tabs as they are on the screen,
        // so past the end is 3 and one is subtracted for the tab that is being taken out.
        assert!(tabs.move_tab(0, 3));
        assert_eq!(tabs.names(), vec!["two", "three", "one"]);
        assert_eq!(tabs.active_index(), 2, "the tab that was moved is the one showing");
        // And back to the front.
        assert!(tabs.move_tab(2, 0));
        assert_eq!(tabs.names(), vec!["one", "two", "three"]);
        assert_eq!(tabs.active_index(), 0);
    }

    #[test]
    fn a_tab_picked_up_and_put_back_moves_nothing() {
        let mut tabs = tabs();
        for name in ["one", "two"] {
            tabs.open_detached(Size::new(8, 40));
            tabs.rename(tabs.count() - 1, name);
        }
        assert!(tabs.move_tab(1, 1));
        assert_eq!(tabs.names(), vec!["one", "two"]);
        assert_eq!(tabs.active_index(), 1);
        assert!(!tabs.move_tab(9, 0), "a tab that is not there cannot be moved");
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
