//! How an adapter is started and spoken to: a child process over its standard streams, or a server
//! on a port.
//!
//! Both shapes are common and Unluminous needs both, which is why this is a type rather than a
//! `Command`. `lldb-dap` ships inside every LLVM distribution and speaks over **stdio**; CodeLLDB
//! and Microsoft's js-debug are **servers** — js-debug is started as `node dapDebugServer.js <port>`
//! and then connected to on localhost, which is a child process *and* a socket. So a
//! [`Transport::Port`] may carry a program to start first, and stopping the session stops both.
//!
//! Nothing here decides *which* adapter to run. That is `unluminous-app`'s registry, because knowing
//! where `lldb-dap` lives on this machine is knowledge about the machine rather than about the
//! protocol, and a crate with no window in it has no business reading a settings file.

use std::io::{BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long to keep trying to connect to a server adapter that is still starting.
///
/// A `node dapDebugServer.js` takes a moment to open its port, and the honest answer to "is it
/// listening yet" is to try again rather than to sleep for a fixed time and hope. Five seconds is
/// far past what any of the three takes and is short enough that a wrong path says so quickly.
const CONNECT_GRACE: Duration = Duration::from_secs(5);
/// How long to wait between attempts.
const CONNECT_PAUSE: Duration = Duration::from_millis(50);

/// Where the messages go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    /// The adapter is a child process and the protocol runs over its standard input and output.
    Stdio,
    /// The adapter listens on a port of the loopback interface. `program` is started first when the
    /// server is one Unluminous has to launch, which is what js-debug needs; a `None` is an adapter
    /// somebody started themselves.
    Port(u16),
}

/// Everything needed to start one adapter and talk to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterCommand {
    /// The program to start, when there is one to start.
    pub program: Option<PathBuf>,
    pub args: Vec<String>,
    /// The folder it starts in. The project, so an adapter that resolves a relative path resolves it
    /// the way the person would.
    pub working_directory: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub transport: Transport,
}

impl AdapterCommand {
    /// A child process spoken to over its standard streams.
    pub fn stdio(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self {
            program: Some(program.into()),
            args,
            working_directory: None,
            env: Vec::new(),
            transport: Transport::Stdio,
        }
    }

    /// A server, started by Unluminous, connected to on `port`.
    pub fn server(program: impl Into<PathBuf>, args: Vec<String>, port: u16) -> Self {
        Self {
            program: Some(program.into()),
            args,
            working_directory: None,
            env: Vec::new(),
            transport: Transport::Port(port),
        }
    }

    pub fn in_folder(mut self, folder: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(folder.into());
        self
    }

    /// What the status bar says was started, which is the program and its arguments as they were
    /// handed over — never a reconstruction with quoting of its own.
    pub fn described(&self) -> String {
        let program = self
            .program
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "an adapter already running".to_owned());
        match self.args.is_empty() {
            true => program,
            false => format!("{program} {}", self.args.join(" ")),
        }
    }
}

/// The two ends of a started adapter: something to write frames into, something to read them out of,
/// and the child process to stop when the session ends.
pub struct Connection {
    pub(crate) writer: Box<dyn Write + Send>,
    pub(crate) reader: Box<dyn Read + Send>,
    /// The adapter's own standard error, when it has one.
    ///
    /// **Read rather than swallowed**, which is `unluminous-git`'s rule about git's stderr: nothing in
    /// Unluminous invents a message, and a debugger explains itself better than an editor could. It is not
    /// the protocol — it is the adapter talking about itself — so it is carried as `console` output
    /// rather than parsed. Measured on CodeLLDB 1.12.3, this is the only place a Python traceback for
    /// a failed evaluation appears at all; with it swallowed, a request the adapter had given up on
    /// was completely silent.
    pub(crate) errors: Option<Box<dyn Read + Send>>,
    pub(crate) child: Option<Child>,
}

/// A pipe has nothing worth printing, so this says what it is and stops there. It exists because a
/// test asserting that starting failed needs the success arm to be printable.
impl std::fmt::Debug for Connection {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "a connection to a debug adapter")
    }
}

