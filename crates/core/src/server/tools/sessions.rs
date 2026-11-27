//! Ghost's tools for the sessions Ghost keeps, as you would act on them:
//! organised as its sidebar does (pinned or not, archived or restored,
//! renamed, tagged), each change kept in the index and said to the desk
//! (`Event::Organized`), which takes it as if you made it there; and what
//! an agent asks in one answered for you, as you allow each time
//! (`Event::Answered`).

use agent_client_protocol::v1::schema::{
    RequestPermissionOutcome, SelectedPermissionOutcome, SessionId,
};
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::Tools;
use crate::{
    acp::Answer,
    proto::Event,
    server::happened::{Happened, Milestone},
    store::Meta,
};

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct AnswerRequest {
    /// The session's agent, by its registry id (as `events` says).
    agent: String,
    /// The session's id.
    session: String,
    /// The question, by its number (`request`, as `events` says it asks).
    request: u64,
    /// The answer, by its option's id (one of `options`, as `events` says).
    option: String,
}

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
    /// Answer what an agent asks in another session and waits on (a
    /// permission, as `events` tells it: its `request` and `options`) with
    /// one of its options. You are asked first, each time.
    #[tool]
    async fn answer_request(
        &self,
        Parameters(ask): Parameters<AnswerRequest>,
    ) -> Result<String, String> {
        let id = SessionId::new(ask.session.clone());
        self.not_mine(&ask.agent, &id)?;
        let asked =
            (self.shared.happened.subscribe().borrow().iter().rev()).find_map(|m: &Milestone| {
                match &m.what {
                    Happened::Asks {
                        title,
                        request,
                        options,
                    } if m.agent == ask.agent && m.session == id && *request == ask.request => {
                        Some((title.clone(), options.clone()))
                    }
                    _ => None,
                }
            });
        let (asks, options) = asked.ok_or("Nothing was asked there under that number")?;
        let option = (options.into_iter())
            .find(|o| *o.option_id.0 == ask.option)
            .ok_or("That is not one of its options")?;
        let agent = (self.shared.running(&ask.agent))
            .filter(|a| a.waits(ask.request))
            .ok_or("It no longer waits for an answer")?;
        let title = format!("Answer “{asks}” with “{}”", option.name);
        (self.allowed_on("answer_request", title, &ask, &ask.agent, &id)).await?;
        let chosen = SelectedPermissionOutcome::new(option.option_id.clone());
        agent.respond(
            ask.request,
            Answer::Permission(RequestPermissionOutcome::Selected(chosen)),
        );
        let told = format!("Answered “{}”.", option.name);
        (self.shared.send)(Event::Answered {
            agent: ask.agent,
            session: id,
            request: ask.request,
            option,
        });
        Ok(told)
    }

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
        (self.allowed_on("organize_session", title, &ask, &ask.agent, &id)).await?;
        // Read again as it is now, once you allowed it: what changed in it
        // while the card was open stays.
        let mut changed = None;
        self.shared.change_index(|index| {
            let pinned = (index.sessions.iter().filter_map(|m| m.flags.pinned)).max();
            let Some(kept) = (index.sessions.iter_mut())
                .find(|m| m.agent == ask.agent && m.id.as_ref() == Some(&id))
            else {
                return;
            };
            *kept = ask.apply(kept.clone(), pinned.unwrap_or(0));
            changed = Some(kept.clone());
        });
        let now = changed.ok_or("Ghost keeps no such session now")?;
        let told = format!("“{}” is organised.", now.title);
        (self.shared.send)(Event::Organized(Box::new(now)));
        Ok(told)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::super::tests::{answer_card, in_session, in_session_as, meta};
    use crate::proto::{Call, Event};

    #[test]
    fn what_an_agent_asks_in_another_session_is_answered_as_you_allow_each_time() {
        let home = tempfile::tempdir().unwrap();
        // An agent whose second session asks before its turn ends, and
        // writes down how it was answered.
        let script = r#"id() { printf '%s' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/'; }
n=0
while read line; do case "$line" in
*'"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1}}\n' "$(id)" ;;
*'"session/new"'*) n=$((n+1)); [ $n = 1 ] && echo "$line" > "$0.opened"
printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s%s"}}\n' "$(id)" "$n" ;;
*'"session/prompt"'*'"s2"'*) turn=$(id)
printf '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s2","toolCall":{"toolCallId":"t1","title":"Delete it"},"options":[{"optionId":"yes","name":"Yes","kind":"allow_once"},{"optionId":"no","name":"No","kind":"reject_once"}]}}\n' ;;
*'"id":"p1"'*) echo "$line" > "$0.answered"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$turn" ;;
*'"session/prompt"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)" ;;
esac; done"#;
        let (server, said, mut client) = in_session_as(home.path(), home.path(), script);
        server.handle(Call::Open {
            agent: "a".into(),
            key: 2,
            folder: home.path().to_path_buf(),
            also: Vec::new(),
        });
        let next = || said.recv_timeout(Duration::from_secs(10)).unwrap();
        while !matches!(next(), Event::Opened { result: Ok(_), .. }) {}
        let mut other = meta(home.path(), "Other");
        other.id = Some(agent_client_protocol::v1::schema::SessionId::new("s2"));
        server.handle(Call::Keep(vec![other]));
        // s1 has s2 carry on; s2's agent asks, and waits.
        client.call(
            "send_to_session",
            &json!({"agent": "a", "session": "s2", "prompt": "Clean up"}),
        );
        answer_card(&server, &said, "once");
        assert!(!client.result().1);
        let asked = loop {
            client.call(
                "events",
                &json!({"after": 0, "session": "s2", "seconds": 10}),
            );
            let (happened, _) = client.result();
            let happened: serde_json::Value = serde_json::from_str(&happened).unwrap();
            if let Some(asks) = (happened["happened"].as_array().unwrap().iter())
                .find_map(|m| m["what"].get("asks").cloned())
            {
                break asks;
            }
        };
        assert_eq!(asked["title"], json!("Delete it"));
        let answer =
            json!({"agent": "a", "session": "s2", "request": asked["request"], "option": "yes"});
        client.call(
            "answer_request",
            &json!({"agent": "a", "session": "s2", "request": asked["request"], "option": "maybe"}),
        );
        assert!(client.result().0.contains("not one of its options"));
        // Asked in s1, each time: no "always" for it.
        client.call("answer_request", &answer);
        let title = answer_card(&server, &said, "always");
        assert_eq!(title, "Answer “Delete it” with “Yes”");
        assert_eq!(client.result(), ("You did not allow it".to_owned(), true));
        client.call("answer_request", &answer);
        answer_card(&server, &said, "once");
        assert_eq!(client.result(), ("Answered “Yes”.".to_owned(), false));
        let told = loop {
            if let Event::Answered { option, .. } = next() {
                break option;
            }
        };
        assert_eq!(&*told.option_id.0, "yes");
        let answered = home.path().join("agent.sh.answered");
        for _ in 0..250 {
            if answered.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let answered = std::fs::read_to_string(answered).unwrap();
        assert!(answered.contains(r#""optionId":"yes""#), "{answered}");
        client.call("answer_request", &answer);
        assert!(client.result().0.contains("no longer waits"));
    }

    #[test]
    fn a_session_is_pinned_renamed_and_tagged_as_you_allow_and_the_desk_is_told() {
        let home = tempfile::tempdir().unwrap();
        let (server, said, mut client) = in_session(home.path(), home.path());
        client.call(
            "organize_session",
            &json!({"agent": "a", "session": "s1", "pin": true,
                "title": "Ship it", "tags": ["release"]}),
        );
        // While the card is open, you archive it on the desk.
        let card = loop {
            if let crate::proto::Event::Said { event, .. } =
                said.recv_timeout(Duration::from_secs(10)).unwrap()
                && let crate::acp::Event::Permission { id, request } = *event
            {
                break (id, request);
            }
        };
        let mut archived = super::super::tests::meta(home.path(), "Work");
        archived.flags.archived = true;
        server.handle(Call::Keep(vec![archived]));
        server.handle(Call::Answer {
            agent: "a".into(),
            request: card.0,
            answer: crate::acp::Answer::Permission(
                agent_client_protocol::v1::schema::RequestPermissionOutcome::Selected(
                    agent_client_protocol::v1::schema::SelectedPermissionOutcome::new("once"),
                ),
            ),
        });
        let title = card.1.tool_call.fields.title.unwrap_or_default();
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
        assert!(
            kept.sessions[0].flags.archived,
            "what you did meanwhile stays"
        );
        client.call("organize_session", &json!({"agent": "a", "session": "s1"}));
        assert!(client.result().1, "nothing to change");
    }
}
