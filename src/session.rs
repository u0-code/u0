//! One session with an agent, as the desk holds it: the thread that the
//! agent's updates build, what it is doing now, and what it offers to be
//! changed (its modes and its options, such as the model).
//!
//! Everything here comes from the agent over ACP. The desk adds only what
//! you said, the permission questions as they were answered, when things
//! started and ended, and what went wrong between the two.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use agent_client_protocol_schema::v1::{
    ContentBlock, EmbeddedResourceResource, PermissionOption, PermissionOptionKind, Plan,
    RequestPermissionRequest, SessionConfigOption, SessionId, SessionInfo, SessionModeState,
    SessionUpdate, StopReason, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolKind, UsageUpdate,
};
use serde_json::Value;

use crate::git;

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
    /// Kept by its agent, not open here: opening it loads what was said
    /// in it from the agent.
    Closed,
    /// It could not be opened: why.
    Broken(String),
}

/// A question the agent asked, and what was chosen.
#[derive(Debug, Clone)]
pub struct Asked {
    /// The JSON-RPC id the answer goes back under.
    pub id: Value,
    pub title: String,
    pub options: Vec<PermissionOption>,
    /// The option chosen, by its name and kind, or `None` while it waits.
    /// A turn stopped first answers it as rejected, named "Cancelled".
    pub chosen: Option<(String, PermissionOptionKind)>,
}

/// A tool call, with when it started and how long it took, as the desk saw.
#[derive(Debug, Clone)]
pub struct Tool {
    pub call: ToolCall,
    pub started: Instant,
    pub took: Option<Duration>,
    /// For each diff in the call's content, the line of the file its new
    /// text starts on, found once the call is done; `None` where the file
    /// could not be read or no longer holds it.
    pub starts: Vec<Option<usize>>,
}

/// One thing in the thread.
#[derive(Debug, Clone)]
pub enum Item {
    You(String),
    /// What the agent said, as it streamed in.
    Agent(String),
    /// What it thought aloud, and for how long once it moved on.
    Thought {
        text: String,
        secs: Option<u64>,
    },
    Tool(Box<Tool>),
    Asked(Asked),
    /// The agent's plan as it stood at this point.
    Plan(Plan),
    /// Something the desk or the agent wants seen: a notice, a refusal.
    Note {
        text: String,
        trouble: bool,
    },
    /// A turn ended, why, and after how long.
    Ended {
        reason: StopReason,
        secs: u64,
    },
}

/// How a file was changed in the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    New,
    Edited,
    Deleted,
}

/// A file the session changed, with its lines added and taken away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub path: PathBuf,
    pub change: Change,
    pub added: usize,
    pub gone: usize,
}

/// The checkpoints of one turn, and what changed in it, once known.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    pub before: Option<String>,
    pub after: Option<String>,
    /// What changed from before to after.
    pub diff: Option<Vec<git::FileDiff>>,
}

