//! The conversations on disk: one JSON file each, in the plugin's own folder.
//!
//! **One file a conversation rather than one file for all of them**, which is the opposite of the
//! choice `services::file_marks` made about highlights, and the reason is the opposite too: there
//! are six hundred source files in a project and opening six hundred files when a project opens
//! would be absurd, while there are twenty conversations and only the one being read is ever opened.
//! A single file would have to be rewritten whole on every token that arrived.
//!
//! **JSON rather than the `name = value` store the settings use**, because a conversation is a tree
//! — messages holding parts holding bytes — and `store::Values` is a flat map. The settings *are* a
//! flat map and go on using it.
//!
//! Written only when something changed, and only by the released binary, which is the rule
//! `services::project_state` and `services::file_marks` already keep: a test must not write into the
//! settings of the person running it, so a store with no folder is a store that does nothing.

use std::path::{Path, PathBuf};

use unluminate_chat::base64;
use unluminate_chat::model::{Conversation, Message, Part, Role, ToolCall, Usage};

/// The folder inside the plugin's own folder that the conversations live in.
const FOLDER: &str = "conversations";

/// One line in the history list: enough to draw a row without reading the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub provider: String,
    /// Seconds since the epoch, so the newest is first.
    pub changed: u64,
    pub messages: usize,
}

/// Where the conversations are, or `None` in a test with no folder.
#[derive(Debug, Default, Clone)]
pub struct Store {
    folder: Option<PathBuf>,
}

impl Store {
    pub fn at(folder: Option<PathBuf>) -> Self {
        Self {
            folder: folder.map(|folder| folder.join(FOLDER)),
        }
    }

    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// An id nothing else has, which is also the file's name.
    ///
    /// The clock plus a counter, because two conversations started inside one second are a thing
    /// somebody does by pressing the new-chat button twice.
    pub fn new_id(&self) -> String {
        let now = seconds_now();
        let mut id = format!("{now:010}");
        let mut count = 0;
        while self.path_of(&id).is_some_and(|path| path.exists()) {
            count += 1;
            id = format!("{now:010}-{count}");
        }
        id
    }

    fn path_of(&self, id: &str) -> Option<PathBuf> {
        if !is_a_safe_id(id) {
            return None;
        }
        self.folder
            .as_ref()
            .map(|folder| folder.join(format!("{id}.json")))
    }

    /// Every conversation there is, newest first.
    ///
    /// Read from each file's own recorded time rather than from its modification time, so copying
    /// the folder between machines does not reorder somebody's history.
    pub fn list(&self, limit: usize) -> Vec<Summary> {
        let Some(folder) = &self.folder else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut found: Vec<Summary> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|kind| kind == "json"))
            .filter_map(|entry| summarise(&entry.path()))
            .collect();
        found.sort_by(|left, right| right.changed.cmp(&left.changed).then(right.id.cmp(&left.id)));
        found.truncate(limit);
        found
    }

    /// Read one conversation back.
    pub fn read(&self, id: &str) -> Option<Conversation> {
        let path = self.path_of(id)?;
        let text = std::fs::read_to_string(path).ok()?;
        let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
        Some(from_json(id, &value))
    }

    /// Write one conversation out, making the folder if it is not there.
    ///
    /// An empty conversation is **removed** rather than written, so pressing new-chat and changing
    /// your mind does not leave a row saying `New chat` in the history for ever.
    pub fn write(&self, chat: &Conversation) -> Result<(), String> {
        let Some(path) = self.path_of(&chat.id) else {
            return Ok(());
        };
        if chat.messages.is_empty() {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        let folder = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(folder)
            .map_err(|problem| format!("{} could not be made: {problem}", folder.display()))?;
        let text = serde_json::to_string(&to_json(chat))
            .map_err(|problem| format!("the conversation could not be written: {problem}"))?;
        std::fs::write(&path, text)
            .map_err(|problem| format!("{} could not be written: {problem}", path.display()))
    }

    /// Throw one away.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let Some(path) = self.path_of(id) else {
            return Ok(());
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(problem) if problem.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(problem) => Err(format!("{} could not be removed: {problem}", path.display())),
        }
    }

    /// Take away everything past `keep`, oldest first.
    ///
    /// Called after a conversation is written, so the folder is bounded by the `chat.history`
    /// setting rather than growing for ever the way `recent.txt` deliberately does not.
    pub fn tidy(&self, keep: usize) {
        let all = self.list(usize::MAX);
        for old in all.into_iter().skip(keep) {
            let _ = self.remove(&old.id);
        }
    }
}

/// Seconds since the epoch, or zero on a clock that will not answer.
pub fn seconds_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// A file name that cannot escape the folder it is in.
///
/// `plugins run agent-chat open ../../../etc/passwd` is a thing an agent will type by accident, and
/// a store that joined it would read whatever it named. Letters, digits and a dash, which is what
/// `new_id` produces — the same rule `keychain::is_a_safe_name` keeps for a keychain entry.
fn is_a_safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|one| one.is_ascii_alphanumeric() || one == '-')
}

