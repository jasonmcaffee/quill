//! The two shapes the unit tests could not spell easily, driven from recorded streams on disk.
//!
//! **A file rather than a byte string literal.** The Responses stream and Anthropic's thinking blocks
//! are JSON inside a JSON string inside a Rust string, and written as a literal every one of the three
//! layers of escaping has to be got right by hand — which is how a test comes to assert something
//! other than what it looks like it asserts. The streams are in `tests/streams/` exactly as a server
//! sends them, so what is being tested is readable, and `sse::Reader` reads them the same way it reads
//! a socket.

use unluminate_chat::model::{Conversation, Message, Role, ToolCall};
use unluminate_chat::provider::{Provider, Wire};
use unluminate_chat::sse;
use unluminate_chat::wire::{self, Decoder, Tool};
use unluminate_chat::Reply;

/// The recorded stream called `name`, read into replies the way the client reads a socket.
///
/// Fed a byte at a time, because that is the property that matters and the one a whole-file read
/// would not check: a socket chooses where a read ends and the framing has to survive it.
fn replies_from(name: &str, wire: Wire) -> Vec<Reply> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/streams")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("{} should be there", path.display()));
    let mut reader = sse::Reader::new();
    let mut decoder = Decoder::new(wire);
    let mut replies = Vec::new();
    for byte in &bytes {
        for event in reader.feed(&[*byte]) {
            replies.extend(decoder.event(&event));
        }
    }
    // `finish` answers the one event a stream that ended mid-frame still had, or nothing -- an
    // `Option` rather than a list, so it is read as one.
    if let Some(event) = reader.finish() {
        replies.extend(decoder.event(&event));
    }
    replies.extend(decoder.finish());
    replies
}

/// An endpoint of `wire`, which is what a person who configured one gets.
///
/// **Built rather than taken from `Provider::defaults`.** The two rows the ticket names run a
/// command line, so the shipped list holds one HTTP row; this file is about the three HTTP request
/// shapes, which are still there for anybody who would rather spend an API key than run an agent.
fn provider(wire: Wire) -> Provider {
    let (url, model) = match wire {
        Wire::Anthropic => ("https://api.anthropic.com/v1/messages", "claude-opus-5"),
        Wire::Responses => ("https://api.openai.com/v1/responses", "gpt-5-codex"),
        _ => ("http://127.0.0.1:8080/v1/chat/completions", "local"),
    };
    Provider {
        name: wire.name().to_owned(),
        wire,
        command: String::new(),
        url: url.to_owned(),
        model: model.to_owned(),
        key_env: String::new(),
        key_entry: String::new(),
        max_tokens: 4096,
    }
}

#[test]
fn the_responses_request_is_a_list_of_items_rather_than_a_list_of_messages() {
    // The shape the Codex models are really served on. A tool call the model made and the answer to
    // it are items in their own right, beside the messages, rather than a field on one message and a
    // message of another role.
    let mut chat = Conversation::new("c1", "codex");
    chat.push(Message::said(1, Role::User, "What does git say?"));
    let mut answering = Message::said(2, Role::Assistant, "Looking.");
    answering.tools.push(ToolCall::new("call_a", "unluminate_git", "{}"));
    chat.push(answering);
    let mut results = Message::new(3, Role::Tool);
    let mut answered = ToolCall::new("call_a", "unluminate_git", "{}");
    answered.answer = Some("clean".to_owned());
    results.tools.push(answered);
    chat.push(results);

    let body = wire::request(&provider(Wire::Responses), &chat, "You are in Unluminate.", &[], true);
    assert_eq!(body["model"], "gpt-5-codex");
    assert_eq!(body["instructions"], "You are in Unluminate.");
    assert_eq!(body["max_output_tokens"], 4096, "its own name for the budget");
    assert!(
        body["max_tokens"].is_null(),
        "the other shape's name is silently ignored here"
    );
    assert_eq!(
        body["store"], false,
        "the transcript is Unluminate's rather than the server's"
    );
    let input = body["input"].as_array().expect("the items");
    assert_eq!(input[0]["type"], "message");
    assert_eq!(input[0]["content"][0]["type"], "input_text");
    assert_eq!(
        input[1]["content"][0]["type"], "output_text",
        "what came back is not input"
    );
    assert_eq!(input[2]["type"], "function_call");
    assert_eq!(input[2]["call_id"], "call_a");
    assert_eq!(input[3]["type"], "function_call_output");
    assert_eq!(input[3]["output"], "clean");

    // A tool is flat here, where the chat-completions shape nests the same three fields.
    let tools = vec![Tool {
        name: "unluminate_git".to_owned(),
        description: "What git says.".to_owned(),
        schema: serde_json::json!({ "type": "object" }),
    }];
    let with_tools = wire::request(&provider(Wire::Responses), &chat, "", &tools, true);
    assert_eq!(with_tools["tools"][0]["name"], "unluminate_git");
    assert!(with_tools["tools"][0]["function"].is_null());
}

