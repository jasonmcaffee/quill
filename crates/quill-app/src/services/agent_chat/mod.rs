//! The Agent-Chat plugin: a pane you talk to a model in.
//!
//! `tasks/task-1767-agent-chat-tdd.md` is the design. `quill-chat` is the half that can be tested
//! with no window — the endpoints, the two wire shapes, the framing, the conversation and the thread
//! — and this is the half that has a window behind it: what is being typed, what is attached, which
//! conversation is open, what the tools are doing, and the one place a command becomes a change.
//!
//! ## The provider decides nothing about the window
//!
//! It draws and it returns [`Request`]s, which is the rule `components::activity_bar` set and every
//! contributed surface since has kept. Running a tool is the same shape in both directions: the
//! provider asks for a command by its catalogue name, the window runs it through
//! `QuillApp::run_cli` — **the one place a command turns into a change** — and hands the answer back
//! on the next frame. So a tool call and a person pressing the same menu entry are the same thing
//! rather than two paths that agree today.
//!
//! ## Nothing is fetched that was not asked for
//!
//! One request is made, when somebody presses send. There is no discovery, no model list, no
//! telemetry and nothing at startup — which is the rule the Markdown preview, the Mermaid reader and
//! the plugin loader all keep, and the reason it is worth stating is that this is the first thing in
//! Quill with a socket in it.

pub mod store;
pub mod tools;

use std::path::{Path, PathBuf};

use quill_chat::model::{Message, Part, Role};
use quill_chat::provider::{Provider, Wire};
use quill_chat::{Client, Conversation, Session, State};

use crate::services::plugin_ui::{Answer, Context, Look, Request, UiProvider};
use crate::services::store::Values;

use store::{Store, Summary};

/// How many rounds of tools one turn may take before the pane stops asking.
pub const DEFAULT_TOOL_LIMIT: u32 = 8;

/// How many conversations are kept.
pub const DEFAULT_HISTORY: usize = 20;

/// The plugin's own settings, in `plugins/agent-chat/settings.conf` beside its manifest.
///
/// Read by the same `store::Values` the window's own settings are read by, in the same
/// `name = value` form, so a person can open it in Quill and change it. **No key is in it** — a
/// provider names the environment variable its key comes from and nothing else, which is
/// `services::agent_tasks::keychain`'s rule: what is written down is the name of the place the
/// secret is.
#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    pub providers: Vec<Provider>,
    /// Which one is used, by name. Empty means the first.
    pub chosen: String,
    pub stream: bool,
    /// Whether Quill's own commands are offered to the model. Off unless somebody says so.
    pub tools: bool,
    pub tool_limit: u32,
    /// The person's own system prompt, added after Quill's own line.
    pub system: String,
    pub history: usize,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            providers: Provider::defaults(),
            chosen: String::new(),
            stream: true,
            // **Off**, which is the precedent the page this pane copies already set: its own robot
            // button is titled "UI controls (let the agent operate the studio)" and is off unless
            // pressed. A pane that could edit files the moment it was opened would be a pane nobody
            // dares open.
            tools: false,
            tool_limit: DEFAULT_TOOL_LIMIT,
            system: String::new(),
            history: DEFAULT_HISTORY,
        }
    }
}

impl Configuration {
    const FILE: &'static str = "settings.conf";

    /// Read the configuration out of the plugin's folder, or the defaults when there is no file.
    pub fn read(folder: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(folder.join(Self::FILE)) else {
            return Self::default();
        };
        Self::of(&Values::parse(&text))
    }

    /// The same, from values already read, so a test needs no file.
    pub fn of(values: &Values) -> Self {
        let mut configuration = Self {
            providers: Vec::new(),
            ..Self::default()
        };
        let count = values.number("providers").unwrap_or(0.0).max(0.0) as usize;
        for index in 0..count.min(32) {
            if let Some(provider) = provider_at(values, index) {
                configuration.providers.push(provider);
            }
        }
        // A file that names no provider at all — or whose every row was refused — gets the three
        // that ship, because a pane with no endpoint is a pane that cannot do anything and a person
        // who wanted none would have switched the plugin off.
        if configuration.providers.is_empty() {
            configuration.providers = Provider::defaults();
        }
        if let Some(chosen) = values.text("chosen") {
            configuration.chosen = chosen.trim().to_owned();
        }
        if let Some(stream) = values.flag("stream") {
            configuration.stream = stream;
        }
        if let Some(tools) = values.flag("tools") {
            configuration.tools = tools;
        }
        if let Some(limit) = values.number("tool-limit") {
            configuration.tool_limit = limit.clamp(1.0, 32.0) as u32;
        }
        if let Some(system) = values.text("system") {
            configuration.system = system.to_owned();
        }
        if let Some(history) = values.number("history") {
            configuration.history = history.clamp(1.0, 500.0) as usize;
        }
        configuration
    }

