//! The two wire shapes: building a request out of a conversation, and reading a stream back into
//! [`Reply`]s.
//!
//! This is the only file in Unluminate that knows what either API looks like. Everything above it — the
//! session, the provider, the pane — sees the same handful of `Reply` values whichever endpoint
//! answered, which is why `components/agent_chat` has never heard of a `content_block_delta`.
//!
//! ## Both are read into one set of values
//!
//! OpenAI streams one delta an event with the tool call's arguments accumulating as a *string*
//! across many of them, and ends with `data: [DONE]`. Anthropic streams **named** events over
//! **indexed** content blocks, so text and a tool call can interleave and each is accumulated
//! against its own index. They are genuinely different protocols rather than one with two
//! spellings, and pretending otherwise is what makes a client that works against one and mangles
//! the other.
//!
//! ## A refusal is the server's own words
//!
//! `unluminate-git` shells out to the real git and shows what git said, because a rejected push explains
//! itself better than Unluminate could. The same holds here for a 401, a 429, a model that does not
//! exist and a URL with a typo in it, so [`Reply::Failed`] carries what the server wrote and nothing
//! is invented on top of it.

use serde_json::{json, Value};

use crate::base64;
use crate::model::{Conversation, Message, Part, Role};
use crate::provider::{Provider, Wire};
use crate::sse::Event;

/// One thing that happened while an answer was arriving.
///
/// The seam between the protocols and everything else. Deliberately as small and as plain as
/// `unluminate_core::mermaid::Scene`'s items: values with no behaviour, so a session can be driven by
/// hand in a test and a stream can be asserted on without a socket.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// The server accepted it and named the model that is answering.
    Started {
        model: String,
    },
    /// The command-line agent's own session id, which the next turn is resumed with.
    ///
    /// Only a program sends this. It is a reply rather than a value read off the thread afterwards
    /// because the turn may be abandoned half way through and the session is still worth keeping —
    /// the agent has one whether or not Unluminate waited for the answer.
    Session(String),
    /// More words.
    Text(String),
    /// More reasoning, which is not the answer and is not drawn as one.
    Thinking(String),
    /// A whole reasoning block, exactly as it arrived, to be sent back untouched.
    ///
    /// See `Message::reasoning`: these are signed, and a continuation whose blocks were rebuilt out
    /// of the text is refused.
    Reasoning(Value),
    /// A tool the model wants run, complete — its arguments have stopped arriving.
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// A tool the **agent ran itself**, and what it got.
    ///
    /// Only a command-line agent sends this: `claude` and `codex` have their own tools, their own
    /// sandbox and their own permission model, so Unluminate does not run anything on their behalf — what
    /// it does with a tool call is *show* it, and this is the other half of showing it. An HTTP
    /// endpoint never sends one, because there the model asks and Unluminate answers.
    ToolAnswer {
        id: String,
        answer: String,
        failed: bool,
    },
    Usage {
        input: u64,
        output: u64,
    },
    /// The answer is over, and why: `stop`, `tool_use`, `length`.
    Finished {
        reason: String,
    },
    /// It did not work, in the server's own words.
    Failed(String),
}

/// One tool Unluminate offers the model.
///
/// Built from `unluminate-cli`'s catalogue rather than written out, which is the rule
/// `unluminate_cli::mcp::tools` already keeps: a command added to Unluminate is a tool the day it is added.
/// This crate has no catalogue, so it is handed the finished description — name, one line, and a
/// JSON Schema for the arguments — and only knows how to put it in each API's envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// The body to POST for this conversation.
///
/// `tools` empty means none are offered, which is the default and is what `chat.tools` switches.
pub fn request(
    provider: &Provider,
    chat: &Conversation,
    system: &str,
    tools: &[Tool],
    stream: bool,
) -> Value {
    match provider.wire {
        Wire::OpenAi => openai_request(provider, chat, system, tools, stream),
        Wire::Anthropic => anthropic_request(provider, chat, system, tools, stream),
        Wire::Responses => responses_request(provider, chat, system, tools, stream),
        // **A program has no wire request at all**, which is the point of it: `agent::command_line`
        // builds a command line instead, and everything about a body, a header and a URL is
        // irrelevant. An empty object rather than a panic, because a caller that got here has made a
        // mistake and a window that fell over would be a worse way to learn about it.
        Wire::ClaudeCli | Wire::CodexCli => Value::Null,
    }
}

// ---------------------------------------------------------------------------------- OpenAI

fn openai_request(
    provider: &Provider,
    chat: &Conversation,
    system: &str,
    tools: &[Tool],
    stream: bool,
) -> Value {
    let mut messages = Vec::new();
    if !system.trim().is_empty() {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &chat.messages {
        // A turn that failed leaves a message holding only Unluminate's own note about what went wrong.
        // It is drawn and it is not sent — see `Message::has_content`.
        if message.role != Role::Tool && !message.has_content() {
            continue;
        }
        match message.role {
            Role::Tool => {
                // Every tool result is a message of its own here, filed under the id of the call it
                // answers. Anthropic gathers them into one user message instead; that difference is
                // the whole reason the two builders are separate functions.
                for tool in &message.tools {
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool.id,
                        "content": tool.answer.clone().unwrap_or_default(),
                    }));
                }
            }
            Role::User | Role::Assistant => {
                let mut one = json!({ "role": message.role.wire_name(), "content": openai_content(message) });
                if !message.tools.is_empty() && message.role == Role::Assistant {
                    one["tool_calls"] = Value::Array(
                        message
                            .tools
                            .iter()
                            .map(|tool| {
                                json!({
                                    "id": tool.id,
                                    "type": "function",
                                    "function": { "name": tool.name, "arguments": tool.arguments },
                                })
                            })
                            .collect(),
                    );
                }
                messages.push(one);
            }
        }
    }
    let mut body = json!({
        "model": provider.model,
        "max_tokens": provider.max_tokens,
        "messages": messages,
        "stream": stream,
    });
    if stream {
        // Without this the usage is simply absent from a streamed answer, and the context meter
        // would have nothing to draw. Servers that do not know the field ignore it.
        body["stream_options"] = json!({ "include_usage": true });
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.schema,
                        },
                    })
                })
                .collect(),
        );
    }
    body
}