/// Start an adapter and connect to it.
///
/// The reply is the connection, or the reason it could not be started — which the window puts in
/// the status bar. **Nothing is invented and nothing is fetched**: a missing adapter is a message
/// naming what was looked for, built by the registry that knew what to look for.
pub(crate) fn start(command: &AdapterCommand) -> Result<Connection, String> {
    match command.transport {
        Transport::Stdio => start_stdio(command),
        Transport::Port(port) => start_server(command, port),
    }
}

fn start_stdio(command: &AdapterCommand) -> Result<Connection, String> {
    let program = command
        .program
        .clone()
        .ok_or_else(|| "A stdio adapter needs a program to start.".to_owned())?;
    let mut child = spawn(&program, command, Stdio::piped(), Stdio::piped(), Stdio::piped())?;
    let writer = child.stdin.take().ok_or_else(|| standard_stream_missing(&program))?;
    let reader = child.stdout.take().ok_or_else(|| standard_stream_missing(&program))?;
    let errors = child.stderr.take();
    Ok(Connection {
        writer: Box::new(writer),
        errors: errors.map(|errors| Box::new(BufReader::new(errors)) as Box<dyn Read + Send>),
        // Buffered, because the decoder is fed whatever one read produced and an unbuffered pipe
        // hands over a byte at a time under load. The decoder copes either way; this is simply less
        // work per frame.
        reader: Box::new(BufReader::new(reader)),
        child: Some(child),
    })
}

/// Dial a server-shaped adapter that is **already running**, without starting anything.
///
/// What a child session is opened with: js-debug's `startDebugging` means "there is a second session
/// waiting on the port you are already talking to", so the program must not be started again.
pub fn connect(command: &AdapterCommand) -> Result<Connection, String> {
    let Transport::Port(port) = command.transport else {
        return Err("Only a server-shaped adapter can be connected to a second time.".to_owned());
    };
    let dialled = AdapterCommand { program: None, ..command.clone() };
    start_server(&dialled, port)
}

fn start_server(command: &AdapterCommand, port: u16) -> Result<Connection, String> {
    // The server's own output is not the protocol — it is `node` complaining, or the server saying
    // which port it took — so it is swallowed rather than mixed into the frames. An adapter that
    // fails to start says so by never answering the connection, which is the message below.
    let mut child = match &command.program {
        Some(program) => Some(spawn(program, command, Stdio::null(), Stdio::null(), Stdio::piped())?),
        None => None,
    };
    let errors = child
        .as_mut()
        .and_then(|child| child.stderr.take())
        .map(|errors| Box::new(BufReader::new(errors)) as Box<dyn Read + Send>);
    // **Both spellings of localhost, in that order.** js-debug binds whatever `localhost` resolves to
    // on the machine, and on Windows that is `::1` before `127.0.0.1` — measured on `task-1692`,
    // where a perfectly healthy adapter refused every connection until the second address was tried.
    // A client that knew only the dotted quad is a client that cannot talk to the adapter it just
    // started.
    let addresses: [SocketAddr; 2] =
        [([127, 0, 0, 1], port).into(), (std::net::Ipv6Addr::LOCALHOST, port).into()];
    let started = Instant::now();
    let stream = loop {
        let mut refused = None;
        let mut connected = None;
        for address in addresses {
            match TcpStream::connect(address) {
                Ok(stream) => {
                    connected = Some(stream);
                    break;
                }
                Err(problem) => refused = Some(problem),
            }
        }
        if let Some(stream) = connected {
            break stream;
        }
        if started.elapsed() >= CONNECT_GRACE {
            let mut child = child;
            if let Some(child) = child.as_mut() {
                let _ = child.kill();
            }
            let problem = match refused {
                Some(problem) => problem.to_string(),
                None => "nothing answered".to_owned(),
            };
            return Err(format!(
                "Unluminous could not reach the debug adapter on 127.0.0.1:{port} or [::1]:{port}: {problem}"
            ));
        }
        std::thread::sleep(CONNECT_PAUSE);
    };
    // Nagle's algorithm holds a small write back waiting for a second one, and every frame here is a
    // small write that the other end is waiting on. A stepping request delayed by forty milliseconds
    // is a debugger that feels broken.
    let _ = stream.set_nodelay(true);
    let reader = stream
        .try_clone()
        .map_err(|problem| format!("Unluminous could not read from the debug adapter: {problem}"))?;
    Ok(Connection {
        writer: Box::new(stream),
        reader: Box::new(BufReader::new(reader)),
        errors,
        child,
    })
}