    /// Write it back into the plugin's folder.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        let mut values = Values::new();
        values.set("providers", self.providers.len().to_string());
        for (index, provider) in self.providers.iter().enumerate() {
            values.set(&format!("provider.{index}.name"), provider.name.clone());
            values.set(&format!("provider.{index}.wire"), provider.wire.name());
            values.set(&format!("provider.{index}.url"), provider.url.clone());
            values.set(&format!("provider.{index}.model"), provider.model.clone());
            values.set(&format!("provider.{index}.key-env"), provider.key_env.clone());
            values.set(&format!("provider.{index}.key-entry"), provider.key_entry.clone());
            values.set(
                &format!("provider.{index}.max-tokens"),
                provider.max_tokens.to_string(),
            );
        }
        values.set("chosen", self.chosen.clone());
        values.set("stream", self.stream.to_string());
        values.set("tools", self.tools.to_string());
        values.set("tool-limit", self.tool_limit.to_string());
        values.set("system", self.system.replace('\n', " "));
        values.set("history", self.history.to_string());
        std::fs::create_dir_all(folder)
            .map_err(|problem| format!("{} could not be made: {problem}", folder.display()))?;
        std::fs::write(
            folder.join(Self::FILE),
            values.to_text_headed(
                "# The Agent-Chat plugin's settings. `Settings -> Agent-Chat` writes this file and reads it back.\n\
                 # A provider names the environment variable its key comes from; the key itself is never written\n\
                 # here or anywhere else by Quill. `wire` is `openai` or `anthropic`.",
            ),
        )
        .map_err(|problem| format!("{} could not be written: {problem}", folder.display()))
    }

    /// The endpoint that is used, which is the chosen one or the first there is.
    pub fn provider(&self) -> Option<&Provider> {
        self.providers
            .iter()
            .find(|one| one.name == self.chosen)
            .or_else(|| self.providers.first())
    }

    /// Choose one by name, or say it is not there.
    pub fn choose(&mut self, name: &str) -> Result<(), String> {
        match self.providers.iter().any(|one| one.name == name) {
            true => {
                self.chosen = name.to_owned();
                Ok(())
            }
            false => Err(format!(
                "there is no endpoint called `{name}`. There is {}.",
                self.providers
                    .iter()
                    .map(|one| one.name.as_str())
                    .collect::<Vec<&str>>()
                    .join(", ")
            )),
        }
    }
}

/// One provider's row, or nothing when the file names a wire this version has not got.
///
/// **Refused with the list rather than half-loaded**, which is the rule `plugin.kind`,
/// `language.renders`, `run.project`, `debug.adapter` and `ui.chrome` all keep. A row whose wire is
/// unknown would be an endpoint every request to which fails obscurely.
fn provider_at(values: &Values, index: usize) -> Option<Provider> {
    let name = values.text(&format!("provider.{index}.name"))?.trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let named = values.text(&format!("provider.{index}.wire")).unwrap_or("openai");
    let wire = Wire::from_name(named)?;
    Some(Provider {
        name,
        wire,
        url: values
            .text(&format!("provider.{index}.url"))
            .unwrap_or_default()
            .trim()
            .to_owned(),
        model: values
            .text(&format!("provider.{index}.model"))
            .unwrap_or_default()
            .trim()
            .to_owned(),
        key_env: values
            .text(&format!("provider.{index}.key-env"))
            .unwrap_or_default()
            .trim()
            .to_owned(),
        key_entry: values
            .text(&format!("provider.{index}.key-entry"))
            .unwrap_or_default()
            .trim()
            .to_owned(),
        max_tokens: values
            .number(&format!("provider.{index}.max-tokens"))
            .unwrap_or(quill_chat::provider::DEFAULT_MAX_TOKENS as f32)
            .clamp(64.0, 200_000.0) as u32,
    })
}

/// A picture waiting to go up with the next message.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    /// Its own number, so the drawing can key a texture on it without keying on the bytes.
    pub id: u64,
    pub name: String,
    pub media: String,
    pub bytes: Vec<u8>,
}

/// A tool call handed to the window and not yet answered.
#[derive(Debug, Clone, PartialEq)]
struct Outstanding {
    id: String,
    /// When it went out, so the block can say how long it took.
    at: std::time::Instant,
}

/// What the drawing keeps between frames.
///
/// On the provider rather than in the component, because a component in Quill takes a rectangle,
/// draws and returns what happened — it holds nothing. `components::agent_chat` is written to that
/// rule and this is where the little it has to remember lives.
#[derive(Default)]
pub struct PaneState {
    /// Put the conversation at the bottom on the next frame, and then stop.
    ///
    /// **The scrolling itself is `egui`'s.** `ScrollArea::stick_to_bottom` follows an answer while the
    /// view is already at the bottom and stops the moment somebody scrolls up, which is
    /// `ChatPage.tsx`'s own `shouldAutoScroll` rule and is better than reimplementing it. What egui
    /// will not do is go *back* to the bottom once somebody has scrolled away, so sending, opening a
    /// conversation and starting a new one each ask for it once — the one-shot shape
    /// `QuillApp::follow_the_open_file` already uses.
    pub jump_to_bottom: bool,
    /// Whether the history list is open over the conversation.
    pub history_open: bool,
    /// Whether the endpoint list is open.
    pub providers_open: bool,
    /// The tool blocks somebody has opened by hand, by their call id.
    pub opened_tools: Vec<String>,
    /// Which message's thinking has been opened.
    pub opened_thinking: Vec<u64>,
    /// The markdown each message came to, kept between frames and keyed on the message.
    ///
    /// Rendering and laying out is the expensive half and the source of a finished message never
    /// changes, so a conversation of forty messages costs nothing while the forty-first is arriving.
    /// `components::markdown_text::Cache` re-renders only when the source or the width has moved,
    /// which bounds a streaming answer at one render a frame — `task-1666`'s rule applied to the one
    /// thing here that changes sixty times a second.
    pub rendered: crate::components::markdown_text::Cache,
    /// The pictures already uploaded to the graphics card, by message and part.
    ///
    /// Keyed on where the picture is rather than on its bytes, so a conversation with twenty pictures
    /// in it does not decode twenty pictures a frame.
    pub pictures: std::collections::HashMap<String, egui::TextureHandle>,
}

/// The pieces of the pane the drawing is handed.
///
/// A component in Quill takes a rectangle, draws and reports what happened — it changes nothing. So
/// the drawing is given borrows of what it reads and one mutable borrow of what it must write into
/// (the draft, and the little the drawing has to remember between frames), and everything else it
/// wants done comes back as a `components::agent_chat::Act`.
pub struct Parts<'a> {
    pub session: &'a Session,
    pub configuration: &'a Configuration,
    pub state: &'a mut PaneState,
    pub draft: &'a mut String,
    pub attachments: &'a [Attachment],
    pub history: &'a [Summary],
    pub problem: Option<&'a str>,
}

