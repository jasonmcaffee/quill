//! The two command-line agents, read from what they really printed.
//!
//! **The ground truth, rather than a hand-written approximation of it.** `agent.rs`'s own tests spell
//! out one event at a time, which is right for saying what a rule *is* and useless for saying whether
//! a real agent's output obeys it: the awkward parts of both streams are the parts nobody would think
//! to write out — Claude Code's two assistant messages in one tool-using turn, its per-message usage
//! under an aggregate `result`, the `rate_limit_event` and `system/status` lines that carry nothing,
//! and Codex's items that arrive twice.
//!
//! `tests/streams/*.jsonl` is `codex exec --json` and `claude -p --output-format stream-json
//! --include-partial-messages` from real runs on a real machine, with that machine's own folders
//! taken out and Claude Code's `system/init` cut to the fields the decoder reads. They are replayed a
//! line at a time, which is how the reader gets them.

use unluminate_chat::agent::Decoder;
use unluminate_chat::model::{Conversation, Message, Role};
use unluminate_chat::provider::Wire;
use unluminate_chat::session::{Session, State};
use unluminate_chat::Reply;

/// The recorded run called `name`, read into replies the way `agent::run` reads a child's output.
fn replies_from(name: &str, wire: Wire) -> (Vec<Reply>, Decoder) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/streams")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} should be there", path.display()));
    let mut decoder = Decoder::new(wire);
    let mut replies = Vec::new();
    for line in text.lines() {
        replies.extend(decoder.line(line.trim()));
    }
    replies.extend(decoder.finish());
    (replies, decoder)
}

/// The same, applied to a session, which is where double counting shows up.
fn session_from(name: &str, wire: Wire) -> Session {
    let (replies, _) = replies_from(name, wire);
    let mut session = Session::new(Conversation::new("c1", "agent"));
    session.ask(Message::said(1, Role::User, "the question that was really asked"));
    for reply in replies {
        session.reply(reply);
    }
    session
}