/// Start the program, with the window suppressed on Windows.
fn spawn(
    program: &std::path::Path,
    command: &AdapterCommand,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
) -> Result<Child, String> {
    let mut process = Command::new(program);
    process.args(&command.args);
    process.stdin(stdin);
    process.stdout(stdout);
    process.stderr(stderr);
    // The adapter's standard error is its own diagnostics rather than the protocol. It is neither
    // inherited — which would print into whatever terminal Unluminous was started from — nor swallowed,
    // because that is where an adapter explains itself. The caller says which; see
    // [`Connection::errors`].
    if let Some(folder) = &command.working_directory {
        process.current_dir(folder);
    }
    for (name, value) in &command.env {
        process.env(name, value);
    }
    #[cfg(target_os = "windows")]
    {
        // Do not flash a console window, which is what `unluminous-git` does for each git command and for
        // the same reason. 0x08000000 is CREATE_NO_WINDOW.
        use std::os::windows::process::CommandExt;
        process.creation_flags(0x0800_0000);
    }
    process.spawn().map_err(|problem| {
        format!("Unluminous could not start {}: {problem}", program.display())
    })
}

fn standard_stream_missing(program: &std::path::Path) -> String {
    format!("{} was started but gave Unluminous no pipe to talk on.", program.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_shapes_are_both_expressible() {
        let stdio = AdapterCommand::stdio("lldb-dap", Vec::new());
        assert_eq!(stdio.transport, Transport::Stdio);
        let server = AdapterCommand::server("node", vec!["dapDebugServer.js".to_owned()], 8123);
        assert_eq!(server.transport, Transport::Port(8123));
    }

    #[test]
    fn what_was_started_is_described_as_it_was_handed_over() {
        let command = AdapterCommand::stdio("C:\\llvm\\bin\\lldb-dap.exe", Vec::new());
        assert_eq!(command.described(), "C:\\llvm\\bin\\lldb-dap.exe");
        let server = AdapterCommand::server("node", vec!["js-debug/src/dapDebugServer.js".to_owned(), "8123".to_owned()], 8123);
        assert_eq!(server.described(), "node js-debug/src/dapDebugServer.js 8123");
    }

    #[test]
    fn a_stdio_adapter_with_no_program_is_refused_rather_than_hung() {
        let command = AdapterCommand {
            program: None,
            args: Vec::new(),
            working_directory: None,
            env: Vec::new(),
            transport: Transport::Stdio,
        };
        let problem = start(&command).expect_err("nothing to start");
        assert!(problem.contains("needs a program"), "{problem}");
    }

    /// A program that is not on the machine is a sentence rather than a panic, because "install
    /// lldb-dap" is exactly what the person needs to be told.
    #[test]
    fn a_program_that_is_not_there_says_so() {
        let command = AdapterCommand::stdio("unluminous-no-such-debug-adapter", Vec::new());
        let problem = start(&command).expect_err("no such program");
        assert!(problem.contains("could not start"), "{problem}");
    }

    /// And a port nothing is listening on gives up rather than waiting for ever — the grace is what
    /// makes "the adapter did not start" a thing a person finds out about.
    #[test]
    fn a_port_nothing_answers_on_gives_up_and_says_where_it_looked() {
        let command = AdapterCommand {
            program: None,
            args: Vec::new(),
            working_directory: None,
            env: Vec::new(),
            // Port 1 needs privileges nothing here has and nothing listens on it, so the connection
            // is refused at once rather than after the whole grace.
            transport: Transport::Port(1),
        };
        let problem = start(&command).expect_err("nothing listening");
        assert!(problem.contains("127.0.0.1:1"), "{problem}");
    }
}
