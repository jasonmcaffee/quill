//! The thread a query runs on.
//!
//! `quill_git::Worker` with a different payload, and the argument for it is the same one: the window
//! draws sixty times a second and a query takes as long as it takes, so nothing that talks to a
//! database may be called from inside a frame. A job goes down a channel, an answer comes back up
//! one, and `wake` brings a frame round when it does — the arrangement the terminal's reader, the
//! symbol index and `quill-dap` all already use.
//!
//! **One thread per connected data source**, holding one connection. Two panes reading the same source
//! therefore queue behind each other, which is what a single connection means and is what IntelliJ's
//! own single-session console does. A second connection would be a second transaction and a second
//! `search_path`, which is a surprise nobody wants from a tree and a grid that look like one thing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use crate::catalog::{Item, Kind, Table};
use crate::edit::Statement;
use crate::engine::{Database, Stopper};
use crate::rows::{Answer, Failure, Rows};
use crate::source::Source;
use crate::value::Value;

/// What a caller asked for.
#[derive(Debug, Clone)]
pub enum Job {
    /// A statement somebody typed.
    Query { sql: String, limit: usize },
    /// A statement Quill composed, with values bound.
    Run { sql: String, values: Vec<Value>, limit: usize },
    Databases,
    Schemas,
    Items { schema: String },
    Describe { schema: String, table: String },
    Ddl { schema: String, table: String, kind: Kind },
    UseSchema { name: String },
    /// Everything pending on a grid, written as one transaction or not at all.
    Write { statements: Vec<Statement> },
    /// Stop reading and close the connection.
    Close,
}

/// What came back.
#[derive(Debug)]
pub enum Reply {
    Rows(Rows),
    Names(Vec<String>),
    Items(Vec<Item>),
    Table(Table),
    Text(String),
    /// How many rows each statement of a write changed.
    Written(Vec<u64>),
    Done,
}

/// One answered job.
#[derive(Debug)]
pub struct Answered {
    /// The number this job was given, so a caller with several outstanding can tell them apart.
    pub ticket: u64,
    pub answer: Answer<Reply>,
}

/// A connection, on a thread of its own.
pub struct Worker {
    jobs: Sender<(u64, Job)>,
    answers: Receiver<Answered>,
    /// What can stop a statement that is running, from this thread.
    stopper: Arc<Mutex<Option<Stopper>>>,
    next: AtomicU64,
    thread: Option<JoinHandle<()>>,
    /// What the server called itself when the connection opened.
    pub version: String,
    pub encrypted: bool,
    pub engine: crate::source::Engine,
    /// How many jobs have been sent and not yet answered, which is what the pane draws a spinner from.
    outstanding: std::cell::Cell<usize>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Worker")
            .field("version", &self.version)
            .field("encrypted", &self.encrypted)
            .field("outstanding", &self.outstanding.get())
            .finish()
    }
}

