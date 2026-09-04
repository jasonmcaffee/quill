# task-1691 — The command channel answers a window that is not drawing, and says so when it cannot

An agent was asked to drive an Unluminate window entirely through the Model Context Protocol: write a
JavaScript file that prints prime numbers, open it in a tab, run it, read the output, edit it, save
it and run it again. All of that worked. Five faults turned up on the way, and `tasks/mcp-issues.md`
is the report — what was seen, measured on a real session, with the code that causes each one. This
is the other half: what to do about them, what was weighed, and what was deliberately left alone.

They are not five unrelated faults. Three of them — the timeouts, the commands that were applied
after being reported as failures, and the screenshot that could not be taken while it was needed —
are the same fault seen from three sides: **a request reaches the queue and nothing ever drains it.**
The other two are about what an agent is told: a run that could not start looked like a success, and
the one argument that controls waiting was not written down anywhere an agent reads.

## 1. A request that reaches the queue and is never drained

### 1.1 What happens

With the window idle, every tool call and every `unluminate-cli` command fails with `timed-out: Unluminate did
not answer <command> within 15000 ms`. The process is alive the whole time, sleeping at 0% processor
use and still listening on its port. Bringing the window to the front fixes it instantly:

```
--- before: Unluminate idle in the background ---
Unluminate did not answer tab.list within 15000 ms.     took 15.09s
--- activating the window with: open -b com.jasonmcaffee.unluminate ---
--- after: Unluminate frontmost ---
1 open                                              took 0.05s
```

Raising the deadline does not help. Sixty-five seconds with an idle window and the queue was still
not drained, so it is not a slow frame that a longer wait would cover. `sample` on the process showed
four `unluminate-control-connection` threads each parked on a semaphore inside `control::serve`, and the
main thread parked in its ordinary event wait with no frame pending.

### 1.2 Why it happens

Every command is answered by the frame loop. That is the design and it is written down as such in
`services::control`: the listener queues the request, wakes the window, and waits, and the window
drains the queue at the top of a frame — which is also what makes a command's effect visible in the
very next screenshot. None of that is wrong. What is wrong is the **wake**.

The wake is one call to `egui::Context::request_repaint`, made once, and a single repaint request is
not a reliable way to wake a window. Two mechanisms lose it, and both are in code Unluminate does not own:

**egui drops a repaint request it has already served.** `ContextImpl::request_repaint_after` calls
the backend's callback only `if delay < viewport.repaint.repaint_delay`, so a request made while the
window is already repainting as fast as it can sends nothing at all. That is correct while frames
keep coming and fatal on the last one.

**eframe drops a repaint request it decides is stale.** The callback carries the pass number the
request was made at, and `WinitAppWrapper::user_event` compares it against the pass number when the
event is delivered:

```rust
if current_pass_nr == cumulative_pass_nr || current_pass_nr == cumulative_pass_nr + 1 {
    // schedule the repaint
} else {
    log::trace!("Got outdated UserEvent::RequestRepaint");
    Ok(EventResult::Wait) // old request - we've already repainted
}
```

A request that arrives more than one pass late is thrown away on the assumption that the repaint it
asked for has already happened. For a wake that means "there is work on a queue", that assumption is
simply untrue: the work is still there and the wake is gone.

The ticket's own evidence is what settles which of the two it is, and it settles it in favour of
"the wake was lost" over "the window cannot draw". The agent was running in a terminal tab inside
the window it was driving, and a Claude Code session draws an animated spinner:

> commands are fast for as long as the agent is producing output and start timing out once the
> terminal goes quiet.

The terminal's own waker is `request_repaint` too. It works because it fires many times a second;
the control channel's fires once. A window that answers a repaint from the terminal thread and not
one from the control thread is not a window that has stopped drawing — it is a window whose single
wake went missing.

### 1.3 What was weighed

**Raise the deadline.** Measured and refused in the ticket: sixty-five seconds changed nothing,
because a lost wake is lost for ever rather than late.

**Send a winit user event through an `EventLoopProxy` instead of asking egui for a repaint.** This is
the ticket's first suggestion and it is the right shape, but it is not reachable from here. `eframe`
owns the event loop and the proxy; `request_repaint` *already is* a proxy `send_event` underneath,
and the two places the request is dropped are both downstream of the proxy. Unluminate would have to stop
using `eframe::run_native` to get at it, which is a change to how the whole application starts in
order to fix one channel.