#[test]
fn a_responses_stream_reads_text_and_a_tool_call_out_of_its_own_named_events() {
    let replies = replies_from("responses.sse", Wire::Responses);
    assert_eq!(
        replies[0],
        Reply::Started {
            model: "gpt-5-codex".to_owned()
        }
    );
    assert_eq!(replies[1], Reply::Text("The ".to_owned()));
    assert_eq!(replies[2], Reply::Text("answer.".to_owned()));
    assert_eq!(
        replies[3],
        Reply::ToolCall {
            id: "call_a".to_owned(),
            name: "unluminate_git".to_owned(),
            arguments: serde_json::json!({ "command": "status" }).to_string(),
        }
    );
    assert_eq!(
        replies[4],
        Reply::Usage {
            input: 31,
            output: 12
        }
    );
    assert_eq!(
        replies[5],
        Reply::Finished {
            reason: "stop".to_owned()
        }
    );
    assert_eq!(replies.len(), 6, "{replies:?}");
}

#[test]
fn an_anthropic_thinking_block_goes_back_up_exactly_as_it_arrived() {
    // Anthropic verifies the signature it put on the block, so a continuation whose blocks were
    // rebuilt out of the displayed words is refused — and a `redacted_thinking` block is encrypted
    // and cannot be rebuilt at all.
    let replies = replies_from("anthropic-thinking.sse", Wire::Anthropic);
    assert_eq!(replies[0], Reply::Thinking("Let me".to_owned()));
    assert_eq!(replies[1], Reply::Thinking(" see.".to_owned()));
    let Reply::Reasoning(block) = &replies[2] else {
        panic!("{replies:?}");
    };
    assert_eq!(block["type"], "thinking");
    assert_eq!(block["thinking"], "Let me see.");
    assert_eq!(block["signature"], "sig-abc", "the signature survived");
    let Reply::Reasoning(redacted) = &replies[3] else {
        panic!("{replies:?}");
    };
    assert_eq!(redacted["type"], "redacted_thinking");
    assert_eq!(redacted["data"], "opaque");

    // And they are replayed ahead of the words, which is where Anthropic wants them.
    let mut chat = Conversation::new("c1", "claude");
    chat.push(Message::said(1, Role::User, "Why?"));
    let mut answering = Message::said(2, Role::Assistant, "Because.");
    answering.reasoning.push(block.clone());
    answering.reasoning.push(redacted.clone());
    answering.tools.push(ToolCall::new("t1", "unluminate_git", "{}"));
    chat.push(answering);
    let body = wire::request(&provider(Wire::Anthropic), &chat, "", &[], true);
    let blocks = body["messages"][1]["content"].as_array().expect("blocks");
    assert_eq!(blocks[0], *block, "byte for byte, and first");
    assert_eq!(blocks[1], *redacted);
    assert_eq!(blocks[2]["type"], "text");
    assert_eq!(blocks[3]["type"], "tool_use");

    // The Responses shape keeps its reasoning items for the same reason: with `store: false` the
    // server holds no copy to carry on from.
    let items = wire::request(&provider(Wire::Responses), &chat, "", &[], true);
    let input = items["input"].as_array().expect("the items");
    assert_eq!(
        input[1], *block,
        "the reasoning comes before the message it belongs to"
    );
}

#[test]
fn a_stream_whose_lines_end_in_a_lone_carriage_return_is_still_read() {
    // Some servers and some proxies do, and the specification says a blank line is two line endings
    // of any spelling. An earlier version looked for `\n\n` and `\r\n\r\n` only, so a stream like this
    // never framed an event at all and the whole answer arrived as nothing.
    let replies = replies_from("carriage-return.sse", Wire::OpenAi);
    assert_eq!(replies[0], Reply::Text("one".to_owned()));
    assert_eq!(replies[1], Reply::Text("two".to_owned()));
    assert_eq!(
        replies[2],
        Reply::Finished {
            reason: "stop".to_owned()
        }
    );
}
