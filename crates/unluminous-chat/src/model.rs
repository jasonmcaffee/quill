//! What a conversation is: messages, the parts they are made of, and the tool calls in them.
//!
//! Plain values with no behaviour beyond the arithmetic that belongs to them, so a test can build
//! any conversation by hand and `components/agent_chat` can draw one without knowing which server it
//! came from. `wire.rs` turns one of these into a request and reads replies back into one; nothing
//! else in the crate knows the shape of either API.
//!
//! ## A message is a list of parts, not a string
//!
//! Because a message really can hold a picture *and* words, in either order, and because a tool
//! result is a message too. A `String` would have forced the picture to be a second field that most
//! code forgets, which is the shape the ai-service page has (`message.imageUrl` beside
//! `message.messageText`) and the reason a picture there can only ever be one and can only ever be
//! last.

/// Who said it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The person typing.
    User,
    /// The model.
    Assistant,
    /// A tool's answer, going back up. Not drawn as a bubble — it belongs to the call it answers.
    Tool,
}

impl Role {
    /// The word both APIs use for it.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            // Anthropic carries a tool result inside a `user` message and OpenAI has a `tool` role
            // of its own, so this is the OpenAI spelling and `wire.rs` is where the difference lives.
            Self::Tool => "tool",
        }
    }
}

/// One piece of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    /// A picture, held as its own bytes rather than as a path.
    ///
    /// **Bytes rather than a path**, so a conversation reopened after the file it came from has been
    /// moved still shows what was really sent. A path would have made the transcript a promise about
    /// somebody else's disk.
    Picture {
        /// `image/png`, `image/jpeg` — what the request has to declare.
        media: String,
        bytes: Vec<u8>,
        /// What it was called when it was attached, for the tooltip and for `plugins view`.
        name: String,
    },
}

impl Part {
    /// The words in this part, which is empty for a picture.
    pub fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Picture { .. } => "",
        }
    }
}

/// A tool the model asked for, and what it was told.
///
/// The answer is `None` while it is running, which is what the pane draws as a live block and what
/// `Session` looks for when it decides whether a turn is finished.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// The id the API gave it, which is what the result has to be filed under.
    pub id: String,
    pub name: String,
    /// The arguments, as the JSON text the model produced.
    ///
    /// Text rather than a `Value`, because it arrives a fragment at a time and is not valid JSON
    /// until the last fragment. Parsed once, when the block stops.
    pub arguments: String,
    /// What the tool answered, once it has.
    pub answer: Option<String>,
    /// Whether that answer was a refusal, which is drawn differently and is still sent back up.
    pub failed: bool,
    /// How long it took, in milliseconds, once it is finished.
    pub took: Option<u64>,
}

impl ToolCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
            answer: None,
            failed: false,
            took: None,
        }
    }

    /// The arguments as a value, or a sentence saying they are not usable.
    ///
    /// **A refusal rather than an empty object.** A model that emits `{"path": ` and stops — or a
    /// stream cut off in the middle of the fragments — has produced arguments that are not JSON, and
    /// reading them as `{}` runs the command *with its defaults*: `explorer new-file` with no path,
    /// `git` with no verb. Silently doing a different thing is the worst of the three possible
    /// answers, so this says so and the caller sends the sentence back to the model.
    ///
    /// Nothing at all **is** an empty object, because that is what a command taking no arguments is
    /// called with.
    pub fn parsed_arguments(&self) -> Result<serde_json::Value, String> {
        if self.arguments.trim().is_empty() {
            return Ok(serde_json::json!({}));
        }
        serde_json::from_str(&self.arguments).map_err(|problem| {
            format!(
                "`{}` was called with arguments that are not JSON ({problem}): {}",
                self.name,
                a_fragment_of(&self.arguments)
            )
        })
    }

    /// The arguments as a value, with anything unusable read as nothing.
    ///
    /// For **drawing and for replaying up the wire**, where a refusal has nowhere to go: what is put
    /// back must be the JSON the model produced or, failing that, something the API will accept.
    /// [`Self::parsed_arguments`] is what decides whether the call is run.
    pub fn arguments_value(&self) -> serde_json::Value {
        self.parsed_arguments().unwrap_or_else(|_| serde_json::json!({}))
    }

    pub fn is_running(&self) -> bool {
        self.answer.is_none()
    }
}