**Set the winit control flow to `Poll` while a request is outstanding.** The ticket's second
suggestion, and the one that is actually right — the queue keeps being drained for as long as
something is on it. The control flow is `eframe`'s to set, but the effect is reachable: asking for a
repaint again, and again, for as long as the request has not been picked up, is what `Poll` means
from outside the event loop. `pump_control` already does exactly this for a *held* request
(`ctx.request_repaint_after(30ms)` while `cli_waiting` is not empty), so this is the same rule
applied one step earlier rather than a new mechanism.

**Answer read-only commands on the connection thread, behind a lock.** The ticket's third suggestion
and the one it ranks last. It gives up the property the module comment is built on — that a command's
effect is in the very next screenshot — for a subset of commands, and it puts a lock around window
state that nothing else needs. Refused.

### 1.4 What was chosen: the wake repeats until the request is picked up

`read_and_queue` stops waiting once and starts waiting in slices. While the request is still sitting
on the queue it wakes the window again every `NUDGE` (50 ms); the moment the window takes it, the
nudging stops and the thread waits as it did before.

```rust
loop {
    match wait.recv_timeout(NUDGE) {
        Ok(reply) => return reply,
        Err(RecvTimeoutError::Disconnected) => return /* Unluminate is closing */,
        Err(RecvTimeoutError::Timeout) => { /* deadline? abandon. taken? just wait. else nudge. */ }
    }
}
```

Three things make that cheap rather than a busy loop.

**It stops the instant the window picks the request up.** `Pending` carries a `taken` flag that
`Server::take` sets, so the ordinary case — a command answered on the very next frame — costs one
extra wake at most, and usually none. A command that is *held* across frames, such as `terminal read
--wait-for` or a git action, is picked up immediately and then nudges nothing at all for the minutes
it may wait; keeping the window drawing for that is `pump_control`'s job and it already does it.

**A window that is drawing is not woken any harder than it already is.** A nudge is a repaint
request, which a window drawing at sixty frames a second discards for free.

**A window that is not drawing is woken twenty times a second until it draws.** Which is the point:
one of those wakes is not going to be dropped as outdated, because the pass number stops moving the
moment the window goes quiet, and that is exactly the state the wake has to get through.

`take` is also the frame's heartbeat. It is called once at the top of every frame and by nothing
else, so counting frames there needs no new call site and no rule anybody has to remember — the
`follow_the_open_file` argument, that a list of the places which have to say "I did this" is a list
whose next entry will be the one that forgot.

## 2. A command reported as a timeout that had already been applied

### 2.1 What happens

The client gives up at fifteen seconds. The window's backstop is a hundred and twenty. So for a
hundred and five seconds the caller has been told the command failed and the command has not been
cancelled — and when something eventually wakes the window, it runs. Three commands in the ticket's
session reported `timed-out` and all three had in fact been applied: `run add bogus` was in
`.unluminate/run-configurations.conf` afterwards, `run remove bogus` was gone afterwards, and `run rerun
primes` had run.

An agent that retries after a timeout applies the command twice. For `run add` that is harmless. For
`editor insert`, `tab close --discard` or `explorer delete` it is not.

### 2.2 What was chosen: three answers to one question, because one was not enough

**The caller's deadline travels with the request.** `deadline_ms` is a fourth field on `Request`,
written by `protocol::ask` from the timeout it was already given, and absent from an older client —
where the backstop still applies, exactly as it does today. When it passes with the request **still
on the queue**, the connection thread marks the `Pending` abandoned and answers with a timeout that
says the command did not happen. `pump_control` drops an abandoned request instead of running it.

**Only a request still on the queue is abandoned**, and that restriction is doing real work. A
command the window has already taken owns its own deadline — `debug start --wait-for-pause` waits
thirty seconds, `git action --wait` waits thirty — and abandoning one of those at the transport's
deadline would silently shorten every waiting command in the catalogue to fifteen seconds. Once the
window has it, the connection thread waits for the window's own answer as it always has.

**The window derives it too, rather than only being told.** `Pending` carries the moment it was
queued and how long its caller was prepared to wait, and `was_abandoned` compares them. This is
`follow_the_open_file`'s rule — a list of the places that have to say "I did this" is a list whose
next entry will be the one that forgot — and it is not theoretical. Driving a real window with its
whole process suspended, the flag alone let `run add` be applied a second after the caller had been
told it timed out: every thread stopped together, so nobody set anything, and on resuming the frame
loop reached the request before the connection thread reached its own deadline.

