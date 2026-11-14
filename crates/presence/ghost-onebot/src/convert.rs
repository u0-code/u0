//! OneBot v11 events and messages, turned into Ghost's and back.
//!
//! Implementations differ in small ways: ids are numbers in some and strings
//! in others, `message` is an array of segments or a CQ-coded string. This
//! module accepts what it is given.

use ghost_link::protocol::{Actor, Inbound, Place, PlaceKind};
use serde_json::{Value, json};

/// A comfortable ceiling for one QQ message, in characters.
pub const MESSAGE_LIMIT: usize = 2000;

/// Who she is on QQ.
#[derive(Debug, Clone)]
pub struct Me {
    pub id: String,
    pub name: String,
}

/// An id, whether the implementation wrote it as a number or a string.
pub fn id(value: &Value) -> Option<String> {
    match value {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// What a message event is to her, or `None` if it is not one she can take in.
pub fn inbound(event: &Value, me: &Me) -> Option<Inbound> {
    if event["post_type"] != "message" {
        return None;
    }
    let user = id(&event["user_id"])?;
    if user == me.id {
        return None;
    }
    let place = match event["message_type"].as_str()? {
        "private" => Place {
            id: format!("private:{user}"),
            kind: PlaceKind::Direct,
            name: None,
        },
        "group" => Place {
            id: format!("group:{}", id(&event["group_id"])?),
            kind: PlaceKind::Group,
            name: event["group_name"].as_str().map(str::to_owned),
        },
        _ => return None,
    };
    let sender = &event["sender"];
    let name = [&sender["card"], &sender["nickname"]]
        .into_iter()
        .filter_map(Value::as_str)
        .find(|s| !s.trim().is_empty())
        .map_or_else(|| user.clone(), str::to_owned);

    let Body {
        text,
        reply_to,
        names_her,
    } = body(&event["message"], event["raw_message"].as_str(), me);
    if text.trim().is_empty() {
        return None;
    }
    Some(Inbound {
        addressed: place.kind == PlaceKind::Direct || names_her,
        place,
        from: Actor {
            id: user,
            name,
            handle: None,
            automated: false,
        },
        id: id(&event["message_id"])?,
        text: text.trim().to_owned(),
        reply_to,
    })
}

struct Body {
    text: String,
    reply_to: Option<String>,
    names_her: bool,
}

/// Flatten message segments into text, naming whatever is not text.
fn body(message: &Value, raw: Option<&str>, me: &Me) -> Body {
    let mut out = Body {
        text: String::new(),
        reply_to: None,
        names_her: false,
    };
    let Some(segments) = message.as_array() else {
        // String format. Keep it as written; CQ codes are at least readable.
        message
            .as_str()
            .or(raw)
            .unwrap_or_default()
            .clone_into(&mut out.text);
        out.names_her = out.text.contains(&format!("[CQ:at,qq={}", me.id));
        return out;
    };
    for segment in segments {
        let data = &segment["data"];
        let piece = match segment["type"].as_str().unwrap_or_default() {
            "text" => data["text"].as_str().unwrap_or_default().to_owned(),
            "at" => {
                let target = id(&data["qq"]).unwrap_or_default();
                if target == me.id {
                    out.names_her = true;
                    format!("@{} ", me.name)
                } else if target == "all" {
                    "@everyone ".to_owned()
                } else {
                    format!("@{} ", data["name"].as_str().unwrap_or(&target))
                }
            }
            "reply" => {
                out.reply_to = id(&data["id"]);
                String::new()
            }
            "face" | "mface" | "bface" | "sface" => "[emoji]".to_owned(),
            "image" => "[image]".to_owned(),
            "record" => "[voice message]".to_owned(),
            "video" => "[video]".to_owned(),
            "file" => "[file]".to_owned(),
            "forward" | "node" => "[forwarded messages]".to_owned(),
            "json" | "xml" | "share" => "[card]".to_owned(),
            "poke" => "[poke]".to_owned(),
            _ => String::new(),
        };
        out.text.push_str(&piece);
    }
    out
}

/// The place someone started typing in, if this is such a notice. Only some
/// implementations send it, and only for private chats.
pub fn typing(event: &Value) -> Option<String> {
    (event["post_type"] == "notice"
        && event["notice_type"] == "notify"
        && event["sub_type"] == "input_status")
        .then(|| id(&event["user_id"]))
        .flatten()
        .map(|user| format!("private:{user}"))
}

/// Where a place id points: the action to send with, and its target parameter.
pub fn target(place: &str) -> Option<(&'static str, &'static str, i64)> {
    let (kind, number) = place.split_once(':')?;
    let number = number.parse().ok()?;
    match kind {
        "private" => Some(("send_private_msg", "user_id", number)),
        "group" => Some(("send_group_msg", "group_id", number)),
        _ => None,
    }
}

/// Message segments for something she says.
pub fn outgoing(text: &str, reply_to: Option<&str>) -> Value {
    let mut segments = Vec::new();
    if let Some(id) = reply_to {
        segments.push(json!({"type": "reply", "data": {"id": id}}));
    }
    segments.push(json!({"type": "text", "data": {"text": text}}));
    Value::Array(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Me {
        Me {
            id: "10001".into(),
            name: "Weiwei".into(),
        }
    }

    #[test]
    fn a_private_message_is_for_her() {
        let event = json!({
            "post_type": "message", "message_type": "private", "sub_type": "friend",
            "message_id": 42, "user_id": 20002, "self_id": 10001,
            "message": [{"type": "text", "data": {"text": "在吗"}}],
            "raw_message": "在吗", "sender": {"user_id": 20002, "nickname": "浩然", "card": ""}
        });
        let m = inbound(&event, &me()).unwrap();
        assert_eq!(m.place.id, "private:20002");
        assert_eq!(m.place.kind, PlaceKind::Direct);
        assert_eq!(
            (m.from.id.as_str(), m.from.name.as_str()),
            ("20002", "浩然")
        );
        assert_eq!((m.id.as_str(), m.text.as_str()), ("42", "在吗"));
        assert!(m.addressed);
    }

    #[test]
    fn in_a_group_an_at_addresses_her_and_a_reply_is_carried() {
        let event = json!({
            "post_type": "message", "message_type": "group",
            "message_id": "43", "user_id": "20003", "group_id": 30003, "group_name": "街坊",
            "message": [
                {"type": "reply", "data": {"id": "40"}},
                {"type": "at", "data": {"qq": "10001"}},
                {"type": "text", "data": {"text": "那家面馆关了你知道吗"}},
                {"type": "image", "data": {"file": "x.jpg"}}
            ],
            "sender": {"nickname": "小梅", "card": "梅子"}
        });
        let m = inbound(&event, &me()).unwrap();
        assert_eq!(m.place.id, "group:30003");
        assert_eq!(m.place.name.as_deref(), Some("街坊"));
        assert_eq!(m.from.name, "梅子");
        assert_eq!(m.text, "@Weiwei 那家面馆关了你知道吗[image]");
        assert_eq!(m.reply_to.as_deref(), Some("40"));
        assert!(m.addressed);

        let plain = json!({
            "post_type": "message", "message_type": "group",
            "message_id": 44, "user_id": 20003, "group_id": 30003,
            "message": [{"type": "at", "data": {"qq": 20009, "name": "老王"}}, {"type": "text", "data": {"text": "吃了吗"}}],
            "sender": {"nickname": "小梅"}
        });
        let plain = inbound(&plain, &me()).unwrap();
        assert!(!plain.addressed);
        assert_eq!(plain.text, "@老王 吃了吗");
    }

    #[test]
    fn string_messages_echoes_and_other_events_are_handled() {
        let cq = json!({
            "post_type": "message", "message_type": "group", "message_id": 45,
            "user_id": 20003, "group_id": 30003,
            "message": "[CQ:at,qq=10001] 出来吃饭", "sender": {"nickname": "小梅"}
        });
        assert!(inbound(&cq, &me()).unwrap().addressed);
        let echo = json!({
            "post_type": "message", "message_type": "private", "message_id": 46,
            "user_id": 10001, "message": [{"type": "text", "data": {"text": "hi"}}]
        });
        assert!(inbound(&echo, &me()).is_none());
        let heartbeat = json!({"post_type": "meta_event", "meta_event_type": "heartbeat"});
        assert!(inbound(&heartbeat, &me()).is_none());
        let typing_notice = json!({
            "post_type": "notice", "notice_type": "notify", "sub_type": "input_status", "user_id": 20002
        });
        assert_eq!(typing(&typing_notice).as_deref(), Some("private:20002"));
        assert!(typing(&heartbeat).is_none());
    }

    #[test]
    fn outgoing_messages_are_addressed_and_quoted() {
        assert_eq!(
            target("private:20002"),
            Some(("send_private_msg", "user_id", 20002))
        );
        assert_eq!(
            target("group:30003"),
            Some(("send_group_msg", "group_id", 30003))
        );
        assert_eq!(target("somewhere"), None);
        assert_eq!(
            outgoing("好", Some("40")),
            json!([{"type": "reply", "data": {"id": "40"}}, {"type": "text", "data": {"text": "好"}}])
        );
    }
}