/// A message's content in the OpenAI shape.
///
/// A plain string when it is only words, and an array of parts when there is a picture in it. Both
/// are legal, and the string is what every OpenAI-compatible server on this machine handles —
/// llama.cpp's own server rejects an array of parts on a model with no vision. So the array is used
/// only when there is something in it that needs one.
fn openai_content(message: &Message) -> Value {
    if message.pictures().next().is_none() {
        return Value::String(message.text());
    }
    Value::Array(
        message
            .parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => json!({ "type": "text", "text": text }),
                Part::Picture { media, bytes, .. } => json!({
                    "type": "image_url",
                    "image_url": { "url": base64::to_data_url(media, bytes) },
                }),
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------------- Anthropic

fn anthropic_request(
    provider: &Provider,
    chat: &Conversation,
    system: &str,
    tools: &[Tool],
    stream: bool,
) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    for message in &chat.messages {
        if message.role != Role::Tool && !message.has_content() {
            continue;
        }
        match message.role {
            Role::Tool => {
                // **A tool result is a `user` message here**, and consecutive ones are gathered into
                // one, because the API refuses two user messages in a row. A turn that called three
                // tools therefore sends one user message with three `tool_result` blocks in it.
                let blocks: Vec<Value> = message
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "tool_result",
                            "tool_use_id": tool.id,
                            "content": tool.answer.clone().unwrap_or_default(),
                            "is_error": tool.failed,
                        })
                    })
                    .collect();
                match messages.last_mut() {
                    Some(last)
                        if last["role"] == "user"
                            && last["content"].is_array()
                            && last["content"][0]["type"] == "tool_result" =>
                    {
                        if let Some(array) = last["content"].as_array_mut() {
                            array.extend(blocks);
                        }
                    }
                    _ => messages.push(json!({ "role": "user", "content": blocks })),
                }
            }
            Role::User | Role::Assistant => {
                messages.push(json!({
                    "role": message.role.wire_name(),
                    "content": anthropic_content(message),
                }));
            }
        }
    }
    let mut body = json!({
        "model": provider.model,
        // Required by this API rather than optional, which is why `Provider` always has a number.
        "max_tokens": provider.max_tokens,
        "messages": messages,
        "stream": stream,
    });
    if !system.trim().is_empty() {
        // A field rather than a message, which is the other shape difference that matters.
        body["system"] = Value::String(system.to_owned());
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.schema,
                    })
                })
                .collect(),
        );
    }
    body
}

fn anthropic_content(message: &Message) -> Value {
    // **The reasoning first, exactly as it arrived.** Anthropic wants the thinking blocks of a turn
    // back, in order, ahead of everything else, and verifies the signature it put on each one — so
    // they are replayed as values rather than rebuilt. See `Message::reasoning`.
    let mut blocks: Vec<Value> = message.reasoning.clone();
    blocks.extend(
        message
            .parts
            .iter()
            .filter_map(|part| match part {
                // An empty text block is refused by the API, and a message that was only a picture has
                // one, so it is dropped rather than sent.
                Part::Text(text) if text.is_empty() => None,
                Part::Text(text) => Some(json!({ "type": "text", "text": text })),
                Part::Picture { media, bytes, .. } => Some(json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": media, "data": base64::encode(bytes) },
                })),
            })
            .collect::<Vec<Value>>(),
    );
    for tool in &message.tools {
        blocks.push(json!({
            "type": "tool_use",
            "id": tool.id,
            "name": tool.name,
            "input": tool.arguments_value(),
        }));
    }
    Value::Array(blocks)
}

// ---------------------------------------------------------------------------- OpenAI's Responses API

/// The body to POST to `/v1/responses`.
///
/// **A flat list of items rather than a list of messages**, which is the shape difference that
/// matters: a tool call the model made and the answer to it are items in their own right, beside the
/// messages, rather than a field on one message and a message of another role. The system prompt is
/// `instructions`, a field of its own, as it is for Anthropic.
fn responses_request(
    provider: &Provider,
    chat: &Conversation,
    system: &str,
    tools: &[Tool],
    stream: bool,
) -> Value {
    let mut input = Vec::new();
    for message in &chat.messages {
        match message.role {
            Role::Tool => {
                for tool in &message.tools {
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": tool.id,
                        "output": tool.answer.clone().unwrap_or_default(),
                    }));
                }
            }
            Role::User | Role::Assistant => {
                // The reasoning first, exactly as it arrived, for the reason `anthropic_content`
                // gives: with `store: false` there is no copy on the server to carry on from.
                for block in &message.reasoning {
                    input.push(block.clone());
                }
                if message.has_content() {
                    let content = responses_content(message);
                    if !content.as_array().map(Vec::is_empty).unwrap_or(true) {
                        input.push(json!({
                            "type": "message",
                            "role": message.role.wire_name(),
                            "content": content,
                        }));
                    }
                }
                // The call the model made, replayed as its own item so the output above can name it.
                for tool in &message.tools {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": tool.id,
                        "name": tool.name,
                        "arguments": tool.arguments,
                    }));
                }
            }
        }
    }
    let mut body = json!({
        "model": provider.model,
        // Its own name for the budget. `max_tokens` is silently ignored here, which is the sort of
        // difference that looks like it works until an answer is cut off at the model's own default.
        "max_output_tokens": provider.max_tokens,
        "input": input,
        "stream": stream,
        // Off, because Unluminate has nothing to do with a conversation the server is keeping: the
        // transcript is `Conversation` and it is sent whole every turn, which is what the other two
        // shapes do. Left on, the server would keep a copy of every conversation somebody has here.
        "store": false,
    });
    if !system.trim().is_empty() {
        body["instructions"] = Value::String(system.to_owned());
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| {
                    // Flat, where the chat-completions shape nests the same three fields under
                    // `function`.
                    json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.schema,
                    })
                })
                .collect(),
        );
    }
    body
}