**And the socket says so at the moment it happens.** The deadline is measured from when the request
was *queued*, so a request that could not even be accepted until the window woke up looks brand new
however long its caller waited for it — which is exactly the shape of the suspended-process case, and
the two rules above still let it through. What closes it is the thing the ticket asked for in so many
words: *the queued request should be cancelled when the caller goes away.* A client that gives up
stops reading and its socket closes, so `caller_has_gone` peeks the connection — `Ok(0)` is the other
end closed — while the request is on the queue, and one whose caller has gone is never queued at all,
or is marked abandoned on the next fifty-millisecond tick. No new wire field, and it is earlier and
more certain than any clock.

The socket is put in non-blocking mode for the wait and back into blocking before the reply is
written, because both handles refer to one socket and the mode belongs to the socket rather than to a
handle. Nothing else touches it in between; that thread owns the connection.

### 2.3 The margin is a share of the deadline

The window aims to answer a little before the caller gives up, because the two say different things:
the client's message is only that nothing came back, and the window's says what was actually going on.

A flat 300 ms was measured and was wrong at both ends of the range. At a fifteen-second deadline it
lost the race often enough to matter — the wait wakes on a fifty-millisecond tick and the client's
clock starts first, so a tenth of a second is inside the noise — and at an eight-hundred-millisecond
deadline half a second would answer before the window had a chance to. So `margin_for` keeps back a
**tenth** of the deadline, capped at 500 ms: 500 at fifteen seconds, 80 at eight hundred
milliseconds.

### 2.4 One thing observed and deliberately not changed

`client_timeout` stretches the client's deadline past whatever the command was *told* to wait for,
but not past what a command waits for **by default**. `debug start --wait-for-pause` with no
`--timeout` waits thirty seconds in the window and fifteen in the client. That is pre-existing, it is
untouched by any of this — because an abandoned request is only ever one that has not been picked up
— and fixing it properly means putting each waiting command's default into the catalogue as data
rather than as prose in its help. Written down here rather than half-fixed.

## 3. A stalled command blocks the screenshot that would have diagnosed it

`window screenshot` is what the server's own instructions recommend for seeing what a command did,
and it travels down the same channel as everything else, so during the stall it timed out along with
the rest. Diagnosis had to fall back on `screencapture` and `sample`.

Most of this follows from §1: a channel that answers is a channel a screenshot comes back down.
What does not follow is that a caller who does time out is told nothing useful, and that is worth
fixing on its own. The listener already knows three things without asking the window anything:

- how many requests are queued and have not been picked up,
- how many frames the window has drawn since the channel opened,
- how long ago the last one was.

So the timeout says them:

```
Unluminate did not answer run.list within 15000 ms. It has not drawn a frame for 65.0 s and has 4
requests queued, so it is not drawing rather than busy. The command was not run.
```

against, for a window that really is working:

```
Unluminate did not answer git.action within 15000 ms. It drew a frame 16 ms ago, so it is busy rather
than stopped. The command was not run.
```

The ticket's other suggestion — answer `status` and `window screenshot` from the last frame's state,
without waiting for a frame — is not taken. A screenshot from the last frame's state would mean
keeping a copy of the framebuffer that nothing else needs, and a `status` answered off the frame loop
would need a mirror of the window's state behind a lock, which is §1.3's rejected third option under
another name. The need behind both suggestions is *tell me whether it is busy or stopped*, and the
sentence above answers it with what is already known.

## 4. A run that could not start reported as a success

### 4.1 What happens

The first attempt at running the prime numbers file used `node primes.js`. Unluminate spawns without a
shell and a window launched from Finder has no nvm directory on its `PATH`, so `node` could not be
found. Through MCP the failure was invisible: `started` false, `state` null, no reason anywhere, and
`isError` false. The same command through `unluminate-cli` prints `Unluminate could not start node: Failed to
spawn command 'node': No such file or directory (os error 2)`.

The reason is that `start_a_run` puts the failure in the status bar message and `cli_run_do` reads
that message and returns `ok` regardless:

```rust
self.message = None;
self.run_a_configuration(what(named));
let said = self.message.clone().unwrap_or_default();
ok(request, said, self.run_value())
```

`reply.error` is never set, so the MCP layer takes the success path and attaches the structured
content a client renders in preference to the text.

### 4.2 What was chosen