impl std::fmt::Debug for PaneState {
    /// Written by hand because a `TextureHandle` has no `Debug` and a rendered markdown cache is a
    /// screenful of glyph positions. What is printed is what a failing assertion wants to see.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("PaneState")
            .field("jump_to_bottom", &self.jump_to_bottom)
            .field("history_open", &self.history_open)
            .field("providers_open", &self.providers_open)
            .field("opened_tools", &self.opened_tools)
            .field("pictures", &self.pictures.len())
            .finish()
    }
}

/// The plugin.
pub struct AgentChat {
    open: bool,
    folder: Option<PathBuf>,
    project: Option<PathBuf>,
    /// The file showing in the window, which the window tells the provider — see
    /// [`UiProvider::showing`]. A chat in an editor that does not know what you are looking at is a
    /// browser tab.
    showing: Option<PathBuf>,
    configuration: Configuration,
    store: Store,
    session: Session,
    client: Client,
    /// What is being typed, and what is attached to it.
    pub draft: String,
    pub attachments: Vec<Attachment>,
    next_attachment: u64,
    outstanding: Vec<Outstanding>,
    /// Requests made outside a draw — running a tool — drained by the window once a frame.
    asking: Vec<Request>,
    history: Vec<Summary>,
    /// Something that went wrong before a request could go out, drawn at the top of the composer.
    pub problem: Option<String>,
    pub ui: PaneState,
    /// Whether anything changed since the conversation was last written down.
    dirty: bool,
}

impl std::fmt::Debug for AgentChat {
    /// Written by hand because a `Client` holds a channel and a `Session` holds a whole transcript,
    /// and what a failing assertion wants to see is neither.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("AgentChat")
            .field("open", &self.open)
            .field(
                "provider",
                &self.configuration.provider().map(|one| one.name.clone()),
            )
            .field("state", &self.session.state().name())
            .field("messages", &self.session.chat.messages.len())
            .finish()
    }
}

impl Default for AgentChat {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentChat {
    pub fn new() -> Self {
        Self {
            open: false,
            folder: None,
            project: None,
            showing: None,
            configuration: Configuration::default(),
            store: Store::at(None),
            session: Session::new(Conversation::new("", "")),
            client: Client::new(),
            draft: String::new(),
            attachments: Vec::new(),
            next_attachment: 1,
            outstanding: Vec::new(),
            asking: Vec::new(),
            history: Vec::new(),
            problem: None,
            ui: PaneState {
                jump_to_bottom: true,
                ..PaneState::default()
            },
            dirty: false,
        }
    }

    /// The pane's own pieces, split so that the drawing can read the conversation while it writes
    /// into the draft.
    pub fn parts(&mut self) -> Parts<'_> {
        Parts {
            session: &self.session,
            configuration: &self.configuration,
            state: &mut self.ui,
            draft: &mut self.draft,
            attachments: &self.attachments,
            history: &self.history,
            problem: self.problem.as_deref(),
        }
    }

