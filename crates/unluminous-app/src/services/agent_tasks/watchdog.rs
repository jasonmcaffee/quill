//! Keeping a ticket moving, and telling apart the two ways an agent stops.
//!
//! **No clock, no database and no terminal.** [`decide`] is handed the instant, the cards the store's
//! candidate query returned, and which sessions are alive and which are paused, and it returns what to
//! do. That is what makes every rule below a unit test with a fixed instant, and it is the arrangement
//! `unluminous_dap`'s session state machine and `dock::regions` already use.
//!
//! ## Two failures, and only one of them takes the ticket away
//!
//! A worker that is **gone** is a card whose lease has expired and whose terminal is no longer running.
//! There is nobody to talk to, so the card is struck once per tick and after
//! [`Thresholds::strikes_before_reclaim`] strikes it goes back to New with its todos and comments
//! intact.
//!
//! A worker that has **stopped** is a card whose terminal is still running while the agent inside it
//! waits at its prompt, has asked a question nobody will answer, or is wedged. That agent can be talked
//! to, so a continue instruction is typed into its terminal rather than the ticket being taken away
//! from it. Two things mark an agent as stopped: the lease has expired, or the terminal has printed
//! nothing for [`Thresholds::silent_minutes`]. An agent that is working prints constantly, so silence
//! that long is an agent that has stopped rather than one that is thinking.
//!
//! ## Only board activity stops the nudges
//!
//! A todo, a comment or a heartbeat clears the counters, because only those prove the agent picked the
//! work back up. Terminal output does not, and the reason is exact: the nudge itself is echoed by the
//! terminal it was typed into, so counting output would let one nudge clear its own count and the
//! escalation would never arrive.
//!
//! ## A paused agent is not a stopped one
//!
//! A frozen process cannot answer and its terminal looks silent, so a pause somebody asked for must
//! never be read as a stall. Both counters are cleared instead, because the silence means nothing.

use super::store::Candidate;

/// The five numbers the watchdog runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// How long a lease is when the ticket has not asked for a longer one.
    pub lease_minutes: i64,
    /// How many strikes a card whose terminal is gone takes before it goes back to New.
    pub strikes_before_reclaim: i64,
    /// How long a live terminal may print nothing before its agent counts as stopped.
    pub silent_minutes: i64,
    /// How long between two continue instructions typed into one terminal.
    pub nudge_interval_minutes: i64,
    /// From which nudge on the instruction also tells the agent how to end a ticket it cannot finish.
    pub nudges_before_block: i64,
}

impl Default for Thresholds {
    /// The numbers the application being replaced runs on, which is where they were measured.
    fn default() -> Self {
        Self {
            lease_minutes: 45,
            strikes_before_reclaim: 3,
            silent_minutes: 10,
            nudge_interval_minutes: 5,
            nudges_before_block: 3,
        }
    }
}

/// What one terminal is doing, as far as the watchdog needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// The process is running and printing.
    Alive {
        /// How long since it last printed anything.
        silent_minutes: i64,
    },
    /// The process is running and stopped by a signal, because somebody paused it.
    Paused,
    /// There is no process. Either it exited or this Unluminous never started it.
    Gone,
}

/// What the watchdog decided about one card.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Nothing is wrong: the agent is inside its lease and its terminal is printing.
    Leave,
    /// Somebody paused it, so both counters go back to nothing.
    ClearCounters,
    /// Its terminal is gone. Record a strike, and post a comment on the first one.
    Strike { first: bool },
    /// Its terminal is gone and it has taken its last strike. Back to New.
    Reclaim,
    /// Its terminal is alive and its agent has stopped. Type this into the terminal.
    Nudge { instruction: String, escalated: bool },
}

