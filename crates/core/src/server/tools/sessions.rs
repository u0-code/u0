//! Ghost's tools for organising the sessions Ghost keeps, as its sidebar
//! does: pinned or not, archived or restored, renamed, tagged. Each
//! change is kept in the index and said to the desk (`Event::Organized`),
//! which takes it as if you made it there.

use agent_client_protocol::v1::schema::SessionId;
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::Tools;
use crate::{proto::Event, store::Meta};

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct Organize {
    /// The session's agent, by its registry id (as `list_sessions` says).
    agent: String,
    /// The session's id.
    session: String,
    /// Pin it on top, or take its pin off; none, as it is.
    pin: Option<bool>,
    /// Archive it, or restore it; none, as it is.
    archive: Option<bool>,
    /// Its title, as you would give it: the agent's no longer replaces it.
    title: Option<String>,
    /// Its tags, all of them, in place of those it has.
    tags: Option<Vec<String>>,
}

impl Organize {
    /// What it does, in a few words, for its card.
    fn said(&self) -> String {
        let mut doing = Vec::new();
        match self.pin {
            Some(true) => doing.push("pin it".to_owned()),
            Some(false) => doing.push("unpin it".to_owned()),
            None => {}
        }
        match self.archive {
            Some(true) => doing.push("archive it".to_owned()),
            Some(false) => doing.push("restore it".to_owned()),
            None => {}
        }
        if let Some(title) = &self.title {
            doing.push(format!("rename it “{title}”"));
        }
        if let Some(tags) = &self.tags {
            doing.push(format!("tag it {}", tags.join(", ")));
        }
        doing.join(", ")
    }

    /// `meta` as it says; pinned last among `pinned`, the highest pin now.
    fn apply(self, mut meta: Meta, pinned: i64) -> Meta {
        match self.pin {
            Some(true) if meta.flags.pinned.is_none() => meta.flags.pinned = Some(pinned + 1),
            Some(false) => meta.flags.pinned = None,
            _ => {}
        }
        if let Some(archive) = self.archive {
            meta.flags.archived = archive;
        }
        if let Some(title) = self.title {
            (meta.title, meta.renamed) = (title, true);
        }
        if let Some(tags) = self.tags {
            meta.tags = tags;
        }
        meta
    }
}

#[tool_router(router = session_router, vis = "pub(super)")]
impl Tools {
    /// Organise a session Ghost keeps: pin it or take its pin off, archive
    /// or restore it, rename it, set its tags. You are asked first, in
    /// that session.
    #[tool]
    async fn organize_session(
        &self,
        Parameters(ask): Parameters<Organize>,
    ) -> Result<String, String> {
        let id = SessionId::new(ask.session.clone());
        if ask.said().is_empty() {
            return Err("Say what to change: pin, archive, title or tags".into());
        }
        let title = format!("Organise this session: {}", ask.said());
        let meta = (self.allowed_on("organize_session", title, &ask, &ask.agent, &id)).await?;
        let mut changed = None;
        self.shared.change_index(|index| {
            let pinned = (index.sessions.iter().filter_map(|m| m.flags.pinned)).max();
            let now = ask.apply(meta, pinned.unwrap_or(0));
            if let Some(kept) =
                (index.sessions.iter_mut()).find(|m| m.agent == now.agent && m.id == now.id)
            {
                kept.clone_from(&now);
            }
            changed = Some(now);
        });
        let now = changed.ok_or("The index of sessions does not read")?;
        let told = format!("“{}” is organised.", now.title);
        (self.shared.send)(Event::Organized(Box::new(now)));
        Ok(told)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::super::tests::{answer_card, in_session};

    #[test]
    fn a_session_is_pinned_renamed_and_tagged_as_you_allow_and_the_desk_is_told() {
        let home = tempfile::tempdir().unwrap();
        let (server, said, mut client) = in_session(home.path(), home.path());
        client.call(
            "organize_session",
            &json!({"agent": "a", "session": "s1", "pin": true,
                "title": "Ship it", "tags": ["release"]}),
        );
        let title = answer_card(&server, &said, "once");
        assert_eq!(
            title,
            "Organise this session: pin it, rename it “Ship it”, tag it release"
        );
        assert!(!client.result().1);
        let told = loop {
            if let crate::proto::Event::Organized(meta) =
                said.recv_timeout(Duration::from_secs(10)).unwrap()
            {
                break meta;
            }
        };
        assert_eq!((told.title.as_str(), told.renamed), ("Ship it", true));
        assert_eq!(
            (told.flags.pinned, told.tags.as_slice()),
            (Some(1), &["release".to_owned()][..])
        );
        let kept = server.shared.index.lock().unwrap().clone().unwrap();
        assert_eq!(kept.sessions[0].title, "Ship it");
        client.call("organize_session", &json!({"agent": "a", "session": "s1"}));
        assert!(client.result().1, "nothing to change");
    }
}