    pub fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    pub fn configuration_mut(&mut self) -> &mut Configuration {
        &mut self.configuration
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// The session, to drive by hand.
    ///
    /// The two callers `UiProvider::as_any_mut` exists for: a test building a conversation the way the
    /// wire would have built it, and the drawing. Nothing outside the plugin reaches it.
    pub fn session_mut(&mut self) -> &mut Session {
        self.dirty = true;
        &mut self.session
    }

    pub fn chat(&self) -> &Conversation {
        &self.session.chat
    }

    pub fn history(&self) -> &[Summary] {
        &self.history
    }

    pub fn project(&self) -> Option<&Path> {
        self.project.as_deref()
    }

    /// The endpoint in use.
    pub fn provider(&self) -> Option<&Provider> {
        self.configuration.provider()
    }

    /// Write the configuration back, and say so if it could not be written.
    pub fn save_the_configuration(&mut self) -> Result<(), String> {
        let Some(folder) = &self.folder else {
            return Ok(());
        };
        self.configuration.write(folder)
    }

    /// Start a new conversation, keeping the one that was open.
    pub fn new_conversation(&mut self) {
        self.write_the_conversation();
        let provider = self.provider().map(|one| one.name.clone()).unwrap_or_default();
        let id = self.store.new_id();
        self.session = Session::new(Conversation::new(id, provider));
        self.attachments.clear();
        self.problem = None;
        self.ui.jump_to_bottom = true;
        self.refresh_the_history();
    }

    /// Open one out of the history.
    pub fn open_conversation(&mut self, id: &str) -> Result<(), String> {
        let Some(chat) = self.store.read(id) else {
            return Err(format!("there is no conversation called `{id}`."));
        };
        self.write_the_conversation();
        if !chat.provider.is_empty() {
            let _ = self.configuration.choose(&chat.provider);
        }
        self.session = Session::new(chat);
        self.problem = None;
        self.ui.jump_to_bottom = true;
        Ok(())
    }

    /// Throw one away.
    pub fn remove_conversation(&mut self, id: &str) -> Result<(), String> {
        self.store.remove(id)?;
        if self.session.chat.id == id {
            self.new_conversation();
        } else {
            self.refresh_the_history();
        }
        Ok(())
    }

    /// Attach a picture from a file.
    ///
    /// Read into bytes here rather than remembered as a path, which is `model::Part::Picture`'s own
    /// reason: a conversation reopened after the file has moved still shows what was sent.
    pub fn attach(&mut self, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path)
            .map_err(|problem| format!("{} could not be read: {problem}", path.display()))?;
        let media = media_type_of(path, &bytes)
            .ok_or_else(|| format!("{} is not a picture Quill can send.", path.display()))?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "picture".to_owned());
        self.attach_bytes(name, media, bytes);
        Ok(())
    }

    /// Attach a picture already in memory, which is what a paste from the clipboard is.
    pub fn attach_bytes(&mut self, name: String, media: String, bytes: Vec<u8>) {
        let id = self.next_attachment;
        self.next_attachment += 1;
        self.attachments.push(Attachment {
            id,
            name,
            media,
            bytes,
        });
    }

    pub fn remove_attachment(&mut self, id: u64) {
        self.attachments.retain(|one| one.id != id);
    }

    /// Send what is being typed.
    ///
    /// Answers the message's id, or a sentence saying why nothing went — which is where a URL with a
    /// typo in it and a missing key are reported, before a request rather than thirty seconds after
    /// one.
    pub fn send(&mut self) -> Result<u64, String> {
        if self.draft.trim().is_empty() && self.attachments.is_empty() {
            return Err("there is nothing to send.".to_owned());
        }
        let Some(provider) = self.provider().cloned() else {
            return Err("no endpoint is configured. Settings -> Agent-Chat is where they go.".to_owned());
        };
        if let Some(why) = provider.why_not() {
            self.problem = Some(why.clone());
            return Err(why);
        }
        let id = self.session.chat.next_id();
        let mut message = Message::new(id, Role::User);
        let said = std::mem::take(&mut self.draft);
        if !said.trim().is_empty() {
            message.parts.push(Part::Text(said.trim_end().to_owned()));
        }
        for attachment in std::mem::take(&mut self.attachments) {
            message.parts.push(Part::Picture {
                media: attachment.media,
                bytes: attachment.bytes,
                name: attachment.name,
            });
        }
        self.session.chat.provider = provider.name.clone();
        let id = self.session.ask(message);
        self.problem = None;
        self.ui.jump_to_bottom = true;
        self.dirty = true;
        self.dispatch(&provider);
        Ok(id)
    }

    /// Put the request on the wire. The session has already been told a turn is starting.
    ///
    /// **Checked again here rather than only at `send`**, because a round after a tool is a request
    /// nobody pressed a button for: a key cleared out of the environment while a turn was running
    /// would otherwise be a request that fails at the far end rather than a sentence in the pane.
    fn dispatch(&mut self, provider: &Provider) {
        if let Some(why) = provider.why_not() {
            self.problem = Some(why.clone());
            self.session.reply(quill_chat::Reply::Failed(why));
            return;
        }
        let tools = match self.configuration.tools {
            true => tools::offered(),
            false => Vec::new(),
        };
        let body = quill_chat::wire::request(
            provider,
            &self.session.chat,
            &self.system_prompt(),
            &tools,
            self.configuration.stream,
        );
        self.client
            .send(provider, body.to_string(), self.configuration.stream);
    }

    /// What Quill tells the model about where it is.
    ///
    /// Quill's own line first, then the person's. **Which project is open and which file is showing,
    /// and not the file's text**: a pane that quietly uploaded whatever was on the screen is a pane
    /// nobody could use on anything confidential. With the tools on, the model can read the file by
    /// asking, which is the right shape for an editor whose every command is already a tool.
    pub fn system_prompt(&self) -> String {
        let mut lines = vec![
            "You are answering inside Quill, a code editor, in a pane beside the person's work. Be brief and concrete; they can see their own screen.".to_owned(),
        ];
        if let Some(project) = &self.project {
            lines.push(format!("The project open is {}.", project.display()));
        }
        if let Some(showing) = &self.showing {
            lines.push(format!("The file showing is {}.", showing.display()));
        }
        if self.configuration.tools {
            lines.push(
                "You can drive this window with the tools you have been given. They are Quill's own commands, so anything you do this way is exactly what the person would get from the menu, and it is one undo step. Read a file before changing it."
                    .to_owned(),
            );
        }
        if !self.configuration.system.trim().is_empty() {
            lines.push(self.configuration.system.trim().to_owned());
        }
        lines.join("\n")
    }

    /// Stop whatever is arriving, keeping it.
    pub fn stop(&mut self) {
        self.client.stop();
        self.outstanding.clear();
        if self.session.is_busy() {
            self.session.stop();
            self.dirty = true;
        }
    }

    /// Read the replies that have arrived and act on what they mean.
    ///
    /// The whole of what happens between frames: replies in, tool calls out, the next round sent
    /// when the tools have all answered. Answers whether anything is still happening.
    fn take_the_replies(&mut self) -> bool {
        let replies = self.client.take();
        let anything = !replies.is_empty();
        for reply in replies {
            self.session.reply(reply);
            self.dirty = true;
        }
        if anything {
            self.session.chat.changed = store::seconds_now();
        }
        if matches!(self.session.state(), State::WaitingForTools) {
            self.ask_for_the_tools();
        }
        if anything && !self.session.is_busy() {
            self.write_the_conversation();
        }
        self.session.is_busy()
    }

    /// Hand every outstanding tool call to the window, once each.
    fn ask_for_the_tools(&mut self) {
        for call in self.session.tools_to_run() {
            if self.outstanding.iter().any(|one| one.id == call.id) {
                continue;
            }
            self.outstanding.push(Outstanding {
                id: call.id.clone(),
                at: std::time::Instant::now(),
            });
            match tools::resolve(&call.name, &call.arguments_value()) {
                Ok(resolved) => self.asking.push(Request::RunCommand {
                    id: call.id.clone(),
                    command: resolved.command.wire(),
                    arguments: resolved.arguments,
                }),
                // A refusal is an answer: it goes straight back up so the model reads it and picks
                // something else, rather than the turn hanging on a call nobody will run.
                Err(problem) => self.answered(&call.id, Err(problem)),
            }
        }
    }

    /// What a tool answered, from the window.
    fn tool_answered(&mut self, id: &str, answer: Result<serde_json::Value, String>) {
        let took = self
            .outstanding
            .iter()
            .find(|one| one.id == id)
            .map(|one| one.at.elapsed().as_millis().min(u64::MAX as u128) as u64)
            .unwrap_or(0);
        self.outstanding.retain(|one| one.id != id);
        let answer = answer.map(|value| shorten_for_a_model(&value));
        self.dirty = true;
        if !self.session.tool_answered(id, answer, took) {
            return;
        }
        // Every tool has answered, so the model is asked again — unless it has been asked enough
        // times already, which is what stops a loop nobody is watching from being funded.
        if self.session.round() >= self.configuration.tool_limit {
            self.session.reply(quill_chat::Reply::Failed(format!(
                "the model asked for tools {} times in one turn, which is the limit in Settings -> Agent-Chat.",
                self.session.round()
            )));
            self.write_the_conversation();
            return;
        }
        let Some(provider) = self.provider().cloned() else {
            return;
        };
        self.session.begin();
        self.dispatch(&provider);
    }

    /// Write the conversation down if anything changed.
    fn write_the_conversation(&mut self) {
        if !self.dirty || self.session.chat.messages.is_empty() {
            return;
        }
        self.dirty = false;
        self.session.chat.changed = store::seconds_now();
        if let Err(problem) = self.store.write(&self.session.chat) {
            self.problem = Some(problem);
        }
        self.store.tidy(self.configuration.history);
        self.refresh_the_history();
    }

    fn refresh_the_history(&mut self) {
        self.history = self.store.list(self.configuration.history);
    }

    /// What the pane holds, as data — which is what `quill-cli plugins view agent-chat` prints.
    fn view_value(&self) -> serde_json::Value {
        serde_json::json!({
            "provider": self.provider().map(|one| serde_json::json!({
                "name": one.name,
                "wire": one.wire.name(),
                "url": one.url,
                "model": one.model,
                "key": one.has_a_key(),
            })),
            "state": self.session.state().name(),
            "model": self.session.model,
            "round": self.session.round(),
            "tools": self.configuration.tools,
            "stream": self.configuration.stream,
            "streaming": self.session.is_busy(),
            "draft": self.draft,
            "attachments": self.attachments.iter().map(|one| serde_json::json!({
                "name": one.name, "media": one.media, "bytes": one.bytes.len(),
            })).collect::<Vec<serde_json::Value>>(),
            "problem": self.problem,
            // Deliberately **not** `total`, which is the key the pane's own header draws as a count
            // beside its name. A board's count is how many tickets there are and is worth reading at a
            // glance; a chat's is how many messages have been said, which is a number nobody wants in
            // a header and which reads as `Agent-Chat 0` on an empty pane.
            "messages": self.session.chat.messages.len(),
            "conversation": self.session.chat.to_json(),
        })
    }
}