impl Worker {
    /// Open the connection **on this thread**, then hand it to a new one.
    ///
    /// Connecting is what fails — a wrong password, a server that is not there, a certificate that
    /// will not verify — and it is the one thing the caller has to be told about straight away, so it
    /// happens before the thread exists rather than being reported through the channel as the first
    /// answer to a job nobody sent.
    ///
    /// `wake` is called whenever an answer is put on the channel. Without it a query that finished
    /// while nobody was moving the pointer would sit there unseen, which is `Context::wake`'s own
    /// reason.
    pub fn open(
        source: &Source,
        password: Option<&str>,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Answer<Worker> {
        let mut database = Database::connect(source, password)?;
        let version = database.version();
        let encrypted = database.is_encrypted();
        let engine = database.engine();
        let stopper = Arc::new(Mutex::new(Some(database.stopper())));
        let (jobs, take_a_job) = mpsc::channel::<(u64, Job)>();
        let (answer, answers) = mpsc::channel::<Answered>();
        let thread = std::thread::Builder::new()
            .name(format!("quill-db {}", source.name))
            .spawn(move || {
                while let Ok((ticket, job)) = take_a_job.recv() {
                    if matches!(job, Job::Close) {
                        database.close();
                        return;
                    }
                    let reply = run(&mut database, job);
                    if answer.send(Answered { ticket, answer: reply }).is_err() {
                        // Nobody is listening any more: the pane has gone.
                        database.close();
                        return;
                    }
                    if let Some(wake) = &wake {
                        wake();
                    }
                }
                database.close();
            })
            .map_err(|why| Failure::said(format!("a thread for this connection could not be started: {why}")))?;
        Ok(Worker {
            jobs,
            answers,
            stopper,
            next: AtomicU64::new(1),
            thread: Some(thread),
            version,
            encrypted,
            engine,
            outstanding: std::cell::Cell::new(0),
        })
    }

    /// Ask for something, and answer with the ticket it will come back under.
    pub fn ask(&self, job: Job) -> Answer<u64> {
        let ticket = self.next.fetch_add(1, Ordering::Relaxed);
        self.jobs
            .send((ticket, job))
            .map_err(|_| Failure::said("this connection's thread has stopped."))?;
        self.outstanding.set(self.outstanding.get() + 1);
        Ok(ticket)
    }

    /// Whatever has been answered since this was last called. Never blocks.
    pub fn take(&self) -> Vec<Answered> {
        let mut out = Vec::new();
        while let Ok(answered) = self.answers.try_recv() {
            self.outstanding.set(self.outstanding.get().saturating_sub(1));
            out.push(answered);
        }
        out
    }

    /// True while something is running, which is what draws the spinner and lights the Stop button.
    pub fn is_busy(&self) -> bool {
        self.outstanding.get() > 0
    }

    /// Ask the engine to stop what it is doing.
    ///
    /// Called from the drawing thread while the worker is inside the engine, which is exactly why the
    /// stopper is a separate value: PostgreSQL opens a second connection and SQLite calls
    /// `sqlite3_interrupt`, and neither needs the connection this thread cannot borrow.
    pub fn stop(&self) -> Answer<()> {
        match self.stopper.lock() {
            Ok(held) => match held.as_ref() {
                Some(stopper) => stopper.stop(),
                None => Err(Failure::said("this connection has already been closed.")),
            },
            Err(_) => Err(Failure::said("this connection's stopper cannot be reached.")),
        }
    }
}

impl Drop for Worker {
    /// Close the connection and wait for the thread.
    ///
    /// Waited for rather than detached, because the thread holds a socket and a `Drop` that left it
    /// running would leave a connection open on somebody's server after the pane that opened it had
    /// gone. It is only ever waiting on a `recv`, or inside a statement somebody can stop.
    fn drop(&mut self) {
        // **Stopped before it is waited for**, which is the whole of why this is three lines rather
        // than one. `Close` is read off the channel *between* jobs, so a worker in the middle of a
        // statement would not see it until that statement finished — and the join below would then
        // block the thread doing the dropping, which is the window, for as long as the query takes.
        // Asking the engine to stop first is what makes the wait short. An earlier version took the
        // stopper away and then waited, which is the same bug with the one thing that could have
        // helped thrown away first.
        if let Ok(held) = self.stopper.lock() {
            if let Some(stopper) = held.as_ref() {
                let _ = stopper.stop();
            }
        }
        let _ = self.jobs.send((0, Job::Close));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Ok(mut held) = self.stopper.lock() {
            held.take();
        }
    }
}

/// One job, on the worker's thread.
fn run(database: &mut Database, job: Job) -> Answer<Reply> {
    match job {
        Job::Query { sql, limit } => database.query(&sql, limit).map(Reply::Rows),
        Job::Run { sql, values, limit } => database.run(&sql, &values, limit).map(Reply::Rows),
        Job::Databases => database.databases().map(Reply::Names),
        Job::Schemas => database.schemas().map(Reply::Names),
        Job::Items { schema } => database.items(&schema).map(Reply::Items),
        Job::Describe { schema, table } => database.table(&schema, &table).map(Reply::Table),
        Job::Ddl { schema, table, kind } => database.ddl(&schema, &table, kind).map(Reply::Text),
        Job::UseSchema { name } => database.use_schema(&name).map(|_| Reply::Done),
        Job::Write { statements } => {
            let work: Vec<(String, Vec<Value>)> =
                statements.into_iter().map(|statement| (statement.sql, statement.values)).collect();
            database.write(&work).map(Reply::Written)
        }
        Job::Close => Ok(Reply::Done),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    fn a_database(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("quill-db-worker-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        let file = folder.join("test.db");
        let _ = std::fs::remove_file(&file);
        let connection = rusqlite::Connection::open(&file).expect("a database");
        connection
            .execute_batch(
                "create table member (id integer primary key, name text not null);
                 insert into member (id, name) values (1, 'Jason'), (2, 'Ada');",
            )
            .expect("a schema");
        file
    }

    /// Wait for one answer, with a deadline rather than for ever.
    ///
    /// **Everything else that arrives is kept**, because `Worker::take` drains the channel: a helper
    /// that threw away the answers it was not waiting for would lose the second of two outstanding
    /// jobs, which is exactly the fault the ticket numbers exist to prevent. A real caller keeps them
    /// the same way.
    fn wait_for(worker: &Worker, ticket: u64, kept: &mut Vec<Answered>) -> Answer<Reply> {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(at) = kept.iter().position(|answered| answered.ticket == ticket) {
                return kept.remove(at).answer;
            }
            if Instant::now() > until {
                panic!("no answer to ticket {ticket} in ten seconds");
            }
            kept.extend(worker.take());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_query_runs_on_the_thread_and_comes_back_with_its_rows() {
        let source = Source::sqlite("test", a_database("query").to_string_lossy());
        let worker = Worker::open(&source, None, None).expect("opened");
        assert!(worker.version.starts_with("SQLite"));
        let ticket = worker.ask(Job::Query { sql: "select name from member order by id".to_owned(), limit: 100 })
            .expect("asked");
        let mut kept = Vec::new();
        let Reply::Rows(rows) = wait_for(&worker, ticket, &mut kept).expect("rows") else { panic!() };
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.rows[0][0], Value::typed("Jason"));
        assert!(!worker.is_busy(), "nothing outstanding once it has been taken");
    }

    #[test]
    fn the_window_is_woken_when_an_answer_arrives() {
        // Without this a query that finished while nobody was moving the pointer would sit there
        // unseen until the next frame happened for some other reason.
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        let source = Source::sqlite("test", a_database("wake").to_string_lossy());
        let worker = Worker::open(&source, None, Some(wake)).expect("opened");
        let ticket = worker.ask(Job::Schemas).expect("asked");
        let _ = wait_for(&worker, ticket, &mut Vec::new());
        assert!(woken.load(Ordering::Relaxed) >= 1, "the window was asked to draw again");
    }

    #[test]
    fn a_failing_statement_comes_back_as_a_refusal_rather_than_stopping_the_thread() {
        let source = Source::sqlite("test", a_database("failing").to_string_lossy());
        let worker = Worker::open(&source, None, None).expect("opened");
        let bad = worker.ask(Job::Query { sql: "select * from nothing_like_this".to_owned(), limit: 10 })
            .expect("asked");
        let mut kept = Vec::new();
        assert!(wait_for(&worker, bad, &mut kept).is_err());
        // And the connection is still usable, which is the point: one bad statement in a console is
        // not a reason to lose the session.
        let good = worker.ask(Job::Query { sql: "select 1".to_owned(), limit: 10 }).expect("asked");
        assert!(wait_for(&worker, good, &mut kept).is_ok());
    }

    #[test]
    fn every_job_keeps_its_own_ticket_so_two_outstanding_do_not_get_confused() {
        let source = Source::sqlite("test", a_database("tickets").to_string_lossy());
        let worker = Worker::open(&source, None, None).expect("opened");
        let first = worker.ask(Job::Query { sql: "select 1".to_owned(), limit: 1 }).expect("asked");
        let second = worker.ask(Job::Query { sql: "select 2".to_owned(), limit: 1 }).expect("asked");
        assert_ne!(first, second);
        let mut kept = Vec::new();
        let Reply::Rows(one) = wait_for(&worker, first, &mut kept).expect("rows") else { panic!() };
        let Reply::Rows(two) = wait_for(&worker, second, &mut kept).expect("rows") else { panic!() };
        assert_eq!(one.rows[0][0], Value::typed("1"));
        assert_eq!(two.rows[0][0], Value::typed("2"));
    }

    #[test]
    fn a_write_is_one_transaction_and_the_file_really_changes() {
        let file = a_database("write");
        let source = Source::sqlite("test", file.to_string_lossy());
        let worker = Worker::open(&source, None, None).expect("opened");
        let statements = vec![Statement {
            sql: "UPDATE \"member\" SET \"name\" = ?1 WHERE \"id\" = ?2".to_owned(),
            values: vec![Value::typed("Grace"), Value::typed("1")],
            what: String::new(),
        }];
        let ticket = worker.ask(Job::Write { statements }).expect("asked");
        let Reply::Written(affected) = wait_for(&worker, ticket, &mut Vec::new()).expect("written") else { panic!() };
        assert_eq!(affected, [1]);
        // Read it back through a connection of its own, so this is the file rather than a cache.
        let connection = rusqlite::Connection::open(&file).expect("opened");
        let name: String = connection
            .query_row("select name from member where id = 1", [], |row| row.get(0))
            .expect("a row");
        assert_eq!(name, "Grace");
    }
}
