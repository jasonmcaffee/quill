//! The arithmetic the lanes need: where a dragged card lands, and how the cards are laid out.
//!
//! No user interface dependency, no database and no window, so every rule is a unit test. That is the
//! same division `app::dock` makes: the model and the arithmetic are here, and `components` draws.

use super::model::{Status, Task};

/// Where in a lane a card dropped at `y` belongs: how many cards it goes after.
///
/// A card goes after every card whose middle the pointer has passed, which is
/// `file_tabs::Strip::position_at`'s rule turned through ninety degrees. It is what makes a drop follow
/// the pointer rather than jump when it crosses an edge.
pub fn position_at(card_tops: &[f32], card_height: f32, y: f32) -> i64 {
    card_tops.iter().filter(|top| *top + card_height / 2.0 < y).count() as i64
}

/// Which lane the pointer at `x` is over, given each lane's left edge and how wide a lane is.
///
/// `None` past the last lane's right edge, so letting go in the empty space to the right of the board
/// puts the card back rather than dropping it into whichever lane happens to be last.
pub fn lane_at(lane_lefts: &[(Status, f32)], lane_width: f32, x: f32) -> Option<Status> {
    lane_lefts
        .iter()
        .find(|(_, left)| x >= *left && x < left + lane_width)
        .map(|(status, _)| *status)
}

/// The index among **all** of a lane's cards that the nth **visible** card sits at.
///
/// A search narrows a lane, and a drop between the second and third card that can be seen is not a drop at index
/// two of the lane: the tickets the query hid are still there and still ordered. Without this, dragging while
/// filtered reordered tickets nobody could see.
pub fn among_all(all: &[Task], query: &str, among_visible: i64) -> i64 {
    let mut seen = 0;
    for (at, task) in all.iter().enumerate() {
        if seen == among_visible {
            return at as i64;
        }
        if matches(task, query) {
            seen += 1;
        }
    }
    all.len() as i64
}

/// True when `query` matches this ticket, which is what the search box filters the lanes by.
///
/// The key, the title and the description, case insensitively. The same three columns the store's own
/// `search` reads, so filtering the lanes and searching the whole board agree.
pub fn matches(task: &Task, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    task.key.to_lowercase().contains(&query)
        || task.title.to_lowercase().contains(&query)
        || task.description.to_lowercase().contains(&query)
}

/// What a card's todo counts read as: `3/7`, or nothing when it has no todos.
pub fn todo_count(task: &Task) -> Option<String> {
    match task.todo_count {
        0 => None,
        total => Some(format!("{}/{total}", task.todo_done_count)),
    }
}

/// True when every todo is done, which is what draws the count in the added colour.
pub fn todos_complete(task: &Task) -> bool {
    task.todo_count > 0 && task.todo_done_count == task.todo_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_tasks::model::{Assignee, Priority, Source};

    fn card(key: &str, title: &str) -> Task {
        Task {
            id: 1,
            key: key.to_owned(),
            title: title.to_owned(),
            description: String::new(),
            priority: Priority::Medium,
            status: Status::New,
            assignee: Assignee::Claude,
            model: None,
            effort: None,
            epic_id: None,
            sprint_id: None,
            position: 0,
            project: None,
            session_id: None,
            heartbeat_at: None,
            lease_minutes: None,
            watchdog_strikes: 0,
            watchdog_nudges: 0,
            watchdog_nudged_at: None,
            source: Source::Local,
            jira_key: None,
            jira_url: None,
            jira_status: None,
            jira_issue_type: None,
            created_at: "2026-08-29T12:00:00Z".to_owned(),
            updated_at: "2026-08-29T12:00:00Z".to_owned(),
            todo_count: 0,
            todo_done_count: 0,
            comment_count: 0,
        }
    }

    #[test]
    fn a_card_goes_after_every_card_whose_middle_the_pointer_has_passed() {
        // Three cards 80 points tall at 0, 80 and 160.
        let tops = [0.0, 80.0, 160.0];
        assert_eq!(position_at(&tops, 80.0, 0.0), 0, "above the first card");
        assert_eq!(position_at(&tops, 80.0, 39.0), 0, "in the first card's top half");
        assert_eq!(position_at(&tops, 80.0, 41.0), 1, "past the first card's middle");
        assert_eq!(position_at(&tops, 80.0, 121.0), 2);
        assert_eq!(position_at(&tops, 80.0, 500.0), 3, "below all of them");
    }

    #[test]
    fn dropping_in_the_empty_space_beside_the_board_lands_in_no_lane() {
        let lanes = [
            (Status::New, 0.0),
            (Status::QaFailed, 300.0),
            (Status::InProgress, 600.0),
            (Status::AgentDone, 900.0),
        ];
        assert_eq!(lane_at(&lanes, 300.0, 10.0), Some(Status::New));
        assert_eq!(lane_at(&lanes, 300.0, 299.0), Some(Status::New));
        assert_eq!(lane_at(&lanes, 300.0, 300.0), Some(Status::QaFailed), "the edge belongs to the next lane");
        assert_eq!(lane_at(&lanes, 300.0, 1100.0), Some(Status::AgentDone));
        assert_eq!(
            lane_at(&lanes, 300.0, 1300.0),
            None,
            "past the last lane the card goes back rather than into whichever lane is last"
        );
        assert_eq!(lane_at(&lanes, 300.0, -10.0), None);
    }

    #[test]
    fn the_search_reads_the_key_the_title_and_the_description() {
        let mut task = card("task-27", "Plugin architecture for UI");
        task.description = "The board should be drawn in Rust.".to_owned();
        assert!(matches(&task, ""), "an empty query matches everything");
        assert!(matches(&task, "task-27"));
        assert!(matches(&task, "TASK-27"), "the search is case insensitive");
        assert!(matches(&task, "plugin"));
        assert!(matches(&task, "rust"), "the description is searched too");
        assert!(!matches(&task, "mermaid"));
    }

    #[test]
    fn a_drop_among_the_cards_that_can_be_seen_is_a_drop_among_all_of_them() {
        // A search hides cards and the hidden ones are still there and still ordered. Dropping between the first
        // and second card that can be seen, in a lane where the second card is hidden, is a drop at index two of
        // the lane rather than at index one.
        let mut lane = vec![card("task-1", "Alpha"), card("task-2", "Hidden"), card("task-3", "Alpha again")];
        lane[1].title = "Nothing like it".to_owned();
        assert_eq!(among_all(&lane, "alpha", 0), 0, "before the first visible card is the top of the lane");
        assert_eq!(
            among_all(&lane, "alpha", 1),
            1,
            "after the first visible card, which is still index one"
        );
        assert_eq!(
            among_all(&lane, "alpha", 2),
            3,
            "after the second visible card is past the hidden one as well"
        );
        // With nothing hidden the two indexes are the same, which is what makes this safe to apply always.
        for at in 0..=3 {
            assert_eq!(among_all(&lane, "", at), at);
        }
    }

    #[test]
    fn the_counts_a_card_shows() {
        let mut task = card("task-1", "A");
        assert_eq!(todo_count(&task), None, "a card with no todos shows no count");
        assert!(!todos_complete(&task), "no todos is not the same as every todo done");
        task.todo_count = 7;
        task.todo_done_count = 3;
        assert_eq!(todo_count(&task).as_deref(), Some("3/7"));
        assert!(!todos_complete(&task));
        task.todo_done_count = 7;
        assert!(todos_complete(&task));
    }
}