/// One message.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: u64,
    pub role: Role,
    pub parts: Vec<Part>,
    /// What the model was thinking, when it says. Kept apart from the answer, because it is not one.
    ///
    /// **For reading.** What goes back up the wire is [`Self::reasoning`], not this.
    pub thinking: String,
    /// The reasoning blocks exactly as they arrived, for sending back.
    ///
    /// **Kept whole and replayed untouched, because they are signed.** Anthropic requires every
    /// `thinking` and `redacted_thinking` block of a turn to come back verbatim with a tool result,
    /// signature and all, and refuses a continuation whose blocks were reconstructed — which is what
    /// building them out of [`Self::thinking`] would be. A `redacted_thinking` block is encrypted and
    /// cannot be reconstructed at all.
    ///
    /// Empty in the ordinary case, because Unluminous does not ask for extended thinking. A gateway that
    /// turns it on is the case this exists for: a client that dropped the blocks would work until the
    /// first tool call and then be refused with a 400 nobody could explain.
    pub reasoning: Vec<serde_json::Value>,
    pub tools: Vec<ToolCall>,
    /// Why this message ended, when it did: `stop`, `tool_use`, `length`, `stopped`.
    pub finish: Option<String>,
    /// The server's own words, when the request failed. Never invented — see `unluminous-git`'s rule.
    pub failure: Option<String>,
}

impl Message {
    pub fn new(id: u64, role: Role) -> Self {
        Self {
            id,
            role,
            parts: Vec::new(),
            thinking: String::new(),
            reasoning: Vec::new(),
            tools: Vec::new(),
            finish: None,
            failure: None,
        }
    }

    /// A message that is just words.
    pub fn said(id: u64, role: Role, text: impl Into<String>) -> Self {
        let mut message = Self::new(id, role);
        message.parts.push(Part::Text(text.into()));
        message
    }

    /// Every word in it, run together — which is what a copy button copies and what `last` answers.
    pub fn text(&self) -> String {
        self.parts.iter().map(Part::text).collect::<Vec<&str>>().concat()
    }

    /// Add `delta` to the end of the words, starting a text part if the last one is a picture.
    ///
    /// The one function that grows a streaming message, so appending is a string push rather than a
    /// rebuild — which matters, because it happens once per token.
    pub fn push_text(&mut self, delta: &str) {
        match self.parts.last_mut() {
            Some(Part::Text(text)) => text.push_str(delta),
            _ => self.parts.push(Part::Text(delta.to_owned())),
        }
    }

    pub fn pictures(&self) -> impl Iterator<Item = (&str, &str, &[u8])> {
        self.parts.iter().filter_map(|part| match part {
            Part::Picture { media, bytes, name } => Some((name.as_str(), media.as_str(), bytes.as_slice())),
            Part::Text(_) => None,
        })
    }

    /// Whether the model said anything in this message.
    ///
    /// **What decides whether a message goes back up the wire.** A turn that failed leaves an
    /// assistant message holding only Unluminous's own note about why — and sent back as
    /// `{"role":"assistant","content":""}` that is a message the model never said, which Anthropic
    /// refuses outright and which OpenAI answers oddly. It is drawn, because somebody wants to see
    /// what went wrong; it is not sent, because the model did not say it.
    ///
    /// Found by reading the body a real request put on the wire in the `task-1767` end-to-end run,
    /// where the failed turn before it had come back in the transcript.
    pub fn has_content(&self) -> bool {
        !self.tools.is_empty()
            || self.parts.iter().any(|part| match part {
                Part::Text(text) => !text.trim().is_empty(),
                Part::Picture { .. } => true,
            })
    }

    /// Nothing in it at all, which is a message not worth drawing or sending.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
            && self.failure.is_none()
            && self.thinking.is_empty()
            && self.parts.iter().all(|part| match part {
                Part::Text(text) => text.is_empty(),
                Part::Picture { .. } => false,
            })
    }
}

/// How many tokens a turn cost, as the server reported it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
}

impl Usage {
    pub fn total(self) -> u64 {
        self.input + self.output
    }
}