fn texts(replies: &[Reply]) -> String {
    replies
        .iter()
        .filter_map(|reply| match reply {
            Reply::Text(said) => Some(said.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_recorded_claude_turn_is_read_as_one_answer_with_one_usage() {
    let (replies, decoder) = replies_from("claude-cli.jsonl", Wire::ClaudeCli);
    assert_eq!(
        replies[0],
        Reply::Session("29611139-4d2a-495a-b9a9-94a6189e509c".to_owned())
    );
    assert_eq!(
        replies[1],
        Reply::Started {
            model: "claude-sonnet-5".to_owned()
        }
    );
    assert_eq!(texts(&replies), "hello from the cli.");
    assert!(decoder.ended, "the `result` line ends the turn");

    // **Exactly one usage.** The nested `message_delta` reports what that message cost and the
    // envelope's `result` reports the whole turn; both were taken, so an ordinary one-message answer
    // was banked twice.
    let usages: Vec<&Reply> = replies
        .iter()
        .filter(|reply| matches!(reply, Reply::Usage { .. }))
        .collect();
    assert_eq!(usages.len(), 1, "{replies:?}");

    // And the same read through a `Session`, which is where a double count would land.
    let session = session_from("claude-cli.jsonl", Wire::ClaudeCli);
    assert!(matches!(session.state(), State::Finished { .. }), "{:?}", session.state());
    assert_eq!(session.chat.session, "29611139-4d2a-495a-b9a9-94a6189e509c");
    // The `result` line's own figures, which are the turn's: 9 out, and 2 asked plus 92,677 written
    // to the cache plus 23,732 read back from it.
    assert_eq!(
        session.chat.usage.output, 9,
        "the turn's own figure, once: {:?}",
        session.chat.usage
    );
    assert_eq!(session.chat.usage.input, 2 + 92_677 + 23_732);
    let answer = session.chat.messages.last().expect("an answer");
    assert_eq!(answer.role, Role::Assistant);
    assert_eq!(answer.text(), "hello from the cli.");
}

#[test]
fn a_recorded_claude_tool_turn_shows_the_call_the_agent_ran_and_the_answer_it_got() {
    // **Two assistant messages in one turn**, which is what a tool-using agent does and what no
    // hand-written fixture would have thought to include: the first holds the call, the second holds
    // the words that came after it, and the `user` line between them carries the result the agent got
    // from its own tool. Unluminate ran none of it.
    let session = session_from("claude-cli-tool.jsonl", Wire::ClaudeCli);
    assert!(matches!(session.state(), State::Finished { .. }), "{:?}", session.state());
    let calls: Vec<&unluminate_chat::ToolCall> = session
        .chat
        .messages
        .iter()
        .flat_map(|message| message.tools.iter())
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", session.chat.messages);
    assert_eq!(calls[0].name, "Bash");
    assert!(calls[0].arguments.contains("git rev-parse"), "{}", calls[0].arguments);
    assert_eq!(calls[0].answer.as_deref(), Some("main"));
    assert!(!calls[0].failed);
    assert!(!calls[0].is_running(), "the agent's own answer finished it");

    // The words are the answer and nothing else — the tool's arguments never became text.
    let said: String = session
        .chat
        .messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .map(|message| message.text())
        .collect();
    assert_eq!(said, "You're on the `main` branch.");

    // **Two assistant messages under one `result`, counted once.** The nested `message_delta`s say
    // 12 and 90; the envelope says 102 for the turn, and 102 is what a turn cost.
    assert_eq!(session.chat.usage.output, 102, "{:?}", session.chat.usage);
    assert_eq!(session.chat.usage.input, 4 + 86_248 + 146_752);
}

#[test]
fn a_recorded_codex_turn_is_read_once_though_its_items_arrive_twice() {
    let (replies, decoder) = replies_from("codex-cli.jsonl", Wire::CodexCli);
    assert_eq!(
        replies[0],
        Reply::Session("01a05d36-e5d2-7ce3-97c5-f6af9f332bcd".to_owned())
    );
    assert_eq!(texts(&replies), "hello from the cli.");
    assert!(decoder.ended);

    let session = session_from("codex-cli.jsonl", Wire::CodexCli);
    assert!(matches!(session.state(), State::Finished { .. }));
    assert_eq!(session.chat.messages.last().expect("an answer").text(), "hello from the cli.");
    assert_eq!(session.chat.usage.output, 9);
    assert_eq!(session.chat.usage.input, 21_429 + 11_008, "asked and cached");
}

#[test]
fn a_recorded_codex_tool_turn_draws_each_command_once_with_what_it_printed() {
    // Two commands, each `item.started` then `item.completed`, and three messages between them. The
    // call must be announced once and answered once, or the pane draws it twice.
    let session = session_from("codex-cli-tool.jsonl", Wire::CodexCli);
    assert!(matches!(session.state(), State::Finished { .. }), "{:?}", session.state());
    let calls: Vec<&unluminate_chat::ToolCall> = session
        .chat
        .messages
        .iter()
        .flat_map(|message| message.tools.iter())
        .collect();
    assert_eq!(calls.len(), 2, "{calls:?}");
    for call in &calls {
        assert_eq!(call.name, "shell");
        assert!(call.arguments.contains("rev-parse"), "{}", call.arguments);
        assert!(call.answer.is_some(), "every call the agent ran has its own answer");
        assert!(!call.is_running());
    }
    // Both of them really failed on the machine this was recorded on, and that is drawn as a failure
    // rather than as an answer — `exit_code` is what says so.
    assert!(calls.iter().all(|call| call.failed), "{calls:?}");

    // Its words, in order, with nothing repeated: three `agent_message` items, each arriving whole.
    let said: String = session
        .chat
        .messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .map(|message| message.text())
        .collect();
    assert!(said.starts_with("I\u{2019}ll check the current Git branch."), "{said}");
    assert!(said.ends_with("the sandbox denied command execution."), "{said}");
    assert_eq!(
        said.matches("I\u{2019}ll check the current Git branch.").count(),
        1,
        "an item that arrives twice is shown once: {said}"
    );
}

#[test]
fn an_agent_that_stops_without_its_last_event_says_so_once() {
    // **One failure, not two.** `Decoder::finish` said the stream ended mid-answer and `run` said the
    // program stopped without finishing; both fired, so a `codex` that exited with an error reported
    // an interrupted stream under an exit code. Codex has no inner decoder to finish at all.
    let mut codex = Decoder::new(Wire::CodexCli);
    codex.line(r#"{"type":"thread.started","thread_id":"t-1"}"#);
    codex.line(r#"{"type":"item.completed","item":{"id":"i0","type":"agent_message","text":"half"}}"#);
    assert!(codex.finish().is_empty(), "codex has no nested stream to interrupt");
    assert!(!codex.ended, "and the caller still knows it never finished");

    // Claude Code does, because its events really are a stream that was cut off.
    let mut claude = Decoder::new(Wire::ClaudeCli);
    claude.line(
        r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#,
    );
    claude.line(
        r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"half"}}}"#,
    );
    let ended = claude.finish();
    assert!(
        matches!(&ended[..], [Reply::Failed(said)] if said.contains("ended before the answer did")),
        "{ended:?}"
    );
}

#[test]
fn a_codex_item_that_rewrites_itself_is_not_spliced_into_nonsense() {
    // The snapshots are compared as a **prefix**, not by length. A revision that changes its own
    // beginning has nothing to do with what was already shown, so the whole of it is sent rather
    // than a slice out of its middle — which is what a length alone would have taken.
    let mut decoder = Decoder::new(Wire::CodexCli);
    let first = decoder.line(r#"{"type":"item.updated","item":{"id":"i0","type":"agent_message","text":"the quick"}}"#);
    assert_eq!(first, vec![Reply::Text("the quick".to_owned())]);
    let grown = decoder.line(r#"{"type":"item.updated","item":{"id":"i0","type":"agent_message","text":"the quick brown"}}"#);
    assert_eq!(grown, vec![Reply::Text(" brown".to_owned())]);
    let rewritten =
        decoder.line(r#"{"type":"item.completed","item":{"id":"i0","type":"agent_message","text":"a slow green fox"}}"#);
    assert_eq!(
        rewritten,
        vec![Reply::Text("a slow green fox".to_owned())],
        "the whole of it, not the tail of something else"
    );
    // And a snapshot that says the same thing again says nothing.
    let again =
        decoder.line(r#"{"type":"item.completed","item":{"id":"i0","type":"agent_message","text":"a slow green fox"}}"#);
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn a_tool_answer_lands_on_the_call_that_is_still_running() {
    // An agent should not use one id for two calls, and if it does, filling in the first one twice
    // would leave the second running for ever — a turn that never ends. `session.rs` says the same
    // thing about a server two calls further down.
    let mut session = Session::new(Conversation::new("c1", "agent"));
    session.ask(Message::said(1, Role::User, "run both"));
    session.reply(Reply::ToolCall {
        id: "same".to_owned(),
        name: "shell".to_owned(),
        arguments: "{}".to_owned(),
    });
    session.reply(Reply::ToolCall {
        id: "same".to_owned(),
        name: "shell".to_owned(),
        arguments: "{}".to_owned(),
    });
    session.reply(Reply::ToolAnswer {
        id: "same".to_owned(),
        answer: "first".to_owned(),
        failed: false,
    });
    session.reply(Reply::ToolAnswer {
        id: "same".to_owned(),
        answer: "second".to_owned(),
        failed: false,
    });
    let calls = &session.chat.messages[1].tools;
    assert_eq!(calls.len(), 2);
    assert!(
        calls.iter().all(|call| !call.is_running()),
        "both were answered: {calls:?}"
    );
    let answers: Vec<&str> = calls
        .iter()
        .filter_map(|call| call.answer.as_deref())
        .collect();
    assert_eq!(answers, ["second", "first"], "newest first, which is the order they were filled");
}