`run_a_configuration` returns `Result<(), String>` — the reason it could not be done, or nothing.
Every arm that today writes an apology into `self.message` returns it instead, and the one caller
that is a menu (`run_action`) puts it in the status bar exactly as before, so nothing a person sees
changes. `cli_run_do` and `cli_run_select` return `no(request, code::FAILED, problem)`, which sets
`reply.error`, which makes the MCP layer answer with a refusal carrying the real reason and makes
`unluminate-cli`'s exit code right at the same time.

This is `run_action`'s own rule restated: the one place a run action turns into a change is also the
one place that knows whether it did.

One behaviour changes beyond what the ticket names, and it is the same defect wearing a different
hat: **`run stop` with nothing running is a failure now** rather than a success carrying "Nothing is
running." in its message. `cli_run_do` is one arm for start, stop and rerun, so the fix reaches all
three — and `run remove` on a name nothing holds has always answered `not-found`, so this is the
family agreeing with itself rather than a new opinion. It is written into the catalogue summary,
which is what an agent reads.

### 4.3 A program that cannot be found is caught at `run add`

`run add` accepts `node primes.js` without complaint and the problem only appears at `run start`. An
agent writes `node`, `python` or `cargo`, and those are exactly the programs a version manager keeps
off a Finder-launched application's `PATH`.

So `run add` resolves the program against `PATH` — `run_configurations::found_on_path`, which walks
`PATH` itself with `PATHEXT` on Windows rather than spawning anything — and **says so without
refusing**. It is a note in the reply's message, not a failure: a configuration may name a program
that will exist by the time it is run, and a `run add` that refused would be a `run add` somebody
cannot use to write down what they are about to install. A relative or absolute path is resolved
against the configuration's own directory instead, and a program with no directory separator in it is
looked for on `PATH`.

The distinction matters and is the same one `task-1675` draws about a `Likely` definition: say what
is known, do not pretend to know more, and never silently do nothing.

## 5. `timeout` is accepted on every call and was written down nowhere

### 5.1 What happens

The MCP tool schemas take a free-form `arguments` object described as "the values the command takes,
by the name in its usage line". `timeout` is a global flag of the command line rather than part of
any usage line, so it appears in no tool description — and the agent only found it by reading
`unluminate-cli/src/parse.rs`.

`DEFAULT_TIMEOUT.max(...)` also makes fifteen seconds a **floor**. A caller can raise the deadline
and cannot lower it, so an agent that wants to fail fast cannot.

### 5.2 What was chosen

**Every tool says it.** `instance` is already a property on every tool in both shapes, for the same
reason — it is about the call rather than about the command — so `timeout` joins it there, generated
from the same place. A tool added tomorrow has it without anybody remembering, which is
`mcp::tools`' whole promise.

**An explicit deadline is used as given, unless the command itself waits.** The floor exists because
of a real hazard: `terminal read --wait-for` and `debug start --wait-for-pause` hold the answer open
on purpose, and a transport that gave up first would report a timeout for something about to work.
But that hazard only exists for the commands that take a `timeout` of their own — and the catalogue
already knows which those are, because it is the list the client parses against.

So `Command::flag("timeout")` decides it. A command that waits keeps the stretch it has today; a
command that does not is given exactly the deadline that was asked for, so `timeout: 500` on `tab
list` fails in half a second. The same rule is applied to `unluminate-cli`'s own `client_timeout`, because
`--timeout 500` had the identical floor and the two must not come to different answers about one
argument.

`timeout` is also stripped from the arguments of a command that has no such flag, so what goes on
the wire is exactly what `unluminate-cli` would have sent — the rule `mcp::driver` is built on.

### 5.3 And it was being dropped on the floor

Driving the real server found the rest of it. In the **grouped** shape a tool call names the verb in
`command`, the values in `arguments`, and the window in `instance` — and `tools::resolve` lifts
`instance` off the top level and never did the same for `timeout`. So a call that asked to fail fast
had its deadline discarded before `timeout_for` ever saw it, and waited the whole default: measured
at **15,016 ms** for a call that asked for 800. It is carried across now, and a `timeout` the caller
put among the command's own values wins, because that one is the command's rather than the call's.

### 5.4 The wording is short on purpose

Every word in a property's description is paid once per tool: eighteen times in the default shape and
a hundred and thirty-six times in the other. A first, fuller sentence for `timeout` took the grouped
shape from 43,364 bytes to 49,250; the one that shipped takes it to 47,684, about 1,100 tokens. That
is the trade `Shape::Grouped` exists to make, so it is made deliberately rather than by writing until
the paragraph reads well. `unluminate-cli mcp tools --count` is how it is measured again.

