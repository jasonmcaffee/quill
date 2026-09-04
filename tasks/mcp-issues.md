# Issues found driving Unluminous through the MCP tools

## What this records

An agent was asked to drive an Unluminous window entirely through the Model Context Protocol (MCP) tools:
write a JavaScript file that prints prime numbers, open it in a tab, run it, read the output, edit
it, save it and run it again. All of that worked. Five problems turned up while doing it, and this
document records them with the code that causes each one.

MCP is the protocol an agent uses to call tools in another program. Unluminous serves it through
`unluminous-cli`, which forwards each tool call to a running window over a loopback TCP connection.

The session used Unluminous 0.12.0, build date 2026-08-26 1:51pm, on macOS 26.5.1 (25F80), driving the
window on `/Users/jason.mcaffee/dev/unluminous-testing` (process 88081, port 64874). The source line
numbers below are from commit
[`db470c6`](https://github.com/jasonmcaffee/unluminous/commit/db470c66ef151f7a02991631fe3e7490a2bcdcb6).

## Jargon used below

| Term | What it means here |
| --- | --- |
| Frame loop | The single thread that draws the window. Unluminous answers every command from this thread, one command batch per drawn frame. |
| Semaphore | A lock a thread waits on until another thread signals it. A thread waiting on one is asleep and uses no processor time. |
| Occluded window | A window macOS has decided is not visible, because another window covers it. macOS stops asking an occluded window to draw. |
| `request_repaint` | The egui call that asks the window to draw another frame. It is how a background thread tells the window there is something to do. |
| Structured content | The machine readable JSON object in an MCP tool reply, as opposed to the human readable text block that accompanies it. |

## Issue 1: an idle window never drains the command queue, so every command times out

This is the one that makes driving Unluminous through MCP unreliable. Everything else in this document is
smaller.

### What happens

When the Unluminous window sits idle, every MCP tool call and every `unluminous-cli` command fails with
`timed-out: Unluminous did not answer <command> within 15000 ms`. The process is alive the whole time,
sleeping at 0% processor use, still listening on its port. It looks exactly like a hung application
and it is not hung.

Generating any user interface event fixes it instantly. This is the measurement, with `tab list`
called twice and nothing changed in between except bringing the window to the front:

**Evidence: two `unluminous-cli tab list` calls, before and after activating the window.**

```
--- before: Unluminous idle in the background ---
Unluminous did not answer tab.list within 15000 ms.     took 15.09s
--- activating the window with: open -b com.jasonmcaffee.unluminous ---
--- after: Unluminous frontmost ---
1 open                                              took 0.05s
```

Raising the timeout does not help. The MCP tool call accepts a `timeout` argument, and it is honoured
correctly, and the command still never gets answered:

**Evidence: the same `run list` tool call twice, with an idle window.**

```
mcp__unluminous__unluminous_run {"command": "list"}
  -> timed-out: Unluminous did not answer run.list within 15000 ms.

mcp__unluminous__unluminous_run {"command": "list", "arguments": {"timeout": 60000}}
  -> timed-out: Unluminous did not answer run.list within 65000 ms.
```

Sixty five seconds with an idle window and the queue was still not drained. So this is not a slow
frame that a longer deadline would cover.

### Why it happens

Every command is answered by the frame loop, which is the correct design and is written down as
such:

**Source:** [`unluminous`, `crates/unluminous-app/src/services/control.rs` lines 27 to 32](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/crates/unluminous-app/src/services/control.rs#L27-L32)

```rust
//! ## Why the window answers rather than the thread
//!
//! Every command changes or reads the window's own state, and the window is a single thread with a
//! frame loop. So the listener does not touch it: it queues the request, wakes the window, and
//! waits for the answer. The window drains the queue at the top of a frame, which is also what
//! makes a command's effect visible in the very next screenshot.
```

The connection thread queues the request, calls `wake()`, and then waits:

**Source:** [`unluminous`, `crates/unluminous-app/src/services/control.rs` lines 285 to 298](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/crates/unluminous-app/src/services/control.rs#L285-L298)

```rust
    let command = request.command.clone();
    let (answer, wait) = mpsc::channel();
    if sender.send(Pending { request, reply: Some(answer) }).is_err() {
        return Reply::failed(&command, code::NOT_RUNNING, "Unluminous is closing.");
    }
    wake();
    match wait.recv_timeout(BACKSTOP) {
        Ok(reply) => reply,
        Err(_) => Reply::failed(
            &command,
            code::TIMED_OUT,
            "Unluminous did not answer. The window may be busy or may have stopped drawing.",
        ),
    }
```

`wake` is `request_repaint` on the egui context:

**Source:** [`unluminous`, `crates/unluminous-app/src/app/mod.rs` lines 689 to 695](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/crates/unluminous-app/src/app/mod.rs#L689-L695)

```rust
    pub fn open_control_channel(&mut self, ctx: &egui::Context) {
        let folder = self.tree.root().to_path_buf();
        // The context rather than `thread_waker`, because this is called before the first frame and
        // the window has not yet been given one to wake.
        let context = ctx.clone();
        self.control =
            control::Server::start(folder, Arc::new(move || context.request_repaint()));
```

`request_repaint` asks for a frame. It cannot make macOS draw a window macOS has decided not to
draw. When the window is occluded or otherwise idle, the request is recorded, no frame is drawn, the
queue is never drained, and the waiting connection thread sleeps until its deadline.

Process stacks taken with `sample 88081` while four commands were stuck confirm this exactly. Four
threads named `unluminous-control-connection` were each parked on the semaphore inside
`unluminous_app::services::control::serve`:

**Evidence: `sample 88081`, one of four identical `unluminous-control-connection` threads.**

```
2495 Thread_13934062: unluminous-control-connection
  2495 unluminous_app::services::control::serve  (in unluminous) + 1936
    2495 _dispatch_semaphore_wait_slow  (in libdispatch.dylib) + 76
      2495 _dispatch_sema4_timedwait  (in libdispatch.dylib) + 64
        2495 semaphore_timedwait_trap  (in libsystem_kernel.dylib) + 8
```

At the same moment the frame loop was parked in its ordinary event wait, with no frame pending:

**Evidence: `sample 88081`, the main thread during the same stall.**

```
2495 Thread_13910223   DispatchQueue_1: com.apple.main-thread  (serial)
  2495 -[NSApplication run]  (in AppKit) + 368
    2495 _DPSNextEvent  (in AppKit) + 576
      2495 __CFRunLoopServiceMachPort  (in CoreFoundation) + 160
        2495 mach_msg2_trap  (in libsystem_kernel.dylib) + 8
```

The comment on the backstop constant already anticipates this state, which suggests it was known to
be possible but was treated as something only a broken window would reach:

**Source:** [`unluminous`, `crates/unluminous-app/src/services/control.rs` lines 49 to 54](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/crates/unluminous-app/src/services/control.rs#L49-L54)

```rust
/// How long the listener will hold a connection open waiting for the window to answer.
///
/// Longer than any command's own wait, because the command's own timeout is the one that should
/// fire and say what it was waiting for. This is the backstop for a window that has stopped drawing
/// altogether, which would otherwise leave the caller hanging for ever.
const BACKSTOP: Duration = Duration::from_secs(120);
```

An idle window is a normal window, not a broken one. An agent driving Unluminous produces exactly this
state, because between two tool calls nothing touches the keyboard or the mouse.

### Suggested fix

Stop making a drawn frame the only thing that drains the queue. Three options, in the order worth
trying:

1. Have the wake send a winit user event through an `EventLoopProxy` rather than only asking egui for
   a repaint, and drain the queue when that user event is handled. A user event wakes the event loop
   whether or not macOS wants to draw the window.
2. While any request is outstanding, set the winit control flow to `Poll` so the event loop keeps
   turning and the queue keeps being drained.
3. Answer the commands that only read state on the connection thread, behind a lock, so reading
   state never depends on the window drawing. This is the largest change of the three and it gives up
   the property that a command's effect shows in the very next screenshot.

Whichever is chosen, a test should cover it: queue a command while the window is not drawing frames
and assert that it is still answered.

## Issue 2: a run configuration that cannot start is reported as a success

### What happens

The first attempt at running the prime numbers file used a run configuration whose command was
`node primes.js`. Unluminous spawns without a shell, and a window launched from Finder does not have the
nvm directory on its `PATH`, so `node` could not be found. Through MCP the failure was invisible.
This is the whole reply the agent received:

**Evidence: the MCP reply to `run start` for a program that does not exist.**

```json
{"activeRun":"primes","configurations":[
  {"command":"definitely-not-a-real-program","directory":"","env":"","exitCode":null,
   "name":"bogus","origin":"permanent","running":false,"started":false,"state":null}],
 "height":260,"runs":["primes"],"selected":"bogus","visible":false}
```

`started` is `false`, `state` is `null`, and there is no reason given anywhere. The call is not
marked as an error. The same command through `unluminous-cli` prints the reason:

**Evidence: `unluminous-cli run start primes` with `node` not on the window's PATH.**

```
Unluminous could not start node: Failed to spawn command 'node': No such file or directory (os error 2)
```

An agent holding only the structured content cannot tell why nothing ran, and cannot tell the
difference between a program that failed to spawn and a program that ran and exited immediately.

### Why it happens

The failure text is put into the window's status bar message:

**Source:** [`unluminous`, `crates/unluminous-app/src/app/mod.rs` lines 1563 to 1570](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/crates/unluminous-app/src/app/mod.rs#L1563-L1570)

```rust
        match self.run.start(configuration, &root, size, waker) {
            Ok(_) => {
                self.show_the_run_tile(true);
                self.focus = Focus::Terminal;
                self.message = Some(format!("Running {name}"));
            }
            Err(problem) => self.message = Some(problem),
        }
```

`cli_run_do` then reads that status bar message and returns `ok` regardless of whether the run
started:

**Source:** [`unluminous`, `crates/unluminous-app/src/app/cli.rs` lines 2927 to 2930](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/crates/unluminous-app/src/app/cli.rs#L2927-L2930)

```rust
        self.message = None;
        self.run_a_configuration(what(named));
        let said = self.message.clone().unwrap_or_default();
        ok(request, said, self.run_value())
```

Because `reply.error` is never set, the MCP layer takes the success path, marks `isError` false, and
attaches the structured content that a client will render in preference to the text:

**Source:** [`unluminous`, `unluminous-cli/src/mcp/server.rs` lines 181 to 195](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/unluminous-cli/src/mcp/server.rs#L181-L195)

```rust
    /// Turn what the window said into what an agent reads.
    fn tool_result(&self, command: &'static Command, reply: &Reply) -> Value {
        if let Some(failure) = &reply.error {
            return refused(format!("{}: {}", failure.code, failure.message));
        }
        let mut content = vec![json!({ "type": "text", "text": spoken(reply) })];
        if let Some(picture) = self.picture_from(command, reply) {
            content.push(picture);
        }
        let mut answer = json!({ "content": content, "isError": false });
        if !reply.result.is_null() {
            answer["structuredContent"] = reply.result.clone();
        }
        answer
    }
```

### Suggested fix

Make `run_a_configuration` report whether it started, and have `cli_run_do` return `no(request,
code::FAILED, problem)` when it did not. That sets `reply.error`, which makes the MCP layer return a
refusal carrying the real reason, and makes the `unluminous-cli` exit code correct at the same time.

Separately, a run configuration whose program cannot be found is worth catching earlier. `run add`
accepts `node primes.js` without complaint and the problem only appears at `run start`. Resolving
the program against `PATH` at `run add` time and saying so when it cannot be found would remove the
most likely first failure an agent hits, because an agent will write `node`, `python` or `cargo` and
those are exactly the programs a version manager keeps off a Finder launched application's `PATH`.

## Issue 3: a command that reports a timeout has usually already been applied

### What happens

Because of the idle window problem in Issue 1, the client gives up while the request is still sitting
in the queue. The window then drains it later, when something wakes it. The command takes effect
after the caller has been told it failed.

Three commands in this session reported `timed-out` over MCP and all three had in fact been applied:

- `run add bogus` reported a timeout. The configuration was in
  `.unluminous/run-configurations.conf` as `run.3` afterwards.
- `run remove bogus` reported a timeout. The configuration was gone afterwards.
- `run rerun primes` reported a timeout. The program had run, the state was `finished`, and the
  output was there.

An agent that retries a command after a timeout will apply it twice. For `run add` that is harmless.
For `editor insert`, `tab close --discard` or `explorer delete` it is not.

### Why it happens

The client deadline is shorter than the window's own backstop, so the client abandons a request the
window will still honour. The client default is 15 seconds:

**Source:** [`unluminous`, `unluminous-cli/src/client.rs` line 16](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/unluminous-cli/src/client.rs#L16)

```rust
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(15_000);
```

The window's backstop is 120 seconds, so there is a window of 105 seconds in which the caller has
been told the command failed and the command has not been cancelled.

### Suggested fix

Fixing Issue 1 removes almost all of these. On top of that, the queued request should be cancelled
when the caller goes away, so a timeout means the command did not happen. The connection thread
already owns the `Pending`; it should mark it abandoned when `recv_timeout` fires, and the frame loop
should discard an abandoned request instead of running it. Until that exists, the timeout message
should say plainly that the command may still be applied, because the current wording reads as a
failure.

## Issue 4: a stalled command blocks the screenshot needed to diagnose it

### What happens

`window screenshot` is the tool the MCP server instructions recommend for seeing what a command
actually did, and it travels over the same channel as everything else. During the stall in Issue 1 it
timed out along with the rest, so the one tool that would show what the window was doing was
unavailable exactly when it was needed. Diagnosis had to fall back on the macOS `screencapture`
command and on `sample` to read the process stacks, neither of which an agent driving Unluminous would
normally reach for.

Four connection threads were stuck at once, each holding a thread for its full deadline.

### Suggested fix

This mostly follows from fixing Issue 1. Two things would still help on their own. Answer `status`
and `window screenshot` without waiting on a frame, from the last frame's state, so that an agent can
always find out what the window thinks is happening. And include in the timeout reply whatever the
server already knows, such as how many requests are queued and when the last frame was drawn, so a
caller can tell a busy window from one that has stopped drawing.

## Issue 5: the MCP tool descriptions do not mention the arguments that control waiting

### What happens

The MCP tool schemas take a free form `arguments` object and describe it as "the values the command
takes, by the name in its usage line". The usage lines cover each command's own arguments. They do
not mention that `timeout` is accepted on every call, because `timeout` is a global flag of the
command line interface rather than part of any usage line:

**Source:** [`unluminous`, `unluminous-cli/src/parse.rs` line 85](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/unluminous-cli/src/parse.rs#L85)

```rust
    ("timeout", Some("milliseconds"), "How long to wait for an answer. 15000 by default."),
```

It is accepted, and the MCP driver reads it and adds five seconds of headroom:

**Source:** [`unluminous`, `unluminous-cli/src/mcp/driver.rs` lines 205 to 214](https://github.com/jasonmcaffee/unluminous/blob/db470c66ef151f7a02991631fe3e7490a2bcdcb6/unluminous-cli/src/mcp/driver.rs#L205-L214)

```rust
/// How long to wait for this call.
fn timeout_for(arguments: &Map<String, Value>) -> Duration {
    let waiting = ["timeout", "wait"]
        .iter()
        .filter_map(|name| arguments.get(*name))
        .filter_map(as_millis)
        .max()
        .unwrap_or(0);
    DEFAULT_TIMEOUT.max(Duration::from_millis(waiting + 5_000))
}
```

The agent only discovered `timeout` by reading this source. Note also that `DEFAULT_TIMEOUT.max(...)`
means 15 seconds is a floor: a caller can raise the deadline but cannot lower it, so an agent that
wants to fail fast cannot.

### Suggested fix

Say in each tool's description that `timeout` in milliseconds is accepted on any call. If a caller
should be able to fail faster than 15 seconds, use the passed value directly rather than taking the
maximum of it and the default.

## One note about the setup, which is not an Unluminous bug

The agent that found all of this was running inside the Unluminous window it was driving. The parent chain
was Unluminous (process 88081) to `/bin/zsh` to `claude` to the shell each command ran in, and the hosting
terminal tab was named "Unluminous MCP UI driving".

That matters for anyone reproducing this. Restarting Unluminous to clear the stall destroys the session
doing the testing, which happened once before the session that produced this document. Recover with
a user interface event instead. To test anything that needs a restart, start a second window with
`unluminous-cli launch <folder>` and drive it with the `instance` argument.

It also affects how the stall presents. A Claude Code session draws an animated spinner in its
terminal, and that animation keeps waking the frame loop, so commands are fast for as long as the
agent is producing output and start timing out once the terminal goes quiet. That is why the problem
looks intermittent when it is not.