/// One file's summary, read without building the whole conversation.
fn summarise(path: &Path) -> Option<Summary> {
    let id = path.file_stem()?.to_str()?.to_owned();
    let text = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    Some(Summary {
        name: value["name"].as_str().unwrap_or("New chat").to_owned(),
        provider: value["provider"].as_str().unwrap_or_default().to_owned(),
        changed: value["changed"].as_u64().unwrap_or(0),
        messages: value["messages"].as_array().map(Vec::len).unwrap_or(0),
        id,
    })
}

/// A conversation as it is written down.
///
/// Pictures are base64 inside it, which is what makes a conversation reopened after the file it came
/// from has moved still show what was really sent — `model::Part::Picture`'s own reason for holding
/// bytes rather than a path, carried through to disk.
fn to_json(chat: &Conversation) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "name": chat.name,
        "provider": chat.provider,
        "changed": chat.changed,
        "usage": { "input": chat.usage.input, "output": chat.usage.output },
        "messages": chat.messages.iter().map(|message| serde_json::json!({
            "id": message.id,
            "role": message.role.wire_name(),
            "thinking": message.thinking,
            "finish": message.finish,
            "failure": message.failure,
            "parts": message.parts.iter().map(|part| match part {
                Part::Text(text) => serde_json::json!({ "type": "text", "text": text }),
                Part::Picture { media, bytes, name } => serde_json::json!({
                    "type": "picture",
                    "media": media,
                    "name": name,
                    "data": base64::encode(bytes),
                }),
            }).collect::<Vec<serde_json::Value>>(),
            "tools": message.tools.iter().map(|tool| serde_json::json!({
                "id": tool.id,
                "name": tool.name,
                "arguments": tool.arguments,
                "answer": tool.answer,
                "failed": tool.failed,
                "took": tool.took,
            })).collect::<Vec<serde_json::Value>>(),
        })).collect::<Vec<serde_json::Value>>(),
    })
}