/// A conversation, which is what the pane shows one of.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    /// Its own id, which is its file name in the plugin's folder.
    pub id: String,
    /// What it is called. The first thing said, cut short, until somebody renames it.
    pub name: String,
    pub messages: Vec<Message>,
    /// The provider it was last spoken to, so reopening it picks the same one back up.
    pub provider: String,
    /// The command-line agent's own session id, so a second question carries the first one on.
    ///
    /// **On the conversation rather than on the provider**, because it is *this* conversation the
    /// agent is holding: starting a new one in the pane has to start a new one in the agent too, and
    /// opening an old one out of the history has to pick that agent session back up. Empty for an
    /// HTTP endpoint, which keeps no session of its own — there the transcript Unluminous holds *is* the
    /// context, and it is sent again every turn.
    pub session: String,
    pub usage: Usage,
    /// When it was last changed, as seconds since the epoch, for ordering the history.
    pub changed: u64,
    /// The next message id. Ids are per conversation and never reused, so a cached render keyed on
    /// one cannot show another message's text.
    next_id: u64,
}

/// How much of the first message becomes the conversation's name.
const NAME_LIMIT: usize = 48;

impl Conversation {
    pub fn new(id: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            messages: Vec::new(),
            provider: provider.into(),
            session: String::new(),
            usage: Usage::default(),
            changed: 0,
            next_id: 1,
        }
    }

    /// The next unused message id.
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Add a message, naming the conversation after the first thing said in it.
    pub fn push(&mut self, message: Message) -> u64 {
        let id = message.id;
        self.next_id = self.next_id.max(id + 1);
        if self.name.is_empty() && message.role == Role::User {
            self.name = shorten(&message.text());
        }
        self.messages.push(message);
        id
    }

    pub fn last(&self) -> Option<&Message> {
        self.messages.last()
    }

    pub fn last_mut(&mut self) -> Option<&mut Message> {
        self.messages.last_mut()
    }

    pub fn message(&self, id: u64) -> Option<&Message> {
        self.messages.iter().find(|message| message.id == id)
    }

    /// The name to show, which is what somebody would call an unnamed conversation.
    pub fn display_name(&self) -> &str {
        match self.name.is_empty() {
            true => "New chat",
            false => &self.name,
        }
    }

    /// This conversation as data, which is what `plugins view agent-chat` prints.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.display_name(),
            "provider": self.provider,
            "session": self.session,
            "changed": self.changed,
            "usage": { "input": self.usage.input, "output": self.usage.output },
            "messages": self.messages.iter().map(message_json).collect::<Vec<serde_json::Value>>(),
        })
    }
}

/// One message as data. Pictures are named and measured rather than printed: a base64 payload in a
/// command line's answer is a screenful of nothing anybody can read.
fn message_json(message: &Message) -> serde_json::Value {
    serde_json::json!({
        "id": message.id,
        "role": message.role.wire_name(),
        "text": message.text(),
        "thinking": message.thinking,
        "finish": message.finish,
        "failure": message.failure,
        "pictures": message
            .pictures()
            .map(|(name, media, bytes)| serde_json::json!({ "name": name, "media": media, "bytes": bytes.len() }))
            .collect::<Vec<serde_json::Value>>(),
        "tools": message
            .tools
            .iter()
            .map(|tool| serde_json::json!({
                "id": tool.id,
                "name": tool.name,
                "arguments": tool.arguments,
                "answer": tool.answer,
                "failed": tool.failed,
                "took_ms": tool.took,
            }))
            .collect::<Vec<serde_json::Value>>(),
    })
}

/// A fragment of `text`, short enough to put in a sentence.
///
/// For quoting back what a model produced when it will not parse. Distinct from [`shorten`], which
/// takes the first *line* because it is naming a conversation.
fn a_fragment_of(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.chars().count() > 120 {
        true => trimmed.chars().take(120).collect::<String>() + "…",
        false => trimmed.to_owned(),
    }
}

