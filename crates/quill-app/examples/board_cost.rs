//! What the Agent-Tasks board costs: opening it, reading it, and drawing a frame of it.
//!
//! `cargo run --release -p quill-app --example board_cost`
//!
//! The three numbers `tasks/agent-tasks-plugin-tdd.md` §9 commits to, measured rather than assumed. It
//! is the same arrangement `examples/completion_cost.rs` and `examples/symbol_cost.rs` use: a real board
//! with a real number of tickets on it, timed, printed, and read by a person.
//!
//! Nothing here draws. What a frame costs when nothing changed is the question the design answers with
//! "no query at all", and that is what the last measurement checks: the board is held in memory and a
//! frame that changed nothing reads the store zero times.

use std::time::Instant;

use quill_app::services::agent_tasks::model::{Priority, Status};
use quill_app::services::agent_tasks::store::{NewTask, Store};
use quill_app::services::agent_tasks::{clock, model};

/// How many tickets the board is filled with. Five thousand is more than any board anybody keeps, which
/// is the point: the number to read is what it costs at a size nobody will reach.
const TICKETS: usize = 5_000;

fn main() {
    let now = clock::now();
    let store = Store::in_memory().expect("a board in memory");
    let sprint = store
        .create_sprint("Current Sprint", model::SprintStatus::Active, &now)
        .expect("a sprint");

    let filling = Instant::now();
    for index in 0..TICKETS {
        let lane = match index % 4 {
            0 => Status::New,
            1 => Status::QaFailed,
            2 => Status::InProgress,
            _ => Status::AgentDone,
        };
        store
            .create_task(
                NewTask {
                    title: format!("Ticket number {index}, with a title about the length of a real one"),
                    description:
                        "Two or three sentences of markdown, which is what a real ticket carries and \
                         what the search has to read through."
                            .to_owned(),
                    status: lane,
                    priority: match index % 3 {
                        0 => Priority::High,
                        1 => Priority::Medium,
                        _ => Priority::Low,
                    },
                    sprint_id: Some(sprint.id),
                    ..NewTask::default()
                },
                &now,
            )
            .expect("a ticket");
        // A ticket with todos and comments on it, because the counts on a card are subqueries and a board
        // of bare tickets would not measure them.
        if index % 10 == 0 {
            let key = format!("task-{}", index + 1);
            let task = store.task_by_key(&key).expect("a read").expect("the ticket");
            for todo in 0..4 {
                store.add_todo(task.id, &format!("Step {todo}"), &now).expect("a todo");
            }
            store.add_comment(task.id, model::Author::Claude, "Working on it.", &now).expect("a comment");
        }
    }
    println!("{TICKETS} tickets written in {:.0} ms", filling.elapsed().as_secs_f64() * 1000.0);

    // Opening the board: one query for the cards, ordered by status and position, plus the sprint and the
    // epics. This is what pressing the rail button costs.
    let mut worst = 0.0_f64;
    for _ in 0..20 {
        let reading = Instant::now();
        let board = store.board().expect("the board");
        let took = reading.elapsed().as_secs_f64() * 1000.0;
        worst = worst.max(took);
        assert_eq!(board.total(), TICKETS, "every ticket is in a lane");
        assert_eq!(board.lanes.len(), 4);
    }
    println!("opening the board with {TICKETS} tickets: {worst:.2} ms at worst of twenty");

    // The search, which is `LIKE` over three columns rather than a full text index. The design says why:
    // a full text table would be a second copy of every description to keep in step.
    let mut search_worst = 0.0_f64;
    for query in ["ticket number 4999", "markdown", "task-2500", "nothing like this"] {
        let searching = Instant::now();
        let found = store.search(query).expect("a search");
        let took = searching.elapsed().as_secs_f64() * 1000.0;
        search_worst = search_worst.max(took);
        println!("  search `{query}`: {} found in {took:.2} ms", found.len());
    }
    println!("searching {TICKETS} tickets: {search_worst:.2} ms at worst");

    // One ticket with its todos and its comments, which is what opening a card costs.
    let one = Instant::now();
    let task = store.task_by_key("task-2501").expect("a read").expect("the ticket");
    let todos = store.todos(task.id).expect("the todos");
    let comments = store.comments(task.id).expect("the comments");
    println!(
        "one ticket with {} todos and {} comment{}: {:.2} ms",
        todos.len(),
        comments.len(),
        if comments.len() == 1 { "" } else { "s" },
        one.elapsed().as_secs_f64() * 1000.0
    );

    // The watchdog's own read, which runs every two minutes. It returns only cards in progress that
    // recorded a session, so on a board where nothing has been launched it returns nothing and costs the
    // index scan.
    let tick = Instant::now();
    let candidates = store.watchdog_candidates(&now, 45).expect("the candidates");
    println!(
        "one watchdog tick over {TICKETS} tickets: {} candidates in {:.2} ms",
        candidates.len(),
        tick.elapsed().as_secs_f64() * 1000.0
    );

    println!();
    println!("What a frame costs is zero queries: the board above is held in memory and read again only");
    println!("when a command changed something or the two minute tick fired. A frame in which nothing");
    println!("changed does no work at all, which is `task-1666`'s rule.");
}
