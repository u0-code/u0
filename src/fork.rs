//! Branching a session off before one of your messages: a new session with
//! what was said up to it, to say it another way.
//!
//! ACP v1, which the window speaks, cannot do this: a fork takes a cursor,
//! and agents announce their cursors only on v2. So the agent is started a
//! second time, briefly, and asked over v2: the session is resumed with its
//! history replayed (which spends nothing), the cursor before your message
//! is found among the cursors that come with it, and the fork is made
//! there. The window's own connection then loads the new session like any
//! other. This is the one place Ghost speaks v2.

use std::path::PathBuf;

use agent_client_protocol_schema::{
    MaybeUndefined, ProtocolVersion,
    v1::{self, CursorAnchor, CursorId, SessionId, SessionUpdate},
    v2,
};
use tokio::sync::mpsc;

use crate::{
    acp::{self, Agent, Failure, Launch},
    hub::{Msg, Sender},
    session::text_of,
};

/// One of your messages: its text, and how many of yours with the same
/// text came before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mine {
    pub text: String,
    pub nth: usize,
}

/// Branch the session `id`, in `folder`, off before `mine`, with a second
/// process of the agent `launch` starts; the new session's id comes back
/// as [`Msg::ForkedAt`].
pub fn at(launch: Launch, from: u64, id: SessionId, folder: PathBuf, mine: Mine, tx: Sender) {
    acp::runtime().spawn(async move {
        let result = fork(&launch, id, folder, &mine).await;
        let _ = tx.send(Msg::ForkedAt {
            from,
            text: mine.text,
            result,
        });
    });
}

async fn fork(
    launch: &Launch,
    id: SessionId,
    folder: PathBuf,
    mine: &Mine,
) -> Result<SessionId, Failure> {
    let (events, mut said) = mpsc::unbounded_channel();
    let agent = Agent::start(launch, events).map_err(|e| Failure {
        code: None,
        message: format!("{} could not be started: {e}", launch.program.display()),
    })?;
    let hello = v2::InitializeRequest::new(
        ProtocolVersion::V2,
        v2::Implementation::new("ghost", env!("CARGO_PKG_VERSION")),
    );
    agent
        .request::<_, v2::InitializeResponse>("initialize", hello)
        .await?;
    let resume = v2::ResumeSessionRequest::new(id.0.to_string(), folder.clone())
        .replay_from(v2::ReplayFrom::Start(v2::ReplayFromStart::new()));
    agent
        .request::<_, v2::ResumeSessionResponse>("session/resume", resume)
        .await?;
    // The replay comes before the answer, and is read in order: it is all
    // here now.
    let mut replay = Vec::new();
    while let Ok(event) = said.try_recv() {
        if let acp::Event::Update(n) = event {
            replay.push(n.update);
        }
    }
    let cursor = cursor(&replay, mine).ok_or_else(|| Failure {
        code: None,
        message: "the agent offers no place to branch off before this message".into(),
    })?;
    let request = v2::ForkSessionRequest::new(id.0.to_string(), folder)
        .cursor_id(v2::CursorId::new(cursor.0.to_string()));
    let forked = agent
        .request::<_, v2::ForkSessionResponse>("session/fork", request)
        .await?;
    Ok(SessionId::new(forked.session_id.0.to_string()))
}

/// The cursor a fork before `mine` starts at, among the updates of a
/// session's replay. A v2 replay's message chunks and cursors read as v1's:
/// they have one shape in both.
fn cursor(replay: &[SessionUpdate], mine: &Mine) -> Option<CursorId> {
    let mut messages: Vec<(v1::MessageId, String)> = Vec::new();
    let mut cursors = Vec::new();
    for update in replay {
        match update {
            SessionUpdate::UserMessageChunk(c) => {
                let Some(id) = &c.message_id else { continue };
                match messages.last_mut() {
                    Some((last, text)) if last == id => text.push_str(&text_of(&c.content)),
                    _ => messages.push((id.clone(), text_of(&c.content))),
                }
            }
            SessionUpdate::CursorUpdate(u) => {
                if let (Some(CursorAnchor::UserMessage(a)), MaybeUndefined::Value(can)) =
                    (&u.anchor, &u.capabilities)
                    && can.fork.is_some()
                {
                    cursors.push((a.message_id.clone(), u.cursor_id.clone()));
                }
            }
            _ => {}
        }
    }
    let (id, _) = messages
        .iter()
        .filter(|(_, text)| text.trim() == mine.text.trim())
        .nth(mine.nth)?;
    cursors
        .into_iter()
        .find(|(at, _)| at == id)
        .map(|(_, cursor)| cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(lines: &[serde_json::Value]) -> Vec<SessionUpdate> {
        lines
            .iter()
            .map(|l| serde_json::from_value(l.clone()).unwrap())
            .collect()
    }

    fn you(id: &str, text: &str) -> serde_json::Value {
        serde_json::json!({"sessionUpdate": "user_message_chunk", "messageId": id,
            "content": {"type": "text", "text": text}})
    }

    fn before(id: &str) -> serde_json::Value {
        serde_json::json!({"sessionUpdate": "cursor_update", "cursorId": format!("c-{id}"),
            "capabilities": {"fork": {}},
            "anchor": {"type": "user_message", "messageId": id, "position": "before"}})
    }

    #[test]
    fn a_fork_starts_at_the_cursor_before_the_message_of_that_text_and_turn() {
        let r = replay(&[
            you("a", "go on"),
            you("b", "fix "),
            you("b", "it"),
            before("b"),
            you("c", "go on"),
            before("c"),
        ]);
        let at = |text: &str, nth| {
            cursor(
                &r,
                &Mine {
                    text: text.into(),
                    nth,
                },
            )
        };
        assert_eq!(at("fix it", 0), Some(CursorId::new("c-b")));
        assert_eq!(at("go on", 1), Some(CursorId::new("c-c")));
        // The first message has nothing before it to branch from.
        assert_eq!(at("go on", 0), None);
        assert_eq!(at("never said", 0), None);
    }
}
