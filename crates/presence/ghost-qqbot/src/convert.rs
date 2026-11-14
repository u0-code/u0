//! QQ Bot events and messages, turned into Ghost's and back.
//!
//! QQ never shows a bot anyone's QQ number. People are known by an `OpenID`,
//! a different one for each bot, and a group by its own; place ids here are
//! `c2c:<user openid>` and `group:<group openid>`.

use ghost_link::protocol::{Actor, Inbound, Place, PlaceKind};
use serde_json::{Value, json};

use crate::replies::Passive;

/// A comfortable ceiling for one QQ message, in characters.
pub const MESSAGE_LIMIT: usize = 2000;

/// Who she is on QQ, from the gateway's `READY`.
#[derive(Debug, Clone)]
pub struct Me {
    pub id: String,
    pub name: String,
}

impl Me {
    pub fn from_ready(user: &Value) -> Option<Self> {
        let id = text(&user["id"])?;
        let name = text(&user["username"]).unwrap_or_else(|| id.clone());
        Some(Self { id, name })
    }
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// What a dispatched event is to her, or `None` if it is not a message she
/// can take in.
pub fn inbound(kind: &str, data: &Value) -> Option<Inbound> {
    let author = &data["author"];
    let (place, user, addressed) = match kind {
        "C2C_MESSAGE_CREATE" => {
            let user = text(&author["user_openid"])?;
            let place = Place {
                id: format!("c2c:{user}"),
                kind: PlaceKind::Direct,
                name: None,
            };
            (place, user, true)
        }
        // Only messages that @ her arrive as the first. The second is every
        // message in a group, for a bot QQ has allowed to see them all.
        "GROUP_AT_MESSAGE_CREATE" | "GROUP_MESSAGE_CREATE" => {
            let place = Place {
                id: format!("group:{}", text(&data["group_openid"])?),
                kind: PlaceKind::Group,
                name: None,
            };
            let names_her =
                kind == "GROUP_AT_MESSAGE_CREATE" || mentions(data).any(|m| m["is_you"] == true);
            (place, text(&author["member_openid"])?, names_her)
        }
        _ => return None,
    };
    let body = body(data);
    if body.is_empty() {
        return None;
    }
    let name = text(&author["username"]).unwrap_or_else(|| short(&user));
    Some(Inbound {
        place,
        from: Actor {
            id: user,
            name,
            handle: None,
            automated: author["bot"] == true,
        },
        id: text(&data["id"])?,
        text: body,
        // QQ quotes by an index of its own, not by message id.
        reply_to: None,
        addressed,
    })
}

fn mentions(data: &Value) -> impl Iterator<Item = &Value> {
    data["mentions"].as_array().into_iter().flatten()
}

/// A name for someone QQ gave no name for: the start of their `OpenID`.
fn short(openid: &str) -> String {
    format!("qq-{}", openid.chars().take(6).collect::<String>())
}

/// The message as plain text, with what is not text named in brackets.
fn body(data: &Value) -> String {
    let mut out = tags(data["content"].as_str().unwrap_or_default(), data);
    for attachment in data["attachments"].as_array().into_iter().flatten() {
        let kind = attachment["content_type"].as_str().unwrap_or_default();
        let piece = if kind.starts_with("image") {
            "[image]".to_owned()
        } else if kind == "voice" || kind.starts_with("audio") {
            // QQ transcribes voice messages; what it heard is worth more than
            // knowing that one was sent.
            match text(&attachment["asr_refer_text"]) {
                Some(heard) => format!("[voice message: {heard}]"),
                None => "[voice message]".to_owned(),
            }
        } else if kind.starts_with("video") {
            "[video]".to_owned()
        } else {
            "[file]".to_owned()
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&piece);
    }
    out
}

/// Replace QQ's inline tags: `<@openid>` and `<@!openid>` mentions become
/// `@name`, and `<faceType=…>` emoji become `[emoji]`.
fn tags(content: &str, data: &Value) -> String {
    let mut out = String::new();
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('>') else {
            rest = &rest[start..];
            break;
        };
        let tag = &rest[start + 1..start + len];
        if let Some(who) = tag.strip_prefix('@') {
            let who = who.trim_start_matches('!');
            let name = mentions(data)
                .find(|m| {
                    [&m["member_openid"], &m["user_openid"], &m["id"]]
                        .iter()
                        .any(|id| id.as_str() == Some(who))
                })
                .and_then(|m| text(&m["nickname"]).or_else(|| text(&m["username"])))
                .unwrap_or_else(|| short(who));
            out.push('@');
            out.push_str(&name);
        } else if tag.starts_with("faceType=") {
            out.push_str("[emoji]");
        } else {
            out.push_str(&rest[start..=start + len]);
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out.trim().to_owned()
}

/// The API path that sends to a place.
pub fn target(place: &str) -> Option<String> {
    let (kind, openid) = place.split_once(':')?;
    if openid.is_empty() || openid.contains('/') {
        return None;
    }
    match kind {
        "c2c" => Some(format!("/v2/users/{openid}/messages")),
        "group" => Some(format!("/v2/groups/{openid}/messages")),
        _ => None,
    }
}

/// A plain text message, answering a received one where it can.
pub fn outgoing(text: &str, passive: Option<&Passive>) -> Value {
    let mut body = json!({"content": text, "msg_type": 0});
    if let Some(passive) = passive {
        body["msg_id"] = json!(passive.msg_id);
        body["msg_seq"] = json!(passive.seq);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_message_is_for_her_and_its_sender_is_known_by_openid() {
        let data = json!({
            "id": "ROBOT1.0_abc", "content": "在吗", "timestamp": "2026-11-14T10:00:00+08:00",
            "author": {"user_openid": "E4F2A9C1D0B7"}
        });
        let m = inbound("C2C_MESSAGE_CREATE", &data).unwrap();
        assert_eq!(m.place.id, "c2c:E4F2A9C1D0B7");
        assert_eq!(m.place.kind, PlaceKind::Direct);
        assert_eq!(m.from.id, "E4F2A9C1D0B7");
        assert_eq!(
            m.from.name, "qq-E4F2A9",
            "no name is given in private chats"
        );
        assert_eq!((m.id.as_str(), m.text.as_str()), ("ROBOT1.0_abc", "在吗"));
        assert!(m.addressed);
        assert!(!m.from.automated);
    }

    #[test]
    fn in_a_group_an_at_addresses_her_and_tags_become_names_and_brackets() {
        let data = json!({
            "id": "m-2", "group_openid": "G77",
            "content": " <@!U9> 那家面馆关了<faceType=1,faceId=\"14\",ext=\"eyJ0ZXh0Ijoi5b6u56yRIn0=\">",
            "author": {"member_openid": "U2", "username": "梅子"},
            "mentions": [{"member_openid": "U9", "nickname": "老王"}],
            "attachments": [
                {"content_type": "image/jpeg", "url": "https://example.invalid/a.jpg"},
                {"content_type": "voice", "url": "https://example.invalid/v.silk",
                 "asr_refer_text": "你知道吗"}
            ]
        });
        let m = inbound("GROUP_AT_MESSAGE_CREATE", &data).unwrap();
        assert_eq!(m.place.id, "group:G77");
        assert_eq!(m.place.kind, PlaceKind::Group);
        assert_eq!(m.from.name, "梅子");
        assert_eq!(
            m.text,
            "@老王 那家面馆关了[emoji] [image] [voice message: 你知道吗]"
        );
        assert!(m.addressed);
    }

    #[test]
    fn a_group_message_that_does_not_name_her_is_only_overheard() {
        let mut data = json!({
            "id": "m-3", "group_openid": "G77", "content": "吃了吗",
            "author": {"member_openid": "U2", "bot": true}
        });
        let m = inbound("GROUP_MESSAGE_CREATE", &data).unwrap();
        assert!(!m.addressed);
        assert!(m.from.automated);
        data["mentions"] = json!([{"member_openid": "ME", "is_you": true}]);
        assert!(inbound("GROUP_MESSAGE_CREATE", &data).unwrap().addressed);
    }

    #[test]
    fn what_is_not_a_message_or_says_nothing_is_left_out() {
        let empty = json!({"id": "m", "content": "  ", "author": {"user_openid": "U1"}});
        assert!(inbound("C2C_MESSAGE_CREATE", &empty).is_none());
        let joined = json!({"group_openid": "G77", "op_member_openid": "U1"});
        assert!(inbound("GROUP_ADD_ROBOT", &joined).is_none());
        let nameless = json!({"id": "m", "content": "hi", "author": {}});
        assert!(inbound("C2C_MESSAGE_CREATE", &nameless).is_none());
        assert_eq!(tags("a < b", &json!({})), "a < b");
    }

    #[test]
    fn she_answers_a_received_message_where_she_can_and_speaks_first_where_she_cannot() {
        assert_eq!(target("c2c:U1").as_deref(), Some("/v2/users/U1/messages"));
        assert_eq!(
            target("group:G77").as_deref(),
            Some("/v2/groups/G77/messages")
        );
        assert_eq!(target("private:10001"), None);
        assert_eq!(target("c2c:../x"), None);
        let passive = Passive {
            msg_id: "m-2".into(),
            seq: 2,
        };
        assert_eq!(
            outgoing("好", Some(&passive)),
            json!({"content": "好", "msg_type": 0, "msg_id": "m-2", "msg_seq": 2})
        );
        assert_eq!(
            outgoing("好", None),
            json!({"content": "好", "msg_type": 0})
        );
        assert_eq!(
            Me::from_ready(&json!({"id": "9001", "username": "薇薇", "bot": true}))
                .map(|me| (me.id, me.name)),
            Some(("9001".into(), "薇薇".into()))
        );
    }
}