/// A message's content in the Responses shape.
///
/// The part names differ by **who said it**: what goes up is `input_text` and `input_image`, and what
/// came back is `output_text`. Sending an assistant message as `input_text` is refused.
fn responses_content(message: &Message) -> Value {
    let mine = message.role == Role::User;
    Value::Array(
        message
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) if text.trim().is_empty() => None,
                Part::Text(text) => Some(match mine {
                    true => json!({ "type": "input_text", "text": text }),
                    false => json!({ "type": "output_text", "text": text, "annotations": [] }),
                }),
                // A picture only ever goes up, so there is no output form of one to write.
                Part::Picture { media, bytes, .. } => Some(json!({
                    "type": "input_image",
                    "image_url": base64::to_data_url(media, bytes),
                })),
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------------- reading

/// What a stream of events comes to.
///
/// Holds the parts of an answer that arrive in pieces: the tool calls, whose arguments are a string
/// growing a fragment at a time, and which content block is which for the shape that indexes them.
#[derive(Debug)]
pub struct Decoder {
    wire: Wire,
    /// Tool calls being built, by the index the protocol files them under.
    ///
    /// A list of pairs rather than a map, because there are never more than a handful and the order
    /// they were opened in is the order they should be run in.
    building: Vec<(usize, Building)>,
    /// The reasoning blocks being built, by the index the protocol files them under.
    thinking: Vec<(usize, Reasoning)>,
    /// Whether `Finished` has been emitted, so a `[DONE]` after a `finish_reason` does not emit two.
    finished: bool,
    /// Whether the server said the answer failed.
    ///
    /// **A stream that reports an error still ends**, and without this the end was reported as an
    /// ordinary stop: `finish` emitted `Finished { reason: "stop" }` a moment after `Failed`, and the
    /// session took the later one — so a rate limit or an overloaded model came out as a short answer
    /// with no reason attached. The end of a failed stream is not news.
    failed: bool,
    /// Whether `Started` has been emitted.
    ///
    /// **Real OpenAI puts `model` in every chunk**, so without this the answer began once a token. It
    /// is harmless downstream and it is still wrong: a `Reply` stream that repeats its own opening is
    /// one nothing else can be written against.
    started: bool,
}

#[derive(Debug, Default, Clone)]
struct Building {
    id: String,
    name: String,
    arguments: String,
}

/// A reasoning block being built, kept as its own fields so it can be put back together exactly.
///
/// The signature is what makes this worth doing: Anthropic verifies it over the block it sent, so a
/// block rebuilt out of the displayed text is refused. See `Message::reasoning`.
#[derive(Debug, Default, Clone)]
struct Reasoning {
    /// `thinking` or `redacted_thinking`.
    kind: String,
    text: String,
    signature: String,
    /// The opaque payload of a `redacted_thinking` block, which cannot be reconstructed at all.
    data: String,
}

impl Reasoning {
    /// The block as it will go back up, which is the block that came down.
    fn to_value(&self) -> Value {
        match self.kind.as_str() {
            "redacted_thinking" => json!({ "type": "redacted_thinking", "data": self.data }),
            _ => json!({
                "type": "thinking",
                "thinking": self.text,
                "signature": self.signature,
            }),
        }
    }
}

impl Decoder {
    pub fn new(wire: Wire) -> Self {
        Self {
            wire,
            building: Vec::new(),
            thinking: Vec::new(),
            finished: false,
            started: false,
            failed: false,
        }
    }

    /// What this event means, as zero or more replies.
    pub fn event(&mut self, event: &Event) -> Vec<Reply> {
        match self.wire {
            Wire::OpenAi => self.openai_event(event),
            Wire::Anthropic => self.anthropic_event(event),
            Wire::Responses => self.responses_event(event),
            // Read by `agent::Decoder`, which owns one of these for the events `claude` nests inside
            // its own envelope and never hands it one of its own.
            Wire::ClaudeCli | Wire::CodexCli => Vec::new(),
        }
    }

    /// The server said the answer is over: `[DONE]`, `message_stop`, `response.completed`.
    ///
    /// Anything still being built is complete, because the server would not have said this otherwise.
    fn done(&mut self, reason: &str) -> Vec<Reply> {
        if self.failed || self.finished {
            return Vec::new();
        }
        let mut replies = self.flush_tools();
        self.finished = true;
        replies.push(Reply::Finished {
            reason: reason.to_owned(),
        });
        replies
    }

    /// The **socket** closed, which is not the same thing as the server saying it had finished.
    ///
    /// A connection cut in the middle of an answer is a real thing, and the honest report of it is
    /// the text that did arrive and a failure saying the connection ended — not a silent `stop` that
    /// looks exactly like a model choosing to say nothing more. That is what this used to do, and it
    /// was worse than it sounds: it also **flushed a half-built tool call**, whose truncated JSON was
    /// then read as an empty object, so a stream cut in the middle of `{"path":"src/` could run the
    /// command with its defaults instead of refusing.
    pub fn finish(&mut self) -> Vec<Reply> {
        // Whatever was half-built goes, either way: a turn that ended before the server said so holds
        // no complete tool call, and one that already failed has nothing to run.
        self.building.clear();
        self.thinking.clear();
        if self.failed || self.finished {
            return Vec::new();
        }
        self.failed = true;
        vec![Reply::Failed(
            "the connection ended before the answer did. What is above is what arrived.".to_owned(),
        )]
    }

    /// Every tool call that has stopped arriving, in the order they were opened.
    fn flush_tools(&mut self) -> Vec<Reply> {
        let building = std::mem::take(&mut self.building);
        building
            .into_iter()
            .filter(|(_, one)| !one.name.is_empty())
            .map(|(index, one)| Reply::ToolCall {
                // A server that sends no id — llama.cpp's tool support does not always — still gets
                // a distinct one, because the result has to be filed against something.
                id: match one.id.is_empty() {
                    true => format!("call_{index}"),
                    false => one.id,
                },
                name: one.name,
                arguments: one.arguments,
            })
            .collect()
    }

    /// The reasoning block being built at `index`, made if it is not there.
    fn reasoning_slot(&mut self, index: usize, kind: &str) -> &mut Reasoning {
        if !self.thinking.iter().any(|(at, _)| *at == index) {
            self.thinking.push((
                index,
                Reasoning {
                    kind: kind.to_owned(),
                    ..Reasoning::default()
                },
            ));
        }
        self.thinking
            .iter_mut()
            .find(|(at, _)| *at == index)
            .map(|(_, one)| one)
            .expect("just inserted")
    }

    fn slot(&mut self, index: usize) -> &mut Building {
        if !self.building.iter().any(|(at, _)| *at == index) {
            self.building.push((index, Building::default()));
        }
        self.building
            .iter_mut()
            .find(|(at, _)| *at == index)
            .map(|(_, one)| one)
            .expect("just inserted")
    }

    fn openai_event(&mut self, event: &Event) -> Vec<Reply> {
        // `[DONE]` is the **server** saying the answer is over, which is a clean end. The socket
        // closing without it is not, and `finish` is what says so.
        if event.data.trim() == "[DONE]" {
            return self.done("stop");
        }
        let Ok(value) = serde_json::from_str::<Value>(&event.data) else {
            return Vec::new();
        };
        // An error can arrive as an event rather than as a status code, which is what a gateway
        // does when it has already sent its headers.
        if let Some(message) = error_message(&value) {
            self.failed = true;
            return vec![Reply::Failed(message)];
        }
        let mut replies = Vec::new();
        if let Some(model) = value["model"].as_str() {
            if !self.started {
                self.started = true;
                replies.push(Reply::Started {
                    model: model.to_owned(),
                });
            }
        }
        let choice = &value["choices"][0];
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str() {
            if !text.is_empty() {
                replies.push(Reply::Text(text.to_owned()));
            }
        }
        // Two spellings, because llama.cpp and DeepSeek's API use `reasoning_content` and several
        // gateways use `reasoning`. Both mean the same thing and neither is the answer.
        for field in ["reasoning_content", "reasoning"] {
            if let Some(text) = delta[field].as_str() {
                if !text.is_empty() {
                    replies.push(Reply::Thinking(text.to_owned()));
                }
            }
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for call in calls {
                let index = call["index"].as_u64().unwrap_or(0) as usize;
                let slot = self.slot(index);
                if let Some(id) = call["id"].as_str() {
                    slot.id = id.to_owned();
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    slot.name.push_str(name);
                }
                if let Some(fragment) = call["function"]["arguments"].as_str() {
                    slot.arguments.push_str(fragment);
                }
            }
        }
        if let Some(usage) = usage_of(&value["usage"], "prompt_tokens", "completion_tokens") {
            replies.push(usage);
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            // The tool calls are complete once the reason has arrived; there is no per-call stop
            // event in this shape, which is the other reason a decoder has to hold state at all.
            replies.extend(self.flush_tools());
            self.finished = true;
            replies.push(Reply::Finished {
                reason: reason.to_owned(),
            });
        }
        replies
    }

    /// One event of a `/v1/responses` stream.
    ///
    /// **Named events over a flat item list**, which is a third arrangement again: text arrives on
    /// `response.output_text.delta`, a tool call is announced whole on `response.output_item.added`
    /// and its arguments then arrive as fragments, and the usage comes with `response.completed`.
    fn responses_event(&mut self, event: &Event) -> Vec<Reply> {
        let Ok(value) = serde_json::from_str::<Value>(&event.data) else {
            return Vec::new();
        };
        match event.name.as_str() {
            "response.created" | "response.in_progress" => {
                let model = value["response"]["model"].as_str().unwrap_or_default().to_owned();
                match self.started {
                    true => Vec::new(),
                    false => {
                        self.started = true;
                        vec![Reply::Started { model }]
                    }
                }
            }
            "response.output_text.delta" => match value["delta"].as_str() {
                Some(text) if !text.is_empty() => vec![Reply::Text(text.to_owned())],
                _ => Vec::new(),
            },
            // The model's own summary of its reasoning, which is not the answer and is not drawn as
            // one — the same place Anthropic's `thinking_delta` goes.
            "response.reasoning_summary_text.delta" => match value["delta"].as_str() {
                Some(text) if !text.is_empty() => vec![Reply::Thinking(text.to_owned())],
                _ => Vec::new(),
            },
            "response.output_item.added" => {
                let item = &value["item"];
                if item["type"] == "function_call" {
                    // Filed under the **output index**, because that is what the argument fragments
                    // carry; the `call_id` is the model's own and is what the answer has to name.
                    let index = value["output_index"].as_u64().unwrap_or(0) as usize;
                    let slot = self.slot(index);
                    slot.id = item["call_id"].as_str().unwrap_or_default().to_owned();
                    slot.name = item["name"].as_str().unwrap_or_default().to_owned();
                    // A call may arrive with its arguments already whole, which is what a cached
                    // response looks like.
                    if let Some(arguments) = item["arguments"].as_str() {
                        slot.arguments.push_str(arguments);
                    }
                }
                Vec::new()
            }
            "response.function_call_arguments.delta" => {
                let index = value["output_index"].as_u64().unwrap_or(0) as usize;
                let fragment = value["delta"].as_str().unwrap_or_default().to_owned();
                self.slot(index).arguments.push_str(&fragment);
                Vec::new()
            }
            "response.output_item.done" => {
                // **A reasoning item is kept whole and replayed**, for the same reason Anthropic's
                // thinking blocks are: with `store: false` the server keeps nothing, so a continuation
                // that dropped them would ask the model to carry on from work it can no longer see.
                if value["item"]["type"] == "reasoning" {
                    return vec![Reply::Reasoning(value["item"].clone())];
                }
                if value["item"]["type"] != "function_call" {
                    return Vec::new();
                }
                let index = value["output_index"].as_u64().unwrap_or(0) as usize;
                let Some(position) = self.building.iter().position(|(at, _)| *at == index) else {
                    return Vec::new();
                };
                let (_, one) = self.building.remove(position);
                // The finished item carries the whole of the arguments, so it is preferred over what
                // was accumulated: a stream that sent no fragments at all still produces a call.
                let arguments = match value["item"]["arguments"].as_str() {
                    Some(whole) if !whole.is_empty() => whole.to_owned(),
                    _ => one.arguments,
                };
                match one.name.is_empty() {
                    true => Vec::new(),
                    false => vec![Reply::ToolCall {
                        id: one.id,
                        name: one.name,
                        arguments,
                    }],
                }
            }
            "response.completed" | "response.incomplete" => {
                let mut replies = Vec::new();
                if let Some(usage) = usage_of(&value["response"]["usage"], "input_tokens", "output_tokens") {
                    replies.push(usage);
                }
                // `incomplete` carries why it stopped, which is nearly always the token budget.
                let reason = value["response"]["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("stop")
                    .to_owned();
                replies.extend(self.done(&reason));
                replies
            }
            "response.failed" | "error" => {
                self.failed = true;
                let said = error_message(&value["response"])
                    .or_else(|| error_message(&value))
                    .unwrap_or_else(|| event.data.clone());
                vec![Reply::Failed(said)]
            }
            _ => Vec::new(),
        }
    }

    fn anthropic_event(&mut self, event: &Event) -> Vec<Reply> {
        let Ok(value) = serde_json::from_str::<Value>(&event.data) else {
            return Vec::new();
        };
        if event.name == "error" || value["type"] == "error" {
            self.failed = true;
            return vec![Reply::Failed(
                error_message(&value).unwrap_or_else(|| event.data.clone()),
            )];
        }
        let index = value["index"].as_u64().unwrap_or(0) as usize;
        match event.name.as_str() {
            "message_start" => {
                self.started = true;
                let mut replies = vec![Reply::Started {
                    model: value["message"]["model"].as_str().unwrap_or_default().to_owned(),
                }];
                if let Some(usage) = usage_of(&value["message"]["usage"], "input_tokens", "output_tokens") {
                    replies.push(usage);
                }
                replies
            }
            "content_block_start" => {
                let block = &value["content_block"];
                match block["type"].as_str() {
                    Some("tool_use") => {
                        let slot = self.slot(index);
                        slot.id = block["id"].as_str().unwrap_or_default().to_owned();
                        slot.name = block["name"].as_str().unwrap_or_default().to_owned();
                    }
                    Some(kind @ ("thinking" | "redacted_thinking")) => {
                        let data = block["data"].as_str().unwrap_or_default().to_owned();
                        let slot = self.reasoning_slot(index, kind);
                        slot.data = data;
                        // A `thinking` block may open with text already in it, and a
                        // `redacted_thinking` one arrives whole — it has no deltas at all.
                        if let Some(text) = block["thinking"].as_str() {
                            slot.text.push_str(text);
                        }
                    }
                    // A `text` block may open with text already in it, which is what a cached
                    // prefix looks like.
                    _ => {
                        if let Some(text) = block["text"].as_str() {
                            if !text.is_empty() {
                                return vec![Reply::Text(text.to_owned())];
                            }
                        }
                    }
                }
                Vec::new()
            }
            "content_block_delta" => {
                let delta = &value["delta"];
                match delta["type"].as_str() {
                    Some("text_delta") => match delta["text"].as_str() {
                        Some(text) if !text.is_empty() => vec![Reply::Text(text.to_owned())],
                        _ => Vec::new(),
                    },
                    Some("thinking_delta") => match delta["thinking"].as_str() {
                        Some(text) if !text.is_empty() => {
                            self.reasoning_slot(index, "thinking").text.push_str(text);
                            vec![Reply::Thinking(text.to_owned())]
                        }
                        _ => Vec::new(),
                    },
                    // **The signature, which is the whole reason the block is kept.** It arrives after
                    // the text and Anthropic verifies it over the block; a continuation whose blocks
                    // were rebuilt out of the displayed words is refused.
                    Some("signature_delta") => {
                        let signature = delta["signature"].as_str().unwrap_or_default().to_owned();
                        self.reasoning_slot(index, "thinking")
                            .signature
                            .push_str(&signature);
                        Vec::new()
                    }
                    Some("input_json_delta") => {
                        let fragment = delta["partial_json"].as_str().unwrap_or_default().to_owned();
                        self.slot(index).arguments.push_str(&fragment);
                        Vec::new()
                    }
                    _ => Vec::new(),
                }
            }
            "content_block_stop" => {
                // A reasoning block that has stopped is whole, and goes up with the next request.
                if let Some(at) = self.thinking.iter().position(|(at, _)| *at == index) {
                    let (_, one) = self.thinking.remove(at);
                    return vec![Reply::Reasoning(one.to_value())];
                }
                // **Here a tool call really is complete**, one block at a time, which is why this
                // shape can report a call while a later block is still arriving.
                let Some(position) = self.building.iter().position(|(at, _)| *at == index) else {
                    return Vec::new();
                };
                let (_, one) = self.building.remove(position);
                match one.name.is_empty() {
                    true => Vec::new(),
                    false => vec![Reply::ToolCall {
                        id: one.id,
                        name: one.name,
                        arguments: one.arguments,
                    }],
                }
            }
            "message_delta" => {
                let mut replies = Vec::new();
                if let Some(usage) = usage_of(&value["usage"], "input_tokens", "output_tokens") {
                    replies.push(usage);
                }
                if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                    self.finished = true;
                    replies.push(Reply::Finished {
                        reason: reason.to_owned(),
                    });
                }
                replies
            }
            "message_stop" => self.done("stop"),
            _ => Vec::new(),
        }
    }
}

/// A usage report, when the value holds one.
///
/// Anthropic sends its input tokens on `message_start` and its output tokens on `message_delta`, so
/// a report with only one of the two is normal and the session adds them up rather than replacing.
fn usage_of(value: &Value, input: &str, output: &str) -> Option<Reply> {
    if value.is_null() {
        return None;
    }
    let input = value[input].as_u64().unwrap_or(0);
    let output = value[output].as_u64().unwrap_or(0);
    match input == 0 && output == 0 {
        true => None,
        false => Some(Reply::Usage { input, output }),
    }
}

/// The server's own words for what went wrong, out of whichever shape it used.
///
/// Four shapes, because four are really sent: `{"error":{"message":…}}` from both APIs,
/// `{"error":"…"}` from several gateways, `{"message":…}` from a load balancer, and a bare string.
/// Nothing is invented — an unrecognised body is handed back whole by the caller.
pub fn error_message(value: &Value) -> Option<String> {
    if let Some(message) = value["error"]["message"].as_str() {
        let kind = value["error"]["type"].as_str().unwrap_or_default();
        return Some(match kind.is_empty() {
            true => message.to_owned(),
            false => format!("{kind}: {message}"),
        });
    }
    if let Some(message) = value["error"].as_str() {
        return Some(message.to_owned());
    }
    if let Some(message) = value["message"].as_str() {
        // Only when there is nothing else in it that says this is an ordinary reply, or every
        // Anthropic `message_start` would be read as a failure.
        if value.get("choices").is_none() && value.get("type").is_none() && value.get("content").is_none() {
            return Some(message.to_owned());
        }
    }
    None
}

/// A whole, unstreamed answer as the same replies a stream would have produced.
///
/// So `chat.stream = off` — which is what a proxy that will not stream needs — goes through exactly
/// the same session code as a streamed one, rather than being a second path that agrees today.
pub fn whole(wire: Wire, body: &str) -> Vec<Reply> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return vec![Reply::Failed(body.to_owned())];
    };
    if let Some(message) = error_message(&value) {
        return vec![Reply::Failed(message)];
    }
    let mut replies = Vec::new();
    match wire {
        // A program is always streamed — there is no second, whole-answer mode to fall back to,
        // because its answer arrives as lines whether or not anybody wanted them one at a time.
        Wire::ClaudeCli | Wire::CodexCli => {}
        Wire::OpenAi => {
            if let Some(model) = value["model"].as_str() {
                replies.push(Reply::Started {
                    model: model.to_owned(),
                });
            }
            let choice = &value["choices"][0];
            for field in ["reasoning_content", "reasoning"] {
                if let Some(text) = choice["message"][field].as_str() {
                    if !text.is_empty() {
                        replies.push(Reply::Thinking(text.to_owned()));
                    }
                }
            }
            if let Some(text) = choice["message"]["content"].as_str() {
                if !text.is_empty() {
                    replies.push(Reply::Text(text.to_owned()));
                }
            }
            if let Some(calls) = choice["message"]["tool_calls"].as_array() {
                for (index, call) in calls.iter().enumerate() {
                    replies.push(Reply::ToolCall {
                        id: call["id"].as_str().unwrap_or(&format!("call_{index}")).to_owned(),
                        name: call["function"]["name"].as_str().unwrap_or_default().to_owned(),
                        arguments: call["function"]["arguments"].as_str().unwrap_or("{}").to_owned(),
                    });
                }
            }
            if let Some(usage) = usage_of(&value["usage"], "prompt_tokens", "completion_tokens") {
                replies.push(usage);
            }
            replies.push(Reply::Finished {
                reason: choice["finish_reason"].as_str().unwrap_or("stop").to_owned(),
            });
        }
        Wire::Responses => {
            if let Some(model) = value["model"].as_str() {
                replies.push(Reply::Started {
                    model: model.to_owned(),
                });
            }
            // A flat list of items rather than a message with fields on it, which is the same
            // difference the streamed form has.
            for item in value["output"].as_array().map(Vec::as_slice).unwrap_or_default() {
                match item["type"].as_str() {
                    Some("message") => {
                        for part in item["content"].as_array().map(Vec::as_slice).unwrap_or_default() {
                            if let Some(text) = part["text"].as_str() {
                                if !text.is_empty() {
                                    replies.push(Reply::Text(text.to_owned()));
                                }
                            }
                        }
                    }
                    Some("function_call") => replies.push(Reply::ToolCall {
                        id: item["call_id"].as_str().unwrap_or_default().to_owned(),
                        name: item["name"].as_str().unwrap_or_default().to_owned(),
                        arguments: item["arguments"].as_str().unwrap_or("{}").to_owned(),
                    }),
                    Some("reasoning") => {
                        for part in item["summary"].as_array().map(Vec::as_slice).unwrap_or_default() {
                            if let Some(text) = part["text"].as_str() {
                                replies.push(Reply::Thinking(text.to_owned()));
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(usage) = usage_of(&value["usage"], "input_tokens", "output_tokens") {
                replies.push(usage);
            }
            replies.push(Reply::Finished {
                reason: value["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("stop")
                    .to_owned(),
            });
        }
        Wire::Anthropic => {
            if let Some(model) = value["model"].as_str() {
                replies.push(Reply::Started {
                    model: model.to_owned(),
                });
            }
            for block in value["content"].as_array().map(Vec::as_slice).unwrap_or_default() {
                match block["type"].as_str() {
                    Some("text") => {
                        if let Some(text) = block["text"].as_str() {
                            replies.push(Reply::Text(text.to_owned()));
                        }
                    }
                    Some("thinking") => {
                        if let Some(text) = block["thinking"].as_str() {
                            replies.push(Reply::Thinking(text.to_owned()));
                        }
                    }
                    Some("tool_use") => replies.push(Reply::ToolCall {
                        id: block["id"].as_str().unwrap_or_default().to_owned(),
                        name: block["name"].as_str().unwrap_or_default().to_owned(),
                        arguments: block["input"].to_string(),
                    }),
                    _ => {}
                }
            }
            if let Some(usage) = usage_of(&value["usage"], "input_tokens", "output_tokens") {
                replies.push(usage);
            }
            replies.push(Reply::Finished {
                reason: value["stop_reason"].as_str().unwrap_or("stop").to_owned(),
            });
        }
    }
    replies
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToolCall;

    fn events(stream: &[u8]) -> Vec<Event> {
        let mut reader = crate::sse::Reader::new();
        let mut all = reader.feed(stream);
        all.extend(reader.finish());
        all
    }

    fn decode(wire: Wire, stream: &[u8]) -> Vec<Reply> {
        let mut decoder = Decoder::new(wire);
        let mut replies = Vec::new();
        for event in events(stream) {
            replies.extend(decoder.event(&event));
        }
        replies
    }

    /// An endpoint of each shape, so a test says which one it is about.
    ///
    /// **Built rather than looked up in `Provider::defaults`.** The two rows the ticket names run a
    /// command line rather than sending to an address, so the shipped list holds one HTTP row and
    /// this file is about the three HTTP shapes — an endpoint pointed at Anthropic's API is
    /// something a person can still configure, and this is what it looks like when they have.
    fn endpoint(wire: Wire, url: &str, model: &str) -> Provider {
        Provider {
            name: wire.name().to_owned(),
            wire,
            command: String::new(),
            url: url.to_owned(),
            model: model.to_owned(),
            key_env: String::new(),
            key_entry: String::new(),
            max_tokens: crate::provider::DEFAULT_MAX_TOKENS,
        }
    }
    fn openai() -> Provider {
        Provider::defaults()
            .into_iter()
            .find(|one| one.wire == Wire::OpenAi)
            .expect("an openai row")
    }
    fn anthropic() -> Provider {
        endpoint(
            Wire::Anthropic,
            "https://api.anthropic.com/v1/messages",
            "claude-opus-5",
        )
    }
    /// The Responses shape's own endpoint, for a test that wants one.
    ///
    /// Kept beside its two siblings even while nothing reads it: the three together are what say the
    /// module has three address shapes, and a helper deleted for being unused is one the next test
    /// has to write again. `task-1804` cleared the four warnings this file had and left this one on
    /// purpose, which is why it says so.
    #[allow(dead_code)]
    fn responses() -> Provider {
        endpoint(
            Wire::Responses,
            "https://api.openai.com/v1/responses",
            "gpt-5-codex",
        )
    }

    fn a_chat() -> Conversation {
        let mut chat = Conversation::new("c1", "claude");
        chat.push(Message::said(1, Role::User, "Why?"));
        chat
    }

    #[test]
    fn the_openai_request_is_what_that_api_documents() {
        let provider = openai();
        let body = request(&provider, &a_chat(), "You are in Unluminate.", &[], true);
        assert_eq!(body["model"], "local");
        assert_eq!(body["stream"], true);
        assert_eq!(
            body["stream_options"]["include_usage"], true,
            "or a streamed answer reports no usage"
        );
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "You are in Unluminate.");
        assert_eq!(body["messages"][1]["role"], "user");
        // A plain string rather than an array of parts, because llama.cpp's server refuses an array
        // on a model with no vision and most messages have no picture in them.
        assert_eq!(body["messages"][1]["content"], "Why?");
        assert!(body["tools"].is_null(), "no tools unless some are offered");
    }

    #[test]
    fn the_anthropic_request_puts_the_system_prompt_in_a_field_of_its_own() {
        let provider = anthropic();
        let body = request(&provider, &a_chat(), "You are in Unluminate.", &[], true);
        assert_eq!(body["system"], "You are in Unluminate.");
        assert_eq!(
            body["messages"].as_array().expect("messages").len(),
            1,
            "the system prompt is not a message"
        );
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][0]["text"], "Why?");
        // Required by this API rather than optional, which is why a provider always carries one.
        assert_eq!(body["max_tokens"], 4096);
    }

    #[test]
    fn a_picture_goes_up_in_each_apis_own_envelope() {
        let mut chat = Conversation::new("c1", "claude");
        let mut message = Message::said(1, Role::User, "What is this?");
        message.parts.push(Part::Picture {
            media: "image/png".to_owned(),
            bytes: vec![1, 2, 3],
            name: "shot.png".to_owned(),
        });
        chat.push(message);

        let openai = request(&openai(), &chat, "", &[], true);
        let parts = openai["messages"][0]["content"]
            .as_array()
            .expect("an array once there is a picture");
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,AQID");

        let anthropic = request(&anthropic(), &chat, "", &[], true);
        let blocks = anthropic["messages"][0]["content"].as_array().expect("blocks");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["type"], "base64");
        assert_eq!(blocks[1]["source"]["media_type"], "image/png");
        assert_eq!(blocks[1]["source"]["data"], "AQID");
    }

    #[test]
    fn a_turn_that_called_two_tools_is_sent_back_the_way_each_api_wants_it() {
        // The one place the two shapes differ in a way that breaks a request rather than looking
        // odd: Anthropic refuses two user messages in a row, so two tool results have to be
        // gathered into one message, and OpenAI wants them as two `tool` messages.
        let mut chat = Conversation::new("c1", "x");
        chat.push(Message::said(1, Role::User, "Do it"));
        let mut answer = Message::new(2, Role::Assistant);
        answer.push_text("Right.");
        answer.tools.push(ToolCall::new("t1", "git.status", "{}"));
        answer
            .tools
            .push(ToolCall::new("t2", "editor.text", "{\"path\":\"a.rs\"}"));
        chat.push(answer);
        let mut results = Message::new(3, Role::Tool);
        let mut first = ToolCall::new("t1", "git.status", "{}");
        first.answer = Some("clean".to_owned());
        let mut second = ToolCall::new("t2", "editor.text", "{}");
        second.answer = Some("no such file".to_owned());
        second.failed = true;
        results.tools.push(first);
        results.tools.push(second);
        chat.push(results);

        let openai = request(&openai(), &chat, "", &[], true);
        let messages = openai["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 4, "user, assistant, and one tool message each");
        assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "git.status");
        assert_eq!(
            messages[1]["tool_calls"][1]["function"]["arguments"],
            "{\"path\":\"a.rs\"}"
        );
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "t1");
        assert_eq!(messages[3]["content"], "no such file");

        let anthropic = request(&anthropic(), &chat, "", &[], true);
        let messages = anthropic["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3, "the two results are one user message");
        assert_eq!(messages[1]["content"][1]["type"], "tool_use");
        assert_eq!(messages[1]["content"][1]["input"], json!({}));
        assert_eq!(messages[2]["role"], "user");
        let blocks = messages[2]["content"].as_array().expect("blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["tool_use_id"], "t1");
        assert_eq!(blocks[1]["is_error"], true);
    }

    #[test]
    fn a_turn_that_failed_is_drawn_and_not_sent_back_up() {
        // Measured on a real request in the `task-1767` end-to-end run: the failed turn before it came
        // back as `{"role":"assistant","content":""}`, which is a message the model never said.
        let mut chat = Conversation::new("c1", "x");
        chat.push(Message::said(1, Role::User, "Are you there?"));
        let mut failed = Message::new(2, Role::Assistant);
        failed.failure = Some("Peer disconnected".to_owned());
        chat.push(failed);
        chat.push(Message::said(3, Role::User, "Are you there now?"));

        let openai = request(&openai(), &chat, "", &[], true);
        let roles: Vec<&str> = openai["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .filter_map(|one| one["role"].as_str())
            .collect();
        assert_eq!(roles, ["user", "user"], "the failed turn is not on the wire");
        let anthropic = request(&anthropic(), &chat, "", &[], true);
        assert_eq!(anthropic["messages"].as_array().expect("messages").len(), 2);
    }

    #[test]
    fn tools_go_in_each_apis_own_envelope() {
        let tools = vec![Tool {
            name: "git_status".to_owned(),
            description: "What git says.".to_owned(),
            schema: json!({ "type": "object", "properties": {} }),
        }];
        let openai = request(&openai(), &a_chat(), "", &tools, true);
        assert_eq!(openai["tools"][0]["type"], "function");
        assert_eq!(openai["tools"][0]["function"]["name"], "git_status");
        assert_eq!(openai["tools"][0]["function"]["parameters"]["type"], "object");
        let anthropic = request(&anthropic(), &a_chat(), "", &tools, true);
        assert_eq!(anthropic["tools"][0]["name"], "git_status");
        assert_eq!(anthropic["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn an_openai_stream_reads_into_words_a_usage_and_a_finish() {
        let stream = b"data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"content\":\"He\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":2}}\n\n\
data: [DONE]\n\n";
        let replies = decode(Wire::OpenAi, stream);
        assert_eq!(
            replies[0],
            Reply::Started {
                model: "gpt-5".to_owned()
            }
        );
        assert_eq!(replies[1], Reply::Text("He".to_owned()));
        assert_eq!(replies[2], Reply::Text("llo".to_owned()));
        assert_eq!(
            replies[3],
            Reply::Finished {
                reason: "stop".to_owned()
            }
        );
        assert_eq!(replies[4], Reply::Usage { input: 9, output: 2 });
        // `[DONE]` after a `finish_reason` must not produce a second finish, or the session would
        // end a turn twice.
        assert_eq!(replies.len(), 5, "{replies:?}");
    }

    #[test]
    fn a_stream_that_names_its_model_in_every_chunk_begins_once() {
        // Real OpenAI puts `model` in every chunk, so without the flag the answer began once a token.
        let stream = b"data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n\
data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\n\
data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let replies = decode(Wire::OpenAi, stream);
        let began = replies
            .iter()
            .filter(|reply| matches!(reply, Reply::Started { .. }))
            .count();
        assert_eq!(began, 1, "{replies:?}");
        assert_eq!(
            replies[0],
            Reply::Started {
                model: "gpt-5".to_owned()
            }
        );
    }

    #[test]
    fn an_openai_tool_call_is_whole_only_once_the_finish_reason_arrives() {
        // Its arguments come as a string in fragments across many deltas and there is no per-call
        // stop event, which is the whole reason the decoder holds state.
        let stream = b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"function\":{\"name\":\"editor_text\",\"arguments\":\"\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"pa\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"a.rs\\\"}\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n\
data: [DONE]\n\n";
        let replies = decode(Wire::OpenAi, stream);
        assert_eq!(
            replies[0],
            Reply::ToolCall {
                id: "call_a".to_owned(),
                name: "editor_text".to_owned(),
                arguments: "{\"path\":\"a.rs\"}".to_owned(),
            }
        );
        assert_eq!(
            replies[1],
            Reply::Finished {
                reason: "tool_calls".to_owned()
            }
        );
    }

    #[test]
    fn an_anthropic_stream_reads_interleaved_text_and_a_tool_call_by_block() {
        // The property this shape has and the other has not: two blocks arrive interleaved and each
        // is accumulated against its own index, so a call is complete while text is still coming.
        let stream = b"event: message_start\ndata: {\"message\":{\"model\":\"claude-opus-5\",\"usage\":{\"input_tokens\":11,\"output_tokens\":1}}}\n\n\
event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Looking\"}}\n\n\
event: content_block_start\ndata: {\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"git_status\"}}\n\n\
event: content_block_delta\ndata: {\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" now\"}}\n\n\
event: content_block_stop\ndata: {\"index\":1}\n\n\
event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":24}}\n\n\
event: message_stop\ndata: {}\n\n";
        let replies = decode(Wire::Anthropic, stream);
        assert_eq!(
            replies[0],
            Reply::Started {
                model: "claude-opus-5".to_owned()
            }
        );
        assert_eq!(replies[1], Reply::Usage { input: 11, output: 1 });
        assert_eq!(replies[2], Reply::Text("Looking".to_owned()));
        assert_eq!(replies[3], Reply::Text(" now".to_owned()));
        assert_eq!(
            replies[4],
            Reply::ToolCall {
                id: "toolu_1".to_owned(),
                name: "git_status".to_owned(),
                arguments: "{}".to_owned()
            }
        );
        assert_eq!(replies[5], Reply::Usage { input: 0, output: 24 });
        assert_eq!(
            replies[6],
            Reply::Finished {
                reason: "tool_use".to_owned()
            }
        );
        assert_eq!(replies.len(), 7, "{replies:?}");
    }

    #[test]
    fn anthropic_thinking_is_kept_apart_from_the_answer() {
        let stream = b"event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"thinking\"}}\n\n\
event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}\n\n\
event: content_block_delta\ndata: {\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"yes\"}}\n\n";
        let replies = decode(Wire::Anthropic, stream);
        assert_eq!(replies[0], Reply::Thinking("hmm".to_owned()));
        assert_eq!(replies[1], Reply::Text("yes".to_owned()));
    }

    #[test]
    fn a_refusal_that_arrives_as_an_event_is_the_servers_own_words() {
        let anthropic = decode(
            Wire::Anthropic,
            b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        );
        assert_eq!(
            anthropic[0],
            Reply::Failed("overloaded_error: Overloaded".to_owned())
        );
        let openai = decode(
            Wire::OpenAi,
            b"data: {\"error\":{\"message\":\"model not found\",\"type\":\"invalid_request_error\"}}\n\n",
        );
        assert_eq!(
            openai[0],
            Reply::Failed("invalid_request_error: model not found".to_owned())
        );
        // A gateway that sends a bare string is read too, and nothing is put in front of it.
        let gateway = decode(Wire::OpenAi, b"data: {\"error\":\"upstream timed out\"}\n\n");
        assert_eq!(gateway[0], Reply::Failed("upstream timed out".to_owned()));
    }

    #[test]
    fn a_stream_that_reported_an_error_does_not_then_report_an_ordinary_end() {
        // The stream still closes after the error, and `finish` used to call that a stop — which the
        // session took, so a rate limit came out as a short answer with nothing to say why.
        let mut decoder = Decoder::new(Wire::Anthropic);
        let mut replies = Vec::new();
        for event in events(
            b"event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Half\"}}\n\n\
event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        ) {
            replies.extend(decoder.event(&event));
        }
        replies.extend(decoder.finish());
        assert_eq!(replies[0], Reply::Text("Half".to_owned()));
        assert_eq!(
            replies[1],
            Reply::Failed("overloaded_error: Overloaded".to_owned())
        );
        assert_eq!(replies.len(), 2, "nothing after the failure: {replies:?}");
    }

    #[test]
    fn a_stream_cut_off_mid_answer_is_a_failure_rather_than_an_ordinary_stop() {
        // A connection dropped after two tokens is a real thing, and calling it a stop looks exactly
        // like a model choosing to say nothing more. Worse, it used to **flush a half-built tool
        // call**, whose truncated JSON was then read as an empty object — so a cut in the middle of a
        // path could run the command with its defaults.
        let mut decoder = Decoder::new(Wire::OpenAi);
        let mut replies = Vec::new();
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"half\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"unluminate_editor\",\"arguments\":\"{\\\"pa\"}}]}}]}\n\n",
        );
        for event in events(stream.as_bytes()) {
            replies.extend(decoder.event(&event));
        }
        replies.extend(decoder.finish());
        assert_eq!(replies[0], Reply::Text("half".to_owned()));
        assert!(
            matches!(&replies[1], Reply::Failed(said) if said.contains("connection ended")),
            "{replies:?}"
        );
        assert_eq!(
            replies.len(),
            2,
            "the half-built call is not offered: {replies:?}"
        );
    }

    #[test]
    fn an_unstreamed_answer_produces_the_same_replies_a_stream_would_have() {
        // Which is what makes `chat.stream = off` the same code path rather than a second one that
        // agrees today.
        let openai = whole(
            Wire::OpenAi,
            "{\"model\":\"m\",\"choices\":[{\"message\":{\"content\":\"Hello\",\"tool_calls\":[{\"id\":\"c1\",\"function\":{\"name\":\"n\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":4}}",
        );
        assert_eq!(
            openai[0],
            Reply::Started {
                model: "m".to_owned()
            }
        );
        assert_eq!(openai[1], Reply::Text("Hello".to_owned()));
        assert_eq!(
            openai[2],
            Reply::ToolCall {
                id: "c1".to_owned(),
                name: "n".to_owned(),
                arguments: "{}".to_owned()
            }
        );
        assert_eq!(openai[3], Reply::Usage { input: 3, output: 4 });
        assert_eq!(
            openai[4],
            Reply::Finished {
                reason: "tool_calls".to_owned()
            }
        );

        let anthropic = whole(
            Wire::Anthropic,
            "{\"model\":\"c\",\"content\":[{\"type\":\"text\",\"text\":\"Hi\"},{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"n\",\"input\":{\"a\":1}}],\"stop_reason\":\"tool_use\",\"usage\":{\"input_tokens\":2,\"output_tokens\":9}}",
        );
        assert_eq!(anthropic[1], Reply::Text("Hi".to_owned()));
        assert_eq!(
            anthropic[2],
            Reply::ToolCall {
                id: "t".to_owned(),
                name: "n".to_owned(),
                arguments: "{\"a\":1}".to_owned()
            }
        );
        assert_eq!(
            anthropic[4],
            Reply::Finished {
                reason: "tool_use".to_owned()
            }
        );

        // And a body that is not JSON at all is handed back whole rather than swallowed, because a
        // proxy's HTML error page is still the most useful thing anybody could be shown.
        let rubbish = whole(Wire::OpenAi, "<html>502 Bad Gateway</html>");
        assert_eq!(
            rubbish[0],
            Reply::Failed("<html>502 Bad Gateway</html>".to_owned())
        );
    }

    #[test]
    fn a_llama_cpp_tool_call_with_no_id_still_gets_one() {
        // Because the result has to be filed against something, and several OpenAI-compatible
        // servers send no id at all.
        let stream = b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"n\",\"arguments\":\"{}\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n";
        let replies = decode(Wire::OpenAi, stream);
        assert!(matches!(&replies[0], Reply::ToolCall { id, .. } if !id.is_empty()));
    }
}
