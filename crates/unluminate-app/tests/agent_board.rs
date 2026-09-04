//! Tests that start a real agent on a real board and grade what happened against Unluminate's own state.
//!
//! `task-28`: "We need agent task integration tests that actual interact with the model to ensure things work as
//! expected." Every test the Agent-Tasks plugin had before this stops at the edge of the process.
//! `agent::launch` is tested as a command line, `Store` is tested in memory, and the widget tree is tested with
//! no agent behind it — so the one thing nobody had watched was the thing the plugin is for: an agent starting,
//! reading its handoff, writing to the board, and the board showing it while it happens.
//!
//! This is the bargain `tools/agent-study` makes, applied to the board. What is asserted is **a state Unluminate
//! holds or a file on disk**, never the agent's wording: grading a model's prose is a test that fails when the
//! model improves.
//!
//! ## How to run them, and why they are not run by default
//!
//! ```text
//! cargo test -p unluminate-app --test agent_board -- --ignored --test-threads=1
//! ```
//!
//! Every test here is `#[ignore]`d for two reasons. They take minutes, and `cargo test` has to stay quick enough
//! that somebody runs it. And they cost tokens: a test that spends money should be one somebody asked for.
//!
//! One at a time, because each starts an agent that reads and writes files in its own temporary folder and the
//! machine has one Iliad key with one rate limit behind it.
//!
//! A machine with no `claude` on its path **skips** with a message naming what was missing rather than failing,
//! which is what `services::debuggers`'s own tests do about `lldb-dap`.
//!
//! ## Run them from a plain terminal
//!
//! Not from inside another Claude Code session. An agent started from one inherits its `CLAUDE_CODE_CHILD_SESSION`
//! marker, does not get a conversation of its own, and shows the parent's transcript instead of reading the ticket
//! it was handed — so the claim assertions pass and everything after them fails for a reason that has nothing to do
//! with the board. `Bench::say_what_the_agent_printed` says so when it sees it.
//!
//! ## Where the model and the key come from
//!
//! `ANTHROPIC_BASE_URL` and `ANTHROPIC_API_KEY` out of the environment, which on this machine is the Iliad
//! gateway `~/.zshrc` exports. `UNLUMINATE_TEST_MODEL` names the model, and without it the ticket names none and the
//! agent uses its own default. Nothing here writes a key anywhere: `Configuration::environment` reads it at the
//! moment of launch, which is the same path the window uses.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use unluminate_app::services::agent_tasks::model::{Assignee, Author, Status};
use unluminate_app::services::agent_tasks::store::{NewTask, Store};
use unluminate_app::services::agent_tasks::{AgentTasks, Configuration};
use unluminate_app::services::plugin_ui::{Context, UiProvider};

/// How long a test waits for an agent to finish one small piece of work.
///
/// Generous on purpose. The failure this guards against is an agent that never starts or never writes, not one
/// that is slow, and a ceiling tight enough to catch slowness is a ceiling that fails on a busy machine.
///
/// **Ten minutes, measured rather than guessed.** Four was not enough: a cold `claude-opus-5` against the Iliad
/// gateway was still on its first reply at three minutes fifty eight seconds, so the test failed on a run where
/// everything was working. `UNLUMINATE_TEST_MODEL` names a quicker model when somebody wants a quicker run, and the
/// wait gives up early anyway when the agent is no longer running — see [`Bench::wait_for`] — so a long ceiling
/// costs nothing on the failures it is there to catch.
const CEILING: Duration = Duration::from_secs(600);

/// How often the board is read while waiting. Every terminal is read on the same beat, which is what the window
/// does through `let_the_plugins_catch_up`.
const BEAT: Duration = Duration::from_millis(500);