/// The same, read back.
///
/// Every field has a default, so a file written by an older version — or one somebody edited by hand
/// — opens as much of itself as still makes sense rather than refusing. A conversation is a
/// transcript, and half of one is worth more than none.
fn from_json(id: &str, value: &serde_json::Value) -> Conversation {
    let mut chat = Conversation::new(id, value["provider"].as_str().unwrap_or_default());
    chat.name = value["name"].as_str().unwrap_or_default().to_owned();
    chat.changed = value["changed"].as_u64().unwrap_or(0);
    chat.usage = Usage {
        input: value["usage"]["input"].as_u64().unwrap_or(0),
        output: value["usage"]["output"].as_u64().unwrap_or(0),
    };
    for one in value["messages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let role = match one["role"].as_str() {
            Some("user") => Role::User,
            Some("tool") => Role::Tool,
            _ => Role::Assistant,
        };
        let mut message = Message::new(one["id"].as_u64().unwrap_or(0), role);
        message.thinking = one["thinking"].as_str().unwrap_or_default().to_owned();
        message.finish = one["finish"].as_str().map(str::to_owned);
        message.failure = one["failure"].as_str().map(str::to_owned);
        for part in one["parts"].as_array().map(Vec::as_slice).unwrap_or_default() {
            match part["type"].as_str() {
                Some("picture") => {
                    if let Some(bytes) = base64::decode(part["data"].as_str().unwrap_or_default()) {
                        message.parts.push(Part::Picture {
                            media: part["media"].as_str().unwrap_or("image/png").to_owned(),
                            name: part["name"].as_str().unwrap_or("picture").to_owned(),
                            bytes,
                        });
                    }
                }
                _ => message
                    .parts
                    .push(Part::Text(part["text"].as_str().unwrap_or_default().to_owned())),
            }
        }
        for tool in one["tools"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let mut call = ToolCall::new(
                tool["id"].as_str().unwrap_or_default(),
                tool["name"].as_str().unwrap_or_default(),
                tool["arguments"].as_str().unwrap_or("{}"),
            );
            call.answer = tool["answer"].as_str().map(str::to_owned);
            call.failed = tool["failed"].as_bool().unwrap_or(false);
            call.took = tool["took"].as_u64();
            message.tools.push(call);
        }
        chat.push(message);
    }
    chat
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("unluminate-agent-chat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    #[test]
    fn a_conversation_with_a_picture_in_it_comes_back_byte_for_byte() {
        let store = Store::at(Some(a_folder("round-trip")));
        let mut chat = Conversation::new(store.new_id(), "claude");
        let mut asked = Message::said(1, Role::User, "What is this?");
        asked.parts.push(Part::Picture {
            media: "image/png".to_owned(),
            bytes: vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF],
            name: "shot.png".to_owned(),
        });
        chat.push(asked);
        let mut answered = Message::said(2, Role::Assistant, "A board.");
        answered.thinking = "looking".to_owned();
        answered.tools.push(ToolCall::new("t1", "unluminate_git", "{}"));
        answered.tools[0].answer = Some("clean".to_owned());
        answered.tools[0].took = Some(12);
        chat.push(answered);
        chat.usage = Usage { input: 20, output: 4 };
        chat.changed = 1234;

        store.write(&chat).expect("written");
        let read = store.read(&chat.id).expect("read back");
        assert_eq!(read.name, chat.name);
        assert_eq!(read.usage, chat.usage);
        assert_eq!(read.messages.len(), 2);
        assert_eq!(
            read.messages[0].pictures().next().expect("a picture").2,
            &[0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF]
        );
        assert_eq!(read.messages[1].thinking, "looking");
        assert_eq!(read.messages[1].tools[0].answer.as_deref(), Some("clean"));
        assert_eq!(read.messages[1].tools[0].took, Some(12));
    }

    #[test]
    fn the_history_is_newest_first_and_is_bounded() {
        let store = Store::at(Some(a_folder("history")));
        for index in 0..5 {
            let mut chat = Conversation::new(format!("00000000{index}"), "claude");
            chat.push(Message::said(1, Role::User, format!("number {index}")));
            chat.changed = 100 + index as u64;
            store.write(&chat).expect("written");
        }
        let all = store.list(usize::MAX);
        assert_eq!(all.len(), 5);
        assert_eq!(all[0].name, "number 4", "newest first");
        assert_eq!(all[0].messages, 1);
        assert_eq!(store.list(2).len(), 2);

        // And tidying keeps the newest, which is what bounds the folder.
        store.tidy(2);
        let left = store.list(usize::MAX);
        assert_eq!(left.len(), 2);
        assert_eq!(left[0].name, "number 4");
        assert_eq!(left[1].name, "number 3");
    }

    #[test]
    fn an_empty_conversation_is_removed_rather_than_written() {
        // Pressing new-chat and changing your mind must not leave a row saying `New chat` for ever.
        let store = Store::at(Some(a_folder("empty")));
        let mut chat = Conversation::new(store.new_id(), "claude");
        chat.push(Message::said(1, Role::User, "hello"));
        store.write(&chat).expect("written");
        assert_eq!(store.list(usize::MAX).len(), 1);
        chat.messages.clear();
        store.write(&chat).expect("written");
        assert_eq!(store.list(usize::MAX).len(), 0);
    }

    #[test]
    fn an_id_that_tries_to_leave_the_folder_reads_and_writes_nothing() {
        // `plugins run agent-chat open ../../secrets` is something an agent types by accident.
        let store = Store::at(Some(a_folder("escape")));
        assert!(store.read("../../../etc/passwd").is_none());
        assert!(store.read("with/slash").is_none());
        assert!(store.read("with\\backslash").is_none());
        let mut escaping = Conversation::new("../escape", "claude");
        escaping.push(Message::said(1, Role::User, "x"));
        assert!(
            store.write(&escaping).is_ok(),
            "refused quietly rather than failing"
        );
        assert_eq!(store.list(usize::MAX).len(), 0);
    }

    #[test]
    fn a_store_with_no_folder_does_nothing_at_all() {
        // Which is what stops a test writing into the settings of the person running it — the rule
        // `UnluminateApp::load_settings` and `services::file_marks` already keep.
        let store = Store::at(None);
        let mut chat = Conversation::new("c1", "claude");
        chat.push(Message::said(1, Role::User, "x"));
        assert!(store.write(&chat).is_ok());
        assert!(store.read("c1").is_none());
        assert!(store.list(10).is_empty());
    }

    #[test]
    fn two_conversations_started_in_one_second_get_different_ids() {
        let store = Store::at(Some(a_folder("ids")));
        let first = store.new_id();
        let mut chat = Conversation::new(first.clone(), "claude");
        chat.push(Message::said(1, Role::User, "x"));
        store.write(&chat).expect("written");
        let second = store.new_id();
        assert_ne!(first, second);
    }

    #[test]
    fn a_file_written_by_something_else_opens_as_much_of_itself_as_makes_sense() {
        // A transcript is worth having in pieces; refusing to open one because a field it has never
        // heard of is missing would throw away somebody's conversation.
        let folder = a_folder("partial");
        std::fs::create_dir_all(folder.join(FOLDER)).expect("the folder");
        std::fs::write(
            folder.join(FOLDER).join("abc.json"),
            "{\"name\":\"half\",\"messages\":[{\"role\":\"user\",\"parts\":[{\"type\":\"text\",\"text\":\"hi\"}]}]}",
        )
        .expect("written");
        let store = Store::at(Some(folder));
        let read = store.read("abc").expect("read");
        assert_eq!(read.name, "half");
        assert_eq!(read.messages[0].text(), "hi");
        assert_eq!(read.usage, Usage::default());
        // And something that is not JSON at all is not a conversation.
        std::fs::write(store.folder().expect("a folder").join("bad.json"), "not json").expect("written");
        assert!(store.read("bad").is_none());
        assert_eq!(
            store.list(usize::MAX).len(),
            1,
            "the unreadable one is skipped, not fatal"
        );
    }
}