/// What one card's decision is at `now`.
///
/// The order of the questions is the design: a pause wins over everything, then a dead terminal, then a
/// live terminal whose agent has stopped. A card that is none of those is left alone.
pub fn decide(
    card: &Candidate,
    terminal: Terminal,
    thresholds: Thresholds,
    supervisor_paused: bool,
) -> Decision {
    if supervisor_paused || terminal == Terminal::Paused {
        return Decision::ClearCounters;
    }
    let expired = card.board_idle_minutes >= card.lease_minutes;
    match terminal {
        Terminal::Paused => Decision::ClearCounters,
        Terminal::Gone => match expired {
            // A card whose worker is gone but whose lease is still running has not failed yet: the
            // agent may be about to be started, and striking inside the lease would reclaim work
            // somebody just pressed start on.
            false => Decision::Leave,
            true => match card.strikes + 1 >= thresholds.strikes_before_reclaim {
                true => Decision::Reclaim,
                false => Decision::Strike { first: card.strikes == 0 },
            },
        },
        Terminal::Alive { silent_minutes } => {
            let stopped = expired || silent_minutes >= thresholds.silent_minutes;
            if !stopped {
                return Decision::Leave;
            }
            // A nudge is never repeated inside the interval, and a card that has never been nudged is
            // never held back: the store hands back a large number when there is no nudge to measure
            // from.
            if card.minutes_since_nudge < thresholds.nudge_interval_minutes {
                return Decision::Leave;
            }
            let escalated = card.nudges + 1 >= thresholds.nudges_before_block;
            Decision::Nudge { instruction: instruction(&card.key, escalated), escalated }
        }
    }
}

/// What is typed into a terminal whose agent has stopped.
///
/// The escalated one names the ending the task protocol requires for work that cannot be finished, and
/// it is the reason a stalled card is never reclaimed while its terminal is alive: the agent is the only
/// thing that knows why it stopped, so it is asked rather than replaced.
pub fn instruction(key: &str, escalated: bool) -> String {
    let mut said = format!(
        "Watchdog nudge for {key}. This is the board reporting that the ticket has gone quiet; it is \
         not a question. Re-read the ticket, take the newest human comments as the specification, and \
         finish the open todos. If the work is simply taking a long time, post a heartbeat with the \
         minutes still needed."
    );
    if escalated {
        said.push_str(
            " If you truly cannot go on, post a comment whose first word is Blocked, naming what \
             stopped you, what is complete, what remains and what the operator has to decide or \
             provide, then set the status to agent_done.",
        );
    }
    said
}

/// The comment the board posts on the first strike against a card whose worker is gone.
pub fn strike_comment(key: &str, strikes: i64, before_reclaim: i64) -> String {
    format!(
        "The worker on {key} is gone: its lease has expired and its terminal is no longer running. \
         Strike {strikes} of {before_reclaim}. After the last strike the ticket goes back to New with \
         its todos and comments intact, for another worker to pick up."
    )
}