/// Whether the agent this ticket needs can be launched at all on this machine.
///
/// A skip rather than a failure: a checkout on a machine with no agent installed is not a broken checkout.
fn the_agent_is_here(program: &str) -> bool {
    let found = std::process::Command::new("which")
        .arg(program)
        .stdout(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !found {
        eprintln!("skipped: `{program}` is not on the path, so there is no agent to drive");
    }
    found
}

/// The folder the agent is allowed to work in, named by `UNLUMINATE_TEST_PROJECT`, or `None` to skip.
///
/// **Found by running these tests, and it is why they are opt in.** Claude Code asks two one-time questions the
/// first time it meets a folder or the bypass mode, and `--dangerously-skip-permissions` answers neither:
///
/// ```text
///  Quick safety check: Is this a project you created or one you trust?
///  ❯ No, exit
///    Yes, I trust this folder
/// ```
///
/// ```text
///  In Bypass Permissions mode, Claude Code will not ask for your approval before running potentially
///  dangerous commands.
///  ❯ No, exit
///    Yes, I accept
/// ```
///
/// An agent the board launches into a folder nobody has opened before therefore sits on the first of those and
/// exits, and the board is left holding a claim, a session and an owner for work nothing is doing.
/// `tasks/unluminate-issues-tdd.md` §16.4 records that as a finding about the plugin.
///
/// A test cannot answer those for somebody. Recording the answers in the real `~/.claude.json` would be a test
/// changing what an agent is allowed to do on the machine that ran it, and a throwaway `CLAUDE_CONFIG_DIR` was
/// tried and does not work: it throws away the bypass answer along with everything else, so the second question is
/// asked instead of the first.
///
/// So the operator names the folder. `UNLUMINATE_TEST_PROJECT` is a folder they have already opened Claude Code in, and
/// naming it is how they say an agent may work there. Without it these tests **skip** and say so, rather than
/// failing on a question nobody can see.
fn the_folder_the_agent_may_work_in() -> Option<PathBuf> {
    let named = std::env::var("UNLUMINATE_TEST_PROJECT").ok().filter(|value| !value.trim().is_empty());
    let Some(named) = named else {
        eprintln!(
            "skipped: set UNLUMINATE_TEST_PROJECT to a folder you have already opened Claude Code in, which is how \
             you say an agent may work there. Claude Code asks a trust question the first time it meets a folder \
             and `--dangerously-skip-permissions` does not answer it, so an agent started in a fresh folder never \
             begins. See tasks/unluminate-issues-tdd.md section 16.4."
        );
        return None;
    };
    let folder = PathBuf::from(named.trim());
    if !folder.is_dir() {
        eprintln!("skipped: UNLUMINATE_TEST_PROJECT names {}, which is not a folder", folder.display());
        return None;
    }
    Some(folder)
}

/// Whether there is a key for the agent to log in with./// Whether there is a key for the agent to log in with.
fn there_is_a_key() -> bool {
    let found = unluminate_app::services::agent_tasks::the_key().is_some();
    if !found {
        eprintln!(
            "skipped: no key in the keychain under `iliad` and no ANTHROPIC_API_KEY in the environment, so the \
             agent has nothing to log in with"
        );
    }
    found
}

/// A folder of its own for one test: a board file, and a project folder the agent works in.
struct Bench {
    folder: PathBuf,
    board: AgentTasks,
    /// The tickets this bench made, so `Drop` can close their terminals.
    keys: Vec<String>,
}

impl Bench {
    /// Build a board with one sprint, in a folder named after the test.
    fn new(name: &str) -> Self {
        let folder = std::env::temp_dir().join(format!("unluminate-agent-board-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        // The **board** is in a folder of its own, so nothing here touches the board on this machine. The agent
        // works in the folder the operator named, because that is the only folder Claude Code will start in
        // without asking a question nobody can answer. See `the_folder_the_agent_may_work_in`.
        std::fs::create_dir_all(&folder).expect("a folder for the board");
        let project = the_folder_the_agent_may_work_in().expect("a folder for the agent, checked by the caller");

        // The board's own settings live in this folder too, so nothing here touches the board on this machine.
        let configuration = Configuration {
            database: Some(folder.join("board.sqlite3")),
            project: Some(project.clone()),
            model: std::env::var("UNLUMINATE_TEST_MODEL").ok().filter(|value| !value.trim().is_empty()),
            lease_minutes: 45,
            ..Configuration::default()
        };
        {
            // Written before the provider opens, because opening is what reads it.
            configuration.write(&folder).expect("the settings");
        }
        let mut board = AgentTasks::new();
        board
            .open(&Context {
                project: Some(project),
                recent_projects: Vec::new(),
                folder: Some(folder.clone()),
                showing: None,
                wake: None,
            })
            .expect("a board opens");
        Self { folder, board, keys: Vec::new() }
    }

    /// The folder the agent is working in, which the operator named.
    fn project(&self) -> PathBuf {
        Configuration::read(&self.folder).project.expect("the project the bench was built with")
    }

    /// The store, opened again for reading, so what is asserted is what is **on disk** rather than what the
    /// provider happens to be holding.
    fn on_disk(&self) -> Store {
        Store::open(self.folder.join("board.sqlite3")).expect("the board file")
    }

    /// Make a ticket for an agent, with `description` as its handoff, and answer its key.
    fn ticket(&mut self, title: &str, description: &str) -> String {
        let store = self.on_disk();
        let sprint = store
            .create_sprint("Current Sprint", unluminate_app::services::agent_tasks::model::SprintStatus::Active, &now())
            .expect("a sprint");
        let task = store
            .create_task(
                NewTask {
                    title: title.to_owned(),
                    description: description.to_owned(),
                    assignee: Assignee::Claude,
                    sprint_id: Some(sprint.id),
                    model: std::env::var("UNLUMINATE_TEST_MODEL").ok().filter(|value| !value.trim().is_empty()),
                    ..NewTask::default()
                },
                &now(),
            )
            .expect("a ticket");
        self.board.refresh().expect("the board reads it");
        self.keys.push(task.key.clone());
        task.key
    }

    /// Read every terminal and ask again, until `answered` says yes or [`CEILING`] runs out.
    ///
    /// `pump` is what the window calls once a frame, so waiting this way exercises the same path the window uses
    /// rather than reading the process directly.
    ///
    /// **It gives up early when the agent has gone.** Waiting the full ceiling on a process that exited four
    /// seconds in wastes four minutes and answers nothing, and the reason it exited is on its own screen — which
    /// is why the screen is printed when this fails. A test that says only "the agent did not write the file" sends
    /// whoever ran it to read a pty they cannot reach.
    fn wait_for(&mut self, what: &str, mut answered: impl FnMut(&mut AgentTasks) -> bool) -> bool {
        let started = Instant::now();
        let mut gone_since: Option<Instant> = None;
        while started.elapsed() < CEILING {
            self.board.pump();
            if answered(&mut self.board) {
                eprintln!("{what}: after {:?}", started.elapsed());
                return true;
            }
            // A terminal that has stopped running is given a moment — the answer may already be true and simply
            // not read yet — and then the wait ends.
            let running = self.board.terminals().iter().any(|terminal| terminal.session.is_running());
            match (running, gone_since) {
                (true, _) => gone_since = None,
                (false, None) => gone_since = Some(Instant::now()),
                (false, Some(at)) if at.elapsed() > Duration::from_secs(3) => {
                    eprintln!("{what}: the agent is no longer running, so there is nothing more to wait for");
                    break;
                }
                (false, Some(_)) => {}
            }
            std::thread::sleep(BEAT);
        }
        eprintln!("{what}: NOT within {CEILING:?}");
        self.say_what_the_agent_printed();
        false
    }

    /// The last screenful of every terminal, which is where the reason lives when one of these fails.
    fn say_what_the_agent_printed(&mut self) {
        self.board.pump();
        for terminal in self.board.terminals() {
            let text = terminal.session.written_text(Some(60));
            eprintln!(
                "--- what the agent printed (ticket {}, running: {}) ---\n{}\n--- end ---",
                terminal.task_id,
                terminal.session.is_running(),
                match text.trim().is_empty() {
                    true => "(nothing at all, so it never started or died before printing)".to_owned(),
                    false => text,
                }
            );
        }
        if self.board.terminals().is_empty() {
            eprintln!("--- there is no terminal at all, so nothing was ever spawned ---");
        }
        // The one failure that is about **where these were run from** rather than about the board. An agent
        // started from inside another Claude Code session inherits its child session marker, does not get a
        // conversation of its own, and shows the parent's transcript instead of reading the handoff. Run these from
        // a plain terminal.
        if self
            .board
            .terminals()
            .iter()
            .any(|terminal| terminal.session.written_text(Some(60)).contains("Transcript saving is off"))
        {
            eprintln!(
                "--- the agent inherited a CLAUDE_CODE_CHILD_SESSION marker, so it did not get a conversation of \
                 its own. These tests have to be run from a plain terminal rather than from inside a Claude Code \
                 session. ---"
            );
        }
    }
}

impl Drop for Bench {
    /// Every helper process this started is closed, which is the task protocol's own rule and which also stops a
    /// failed test leaving an agent running against the gateway.
    fn drop(&mut self) {
        // Every helper process this started is closed, through the command the window itself uses.
        for key in self.keys.clone() {
            let _ = self.board.command("stop", &[key]);
        }
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn now() -> String {
    unluminate_app::services::agent_tasks::clock::now()
}

/// What the agent is asked to do: write a file with a line in it. Small, quick, and provable without reading a
/// word the agent said.
fn write_a_file(name: &str, line: &str) -> String {
    format!(
        "Write a file called `{name}` in this folder whose only contents are the line `{line}`. Do not create \
         any other file. When the file exists, you are done: post a comment on this ticket saying so and set \
         the ticket to agent_done."
    )
}

/// A ticket is claimed, worked, and the work is on disk.
///
/// **This is the test that would have caught the fault `task-28` reported.** Claiming is the first thing a start
/// does, and on a board file made before the `owner` column existed the claim failed with `no such column:
/// owner`, so no agent could be launched at all.
#[test]
#[ignore = "starts a real agent: run with --ignored"]
fn a_ticket_is_claimed_and_the_agent_does_the_work() {
    if !the_agent_is_here("claude") || !there_is_a_key() || the_folder_the_agent_may_work_in().is_none() {
        return;
    }
    let mut bench = Bench::new("claimed");
    let wanted = "unluminate-was-here";
    // Named for this run, so a folder the operator uses for real does not collect a file called `proof.txt`.
    let file = format!("unluminate-agent-board-proof-{}.txt", std::process::id());
    let key = bench.ticket("Write a file", &write_a_file(&file, wanted));

    let answer = bench.board.start(&key).expect("the agent starts");
    assert!(answer.message.contains(&key), "the answer names the ticket: {}", answer.message);

    // Claimed, on disk, at once — before anything the agent does.
    let store = bench.on_disk();
    let task = store.task_by_key(&key).expect("a read").expect("the ticket");
    assert_eq!(task.status, Status::InProgress, "starting a ticket claims it");
    assert!(task.session_id.is_some(), "and records the conversation it is having");
    assert_eq!(
        store.owner_of(task.id).expect("the owner"),
        Some(format!("pid:{}", std::process::id())),
        "and records which window owns it, which is the column that used to be missing"
    );

    // Then the work itself, waited for on the file rather than on anything the agent said.
    let proof = bench.project().join(&file);
    let done = bench.wait_for("the file the ticket asked for", |_| {
        std::fs::read_to_string(&proof).is_ok_and(|text| text.contains(wanted))
    });
    // Taken away whatever happened, because it is in a folder the operator uses.
    let there = std::fs::read_to_string(&proof).is_ok();
    let _ = std::fs::remove_file(&proof);
    assert!(done && there, "the agent did not write {}", proof.display());
}

/// The board changes while the agent works, read the way the window reads it.
#[test]
#[ignore = "starts a real agent: run with --ignored"]
fn the_board_shows_the_agents_work_while_it_is_happening() {
    if !the_agent_is_here("claude") || !there_is_a_key() || the_folder_the_agent_may_work_in().is_none() {
        return;
    }
    let mut bench = Bench::new("live");
    let said = format!("unluminate-agent-board-said-{}.txt", std::process::id());
    let key = bench.ticket("Say something", &write_a_file(&said, "anything at all"));
    bench.board.start(&key).expect("the agent starts");
    let id = bench.on_disk().task_by_key(&key).expect("a read").expect("the ticket").id;

    // The terminal prints, and `pump` is what notices — which is the call the window makes once a frame.
    let printed = bench.wait_for("the terminal printing", move |board| {
        board
            .terminal_for(id)
            .is_some_and(|terminal| terminal.session.written_text(Some(60)).trim().len() > 40)
    });
    assert!(printed, "the agent's terminal printed nothing the window could see");

    // And the row moves under it. `heartbeat_at` is what the agent writes through the board's own protocol, so
    // this is the whole loop: agent, board file, window.
    let moved = bench.wait_for("the ticket being written to", |board| {
        board
            .board()
            .lanes
            .iter()
            .flat_map(|lane| lane.tasks.iter())
            .any(|card| card.key == key && (card.todo_count > 0 || card.comment_count > 0))
    });
    let _ = std::fs::remove_file(bench.project().join(&said));
    assert!(moved, "the agent wrote no todo and no comment, so nothing on the board moved");
}

/// A comment reaches the agent, and the agent answers on the board.
///
/// The whole loop the task protocol depends on: board, terminal, agent, board.
#[test]
#[ignore = "starts a real agent: run with --ignored"]
fn a_comment_reaches_the_agent_and_its_answer_comes_back_on_the_board() {
    if !the_agent_is_here("claude") || !there_is_a_key() || the_folder_the_agent_may_work_in().is_none() {
        return;
    }
    let mut bench = Bench::new("comment");
    let key = bench.ticket(
        "Answer a question",
        "Wait for a comment on this ticket. Do nothing else until one arrives.",
    );
    bench.board.start(&key).expect("the agent starts");
    let id = bench.on_disk().task_by_key(&key).expect("a read").expect("the ticket").id;

    // A word nothing else would produce, so an answer holding it is an answer to this.
    let word = format!("pomegranate-{}", std::process::id());
    let ready = bench.wait_for("the agent's prompt", move |board| {
        board.terminal_for(id).is_some_and(|terminal| terminal.session.written_text(Some(40)).len() > 200)
    });
    assert!(ready, "the agent never printed a prompt to type into");
    // The command the modal's own `Send to terminal` runs: the comment is written on the board and typed into the
    // agent, which is the loop being tested.
    bench
        .board
        .command(
            "comment-send",
            &[key.clone(), format!("Post a comment on this ticket whose body is exactly `{word}`.")],
        )
        .expect("the comment is posted and typed in");

    let answered = bench.wait_for("the agent's own comment", |board| {
        board
            .detail()
            .comments
            .iter()
            .any(|comment| comment.author != Author::Human && comment.body.contains(&word))
    });
    // Read off the file as well, because what the provider is holding and what is written down have to agree.
    if answered {
        let store = bench.on_disk();
        let comments = store.comments(id).expect("the comments");
        assert!(
            comments.iter().any(|comment| comment.author != Author::Human && comment.body.contains(&word)),
            "the answer is on the board file, not only in the window"
        );
    }
    assert!(answered, "the agent never answered the comment on the board");
}

/// A retired session is resumed and remembers, on a ticket that is already in Agent Done.
///
/// `task-28`: "Verify that the terminal can be resumed regardless of agent done, etc." Resuming is the
/// conversation rather than the process, and this is what proves it: the agent is told a word, its terminal is
/// closed, the ticket is resumed, and the word is asked for back.
#[test]
#[ignore = "starts a real agent: run with --ignored"]
fn a_resumed_agent_remembers_the_conversation_even_in_agent_done() {
    if !the_agent_is_here("claude") || !there_is_a_key() || the_folder_the_agent_may_work_in().is_none() {
        return;
    }
    let mut bench = Bench::new("resume");
    let word = format!("marmalade-{}", std::process::id());
    let key = bench.ticket(
        "Remember a word",
        &format!("Remember the word `{word}`. Say `ready` and then wait."),
    );
    bench.board.start(&key).expect("the agent starts");
    let id = bench.on_disk().task_by_key(&key).expect("a read").expect("the ticket").id;
    let looking_for = word.clone();
    let told = bench.wait_for("the agent reading its handoff", move |board| {
        board
            .terminal_for(id)
            .is_some_and(|terminal| terminal.session.written_text(Some(80)).contains(&looking_for))
    });
    assert!(told, "the agent never read the word out of its handoff");

    // Put the ticket where the ticket says it must still be resumable, and close the terminal.
    let store = bench.on_disk();
    store.move_task(id, Status::AgentDone, i64::MAX, &now()).expect("into Agent Done");
    bench.board.refresh().expect("the board reads it");
    bench.board.command("stop", &[key.clone()]).expect("the terminal closes");
    assert!(
        bench.board.terminal_for(id).is_none_or(|terminal| !terminal.session.is_running()),
        "the terminal is not running any more"
    );

    // Resumed. The lane does not change, which is the rule `AgentTasks::resume` is written to keep.
    let answer = bench.board.resume(&key).expect("the session resumes");
    assert!(answer.message.contains("resumed"), "{}", answer.message);
    assert_eq!(
        bench.on_disk().task_by_key(&key).expect("a read").expect("the ticket").status,
        Status::AgentDone,
        "resuming a conversation is not a claim on the work, so the lane is untouched"
    );

    let asked = "What word were you asked to remember? Answer with the word and nothing else.";
    let ready = bench.wait_for("the resumed agent's prompt", move |board| {
        board.terminal_for(id).is_some_and(|terminal| terminal.session.written_text(Some(40)).len() > 200)
    });
    assert!(ready, "the resumed agent never printed a prompt");
    bench.board.send(&key, asked).expect("the question is typed in");
    let remembered = bench.wait_for("the word, out of the resumed conversation", move |board| {
        board
            .terminal_for(id)
            .is_some_and(|terminal| terminal.session.written_text(Some(40)).matches(&word).count() >= 1)
    });
    assert!(remembered, "the resumed agent did not remember the word, so the conversation was not resumed");
}

/// A claim whose lease has expired and whose worker is gone is struck, and the board says so.
///
/// `watchdog::decide` is pure and has its own tests for every decision it can reach, including the nudges and the
/// reclaim. What those cannot test is that a decision is **written down**: the strike lands on the row, a system
/// comment explains it, and the card does not move while the board is only warning. That is this test, and it
/// needs no agent — a claimed ticket that this window has no terminal for is a ticket whose worker is gone, which
/// is the case the watchdog exists to notice.
///
/// It lives in this file rather than beside the unit tests because it walks the whole tick against a board file on
/// disk, which is slower than a unit test should be.
///
/// **One thing found while writing this, which is behaviour rather than a fault in the test.** A strike posts a
/// system comment, and a comment counts as board activity — `store` has a test saying so by name — so the
/// watchdog's own comment resets the idleness it has just measured. The strike counter still goes up, but the next
/// strike waits another whole lease, so reaching `strikes_before_reclaim` takes three leases rather than three
/// ticks. Whether that is what the escalation should do is a decision for the operator, not something to paper
/// over here, so this test asserts one round and says why it does not assert three.
#[test]
#[ignore = "walks the whole watchdog tick against a board on disk: run with --ignored"]
fn a_claim_whose_worker_is_gone_is_struck_and_the_board_says_why() {
    // No agent is started, but `Bench` needs a folder for the ticket to name, and the one the operator named is the
    // one to use so this test agrees with the others about what a bench is.
    if the_folder_the_agent_may_work_in().is_none() {
        return;
    }
    let mut bench = Bench::new("watchdog");
    let key = bench.ticket("Go quiet", "Nothing.");
    let store = bench.on_disk();
    let id = store.task_by_key(&key).expect("a read").expect("the ticket").id;
    store
        .claim(id, "a-session", Assignee::Claude, &format!("pid:{}", std::process::id()), &now())
        .expect("a claim");

    // A heartbeat long enough ago that the lease has expired. The clock is a value rather than a wait, which is
    // why this takes a moment and not the three quarters of an hour the lease is.
    let an_hour_ago = unluminate_app::services::agent_tasks::clock::from_unix(
        unluminate_app::services::agent_tasks::clock::to_unix(&now()).expect("now") - 60 * 60,
    );
    store.heartbeat(id, None, &an_hour_ago).expect("an old heartbeat");
    bench.board.refresh().expect("the board reads it");

    let acted = bench.board.watchdog_tick(&now()).expect("a tick");
    assert_eq!(acted.len(), 1, "one ticket, and it is the one whose lease expired: {acted:?}");
    assert_eq!(acted[0].0, key);
    assert!(
        format!("{:?}", acted[0].1).contains("Strike"),
        "the board warns before it takes work back: {:?}",
        acted[0].1
    );

    // What that means, read off the file rather than off the decision.
    let candidates = bench.on_disk().watchdog_candidates(&now(), 45).expect("the candidates");
    let after = candidates.iter().find(|card| card.key == key).expect("the ticket is still a candidate");
    assert_eq!(after.strikes, 1, "the strike is recorded on the row");
    let card = bench.on_disk().task_by_key(&key).expect("a read").expect("the ticket");
    assert_eq!(card.status, Status::InProgress, "a warning does not move the card");
    assert!(card.session_id.is_some(), "and does not take its conversation away");
    let comments = bench.on_disk().comments(id).expect("the comments");
    assert!(
        comments.iter().any(|comment| comment.author == Author::System),
        "the board says why, rather than a counter going up where nobody can see it"
    );

    // And a ticket that has just said something is left alone, which is what makes a long job survivable.
    bench.on_disk().heartbeat(id, Some(45), &now()).expect("a heartbeat now");
    bench.board.refresh().expect("the board reads it");
    let acted = bench.board.watchdog_tick(&now()).expect("a second tick");
    assert!(acted.is_empty(), "an agent that has just said something is not touched: {acted:?}");
    assert_eq!(
        bench
            .on_disk()
            .watchdog_candidates(&now(), 45)
            .expect("the candidates")
            .iter()
            .find(|card| card.key == key)
            .map(|card| card.strikes),
        Some(0),
        "and the heartbeat cleared the strike it had"
    );
}

/// The board file on **this machine** opens, migrates and can be claimed on.
///
/// `task-28` reported `no such column: owner` against the real file, and `store.rs` has a unit test that
/// reproduces that failure on a board built for it. This is the same question asked of the actual file, because a
/// migration that works on a board a test made and not on the one somebody has been using is a migration that has
/// not been proved. Nothing is written to it: it is copied first and the copy is what is opened.
#[test]
#[ignore = "reads the board on this machine: run with --ignored"]
fn the_board_on_this_machine_opens_and_can_be_claimed_on() {
    let real = Store::default_path();
    if !real.exists() {
        eprintln!("skipped: there is no board at {}", real.display());
        return;
    }
    let folder = std::env::temp_dir().join(format!("unluminate-real-board-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a folder");
    let copy = folder.join("board.sqlite3");
    // Through Unluminate's own copy, which is `VACUUM INTO` rather than a file copy: this board is in write ahead
    // logging mode and its newest rows are in the `-wal` file, so copying the one file copies a board missing
    // whatever was written most recently.
    Store::open(&real).expect("the real board opens").copy_the_file(&copy).expect("a copy of it");

    let store = Store::open(&copy).expect("the copy opens as a board");
    let task = store
        .create_task(NewTask { title: "Claim me".to_owned(), ..NewTask::default() }, &now())
        .expect("a ticket");
    assert!(
        store.claim(task.id, "a-session", Assignee::Claude, "pid:1", &now()).expect("the claim"),
        "a ticket on this machine's own board can be claimed, which is what `no such column: owner` stopped"
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// Nothing in this file may be run by `cargo test` by accident, because each one costs money or minutes.
#[test]
fn every_test_here_is_ignored_by_default() {
    // `file!()` is relative to the workspace root and a test's working folder is the package root, so the
    // package's own folder is what this is resolved against.
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("agent_board.rs"),
    )
    .expect("this file");
    let tests = source.matches("\n#[test]").count();
    let ignored = source.matches("\n#[ignore").count();
    assert_eq!(
        tests,
        ignored + 1,
        "every test here is #[ignore]d except this one, or `cargo test` starts agents"
    );
}
