//! One session with an agent, as the desk holds it: the thread that the
//! agent's updates build, what it is doing now, and what it offers to be
//! changed (its modes and its options, such as the model).
//!
//! Everything here comes from the agent over ACP. The desk adds only what
//! you said, the permission questions as they were answered, and what went
//! wrong between the two.

use std::path::PathBuf;

use agent_client_protocol_schema::v1::{
    ContentBlock, EmbeddedResourceResource, PermissionOption, Plan, RequestPermissionRequest,
    SessionConfigOption, SessionId, SessionModeState, SessionUpdate, StopReason, ToolCall,
    ToolCallId, ToolCallUpdate, UsageUpdate,
};
use serde_json::Value;

/// What a session is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing said yet: the agent is asked for a session with the first
    /// thing you say, so that a session begun and left costs nothing.
    Draft,
    /// The agent is starting, or opening the session.
    Opening,
    /// The agent asks to be signed in before it opens the session.
    SignIn,
    /// Waiting for you.
    Idle,
    /// Working on what you said.
    Working,
    /// It could not be opened or its agent stopped: why.
    Broken(String),
}

/// A question the agent asked, and what was chosen.
#[derive(Debug, Clone)]
pub struct Asked {
    /// The JSON-RPC id the answer goes back under.
    pub id: Value,
    pub title: String,
    pub options: Vec<PermissionOption>,
    /// The name of the option chosen, or `None` while it waits; "Cancelled"
    /// when the turn was stopped first.
    pub chosen: Option<String>,
}

/// One thing in the thread.
#[derive(Debug, Clone)]
pub enum Item {
    You(String),
    /// What the agent said, as it streamed in.
    Agent(String),
    /// What it thought aloud.
    Thought(String),
    Tool(Box<ToolCall>),
    Asked(Asked),
    /// Something the desk or the agent wants seen: a notice, a refusal.
    Note {
        text: String,
        trouble: bool,
    },
    /// A turn ended other than by the agent finishing it.
    Ended(StopReason),
}

#[derive(Debug, Clone)]
pub struct Session {
    /// The desk's own key for it, stable from before the agent names it.
    pub key: u64,
    /// The registry id of its agent.
    pub agent: String,
    /// The agent's id for it, once opened.
    pub id: Option<SessionId>,
    pub folder: PathBuf,
    pub title: String,
    pub state: State,
    pub items: Vec<Item>,
    pub plan: Option<Plan>,
    pub modes: Option<SessionModeState>,
    pub options: Vec<SessionConfigOption>,
    pub usage: Option<UsageUpdate>,
    /// When the turn going on started, for the time beside it.
    pub since: Option<std::time::Instant>,
    /// What you said before the session was open, sent once it is.
    pub pending: Option<String>,
}

