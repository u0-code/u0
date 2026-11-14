//! Telegram's idea of a message, turned into Ghost's.

use ghost_link::protocol::{Actor, Inbound, Place, PlaceKind};
use serde_json::Value;

use crate::api::{Message, User};

/// Telegram's ceiling on one message, in characters.
pub const MESSAGE_LIMIT: usize = 4096;

pub fn actor(user: &User) -> Actor {
    let name = match &user.last_name {
        Some(last) if !last.is_empty() => format!("{} {last}", user.first_name),
        _ => user.first_name.clone(),
    };
    Actor {
        id: user.id.to_string(),
        name: if name.trim().is_empty() {
            user.username.clone().unwrap_or_else(|| user.id.to_string())
        } else {
            name
        },
        handle: user.username.clone(),
        automated: user.is_bot,
    }
}

/// What a message is to her, or `None` if it is nothing she can take in.
pub fn inbound(message: &Message, me: &User) -> Option<Inbound> {
    let from = message.from.as_ref()?;
    if from.id == me.id {
        return None;
    }
    let text = describe(message)?;
    let kind = match message.chat.kind.as_str() {
        "private" => PlaceKind::Direct,
        "channel" => PlaceKind::Public,
        _ => PlaceKind::Group,
    };
    let replies_to_her = message
        .reply_to_message
        .as_ref()
        .and_then(|m| m.from.as_ref())
        .is_some_and(|u| u.id == me.id);
    let names_her = me.username.as_ref().is_some_and(|handle| {
        text.to_lowercase()
            .contains(&format!("@{}", handle.to_lowercase()))
    }) || message
        .entities
        .iter()
        .chain(&message.caption_entities)
        .any(|e| e.kind == "text_mention" && e.user.as_ref().is_some_and(|u| u.id == me.id));
    Some(Inbound {
        place: Place {
            id: message.chat.id.to_string(),
            kind,
            name: message.chat.title.clone(),
        },
        from: actor(from),
        id: message.message_id.to_string(),
        text,
        reply_to: message
            .reply_to_message
            .as_ref()
            .map(|m| m.message_id.to_string()),
        addressed: kind == PlaceKind::Direct || replies_to_her || names_her,
        images: Vec::new(),
    })
}

