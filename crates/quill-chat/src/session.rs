//! What a [`Reply`] does to a conversation.
//!
//! One state machine, driven by values, with no socket in it — so every awkward case can be built by
//! hand in a test: an answer that arrives in one chunk, one that stops halfway, one that calls two
//! tools, one that fails after saying something. `quill_dap::session` is arranged the same way and
//! for the same reason.
//!
//! ## The turn is the unit, not the message
//!
//! A model that calls a tool has not finished answering: the tool is run, its result goes back up,
//! and the model carries on in a **new** assistant message. So a turn is one or more messages, and
//! [`State::WaitingForTools`] is the state between them. [`Session::round`] counts them, and the
//! caller stops at a limit rather than funding a loop nobody is watching.

use crate::model::{Conversation, Message, Role, ToolCall, Usage};
use crate::wire::Reply;

/// What the session is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing is in flight.
    Idle,
    /// A request has gone and nothing has come back yet.
    Sending,
    /// Words are arriving.
    Streaming,
    /// The model asked for tools and is waiting for their answers.
    WaitingForTools,
    /// The turn ended, and why.
    Finished { reason: String },
    /// It did not work, in the server's own words.
    Failed(String),
}

impl State {
    /// Whether something is still happening, which is what keeps the window drawing.
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Sending | Self::Streaming | Self::WaitingForTools)
    }

    /// The one word `plugins run agent-chat state` answers with.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Sending => "sending",
            Self::Streaming => "streaming",
            Self::WaitingForTools => "waiting-for-tools",
            Self::Finished { .. } => "finished",
            Self::Failed(_) => "failed",
        }
    }
}

/// A conversation and what is happening to it.
#[derive(Debug)]
pub struct Session {
    pub chat: Conversation,
    state: State,
    /// Which message the answer is being written into, while one is arriving.
    answering: Option<u64>,
    /// The model that answered, as the server named it, which may not be what was asked for.
    pub model: String,
    /// How many times the model has been asked in this turn, so a tool loop can be bounded.
    round: u32,
    /// What this **message** has cost, as the server has reported it so far.
    ///
    /// Held apart from the conversation's total because Anthropic's reports are cumulative for the
    /// message rather than incremental: `message_start` says the input tokens and one output token,
    /// and `message_delta` says the whole output again. Added, a turn's output came out one too many
    /// every time. The largest report wins here, and the message's final figure is added to the
    /// conversation when the message ends.
    turn: Usage,
}