/// The comment the board posts when a card goes back to New.
pub fn reclaim_comment(key: &str) -> String {
    format!(
        "{key} has been returned to New. Its worker was gone and its lease had expired. Its todos and \
         comments are as the previous worker left them, and the recorded session has been cleared \
         because the conversation it named belonged to that worker."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A card that has just been heard from, with no strikes and no nudges.
    fn card() -> Candidate {
        Candidate {
            id: 1,
            key: "task-27".to_owned(),
            session_id: "a-session".to_owned(),
            strikes: 0,
            nudges: 0,
            nudged_at: None,
            board_idle_minutes: 0,
            lease_minutes: 45,
            // What the store hands back when there has been no nudge to measure from, so a first nudge
            // is never held back by an interval it has not had yet.
            minutes_since_nudge: 1_000_000,
        }
    }

    #[test]
    fn an_agent_that_is_working_is_left_alone() {
        let alive = Terminal::Alive { silent_minutes: 0 };
        assert_eq!(decide(&card(), alive, Thresholds::default(), false), Decision::Leave);
    }

    #[test]
    fn a_terminal_that_is_gone_inside_its_lease_is_not_struck() {
        // Striking here would reclaim work somebody has just pressed start on, before its agent has
        // written anything to the board.
        assert_eq!(decide(&card(), Terminal::Gone, Thresholds::default(), false), Decision::Leave);
    }

    #[test]
    fn a_worker_that_is_gone_is_struck_and_then_reclaimed() {
        let thresholds = Thresholds::default();
        let mut gone = card();
        gone.board_idle_minutes = 60;
        assert_eq!(
            decide(&gone, Terminal::Gone, thresholds, false),
            Decision::Strike { first: true },
            "the first strike is the one that posts a comment"
        );
        gone.strikes = 1;
        assert_eq!(decide(&gone, Terminal::Gone, thresholds, false), Decision::Strike { first: false });
        gone.strikes = 2;
        assert_eq!(
            decide(&gone, Terminal::Gone, thresholds, false),
            Decision::Reclaim,
            "the third strike is the reclaim, because strikes_before_reclaim is 3"
        );
    }

    #[test]
    fn a_worker_that_has_stopped_is_talked_to_rather_than_replaced() {
        // The whole distinction: this terminal is alive, so the ticket is never taken away from it.
        let mut stalled = card();
        stalled.board_idle_minutes = 60;
        let decision = decide(&stalled, Terminal::Alive { silent_minutes: 0 }, Thresholds::default(), false);
        let Decision::Nudge { instruction, escalated } = decision else {
            panic!("a live terminal should be nudged, got {decision:?}");
        };
        assert!(!escalated, "the first nudge does not name the Blocked ending");
        assert!(instruction.starts_with("Watchdog nudge"), "{instruction}");
        assert!(instruction.contains("task-27"));
        assert!(instruction.contains("not a question"));
        assert!(!instruction.contains("Blocked"));
    }

    #[test]
    fn silence_on_a_live_terminal_marks_the_agent_as_stopped_inside_its_lease() {
        // An agent that is working prints constantly, so ten minutes of nothing is an agent that has
        // stopped rather than one that is thinking. This card's lease has not expired.
        let quiet = card();
        assert!(!quiet.lease_expired());
        let decision = decide(&quiet, Terminal::Alive { silent_minutes: 10 }, Thresholds::default(), false);
        assert!(matches!(decision, Decision::Nudge { .. }), "{decision:?}");
        let decision = decide(&quiet, Terminal::Alive { silent_minutes: 9 }, Thresholds::default(), false);
        assert_eq!(decision, Decision::Leave, "nine minutes is not ten");
    }

    #[test]
    fn a_nudge_is_not_repeated_inside_its_interval() {
        let mut stalled = card();
        stalled.board_idle_minutes = 60;
        stalled.nudges = 1;
        stalled.minutes_since_nudge = 4;
        assert_eq!(
            decide(&stalled, Terminal::Alive { silent_minutes: 30 }, Thresholds::default(), false),
            Decision::Leave,
            "four minutes after the last nudge is inside the five minute interval"
        );
        stalled.minutes_since_nudge = 5;
        assert!(matches!(
            decide(&stalled, Terminal::Alive { silent_minutes: 30 }, Thresholds::default(), false),
            Decision::Nudge { .. }
        ));
    }

    #[test]
    fn the_third_nudge_names_the_blocked_ending() {
        let mut stalled = card();
        stalled.board_idle_minutes = 60;
        stalled.nudges = 2;
        let decision = decide(&stalled, Terminal::Alive { silent_minutes: 30 }, Thresholds::default(), false);
        let Decision::Nudge { instruction, escalated } = decision else {
            panic!("expected a nudge, got {decision:?}");
        };
        assert!(escalated);
        assert!(instruction.contains("Blocked"), "{instruction}");
        assert!(instruction.contains("agent_done"), "{instruction}");
    }

    #[test]
    fn a_paused_agent_is_not_a_stopped_one() {
        // A frozen process cannot answer, and a pause makes its terminal look silent. Reading that as a
        // stall would nudge an agent somebody deliberately stopped, and then reclaim its ticket.
        let mut stalled = card();
        stalled.board_idle_minutes = 600;
        stalled.strikes = 2;
        assert_eq!(
            decide(&stalled, Terminal::Paused, Thresholds::default(), false),
            Decision::ClearCounters
        );
        assert_eq!(
            decide(&stalled, Terminal::Alive { silent_minutes: 600 }, Thresholds::default(), true),
            Decision::ClearCounters,
            "the supervisor being paused stops everything, whatever one terminal looks like"
        );
        assert_eq!(
            decide(&stalled, Terminal::Gone, Thresholds::default(), true),
            Decision::ClearCounters,
            "and it stops a reclaim as well, because a stopped process cannot answer"
        );
    }

    #[test]
    fn the_thresholds_are_the_ones_the_board_being_replaced_runs_on() {
        let thresholds = Thresholds::default();
        assert_eq!(thresholds.lease_minutes, 45);
        assert_eq!(thresholds.strikes_before_reclaim, 3);
        assert_eq!(thresholds.silent_minutes, 10);
        assert_eq!(thresholds.nudge_interval_minutes, 5);
        assert_eq!(thresholds.nudges_before_block, 3);
    }
}