/// The message as text, with anything that is not text named in brackets.
fn describe(message: &Message) -> Option<String> {
    if let Some(text) = &message.text {
        return Some(match text.trim() {
            // What Telegram sends when someone first opens a chat with a bot.
            "/start" => "[opened a chat with you]".to_owned(),
            other => other.to_owned(),
        })
        .filter(|t| !t.is_empty());
    }
    let seconds = |v: &Value| {
        let s = v["duration"].as_i64().unwrap_or(0);
        format!("{}:{:02}", s / 60, s % 60)
    };
    let media = if message.photo.is_some() {
        "[photo]".to_owned()
    } else if let Some(sticker) = &message.sticker {
        format!("[sticker {}]", sticker["emoji"].as_str().unwrap_or("")).replace(" ]", "]")
    } else if let Some(voice) = &message.voice {
        format!("[voice message {}]", seconds(voice))
    } else if let Some(audio) = &message.audio {
        format!("[audio {}]", seconds(audio))
    } else if message.animation.is_some() {
        "[gif]".to_owned()
    } else if message.video.is_some() || message.video_note.is_some() {
        "[video]".to_owned()
    } else if let Some(document) = &message.document {
        format!("[file {}]", document["file_name"].as_str().unwrap_or("")).replace(" ]", "]")
    } else if message.location.is_some() {
        "[a location]".to_owned()
    } else if message.contact.is_some() {
        "[a contact]".to_owned()
    } else if let Some(poll) = &message.poll {
        format!("[poll: {}]", poll["question"].as_str().unwrap_or(""))
    } else {
        return None;
    };
    Some(match message.caption.as_deref().map(str::trim) {
        Some(caption) if !caption.is_empty() => format!("{media} {caption}"),
        _ => media,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> User {
        serde_json::from_value(serde_json::json!({
            "id": 99, "is_bot": true, "first_name": "Weiwei", "username": "weiwei_ghost_bot"
        }))
        .unwrap()
    }

    fn message(json: serde_json::Value) -> Message {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_private_message_is_for_her() {
        let m = message(serde_json::json!({
            "message_id": 7,
            "from": {"id": 1, "is_bot": false, "first_name": "Hao", "last_name": "Ran", "username": "haoran"},
            "chat": {"id": 1, "type": "private", "first_name": "Hao"},
            "date": 1_762_900_000,
            "text": "hey"
        }));
        let inbound = inbound(&m, &me()).unwrap();
        assert_eq!(inbound.place.kind, PlaceKind::Direct);
        assert_eq!(inbound.place.id, "1");
        assert_eq!(inbound.from.name, "Hao Ran");
        assert_eq!(inbound.from.handle.as_deref(), Some("haoran"));
        assert_eq!(inbound.id, "7");
        assert!(inbound.addressed);
    }

    #[test]
    fn in_a_group_only_mentions_and_replies_are_for_her() {
        let base = |extra: serde_json::Value| {
            let mut v = serde_json::json!({
                "message_id": 8,
                "from": {"id": 2, "first_name": "Mei"},
                "chat": {"id": -100_123, "type": "supergroup", "title": "neighbours"},
                "text": "anyone around?"
            });
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            message(v)
        };
        let plain = inbound(&base(serde_json::json!({})), &me()).unwrap();
        assert_eq!(plain.place.kind, PlaceKind::Group);
        assert_eq!(plain.place.name.as_deref(), Some("neighbours"));
        assert!(!plain.addressed);

        let mention = base(serde_json::json!({"text": "what do you think @Weiwei_Ghost_Bot"}));
        assert!(inbound(&mention, &me()).unwrap().addressed);

        let reply = base(serde_json::json!({"reply_to_message": {
            "message_id": 5, "from": {"id": 99, "is_bot": true, "first_name": "Weiwei"},
            "chat": {"id": -100_123, "type": "supergroup"}, "text": "hm"
        }}));
        let reply = inbound(&reply, &me()).unwrap();
        assert!(reply.addressed);
        assert_eq!(reply.reply_to.as_deref(), Some("5"));
    }

    #[test]
    fn media_is_named_and_her_own_echo_is_dropped() {
        let photo = message(serde_json::json!({
            "message_id": 9, "from": {"id": 1, "first_name": "Hao"},
            "chat": {"id": 1, "type": "private"},
            "photo": [{"file_id": "x"}], "caption": "the teahouse"
        }));
        assert_eq!(inbound(&photo, &me()).unwrap().text, "[photo] the teahouse");
        let voice = message(serde_json::json!({
            "message_id": 10, "from": {"id": 1, "first_name": "Hao"},
            "chat": {"id": 1, "type": "private"}, "voice": {"duration": 72}
        }));
        assert_eq!(inbound(&voice, &me()).unwrap().text, "[voice message 1:12]");
        let start = message(serde_json::json!({
            "message_id": 11, "from": {"id": 1, "first_name": "Hao"},
            "chat": {"id": 1, "type": "private"}, "text": "/start"
        }));
        assert_eq!(
            inbound(&start, &me()).unwrap().text,
            "[opened a chat with you]"
        );
        let echo = message(serde_json::json!({
            "message_id": 12, "from": {"id": 99, "is_bot": true, "first_name": "Weiwei"},
            "chat": {"id": 1, "type": "private"}, "text": "hi"
        }));
        assert!(inbound(&echo, &me()).is_none());
        let service = message(serde_json::json!({
            "message_id": 13, "from": {"id": 1, "first_name": "Hao"},
            "chat": {"id": -5, "type": "group"}, "new_chat_title": "x"
        }));
        assert!(inbound(&service, &me()).is_none());
    }
}