impl Session {
    pub fn new(chat: Conversation) -> Self {
        Self {
            chat,
            state: State::Idle,
            answering: None,
            model: String::new(),
            round: 0,
            turn: Usage::default(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn is_busy(&self) -> bool {
        self.state.is_busy()
    }

    /// Add what somebody typed and mark the turn as started.
    ///
    /// Returns the message's id, which is what `plugins run agent-chat send` answers with.
    pub fn ask(&mut self, message: Message) -> u64 {
        let id = self.chat.next_id().max(message.id);
        let mut message = message;
        message.id = id;
        self.chat.push(message);
        self.round = 0;
        self.begin();
        id
    }

    /// A request has gone out, for this turn or for the round after a tool.
    pub fn begin(&mut self) {
        self.state = State::Sending;
        self.answering = None;
        self.round += 1;
    }

    /// Act on one reply.
    pub fn reply(&mut self, reply: Reply) {
        match reply {
            Reply::Started { model } => {
                if !model.is_empty() {
                    self.model = model;
                }
                self.state = State::Streaming;
            }
            Reply::Text(delta) => {
                self.state = State::Streaming;
                self.answer().push_text(&delta);
            }
            Reply::Thinking(delta) => {
                self.state = State::Streaming;
                self.answer().thinking.push_str(&delta);
            }
            // Kept beside the words rather than instead of them: one is what a person reads and the
            // other is what goes back up. See `Message::reasoning`.
            Reply::Reasoning(block) => self.answer().reasoning.push(block),
            Reply::ToolCall { id, name, arguments } => {
                self.answer().tools.push(ToolCall::new(id, name, arguments));
            }
            // **The agent's own session, kept on the conversation**, so a second question carries
            // the first one on rather than starting the agent again with no memory of it.
            Reply::Session(session) => self.chat.session = session,
            // **A tool the agent ran itself.** Quill did not run it and has nothing to send back: it
            // is filled in on the call that is already showing, which is what turns the running block
            // in the pane into a finished one. A call this conversation has never heard of is
            // ignored rather than invented, because a block with no call above it would be a report
            // of something nobody asked for.
            Reply::ToolAnswer { id, answer, failed } => {
                self.state = State::Streaming;
                for message in self.chat.messages.iter_mut().rev() {
                    if let Some(call) = message.tools.iter_mut().find(|call| call.id == id) {
                        call.answer = Some(answer);
                        call.failed = failed;
                        break;
                    }
                }
            }
            Reply::Usage { input, output } => {
                // **The largest report of a turn wins, and the turns are added up.**
                //
                // Anthropic reports the input tokens on `message_start` and the output tokens twice —
                // once as `1` on `message_start` and again as the real total on `message_delta`, both
                // **cumulative for that message**. Adding every report made a turn's output 1 too many
                // every time. OpenAI reports once at the end, which the same rule handles: one report
                // is trivially the largest.
                self.turn.input = self.turn.input.max(input);
                self.turn.output = self.turn.output.max(output);
            }
            // **A turn that failed stays failed.** A stream that reports an error still ends, and the
            // end used to arrive as an ordinary stop a moment later and take the reason away — so a
            // rate limit came out as a short answer with nothing to say why. The failure is the news
            // and the end is not; `Decoder::finish` stops saying it as well, and this is the half that
            // holds whatever the transport does.
            Reply::Finished { .. } if matches!(self.state, State::Failed(_)) => {}
            Reply::Finished { reason } => {
                // **Only a call nothing has answered leaves the turn waiting.** An agent runs its own
                // tools, so by the time its turn ends every call it made has an answer already —
                // which is what stops a command-line turn sitting in `waiting-for-tools` for ever
                // with nobody to answer it.
                let wants_tools = self
                    .chat
                    .message(self.answering.unwrap_or_default())
                    .is_some_and(|message| message.tools.iter().any(ToolCall::is_running));
                if let Some(message) = self.answering.and_then(|id| self.message_mut(id)) {
                    message.finish = Some(reason.clone());
                }
                self.drop_an_empty_answer();
                self.bank_the_turn();
                self.state = match wants_tools {
                    true => State::WaitingForTools,
                    false => State::Finished { reason },
                };
            }
            Reply::Failed(problem) => {
                // What arrived before the failure is kept: throwing away half an answer somebody
                // was reading is worse than keeping it beside the reason it stopped.
                if let Some(message) = self.answering.and_then(|id| self.message_mut(id)) {
                    message.failure = Some(problem.clone());
                } else {
                    let id = self.chat.next_id();
                    let mut message = Message::new(id, Role::Assistant);
                    message.failure = Some(problem.clone());
                    self.chat.push(message);
                }
                self.drop_an_empty_answer_keeping_failures();
                self.bank_the_turn();
                self.state = State::Failed(problem);
            }
        }
    }

    /// Add what this message cost to the conversation's total, and start counting again.
    ///
    /// Called when a message ends, however it ends — a turn that failed still cost what it cost.
    fn bank_the_turn(&mut self) {
        self.chat.usage.input += self.turn.input;
        self.chat.usage.output += self.turn.output;
        self.turn = Usage::default();
    }

    /// Stop where it is, keeping whatever had arrived.
    ///
    /// There is no request to a server: HTTP has no cancellation and every one of these APIs treats
    /// a closed connection as one. What this does is record that the answer is short because
    /// somebody said so, rather than because the model had finished.
    pub fn stop(&mut self) {
        if let Some(message) = self.answering.and_then(|id| self.message_mut(id)) {
            message.finish = Some("stopped".to_owned());
        }
        self.drop_an_empty_answer();
        self.bank_the_turn();
        self.state = State::Finished {
            reason: "stopped".to_owned(),
        };
    }

    /// Every tool the model is waiting on, with **where it sits** in the message that asked.
    ///
    /// **The position rather than the id, because a server can send two calls with one id.** It
    /// should not, and one does: the caller used to file its outstanding work under the id, skip the
    /// second call as already outstanding, and then answer only the first — leaving the second
    /// running for ever and the turn stopped with no way out of it. A position is Quill's own and
    /// cannot collide.
    pub fn tools_to_run(&self) -> Vec<(usize, ToolCall)> {
        self.chat
            .last()
            .map(|message| {
                message
                    .tools
                    .iter()
                    .enumerate()
                    .filter(|(_, tool)| tool.is_running())
                    .map(|(at, tool)| (at, tool.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Record what the tool at `at` answered, and add the result message once they have all answered.
    ///
    /// Answers whether the model can be asked again, which is the caller's cue to send. `at` is the
    /// position [`Self::tools_to_run`] gave, for the reason written there.
    pub fn tool_answered(&mut self, at: usize, answer: Result<String, String>, took: u64) -> bool {
        let (text, failed) = match answer {
            Ok(text) => (text, false),
            Err(problem) => (problem, true),
        };
        if let Some(message) = self.chat.last_mut() {
            if let Some(tool) = message.tools.get_mut(at) {
                tool.answer = Some(text);
                tool.failed = failed;
                tool.took = Some(took);
            }
        }
        if !self.tools_to_run().is_empty() {
            return false;
        }
        // All of them have answered, so the results become a message of their own — which is what
        // both APIs want sent back and what the pane draws under the call it answers.
        let answered = self
            .chat
            .last()
            .map(|message| message.tools.clone())
            .unwrap_or_default();
        let id = self.chat.next_id();
        let mut results = Message::new(id, Role::Tool);
        results.tools = answered;
        self.chat.push(results);
        true
    }

    /// The message the answer is being written into, made on the first thing that arrives.
    ///
    /// Made lazily rather than when the request goes out, so a turn that fails before a byte arrives
    /// leaves no empty bubble behind.
    fn answer(&mut self) -> &mut Message {
        if self.answering.is_none() {
            let id = self.chat.next_id();
            self.chat.push(Message::new(id, Role::Assistant));
            self.answering = Some(id);
        }
        let id = self.answering.expect("just made");
        self.message_mut(id).expect("just pushed")
    }

    fn message_mut(&mut self, id: u64) -> Option<&mut Message> {
        self.chat.messages.iter_mut().find(|message| message.id == id)
    }

    /// Take away the answer if nothing ever went in it.
    ///
    /// A model that answers with nothing at all is rare and real — a content filter, a zero token
    /// budget — and an empty bubble is a bubble somebody reports as a drawing fault.
    fn drop_an_empty_answer(&mut self) {
        let Some(id) = self.answering else {
            return;
        };
        if self.chat.message(id).is_some_and(Message::is_empty) {
            self.chat.messages.retain(|message| message.id != id);
        }
        self.answering = None;
    }

    /// The same, except that a message holding only a failure is kept — it is the whole report.
    fn drop_an_empty_answer_keeping_failures(&mut self) {
        let Some(id) = self.answering else {
            return;
        };
        let empty = self
            .chat
            .message(id)
            .is_some_and(|message| message.is_empty() && message.failure.is_none());
        if empty {
            self.chat.messages.retain(|message| message.id != id);
        }
        self.answering = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_session() -> Session {
        let mut session = Session::new(Conversation::new("c1", "claude"));
        session.ask(Message::said(0, Role::User, "Why?"));
        session
    }

    #[test]
    fn an_ordinary_answer_goes_into_one_message_and_ends_the_turn() {
        let mut session = a_session();
        assert_eq!(*session.state(), State::Sending);
        session.reply(Reply::Started {
            model: "claude-opus-5".to_owned(),
        });
        assert_eq!(*session.state(), State::Streaming);
        session.reply(Reply::Text("Be".to_owned()));
        session.reply(Reply::Text("cause".to_owned()));
        session.reply(Reply::Usage { input: 10, output: 2 });
        session.reply(Reply::Finished {
            reason: "stop".to_owned(),
        });
        assert_eq!(
            *session.state(),
            State::Finished {
                reason: "stop".to_owned()
            }
        );
        assert_eq!(session.chat.messages.len(), 2);
        assert_eq!(session.chat.last().expect("an answer").text(), "Because");
        assert_eq!(session.model, "claude-opus-5");
        assert_eq!(session.chat.usage.total(), 12);
        assert!(!session.is_busy());
    }

    #[test]
    fn anthropics_cumulative_usage_reports_are_not_added_up() {
        // `message_start` says the input tokens and **one** output token; `message_delta` says the
        // whole output again. Both are cumulative for that message, so adding them made every turn's
        // output one too many. The largest report of a message is what it cost.
        let mut session = a_session();
        session.reply(Reply::Usage { input: 11, output: 1 });
        session.reply(Reply::Usage { input: 0, output: 24 });
        session.reply(Reply::Finished {
            reason: "stop".to_owned(),
        });
        assert_eq!(session.chat.usage.input, 11);
        assert_eq!(session.chat.usage.output, 24, "24 in total, not 25");

        // And a second turn is added to the first, because those really are separate messages.
        session.ask(Message::said(0, Role::User, "Again"));
        session.reply(Reply::Text("Yes.".to_owned()));
        session.reply(Reply::Usage { input: 30, output: 5 });
        session.reply(Reply::Finished {
            reason: "stop".to_owned(),
        });
        assert_eq!(session.chat.usage.input, 41);
        assert_eq!(session.chat.usage.output, 29);
    }

    #[test]
    fn a_turn_that_calls_a_tool_waits_rather_than_finishing() {
        let mut session = a_session();
        session.reply(Reply::Text("Looking".to_owned()));
        session.reply(Reply::ToolCall {
            id: "t1".to_owned(),
            name: "git_status".to_owned(),
            arguments: "{}".to_owned(),
        });
        session.reply(Reply::Finished {
            reason: "tool_use".to_owned(),
        });
        assert_eq!(*session.state(), State::WaitingForTools);
        assert!(
            session.is_busy(),
            "the turn is not over while a tool is outstanding"
        );
        assert_eq!(session.tools_to_run().len(), 1);
        assert_eq!(
            session.tools_to_run()[0].0,
            0,
            "the first call is at position nought"
        );

        // The answer is recorded, a result message appears, and the caller is told it may send.
        assert!(session.tool_answered(0, Ok("clean".to_owned()), 12));
        assert!(session.tools_to_run().is_empty());
        let results = session.chat.last().expect("a result message");
        assert_eq!(results.role, Role::Tool);
        assert_eq!(results.tools[0].answer.as_deref(), Some("clean"));
        assert_eq!(results.tools[0].took, Some(12));

        // The next round is a message of its own, which is what both APIs want and what the pane
        // draws as a second bubble under the tool block.
        session.begin();
        assert_eq!(session.round(), 2);
        session.reply(Reply::Text("It is clean.".to_owned()));
        session.reply(Reply::Finished {
            reason: "stop".to_owned(),
        });
        assert_eq!(
            session.chat.messages.len(),
            4,
            "ask, answer with the call, results, answer"
        );
        assert_eq!(session.chat.last().expect("an answer").text(), "It is clean.");
    }

    #[test]
    fn two_tools_are_both_waited_for_before_the_model_is_asked_again() {
        let mut session = a_session();
        session.reply(Reply::ToolCall {
            id: "a".to_owned(),
            name: "one".to_owned(),
            arguments: "{}".to_owned(),
        });
        session.reply(Reply::ToolCall {
            id: "b".to_owned(),
            name: "two".to_owned(),
            arguments: "{}".to_owned(),
        });
        session.reply(Reply::Finished {
            reason: "tool_use".to_owned(),
        });
        assert_eq!(session.tools_to_run().len(), 2);
        assert!(
            !session.tool_answered(0, Ok("one".to_owned()), 1),
            "still one outstanding"
        );
        assert!(session.tool_answered(1, Err("no".to_owned()), 2));
        let results = session.chat.last().expect("results");
        assert_eq!(results.tools.len(), 2);
        assert!(
            results.tools[1].failed,
            "a refusal is still an answer and still goes back up"
        );
    }

    #[test]
    fn a_failure_keeps_what_had_already_arrived() {
        let mut session = a_session();
        session.reply(Reply::Text("Half an ans".to_owned()));
        session.reply(Reply::Failed("overloaded_error: Overloaded".to_owned()));
        let answer = session.chat.last().expect("the answer");
        assert_eq!(answer.text(), "Half an ans");
        assert_eq!(answer.failure.as_deref(), Some("overloaded_error: Overloaded"));
        assert_eq!(
            *session.state(),
            State::Failed("overloaded_error: Overloaded".to_owned())
        );
        assert!(!session.is_busy());
    }

    #[test]
    fn an_ordinary_end_after_a_failure_leaves_the_failure_standing() {
        // The transport reports the end of the stream whatever ended it, and taking the later of the
        // two threw away the only thing that said why.
        let mut session = a_session();
        session.reply(Reply::Text("Half an ans".to_owned()));
        session.reply(Reply::Failed("HTTP 429: rate_limit_error".to_owned()));
        session.reply(Reply::Finished {
            reason: "stop".to_owned(),
        });
        assert_eq!(
            *session.state(),
            State::Failed("HTTP 429: rate_limit_error".to_owned())
        );
        let answer = session.chat.last().expect("the answer");
        assert_eq!(answer.text(), "Half an ans");
        assert_eq!(answer.failure.as_deref(), Some("HTTP 429: rate_limit_error"));
        // And asking again clears it, because a new turn is a new turn.
        session.ask(Message::said(0, Role::User, "Again?"));
        assert_eq!(*session.state(), State::Sending);
    }

    #[test]
    fn two_calls_carrying_one_id_are_both_answered_rather_than_wedging_the_turn() {
        // A server should not send two calls with one id, and one does. Answered by id, the second
        // stayed running for ever and the turn stopped with no way out of it; answered by position,
        // there is nothing to collide.
        let mut session = a_session();
        for _ in 0..2 {
            session.reply(Reply::ToolCall {
                id: "same".to_owned(),
                name: "quill_git".to_owned(),
                arguments: "{}".to_owned(),
            });
        }
        session.reply(Reply::Finished {
            reason: "tool_use".to_owned(),
        });
        let waiting = session.tools_to_run();
        assert_eq!(waiting.len(), 2);
        assert_eq!(waiting[0].0, 0);
        assert_eq!(waiting[1].0, 1);
        assert!(!session.tool_answered(0, Ok("first".to_owned()), 1));
        assert!(
            session.tool_answered(1, Ok("second".to_owned()), 2),
            "the turn can go on"
        );
        let results = session.chat.last().expect("results");
        assert_eq!(results.tools[0].answer.as_deref(), Some("first"));
        assert_eq!(results.tools[1].answer.as_deref(), Some("second"));
    }

    #[test]
    fn a_failure_before_a_single_byte_arrived_is_still_reported() {
        let mut session = a_session();
        session.reply(Reply::Failed("401 Unauthorized".to_owned()));
        assert_eq!(session.chat.messages.len(), 2);
        assert_eq!(
            session.chat.last().expect("a report").failure.as_deref(),
            Some("401 Unauthorized")
        );
    }

    #[test]
    fn a_model_that_says_nothing_at_all_leaves_no_empty_bubble() {
        let mut session = a_session();
        session.reply(Reply::Started {
            model: "m".to_owned(),
        });
        session.reply(Reply::Finished {
            reason: "stop".to_owned(),
        });
        assert_eq!(session.chat.messages.len(), 1, "the question and nothing else");
    }

    #[test]
    fn stopping_keeps_the_words_and_says_why_it_is_short() {
        let mut session = a_session();
        session.reply(Reply::Text("As I was say".to_owned()));
        session.stop();
        let answer = session.chat.last().expect("the answer");
        assert_eq!(answer.text(), "As I was say");
        assert_eq!(answer.finish.as_deref(), Some("stopped"));
        assert!(!session.is_busy());
    }

    #[test]
    fn asking_again_starts_the_round_count_over() {
        // Which is what bounds a tool loop: the limit is per turn, not for the life of the pane.
        let mut session = a_session();
        session.begin();
        assert_eq!(session.round(), 2);
        session.ask(Message::said(0, Role::User, "And now?"));
        assert_eq!(session.round(), 1);
    }
}