/// A tool's answer, cut to something worth sending back up.
///
/// A command like `explorer tree` can answer with a megabyte, and sending a megabyte back into the
/// conversation costs the person money and fills the model's context with one answer. `task-1695`
/// measured the other half of this: an agent handed 3,000 tokens to learn one number stops asking.
/// So it is cut, and the cut says so, which is the honest form.
fn shorten_for_a_model(value: &serde_json::Value) -> String {
    const LIMIT: usize = 8000;
    let text = match value {
        serde_json::Value::String(said) => said.clone(),
        serde_json::Value::Null => "ok".to_owned(),
        other => other.to_string(),
    };
    match text.len() > LIMIT {
        true => format!(
            "{}\n… cut short at {LIMIT} characters.",
            &text[..floor_char_boundary(&text, LIMIT)]
        ),
        false => text,
    }
}

/// The largest index at or below `at` that is a character boundary.
///
/// Written out because `str::floor_char_boundary` is not stable, and slicing a `String` of somebody
/// else's JSON in the middle of a character panics.
fn floor_char_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// What kind of picture this is, from its own first bytes and then from its name.
///
/// The bytes first, because a screenshot saved as `.png` that is really a JPEG is a thing that
/// happens and an API told the wrong media type refuses the whole request.
fn media_type_of(path: &Path, bytes: &[u8]) -> Option<String> {
    let sniffed = match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    };
    if let Some(media) = sniffed {
        return Some(media.to_owned());
    }
    match path
        .extension()
        .and_then(|kind| kind.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png".to_owned()),
        Some("jpg") | Some("jpeg") => Some("image/jpeg".to_owned()),
        Some("gif") => Some("image/gif".to_owned()),
        Some("webp") => Some("image/webp".to_owned()),
        _ => None,
    }
}