/// The text of a content block, or a word for what it is.
pub fn text_of(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text(t) => t.text.clone(),
        ContentBlock::Image(_) => "[image]".into(),
        ContentBlock::Audio(_) => "[audio]".into(),
        ContentBlock::ResourceLink(r) => format!("[{}]({})", r.name, r.uri),
        ContentBlock::Resource(r) => match &r.resource {
            EmbeddedResourceResource::TextResourceContents(t) => t.text.clone(),
            EmbeddedResourceResource::BlobResourceContents(b) => format!("[{}]", b.uri),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

impl Session {
    pub fn new(key: u64, agent: &str, folder: PathBuf) -> Self {
        let title = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        Self {
            key,
            agent: agent.to_owned(),
            id: None,
            folder,
            title,
            state: State::Draft,
            items: Vec::new(),
            plan: None,
            modes: None,
            options: Vec::new(),
            usage: None,
            since: None,
            pending: None,
        }
    }

    /// What you said, put in the thread as the turn begins.
    pub fn said(&mut self, text: &str) {
        if self.items.is_empty() {
            self.title = text
                .lines()
                .next()
                .unwrap_or(text)
                .chars()
                .take(60)
                .collect();
        }
        self.items.push(Item::You(text.to_owned()));
        self.state = State::Working;
        self.since = Some(std::time::Instant::now());
    }

    /// The turn is over.
    pub fn ended(&mut self, reason: Result<StopReason, String>) {
        match reason {
            Ok(StopReason::EndTurn) => {}
            Ok(reason) => self.items.push(Item::Ended(reason)),
            Err(why) => self.items.push(Item::Note {
                text: why,
                trouble: true,
            }),
        }
        for item in &mut self.items {
            if let Item::Asked(a) = item
                && a.chosen.is_none()
            {
                a.chosen = Some("Cancelled".into());
            }
        }
        self.state = State::Idle;
        self.since = None;
    }

    /// Take in one update from the agent.
    pub fn apply(&mut self, update: SessionUpdate) {
        match update {
            SessionUpdate::UserMessageChunk(c) => {
                // A session being reopened replays what you said.
                let text = text_of(&c.content);
                match self.items.last_mut() {
                    Some(Item::You(t)) => t.push_str(&text),
                    _ => self.items.push(Item::You(text)),
                }
            }
            SessionUpdate::AgentMessageChunk(c) => {
                let text = text_of(&c.content);
                match self.items.last_mut() {
                    Some(Item::Agent(t)) => t.push_str(&text),
                    _ => self.items.push(Item::Agent(text)),
                }
            }
            SessionUpdate::AgentThoughtChunk(c) => {
                let text = text_of(&c.content);
                match self.items.last_mut() {
                    Some(Item::Thought(t)) => t.push_str(&text),
                    _ => self.items.push(Item::Thought(text)),
                }
            }
            SessionUpdate::ToolCall(call) => {
                if let Some(old) = self.tool_mut(&call.tool_call_id) {
                    *old = call;
                } else {
                    self.items.push(Item::Tool(Box::new(call)));
                }
            }
            SessionUpdate::ToolCallUpdate(update) => self.update_tool(update),
            SessionUpdate::Plan(plan) => self.plan = Some(plan),
            SessionUpdate::CurrentModeUpdate(m) => {
                if let Some(modes) = &mut self.modes {
                    modes.current_mode_id = m.current_mode_id;
                }
            }
            SessionUpdate::ConfigOptionUpdate(o) => self.options = o.config_options,
            SessionUpdate::SessionInfoUpdate(info) => {
                if let Some(title) = info.title.take().filter(|t| !t.is_empty()) {
                    self.title = title;
                }
            }
            SessionUpdate::UsageUpdate(u) => self.usage = Some(u),
            SessionUpdate::Notice(n) => self.items.push(Item::Note {
                text: match n.description {
                    Some(d) => format!("{} — {d}", n.title),
                    None => n.title,
                },
                trouble: false,
            }),
            _ => {}
        }
    }

    fn tool_mut(&mut self, id: &ToolCallId) -> Option<&mut ToolCall> {
        self.items.iter_mut().rev().find_map(|i| match i {
            Item::Tool(t) if &t.tool_call_id == id => Some(&mut **t),
            _ => None,
        })
    }

    fn update_tool(&mut self, update: ToolCallUpdate) {
        if let Some(call) = self.tool_mut(&update.tool_call_id) {
            call.update(update.fields);
        } else if let Ok(call) = ToolCall::try_from(update) {
            self.items.push(Item::Tool(Box::new(call)));
        }
    }

    /// The agent asks a person before it goes on.
    pub fn asked(&mut self, id: Value, request: RequestPermissionRequest) {
        let title = request
            .tool_call
            .fields
            .title
            .clone()
            .or_else(|| {
                self.tool_mut(&request.tool_call.tool_call_id)
                    .map(|t| t.title.clone())
            })
            .unwrap_or_else(|| "The agent asks to go on".into());
        self.update_tool(request.tool_call);
        self.items.push(Item::Asked(Asked {
            id,
            title,
            options: request.options,
            chosen: None,
        }));
    }

    /// The question waiting on you, if one is.
    pub fn waiting(&self) -> Option<&Asked> {
        self.items.iter().rev().find_map(|i| match i {
            Item::Asked(a) if a.chosen.is_none() => Some(a),
            _ => None,
        })
    }

    /// Note that the question asked under `id` was answered with `name`.
    pub fn answered(&mut self, id: &Value, name: &str) {
        for item in &mut self.items {
            if let Item::Asked(a) = item
                && &a.id == id
            {
                a.chosen = Some(name.to_owned());
            }
        }
    }

    /// The files the agent changed in this session, each once, in the order
    /// first changed, with the lines added and taken away.
    pub fn changed(&self) -> Vec<(PathBuf, usize, usize)> {
        use agent_client_protocol_schema::v1::ToolCallContent;
        let mut files: Vec<(PathBuf, usize, usize)> = Vec::new();
        for item in &self.items {
            let Item::Tool(t) = item else { continue };
            for c in &t.content {
                let ToolCallContent::Diff(d) = c else {
                    continue;
                };
                let lines =
                    crate::thread::kinds::diff(d.old_text.as_deref().unwrap_or(""), &d.new_text);
                let (gone, added) = crate::thread::kinds::counted(&lines);
                match files.iter_mut().find(|f| f.0 == d.path) {
                    Some(f) => {
                        f.1 += added;
                        f.2 += gone;
                    }
                    None => files.push((d.path.clone(), added, gone)),
                }
            }
        }
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(json: &str) -> SessionUpdate {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn chunks_of_one_message_join_and_a_tool_call_between_starts_a_new_one() {
        let mut s = Session::new(1, "codex-acp", "/tmp/repo".into());
        s.said("fix it");
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Look"}}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ing"}}"#,
        ));
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Read lib.rs","kind":"read","status":"in_progress"}"#));
        s.apply(update(
            r#"{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed"}"#,
        ));
        s.apply(update(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done"}}"#,
        ));
        assert_eq!(s.title, "fix it");
        assert!(matches!(&s.items[1], Item::Agent(t) if t == "Looking"));
        assert!(
            matches!(&s.items[2], Item::Tool(t) if t.status == agent_client_protocol_schema::v1::ToolCallStatus::Completed)
        );
        assert!(matches!(&s.items[3], Item::Agent(t) if t == "Done"));
    }

    #[test]
    fn a_question_waits_until_answered_and_a_stopped_turn_cancels_it() {
        let mut s = Session::new(1, "claude-acp", "/tmp".into());
        s.said("go");
        let request: RequestPermissionRequest = serde_json::from_str(
            r#"{"sessionId":"s","toolCall":{"toolCallId":"t","title":"rm -rf build"},"options":[{"optionId":"a","name":"Allow","kind":"allow_once"}]}"#,
        )
        .unwrap();
        s.asked(Value::from(5), request);
        assert_eq!(s.waiting().unwrap().title, "rm -rf build");
        s.ended(Ok(StopReason::Cancelled));
        assert!(s.waiting().is_none());
        assert!(matches!(
            s.items.last(),
            Some(Item::Ended(StopReason::Cancelled))
        ));
    }

    #[test]
    fn the_files_changed_are_counted_once_each() {
        let mut s = Session::new(1, "x", "/r".into());
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"Edit","kind":"edit","content":[{"type":"diff","path":"/r/a.rs","oldText":"x\n","newText":"y\nz\n"}]}"#));
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"b","title":"Edit","kind":"edit","content":[{"type":"diff","path":"/r/a.rs","oldText":"y\n","newText":"w\n"}]}"#));
        let files = s.changed();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, PathBuf::from("/r/a.rs"));
        assert_eq!((files[0].1, files[0].2), (3, 2));
    }

    #[test]
    fn codexs_own_answer_to_a_new_session_is_read_with_its_modes_and_options() {
        // Recorded from @agentclientprotocol/codex-acp 2.10.0.
        let r: agent_client_protocol_schema::v1::NewSessionResponse =
            serde_json::from_str(include_str!("fixtures/codex-session-new.json")).unwrap();
        assert!(r.modes.is_some());
        let options = r.config_options.unwrap();
        assert!(options.iter().any(|o| &*o.id.0 == "model"));
    }
}
