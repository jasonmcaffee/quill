//! What a ticket, a todo, a comment, a sprint and an epic are.
//!
//! No user interface dependency and no database: these are the values the store reads and writes and
//! the board draws, so they are testable with no window and with no file. The four closed lists —
//! [`Status`], [`Priority`], [`Assignee`] and [`SprintStatus`] — are the four check constraints the
//! application being replaced put in its schema, kept as Rust enums so the compiler names every place
//! that has to answer for a new value.
//!
//! **An unknown value from the file is refused rather than defaulted.** A row nobody can explain is
//! worse than an error: a ticket whose status did not parse would draw in whichever lane the default
//! named, and somebody would move it back and watch it move again.

use std::fmt;

/// A lane on the board. Four, in the order they are drawn.
///
/// `AgentDone` is the last lane and means the work is finished and waiting on human review. There is
/// deliberately no fifth `done`: a person moving a card out of Agent Done is the review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Status {
    New,
    QaFailed,
    InProgress,
    AgentDone,
}

impl Status {
    /// Left to right, which is the order the lanes are drawn in and the order the design image shows.
    pub const ALL: [Status; 4] = [Status::New, Status::QaFailed, Status::InProgress, Status::AgentDone];

    /// The word the database, the command line and the manifest use.
    pub fn name(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::QaFailed => "qa_failed",
            Status::InProgress => "in_progress",
            Status::AgentDone => "agent_done",
        }
    }

    /// What a person reads at the top of the lane.
    pub fn label(self) -> &'static str {
        match self {
            Status::New => "NEW",
            Status::QaFailed => "QA FAILED",
            Status::InProgress => "IN PROGRESS",
            Status::AgentDone => "AGENT DONE",
        }
    }

    pub fn parse(name: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|status| status.name() == name)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.name())
    }
}

/// How urgent a ticket is. Drawn as one of three chevrons on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
}

impl Priority {
    pub const ALL: [Priority; 3] = [Priority::Low, Priority::Medium, Priority::High];

    pub fn name(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Medium => "medium",
            Priority::High => "high",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Priority::Low => "Low priority",
            Priority::Medium => "Medium priority",
            Priority::High => "High priority",
        }
    }

    pub fn parse(name: &str) -> Option<Priority> {
        Priority::ALL.into_iter().find(|priority| priority.name() == name)
    }
}

/// Who is working a ticket.
///
/// Two agents and a person. `Human` is what the application being replaced calls `jason`, spelled here
/// as what it means rather than as whose machine this is, because a settings file is copied between
/// machines and a name in an enum is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Assignee {
    #[default]
    Claude,
    Codex,
    Human,
}

impl Assignee {
    pub const ALL: [Assignee; 3] = [Assignee::Claude, Assignee::Codex, Assignee::Human];

    pub fn name(self) -> &'static str {
        match self {
            Assignee::Claude => "claude",
            Assignee::Codex => "codex",
            Assignee::Human => "human",
        }
    }

    pub fn parse(name: &str) -> Option<Assignee> {
        Assignee::ALL.into_iter().find(|assignee| assignee.name() == name)
    }

    /// True for the two that can be launched in a terminal.
    ///
    /// What decides whether a card draws a start button and whether the watchdog has anybody to talk
    /// to. A ticket a person is working has no agent and no lease.
    pub fn is_an_agent(self) -> bool {
        !matches!(self, Assignee::Human)
    }
}

/// Who wrote a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Author {
    Human,
    #[default]
    Claude,
    Codex,
    /// The board itself: the watchdog's first strike, and a reclaim.
    System,
}

impl Author {
    pub const ALL: [Author; 4] = [Author::Human, Author::Claude, Author::Codex, Author::System];

    pub fn name(self) -> &'static str {
        match self {
            Author::Human => "human",
            Author::Claude => "claude",
            Author::Codex => "codex",
            Author::System => "system",
        }
    }

    pub fn parse(name: &str) -> Option<Author> {
        Author::ALL.into_iter().find(|author| author.name() == name)
    }
}

/// Where a sprint is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SprintStatus {
    #[default]
    Planned,
    Active,
    Completed,
}

impl SprintStatus {
    pub const ALL: [SprintStatus; 3] =
        [SprintStatus::Planned, SprintStatus::Active, SprintStatus::Completed];

    pub fn name(self) -> &'static str {
        match self {
            SprintStatus::Planned => "planned",
            SprintStatus::Active => "active",
            SprintStatus::Completed => "completed",
        }
    }

    pub fn parse(name: &str) -> Option<SprintStatus> {
        SprintStatus::ALL.into_iter().find(|status| status.name() == name)
    }
}

/// Where a ticket came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    Local,
    Jira,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Local => "local",
            Source::Jira => "jira",
        }
    }

    pub fn parse(name: &str) -> Option<Source> {
        match name {
            "local" => Some(Source::Local),
            "jira" => Some(Source::Jira),
            _ => None,
        }
    }
}