impl UiProvider for AgentChat {
    fn id(&self) -> &'static str {
        "agent-chat"
    }

    fn open(&mut self, context: &Context) -> Result<(), String> {
        self.folder = context.folder.clone();
        self.project = context.project.clone();
        self.store = Store::at(self.folder.clone());
        if let Some(folder) = &self.folder {
            self.configuration = Configuration::read(folder);
        }
        self.client.set_waker(context.wake.clone());
        self.refresh_the_history();
        // The newest conversation is reopened, because a pane that came back empty every time would
        // be a pane you cannot leave a question in — which is what `task-1693` asks a project to
        // remember about everything else in the window.
        let newest = self.history.first().map(|one| one.id.clone());
        match newest.and_then(|id| self.store.read(&id)) {
            Some(chat) => {
                if !chat.provider.is_empty() {
                    let _ = self.configuration.choose(&chat.provider);
                }
                self.session = Session::new(chat);
                self.ui.jump_to_bottom = true;
            }
            None => {
                let id = self.store.new_id();
                let provider = self.provider().map(|one| one.name.clone()).unwrap_or_default();
                self.session = Session::new(Conversation::new(id, provider));
            }
        }
        self.open = true;
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open
    }

    /// The decoration `egui` cannot draw: the raised bubbles, the pressed wells, the gradient send
    /// button and its glow. See `services::vello_canvas`.
    fn draws_chrome(&self) -> bool {
        true
    }

    fn pane(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        crate::components::agent_chat::pane(self, ui, look)
    }

    fn settings(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        crate::components::agent_chat::settings_page::show(self, ui, look)
    }

    fn command(&mut self, command: &str, arguments: &[String]) -> Result<Answer, String> {
        let rest = arguments.join(" ");
        match command {
            // The window's own name for "put this pane on the screen", which `run_plugin_command`
            // acts on: the menu entry, the rail button and the command line are one path.
            "open-pane" => Ok(Answer::said("the chat")),
            "new" => {
                self.new_conversation();
                Ok(
                    Answer::said("a new conversation")
                        .with(serde_json::json!({ "id": self.session.chat.id })),
                )
            }
            "send" => {
                if !rest.trim().is_empty() {
                    self.draft = rest;
                }
                let id = self.send()?;
                // **It does not wait.** `command` is called inside a frame, and a command that
                // blocked would stop the window drawing for the length of a model's answer — which
                // is the sentence `quill_git::Worker` exists for. `state` says when it has finished.
                Ok(Answer::said("sent")
                    .with(serde_json::json!({ "id": id, "state": self.session.state().name() })))
            }
            "stop" => {
                self.stop();
                Ok(Answer::said("stopped"))
            }
            "state" => Ok(Answer::said(self.session.state().name()).with(serde_json::json!({
                "state": self.session.state().name(),
                "busy": self.session.is_busy(),
                "round": self.session.round(),
                "characters": self.session.chat.last().map(|last| last.text().len()).unwrap_or(0),
                "problem": self.problem,
            }))),
            "messages" => Ok(
                Answer::said(format!("{} messages", self.session.chat.messages.len()))
                    .with(self.session.chat.to_json()),
            ),
            "last" => {
                let last = self
                    .session
                    .chat
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.role == Role::Assistant);
                Ok(
                    Answer::said(last.map(quill_chat::Message::text).unwrap_or_default()).with(
                        serde_json::json!({
                            "text": last.map(quill_chat::Message::text),
                            "failure": last.and_then(|message| message.failure.clone()),
                            "finish": last.and_then(|message| message.finish.clone()),
                        }),
                    ),
                )
            }
            "attach" => {
                let path = PathBuf::from(rest.trim());
                if path.as_os_str().is_empty() {
                    return Err("attach takes the path of a picture.".to_owned());
                }
                self.attach(&path)?;
                Ok(Answer::said(format!("attached {}", path.display()))
                    .with(serde_json::json!({ "attachments": self.attachments.len() })))
            }
            "providers" => Ok(Answer::said(
                self.configuration
                    .providers
                    .iter()
                    .map(|one| one.name.as_str())
                    .collect::<Vec<&str>>()
                    .join(", "),
            )
            .with(serde_json::json!({
                "chosen": self.provider().map(|one| one.name.clone()),
                "providers": self.configuration.providers.iter().map(|one| serde_json::json!({
                    "name": one.name,
                    "wire": one.wire.name(),
                    "url": one.url,
                    "model": one.model,
                    "key_env": one.key_env,
                    "key": one.has_a_key(),
                    "why_not": one.why_not(),
                })).collect::<Vec<serde_json::Value>>(),
            }))),
            "use" => {
                self.configuration.choose(rest.trim())?;
                self.save_the_configuration()?;
                Ok(Answer::said(format!("talking to {}", rest.trim())))
            }
            "history" => Ok(
                Answer::said(format!("{} conversations", self.history.len())).with(serde_json::json!(self
                    .history
                    .iter()
                    .map(|one| serde_json::json!({
                        "id": one.id,
                        "name": one.name,
                        "provider": one.provider,
                        "messages": one.messages,
                        "changed": one.changed,
                    }))
                    .collect::<Vec<serde_json::Value>>())),
            ),
            "open" => {
                self.open_conversation(rest.trim())?;
                Ok(Answer::said(format!(
                    "opened {}",
                    self.session.chat.display_name()
                )))
            }
            "remove" => {
                self.remove_conversation(rest.trim())?;
                Ok(Answer::said("removed"))
            }
            "tools" => {
                match rest.trim() {
                    "on" => self.configuration.tools = true,
                    "off" => self.configuration.tools = false,
                    // With nothing said it toggles, because the menu entry that runs it is a
                    // `Toggle` and a menu row cannot carry an argument.
                    "" => self.configuration.tools = !self.configuration.tools,
                    other => return Err(format!("tools takes `on` or `off`, not `{other}`.")),
                }
                self.save_the_configuration()?;
                Ok(Answer::said(match self.configuration.tools {
                    true => "Quill's own commands are offered to the model",
                    false => "no tools are offered",
                })
                .with(serde_json::json!({ "tools": self.configuration.tools })))
            }
            "view" => Ok(Answer::said("the pane").with(self.view_value())),
            other => Err(format!("`{other}` is not one of Agent-Chat's commands.")),
        }
    }

    fn commands(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("open-pane", "Show the chat pane."),
            ("new", "Start a new conversation."),
            (
                "send",
                "Add a message and start the answer. Does not wait; `state` says when it has finished.",
            ),
            ("stop", "Stop the answer, keeping what has arrived."),
            (
                "state",
                "Idle, sending, streaming, waiting-for-tools, finished or failed.",
            ),
            ("messages", "The whole conversation as data."),
            ("last", "Just the last answer."),
            ("attach", "Attach a picture to the message being composed."),
            (
                "providers",
                "The endpoints, their URLs and models, and whether each has a key.",
            ),
            ("use", "Talk to one of the endpoints by name."),
            ("history", "The conversations kept, newest first."),
            ("open", "Open one of them by its id."),
            ("remove", "Throw one away."),
            (
                "tools",
                "`on` or `off`: whether Quill's own commands are offered to the model.",
            ),
            ("view", "Everything the pane is showing, as data."),
        ]
    }

    fn view(&self) -> serde_json::Value {
        self.view_value()
    }

    fn catch_up(&mut self) -> bool {
        self.take_the_replies()
    }

    fn asking(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.asking)
    }

    fn answered(&mut self, id: &str, answer: Result<serde_json::Value, String>) {
        self.tool_answered(id, answer);
    }

    fn showing(&mut self, project: Option<&Path>, file: Option<&Path>) {
        self.project = project.map(Path::to_path_buf);
        self.showing = file.map(Path::to_path_buf);
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn close(&mut self) {
        self.stop();
        self.write_the_conversation();
        self.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("quill-chat-plugin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    fn opened(name: &str) -> AgentChat {
        let mut chat = AgentChat::new();
        chat.open(&Context {
            folder: Some(a_folder(name)),
            ..Context::default()
        })
        .expect("opened");
        chat
    }

    #[test]
    fn the_configuration_round_trips_through_the_plugins_own_folder() {
        let folder = a_folder("configuration");
        let mut configuration = Configuration::default();
        configuration.chosen = "codex".to_owned();
        configuration.tools = true;
        configuration.tool_limit = 3;
        configuration.system = "Be terse.".to_owned();
        configuration.providers[2].url = "http://127.0.0.1:9999/v1/chat/completions".to_owned();
        configuration.write(&folder).expect("written");
        let read = Configuration::read(&folder);
        assert_eq!(read, configuration);
        // And no key is anywhere in the file, whatever a provider names.
        let text = std::fs::read_to_string(folder.join(Configuration::FILE)).expect("the file");
        assert!(
            text.contains("ANTHROPIC_API_KEY"),
            "the name of the variable is written down"
        );
        assert!(!text.to_lowercase().contains("secret"));
    }

    #[test]
    fn a_row_naming_a_wire_this_version_has_not_got_is_refused_rather_than_half_loaded() {
        // The rule `plugin.kind`, `language.renders`, `run.project`, `debug.adapter` and `ui.chrome`
        // all keep: an endpoint every request to which fails obscurely is worse than a refusal.
        let values = Values::parse(
            "providers = 2\n\
             provider.0.name = gemini\n\
             provider.0.wire = gemini\n\
             provider.0.url = https://example.com\n\
             provider.1.name = mine\n\
             provider.1.wire = openai\n\
             provider.1.url = http://127.0.0.1:8080/v1/chat/completions\n\
             provider.1.model = m\n",
        );
        let configuration = Configuration::of(&values);
        assert_eq!(configuration.providers.len(), 1);
        assert_eq!(configuration.providers[0].name, "mine");
        assert!(quill_chat::provider::WIRES.contains(&configuration.providers[0].wire.name()));
    }

    #[test]
    fn a_file_that_names_no_provider_gets_the_three_that_ship() {
        // A pane with no endpoint cannot do anything, and somebody who wanted none would have
        // switched the plugin off.
        let configuration = Configuration::of(&Values::parse("stream = false\n"));
        assert_eq!(configuration.providers.len(), 3);
        assert!(!configuration.stream);
    }

    #[test]
    fn choosing_an_endpoint_that_is_not_there_names_the_ones_that_are() {
        let mut configuration = Configuration::default();
        let problem = configuration.choose("gemini").expect_err("a refusal");
        assert!(problem.contains("claude"), "{problem}");
        assert!(problem.contains("codex"), "{problem}");
        configuration.choose("codex").expect("chosen");
        assert_eq!(configuration.provider().expect("one").name, "codex");
    }

    #[test]
    fn sending_with_nothing_typed_and_with_a_broken_endpoint_both_refuse_before_a_request_goes_out() {
        let mut chat = opened("refusals");
        assert!(chat
            .send()
            .expect_err("nothing to send")
            .contains("nothing to send"));
        chat.draft = "hello".to_owned();
        chat.configuration.choose("claude").expect("chosen");
        chat.configuration.providers[0].key_env = "QUILL_A_VARIABLE_NOTHING_SETS".to_owned();
        let problem = chat.send().expect_err("no key");
        assert!(problem.contains("QUILL_A_VARIABLE_NOTHING_SETS"), "{problem}");
        // And the draft is still there, because a refusal must not eat what somebody typed.
        assert_eq!(chat.draft, "hello");
        assert_eq!(chat.session.chat.messages.len(), 0);
    }

    #[test]
    fn the_system_prompt_says_where_it_is_and_never_sends_the_file() {
        let mut chat = opened("system");
        chat.showing(Some(Path::new("/p")), Some(Path::new("/p/src/main.rs")));
        chat.configuration.system = "Be terse.".to_owned();
        let prompt = chat.system_prompt();
        assert!(prompt.contains("Quill"), "{prompt}");
        assert!(prompt.contains("/p/src/main.rs"), "{prompt}");
        assert!(prompt.contains("Be terse."), "{prompt}");
        assert!(
            !prompt.contains("tools you have been given"),
            "tools are off by default"
        );
        chat.configuration.tools = true;
        assert!(chat.system_prompt().contains("tools you have been given"));
    }

    #[test]
    fn a_tool_call_becomes_a_request_the_window_runs_and_its_answer_goes_back_up() {
        let mut chat = opened("tools");
        chat.configuration.tools = true;
        chat.session
            .ask(Message::said(0, Role::User, "what does git say?"));
        chat.session.reply(quill_chat::Reply::ToolCall {
            id: "t1".to_owned(),
            name: "quill_git".to_owned(),
            arguments: "{\"command\":\"status\"}".to_owned(),
        });
        chat.session.reply(quill_chat::Reply::Finished {
            reason: "tool_use".to_owned(),
        });
        chat.ask_for_the_tools();
        let asked = chat.asking();
        assert_eq!(asked.len(), 1);
        let Request::RunCommand { id, command, .. } = &asked[0] else {
            panic!("{asked:?}");
        };
        assert_eq!(id, "t1");
        assert_eq!(command, "git.status");

        // The answer goes back into the conversation as a tool result. Found by its role rather than
        // as the last message, because answering the last outstanding tool is what starts the next
        // round — and here that round is refused for want of a key, which is a message of its own.
        chat.answered("t1", Ok(serde_json::json!({ "branch": "main" })));
        let results = chat
            .session
            .chat
            .messages
            .iter()
            .find(|message| message.role == Role::Tool)
            .expect("a result message");
        assert!(results.tools[0].answer.as_deref().expect("an answer").contains("main"));
    }

    #[test]
    fn a_tool_that_cannot_be_resolved_answers_at_once_rather_than_hanging_the_turn() {
        let mut chat = opened("bad-tool");
        chat.configuration.tools = true;
        chat.session.ask(Message::said(0, Role::User, "do something odd"));
        chat.session.reply(quill_chat::Reply::ToolCall {
            id: "t1".to_owned(),
            name: "quill_levitate".to_owned(),
            arguments: "{}".to_owned(),
        });
        chat.session.reply(quill_chat::Reply::Finished {
            reason: "tool_use".to_owned(),
        });
        chat.ask_for_the_tools();
        assert!(chat.asking().is_empty(), "nothing is asked of the window");
        let results = chat
            .session
            .chat
            .messages
            .iter()
            .find(|message| message.role == Role::Tool)
            .expect("a result message");
        assert!(results.tools[0].failed);
        assert!(results.tools[0].answer.as_deref().expect("a reason").contains("quill_levitate"));
    }

    #[test]
    fn a_tool_answer_is_cut_short_rather_than_billed_whole() {
        let long = serde_json::Value::String("x".repeat(20_000));
        let cut = shorten_for_a_model(&long);
        assert!(cut.len() < 9_000, "{}", cut.len());
        assert!(
            cut.ends_with("characters."),
            "the cut says so rather than being silent"
        );
        // A short one is untouched, and null is `ok` rather than the word `null`.
        assert_eq!(shorten_for_a_model(&serde_json::json!("fine")), "fine");
        assert_eq!(shorten_for_a_model(&serde_json::Value::Null), "ok");
        // And a cut never lands in the middle of a character.
        let wide = serde_json::Value::String("é".repeat(20_000));
        assert!(shorten_for_a_model(&wide).len() > 0);
    }

    #[test]
    fn a_conversation_is_written_when_the_turn_ends_and_read_back_when_the_pane_is_opened_again() {
        let folder = a_folder("persist");
        let mut chat = AgentChat::new();
        chat.open(&Context {
            folder: Some(folder.clone()),
            ..Context::default()
        })
        .expect("opened");
        chat.session.ask(Message::said(0, Role::User, "Remember me"));
        chat.session.reply(quill_chat::Reply::Text("I will.".to_owned()));
        chat.session.reply(quill_chat::Reply::Finished {
            reason: "stop".to_owned(),
        });
        chat.dirty = true;
        chat.write_the_conversation();

        let mut again = AgentChat::new();
        again
            .open(&Context {
                folder: Some(folder),
                ..Context::default()
            })
            .expect("opened");
        assert_eq!(again.session.chat.messages.len(), 2);
        assert_eq!(again.session.chat.messages[0].text(), "Remember me");
        assert_eq!(again.history().len(), 1);
    }

    #[test]
    fn a_picture_is_sniffed_from_its_bytes_rather_than_trusted_from_its_name() {
        // A screenshot saved as `.png` that is really a JPEG is a thing that happens, and an API
        // told the wrong media type refuses the whole request.
        assert_eq!(
            media_type_of(Path::new("shot.png"), &[0xFF, 0xD8, 0xFF, 0xE0]).as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            media_type_of(Path::new("shot.png"), &[]).as_deref(),
            Some("image/png")
        );
        assert_eq!(media_type_of(Path::new("notes.txt"), &[1, 2, 3]), None);
    }

    #[test]
    fn attaching_and_taking_off_a_picture_leaves_the_draft_alone() {
        let mut chat = opened("attach");
        chat.draft = "look".to_owned();
        chat.attach_bytes("a.png".to_owned(), "image/png".to_owned(), vec![1, 2, 3]);
        chat.attach_bytes("b.png".to_owned(), "image/png".to_owned(), vec![4]);
        assert_eq!(chat.attachments.len(), 2);
        let first = chat.attachments[0].id;
        chat.remove_attachment(first);
        assert_eq!(chat.attachments.len(), 1);
        assert_eq!(chat.attachments[0].name, "b.png");
        assert_eq!(chat.draft, "look");
    }

    #[test]
    fn every_command_it_lists_is_a_command_it_answers() {
        // The rule `every_registered_provider_can_be_built` keeps for the registry, kept for the
        // commands: one listed with no arm would be a command `plugins show` offers and `plugins run`
        // refuses.
        let mut chat = opened("commands");
        for (name, help) in chat.commands() {
            assert!(!help.is_empty(), "{name} says nothing");
            let answered = chat.command(name, &[]);
            // Some of them need an argument, and refusing for want of one is still answering.
            if let Err(problem) = answered {
                assert!(
                    !problem.contains("is not one of Agent-Chat's commands"),
                    "{name} is listed and not answered: {problem}"
                );
            }
        }
        assert!(chat.command("levitate", &[]).is_err());
    }

    #[test]
    fn the_view_answers_what_the_pane_is_showing_rather_than_a_screenshot() {
        let mut chat = opened("view");
        chat.session.ask(Message::said(0, Role::User, "hello"));
        let view = chat.view();
        assert_eq!(view["state"], "sending");
        assert_eq!(view["messages"], 1);
        assert!(view["total"].is_null(), "a chat's count is not drawn beside its pane's name");
        assert_eq!(view["conversation"]["messages"][0]["text"], "hello");
        assert_eq!(view["tools"], false);
        assert!(view["provider"]["name"].is_string());
    }
}