## 6. What is deliberately not here

- **Leaving `eframe`.** §1.3. The channel is fixed from outside the event loop, which is where Unluminate
  sits.
- **Answering commands off the frame loop.** §1.3 and §3. It trades away the property the whole
  module is built on to fix a wake.
- **A last-frame screenshot cache.** §3.
- **Cancelling a command the window has already begun.** §2.2. It cannot be unrun, and the ticket
  asks for the queued case, which is the one that actually happened.
- **The `--wait-for-pause` default-wait mismatch.** §2.4, recorded rather than half-fixed.
- **Refusing a `run add` whose program is not on `PATH`.** §4.3.

## 7. Tests

- `a_request_is_answered_even_when_the_first_wake_is_lost` — the ticket's own ask, in
  `services::control`. The stand-in window ignores the first wake entirely and only drains the queue
  on a later one, which is precisely what egui and eframe do to a request they think is stale. It
  fails on the code as it was, because that code wakes once.
- `a_request_the_caller_gave_up_on_is_marked_abandoned_and_never_run` — the deadline passes with the
  window never draining, and the `Pending` the window eventually takes says it must not be run.
- `the_timeout_says_whether_the_window_is_drawing_or_stopped` — a server that has never drawn a frame
  and one that drew a moment ago say different things.
- `a_deadline_that_arrives_with_the_request_is_read_back` — `Request` round-trips its fourth field,
  and a request without one still parses.
- `a_run_that_could_not_start_is_a_failure_rather_than_a_success` — over `run_cli`, against a
  configuration naming a program that does not exist: `reply.error` is set and its message carries
  the spawn failure.
- `adding_a_configuration_whose_program_is_not_on_the_path_says_so_and_still_adds_it` — the note is
  in the message and the configuration is there.
- `a_caller_that_closed_the_connection_is_never_waited_for` — §2.2's last rule: the request is
  written, the socket is closed with nobody reading, and what the window later picks up says it must
  not be run.
- `a_request_that_sat_on_the_queue_past_the_deadline_is_abandoned_without_being_told` — the derived
  half, for the case where every thread stopped together and nobody set the flag.
- `the_margin_is_a_share_of_the_deadline_up_to_half_a_second` — §2.3.
- `a_command_that_does_not_wait_is_given_exactly_the_deadline_it_asked_for` and
  `a_command_that_waits_still_outlasts_its_own_wait` — the two halves of §5.2, in `mcp::driver` and
  in `unluminate-cli`'s `client_timeout`.
- `every_tool_says_how_to_change_the_deadline` — `timeout` is a property of every tool in both
  shapes, beside `instance`.
- `an_area_tools_timeout_reaches_the_command_it_names` — §5.3, which is what it was not doing.
- `a_program_is_looked_for_on_the_path_and_a_path_is_looked_for_where_it_points` and
  `a_bare_name_on_windows_is_completed_from_pathext` — §4.3, with nothing spawned.

## 8. What was verified against a real window

The ticket measured this on macOS and the work was done on Windows, where the fault does not happen
by itself — Windows delivers a redraw to a covered window, so an idle Unluminate here answers `tab list`
in 74 ms. So the fault was made to happen, twice over, and both shapes were driven live.

`SuspendThread` on the **main thread alone** stops the frame loop and leaves the listener running,
which is the real fault's exact shape:

```
Unluminate did not answer tab.list within 3000 ms. It has not drawn a frame for 21.2 s and 1 request
is queued, so it is not drawing rather than busy. The command was not run.
        answered at 2,756 ms — the window's own refusal, ahead of the client's deadline
```

`NtSuspendProcess` on the **whole process** is the harder shape, where nothing in Unluminate can run at
all and the request is not even accepted until it wakes:

```
run add ghost   ->  timed-out at 2,032 ms
        ... resume ...
run list        ->  0 run configurations          the command was not applied
```

And the other three, through the MCP server over stdio and through `unluminate-cli`:

```
tools/list                        18 tools, 0 without a timeout property
tools/call unluminate_tab timeout 800  754 ms          (the fifteen second floor is gone)
run start ghost                   isError true, "failed: Unluminate could not start
                                  definitely-not-a-real-program: The system cannot find the
                                  file specified. (os error 2)",  exit code 1
run add ghost …                   "Added ghost, but definitely-not-a-real-program could not be
                                  found on this window's PATH. …",  and it was still added
```