/// What the last git action said: a commit, a push, a pull request.
#[derive(Debug, Clone)]
pub struct GitNote {
    pub text: String,
    pub trouble: bool,
    /// An address to open: a pull request.
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Session {
    /// The desk's own key for it, stable from before the agent names it.
    pub key: u64,
    /// The registry id of its agent.
    pub agent: String,
    /// The agent's id for it, once opened.
    pub id: Option<SessionId>,
    /// The folder it works in: its project, or its worktree once it has one.
    pub folder: PathBuf,
    /// The project it is listed under: the folder it was begun in, which
    /// stays when it moves into a worktree.
    pub project: PathBuf,
    /// The git branch the folder was on when the session began.
    pub branch: Option<String>,
    /// Whether the folder is in a git repository: then each turn is
    /// checkpointed, and can be undone.
    pub repo: bool,
    /// For a draft: whether it is to work in a worktree of its own.
    pub wants_worktree: bool,
    /// The worktree it works in, when it has one.
    pub worktree: Option<git::Worktree>,
    /// Each turn's checkpoints, by turn, the first turn first.
    pub turns: Vec<Turn>,
    /// What changed from before the first turn to after the latest.
    pub total: Option<Vec<git::FileDiff>>,
    /// A turn undone, and the checkpoint that puts it back.
    pub redo: Option<(usize, String)>,
    pub git_note: Option<GitNote>,
    pub title: String,
    pub state: State,
    pub items: Vec<Item>,
    pub plan: Option<Plan>,
    pub modes: Option<SessionModeState>,
    pub options: Vec<SessionConfigOption>,
    pub usage: Option<UsageUpdate>,
    /// When it began, and when anything last happened in it.
    pub begun: SystemTime,
    pub updated: SystemTime,
    /// When the turn going on started, for the time beside it.
    pub since: Option<Instant>,
    /// When the thought being streamed began.
    thinking: Option<Instant>,
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

/// The line of `file` that `text` starts on, counting from one.
fn line_of(file: &Path, text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let whole = std::fs::read_to_string(file).ok()?;
    let at = whole.find(text)?;
    Some(whole[..at].matches('\n').count() + 1)
}

impl Session {
    pub fn new(key: u64, agent: &str, folder: PathBuf) -> Self {
        let mut s = Self::blank(key, agent, folder);
        s.look();
        s
    }

    /// A session its agent listed, in `project`: closed until opened.
    pub fn listed(key: u64, agent: &str, info: SessionInfo, project: PathBuf) -> Self {
        let mut s = Self::blank(key, agent, info.cwd);
        s.project = project;
        s.id = Some(info.session_id);
        if let Some(title) = info.title.filter(|t| !t.trim().is_empty()) {
            s.title = title;
        }
        if let Some(t) = info.updated_at.as_deref().and_then(crate::when::parse) {
            s.begun = t;
            s.updated = t;
        }
        s.state = State::Closed;
        s
    }

    /// Whether it can be opened where it is: not while the worktree it is
    /// to work in is being made.
    pub fn worktree_ready(&self) -> bool {
        !(self.wants_worktree && self.worktree.is_none())
    }

    /// Read the folder's branch, and whether it is in a repository.
    pub fn look(&mut self) {
        self.branch = git::branch(&self.folder);
        self.repo = git::root(&self.folder).is_some();
    }

    fn blank(key: u64, agent: &str, folder: PathBuf) -> Self {
        let title = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let now = SystemTime::now();
        Self {
            key,
            agent: agent.to_owned(),
            id: None,
            branch: None,
            repo: false,
            wants_worktree: false,
            worktree: None,
            turns: Vec::new(),
            total: None,
            redo: None,
            git_note: None,
            project: folder.clone(),
            folder,
            title,
            state: State::Draft,
            items: Vec::new(),
            plan: None,
            modes: None,
            options: Vec::new(),
            usage: None,
            begun: now,
            updated: now,
            since: None,
            thinking: None,
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
            self.begun = SystemTime::now();
        }
        self.items.push(Item::You(text.to_owned()));
        self.state = State::Working;
        self.since = Some(Instant::now());
        self.updated = SystemTime::now();
    }

    /// The turn is over.
    pub fn ended(&mut self, reason: Result<StopReason, String>) {
        self.thought_over();
        let secs = self.since.map_or(0, |t| t.elapsed().as_secs());
        match reason {
            Ok(reason) => self.items.push(Item::Ended { reason, secs }),
            Err(why) => self.items.push(Item::Note {
                text: why,
                trouble: true,
            }),
        }
        for item in &mut self.items {
            if let Item::Asked(a) = item
                && a.chosen.is_none()
            {
                a.chosen = Some(("Cancelled".into(), PermissionOptionKind::RejectOnce));
            }
        }
        self.state = State::Idle;
        self.since = None;
        self.updated = SystemTime::now();
    }

    /// A thought being streamed is over: it is timed.
    fn thought_over(&mut self) {
        if let Some(from) = self.thinking.take()
            && let Some(Item::Thought { secs, .. }) = self
                .items
                .iter_mut()
                .rev()
                .find(|i| matches!(i, Item::Thought { .. }))
        {
            *secs = Some(from.elapsed().as_secs().max(1));
        }
    }

    /// Take in one update from the agent.
    pub fn apply(&mut self, update: SessionUpdate) {
        self.updated = SystemTime::now();
        if !matches!(update, SessionUpdate::AgentThoughtChunk(_)) {
            self.thought_over();
        }
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
                if let Some(Item::Thought { text: t, .. }) = self.items.last_mut() {
                    t.push_str(&text);
                } else {
                    self.thinking = Some(Instant::now());
                    self.items.push(Item::Thought { text, secs: None });
                }
            }
            SessionUpdate::ToolCall(call) => {
                let id = call.tool_call_id.clone();
                if let Some(old) = self.tool_mut(&id) {
                    old.call = call;
                } else {
                    self.items.push(Item::Tool(Box::new(Tool {
                        call,
                        started: Instant::now(),
                        took: None,
                        starts: Vec::new(),
                    })));
                }
                self.settle(&id);
            }
            SessionUpdate::ToolCallUpdate(update) => self.update_tool(update),
            SessionUpdate::Plan(plan) => {
                match self.items.last_mut() {
                    Some(Item::Plan(p)) => *p = plan.clone(),
                    _ => self.items.push(Item::Plan(plan.clone())),
                }
                self.plan = Some(plan);
            }
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

    fn tool_mut(&mut self, id: &ToolCallId) -> Option<&mut Tool> {
        self.items.iter_mut().rev().find_map(|i| match i {
            Item::Tool(t) if &t.call.tool_call_id == id => Some(&mut **t),
            _ => None,
        })
    }

    fn update_tool(&mut self, update: ToolCallUpdate) {
        let id = update.tool_call_id.clone();
        if let Some(tool) = self.tool_mut(&id) {
            tool.call.update(update.fields);
        } else if let Ok(call) = ToolCall::try_from(update) {
            self.items.push(Item::Tool(Box::new(Tool {
                call,
                started: Instant::now(),
                took: None,
                starts: Vec::new(),
            })));
        }
        self.settle(&id);
    }

    /// A call that has just ended is timed, and its diffs placed in their
    /// files.
    fn settle(&mut self, id: &ToolCallId) {
        let Some(tool) = self.tool_mut(id) else {
            return;
        };
        let done = matches!(
            tool.call.status,
            ToolCallStatus::Completed | ToolCallStatus::Failed
        );
        if !done || tool.took.is_some() {
            return;
        }
        tool.took = Some(tool.started.elapsed());
        tool.starts = tool
            .call
            .content
            .iter()
            .filter_map(|c| match c {
                ToolCallContent::Diff(d) => Some(line_of(&d.path, &d.new_text)),
                _ => None,
            })
            .collect();
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
                    .map(|t| t.call.title.clone())
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

    /// Note that the question asked under `id` was answered with `option`.
    pub fn answered(&mut self, id: &Value, option: &PermissionOption) {
        for item in &mut self.items {
            if let Item::Asked(a) = item
                && &a.id == id
            {
                a.chosen = Some((option.name.clone(), option.kind));
            }
        }
    }

    /// The tool calls, in order.
    pub fn tools(&self) -> impl Iterator<Item = &Tool> {
        self.items.iter().filter_map(|i| match i {
            Item::Tool(t) => Some(&**t),
            _ => None,
        })
    }

    /// The commands the agent ran, in order.
    pub fn commands(&self) -> impl Iterator<Item = &Tool> {
        self.tools().filter(|t| t.call.kind == ToolKind::Execute)
    }

    /// The files the agent changed in this session, each once, in the order
    /// first changed, with the lines added and taken away.
    pub fn changed(&self) -> Vec<Changed> {
        let mut files: Vec<Changed> = Vec::new();
        for t in self.tools() {
            if t.call.kind == ToolKind::Delete {
                for l in &t.call.locations {
                    files.retain(|f| f.path != l.path);
                    files.push(Changed {
                        path: l.path.clone(),
                        change: Change::Deleted,
                        added: 0,
                        gone: 0,
                    });
                }
            }
            for c in &t.call.content {
                let ToolCallContent::Diff(d) = c else {
                    continue;
                };
                let lines =
                    crate::thread::kinds::diff(d.old_text.as_deref().unwrap_or(""), &d.new_text);
                let (gone, added) = crate::thread::kinds::counted(&lines);
                match files.iter_mut().find(|f| f.path == d.path) {
                    Some(f) => {
                        f.added += added;
                        f.gone += gone;
                    }
                    None => files.push(Changed {
                        path: d.path.clone(),
                        change: if d.old_text.is_none() {
                            Change::New
                        } else {
                            Change::Edited
                        },
                        added,
                        gone,
                    }),
                }
            }
        }
        files
    }

    /// Every file the session touched, read or changed, each once.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for t in self.tools() {
            let diffs = t.call.content.iter().filter_map(|c| match c {
                ToolCallContent::Diff(d) => Some(&d.path),
                _ => None,
            });
            for p in t.call.locations.iter().map(|l| &l.path).chain(diffs) {
                if !out.contains(p) {
                    out.push(p.clone());
                }
            }
        }
        out
    }

    /// How many turns there have been.
    pub fn turn_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i, Item::You(_)))
            .count()
    }

    /// The name its checkpoints are kept under: unique across runs.
    pub fn uid(&self) -> String {
        let secs = self
            .begun
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        format!("{secs}-{}", self.key)
    }

    /// The turn the item at `i` belongs to, counting from zero.
    pub fn turn_of(&self, i: usize) -> Option<usize> {
        self.items
            .iter()
            .take(i + 1)
            .filter(|x| matches!(x, Item::You(_)))
            .count()
            .checked_sub(1)
    }

    /// The checkpoint before the first turn.
    pub fn first(&self) -> Option<&str> {
        self.turns.first().and_then(|t| t.before.as_deref())
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
            matches!(&s.items[2], Item::Tool(t) if t.call.status == ToolCallStatus::Completed && t.took.is_some())
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
            Some(Item::Ended {
                reason: StopReason::Cancelled,
                ..
            })
        ));
    }

    #[test]
    fn the_files_changed_are_counted_once_each() {
        let mut s = Session::new(1, "x", "/r".into());
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"a","title":"Edit","kind":"edit","content":[{"type":"diff","path":"/r/a.rs","oldText":"x\n","newText":"y\nz\n"}]}"#));
        s.apply(update(r#"{"sessionUpdate":"tool_call","toolCallId":"b","title":"Edit","kind":"edit","content":[{"type":"diff","path":"/r/a.rs","oldText":"y\n","newText":"w\n"}]}"#));
        let files = s.changed();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("/r/a.rs"));
        assert_eq!(files[0].change, Change::Edited);
        assert_eq!((files[0].added, files[0].gone), (3, 2));
    }

    #[test]
    fn a_thought_is_timed_when_the_agent_moves_on_and_a_plan_update_replaces_the_last() {
        let mut s = Session::new(1, "x", "/r".into());
        s.said("go");
        s.apply(update(
            r#"{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hm"}}"#,
        ));
        s.apply(update(r#"{"sessionUpdate":"plan","entries":[{"content":"a","priority":"high","status":"pending"}]}"#));
        s.apply(update(r#"{"sessionUpdate":"plan","entries":[{"content":"a","priority":"high","status":"completed"}]}"#));
        assert!(matches!(&s.items[1], Item::Thought { secs: Some(_), .. }));
        assert_eq!(s.items.len(), 3);
    }

    #[test]
    fn a_diffs_place_is_found_in_its_file_once_the_call_is_done() {
        let dir = std::env::temp_dir().join(format!("ghost-line-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.rs");
        std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
        let mut s = Session::new(1, "x", dir.clone());
        let json = format!(
            r#"{{"sessionUpdate":"tool_call","toolCallId":"e","title":"Edit","kind":"edit","status":"completed","content":[{{"type":"diff","path":"{}","oldText":"3\n","newText":"three\nfour\n"}}]}}"#,
            file.display()
        );
        s.apply(update(&json));
        let tool = s.tools().next().unwrap();
        assert_eq!(tool.starts, vec![Some(3)]);
        let _ = std::fs::remove_dir_all(&dir);
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