/// One ticket.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: i64,
    /// `task-27`. Unique, and what the command line and an agent's handoff line name.
    pub key: String,
    pub title: String,
    /// Markdown, which is what Quill already reads and draws.
    pub description: String,
    pub priority: Priority,
    pub status: Status,
    pub assignee: Assignee,
    /// The model the agent is launched with, when one was chosen.
    pub model: Option<String>,
    /// `low`, `medium`, `high`, `xhigh` or `max`. Passed to Claude as `--effort`.
    pub effort: Option<String>,
    pub epic_id: Option<i64>,
    pub sprint_id: Option<i64>,
    /// Where in its lane it sits. Contiguous within a lane.
    pub position: i64,
    /// The project folder the agent is launched in.
    pub project: Option<String>,
    /// The agent's own conversation id, which is what `Resume session` resumes.
    ///
    /// Written when a terminal launches for this ticket, and never cleared: a session that ended is
    /// still a conversation the agent can be brought back into, which is the whole of how resuming
    /// works without a process that outlives the editor.
    pub session_id: Option<String>,
    /// When the agent last proved it was working, by a todo, a comment or a heartbeat.
    pub heartbeat_at: Option<String>,
    /// How long this ticket's lease is, when the agent asked for a longer one than the default.
    pub lease_minutes: Option<i64>,
    /// How many times the watchdog has struck a ticket whose terminal is gone.
    pub watchdog_strikes: i64,
    /// How many times the watchdog has typed a continue instruction into a terminal that is alive.
    pub watchdog_nudges: i64,
    pub watchdog_nudged_at: Option<String>,
    pub source: Source,
    pub jira_key: Option<String>,
    pub jira_url: Option<String>,
    pub jira_status: Option<String>,
    pub jira_issue_type: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// How many todos it has, and how many are done. Counted by the board's own read rather than
    /// stored, so they cannot go stale.
    pub todo_count: i64,
    pub todo_done_count: i64,
    pub comment_count: i64,
}

impl Task {
    /// What the card shows when the title is empty, which it is for the moment between `Add Task`
    /// creating the row and somebody typing.
    pub fn display_title(&self) -> &str {
        match self.title.trim().is_empty() {
            true => "Untitled",
            false => self.title.trim(),
        }
    }

    /// True when this ticket has an agent that could be talked to or reclaimed.
    pub fn has_a_worker(&self) -> bool {
        self.status == Status::InProgress && self.session_id.is_some()
    }
}

/// One todo under a ticket.
#[derive(Debug, Clone, PartialEq)]
pub struct Todo {
    pub id: i64,
    pub task_id: i64,
    pub text: String,
    pub done: bool,
    pub position: i64,
    pub created_at: String,
}

/// One comment on a ticket.
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub id: i64,
    pub task_id: i64,
    pub author: Author,
    /// Markdown, drawn by the same reader the Markdown preview uses.
    pub body: String,
    pub created_at: String,
}

/// One sprint.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprint {
    pub id: i64,
    pub name: String,
    pub status: SprintStatus,
    pub position: i64,
    pub created_at: String,
}

/// One epic: a name and a colour, which is the coloured edge and the chip on a card.
///
/// The colour is the one thing on the board that comes from the data rather than from `theme::color`,
/// and it is confined to the card's left edge and its chip. That is the same allowance the file icons
/// already have, and it is why a plugin still cannot repaint anything.
#[derive(Debug, Clone, PartialEq)]
pub struct Epic {
    pub id: i64,
    pub name: String,
    /// `#RRGGBB`, read by `plugins::colour`, which is the reader every theme colour already goes
    /// through. A value that does not parse draws with no colour rather than refusing the row.
    pub color: String,
    pub position: i64,
}

/// One lane, with its cards in the order they are drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub status: Status,
    pub tasks: Vec<Task>,
}

impl Lane {
    pub fn count(&self) -> usize {
        self.tasks.len()
    }
}

/// The whole board: the sprint that is active, its four lanes, and the epics the cards refer to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Board {
    pub sprint: Option<Sprint>,
    pub lanes: Vec<Lane>,
    pub epics: Vec<Epic>,
}

impl Board {
    pub fn lane(&self, status: Status) -> Option<&Lane> {
        self.lanes.iter().find(|lane| lane.status == status)
    }

    pub fn epic(&self, id: i64) -> Option<&Epic> {
        self.epics.iter().find(|epic| epic.id == id)
    }

    pub fn total(&self) -> usize {
        self.lanes.iter().map(Lane::count).sum()
    }

    /// Every card in every lane, which is what a search and the watchdog read.
    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.lanes.iter().flat_map(|lane| lane.tasks.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_closed_list_round_trips_through_its_word() {
        for status in Status::ALL {
            assert_eq!(Status::parse(status.name()), Some(status));
            assert!(!status.label().is_empty());
        }
        for priority in Priority::ALL {
            assert_eq!(Priority::parse(priority.name()), Some(priority));
        }
        for assignee in Assignee::ALL {
            assert_eq!(Assignee::parse(assignee.name()), Some(assignee));
        }
        for author in Author::ALL {
            assert_eq!(Author::parse(author.name()), Some(author));
        }
        for status in SprintStatus::ALL {
            assert_eq!(SprintStatus::parse(status.name()), Some(status));
        }
    }

    #[test]
    fn a_word_the_board_does_not_know_is_refused_rather_than_defaulted() {
        // A row nobody can explain is worse than an error. A status that quietly became `new` would
        // draw in the wrong lane, and somebody would move it back and watch it move again.
        assert_eq!(Status::parse("done"), None, "there is no fifth lane");
        assert_eq!(Status::parse(""), None);
        assert_eq!(Priority::parse("urgent"), None);
        assert_eq!(Assignee::parse("gemini"), None);
        assert_eq!(Source::parse("github"), None);
    }

    #[test]
    fn the_lanes_are_in_the_order_they_are_drawn() {
        let names: Vec<&str> = Status::ALL.iter().map(|status| status.name()).collect();
        assert_eq!(names, ["new", "qa_failed", "in_progress", "agent_done"]);
    }

    #[test]
    fn only_the_two_agents_can_be_launched() {
        assert!(Assignee::Claude.is_an_agent());
        assert!(Assignee::Codex.is_an_agent());
        assert!(!Assignee::Human.is_an_agent(), "a person is not launched in a terminal");
    }
}