/// The first line of `text`, cut to something that fits a header.
///
/// The first **line**, because a message that opens with a heading and then a paragraph should be
/// named after the heading rather than after the two run together.
fn shorten(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or("").trim();
    let mut out = String::new();
    for (count, character) in first.chars().enumerate() {
        if count >= NAME_LIMIT {
            out.push('…');
            break;
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conversation_is_named_after_the_first_thing_said_in_it() {
        let mut chat = Conversation::new("c1", "claude");
        assert_eq!(chat.display_name(), "New chat");
        chat.push(Message::said(
            1,
            Role::User,
            "Why is relayout keeping a paragraph?\nIt should not.",
        ));
        assert_eq!(chat.name, "Why is relayout keeping a paragraph?");
        // And the model's answer does not rename it.
        chat.push(Message::said(2, Role::Assistant, "Because the fingerprint…"));
        assert_eq!(chat.name, "Why is relayout keeping a paragraph?");
    }

    #[test]
    fn a_very_long_first_line_is_cut_rather_than_drawn_off_the_edge() {
        let mut chat = Conversation::new("c1", "claude");
        chat.push(Message::said(1, Role::User, "x".repeat(200)));
        assert_eq!(
            chat.name.chars().count(),
            NAME_LIMIT + 1,
            "cut, with an ellipsis on the end"
        );
        assert!(chat.name.ends_with('…'));
    }

    #[test]
    fn appending_a_delta_grows_the_last_text_part_rather_than_adding_one() {
        // Once per token, so it has to be a push rather than a rebuild — and it must not run a
        // picture and the words after it together into one part.
        let mut message = Message::new(1, Role::Assistant);
        message.push_text("Be");
        message.push_text("cause");
        assert_eq!(message.parts.len(), 1);
        assert_eq!(message.text(), "Because");
        message.parts.push(Part::Picture {
            media: "image/png".to_owned(),
            bytes: vec![1, 2, 3],
            name: "shot.png".to_owned(),
        });
        message.push_text(" and");
        assert_eq!(
            message.parts.len(),
            3,
            "the words after a picture are their own part"
        );
        assert_eq!(message.text(), "Because and");
        assert_eq!(message.pictures().count(), 1);
    }

    #[test]
    fn a_tool_calls_arguments_are_a_value_only_once_they_are_whole() {
        // They arrive a fragment at a time, so half of them is not JSON — and the honest answer to
        // half of them is an empty object rather than a panic.
        let mut call = ToolCall::new("t1", "editor.text", "");
        assert_eq!(call.arguments_value(), serde_json::json!({}));
        call.arguments.push_str("{\"path\":");
        assert_eq!(call.arguments_value(), serde_json::json!({}));
        call.arguments.push_str("\"a.rs\"}");
        assert_eq!(call.arguments_value(), serde_json::json!({ "path": "a.rs" }));
        assert!(call.is_running());
        call.answer = Some("ok".to_owned());
        assert!(!call.is_running());
    }

    #[test]
    fn a_message_holding_only_a_failure_has_nothing_the_model_said_in_it() {
        // Drawn, because somebody wants to see what went wrong; not sent, because the model never
        // said it. `wire` reads this.
        let mut failed = Message::new(1, Role::Assistant);
        failed.failure = Some("HTTP 429".to_owned());
        assert!(!failed.has_content());
        assert!(!failed.is_empty(), "it is still worth drawing");
        failed.push_text("Half an ans");
        assert!(
            failed.has_content(),
            "what did arrive is still what the model said"
        );
        // Whitespace alone is not content either: a model that emitted one newline before failing
        // would otherwise put an empty turn back on the wire.
        let mut blank = Message::said(
            2,
            Role::Assistant,
            "   
 ",
        );
        assert!(!blank.has_content());
        blank.tools.push(ToolCall::new("t", "n", "{}"));
        assert!(blank.has_content(), "a tool call is something it said");
    }

    #[test]
    fn an_empty_message_says_so_and_a_picture_alone_does_not() {
        assert!(Message::new(1, Role::Assistant).is_empty());
        assert!(Message::said(1, Role::User, "").is_empty());
        let mut with_a_picture = Message::new(2, Role::User);
        with_a_picture.parts.push(Part::Picture {
            media: "image/png".to_owned(),
            bytes: vec![0],
            name: "a.png".to_owned(),
        });
        assert!(
            !with_a_picture.is_empty(),
            "a picture with no words is still a message"
        );
    }

    #[test]
    fn ids_are_never_reused_even_when_a_conversation_is_read_back_in() {
        // A cached render is keyed on a message id, so an id that came round again would show one
        // message's text under another's key.
        let mut chat = Conversation::new("c1", "claude");
        chat.push(Message::said(7, Role::User, "hello"));
        assert!(chat.next_id() > 7);
    }
}
