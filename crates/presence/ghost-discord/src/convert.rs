//! Discord's idea of a message, turned into Ghost's.

use ghost_link::protocol::{Actor, Inbound, Place, PlaceKind};
use serenity::all::{Message, MessageType, User, UserId};

/// Discord's ceiling on one message, in characters.
pub const MESSAGE_LIMIT: usize = 2000;

pub fn actor(user: &User, nick: Option<&str>) -> Actor {
    Actor {
        id: user.id.to_string(),
        name: nick
            .map(str::to_owned)
            .or_else(|| user.global_name.clone())
            .unwrap_or_else(|| user.name.clone()),
        handle: Some(user.name.clone()),
        automated: user.bot,
    }
}

/// What a message is to her, or `None` if it is nothing she can take in.
/// `channel_name` is the name of the guild channel it was sent in, if known.
pub fn inbound(message: &Message, me: UserId, channel_name: Option<String>) -> Option<Inbound> {
    if message.author.id == me
        || !matches!(
            message.kind,
            MessageType::Regular | MessageType::InlineReply
        )
    {
        return None;
    }
    let text = describe(message)?;
    let direct = message.guild_id.is_none();
    let replies_to_her = message
        .referenced_message
        .as_ref()
        .is_some_and(|m| m.author.id == me);
    let names_her = message.mentions.iter().any(|u| u.id == me);
    Some(Inbound {
        place: Place {
            id: message.channel_id.to_string(),
            kind: if direct {
                PlaceKind::Direct
            } else {
                PlaceKind::Group
            },
            name: channel_name.map(|n| format!("#{n}")),
        },
        from: actor(
            &message.author,
            message.member.as_ref().and_then(|m| m.nick.as_deref()),
        ),
        id: message.id.to_string(),
        text,
        reply_to: message
            .referenced_message
            .as_ref()
            .map(|m| m.id.to_string()),
        addressed: direct || replies_to_her || names_her,
        images: Vec::new(),
    })
}

/// The message as text: mentions by name, attachments named in brackets.
fn describe(message: &Message) -> Option<String> {
    let mut text = message.content.clone();
    for user in &message.mentions {
        let name = user.global_name.as_deref().unwrap_or(&user.name);
        for form in [format!("<@{}>", user.id), format!("<@!{}>", user.id)] {
            text = text.replace(&form, &format!("@{name}"));
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if !text.trim().is_empty() {
        parts.push(text.trim().to_owned());
    }
    for attachment in &message.attachments {
        let kind = match attachment.content_type.as_deref() {
            Some(t) if t.starts_with("image/") => "image",
            Some(t) if t.starts_with("video/") => "video",
            Some(t) if t.starts_with("audio/") => "audio",
            _ => "file",
        };
        parts.push(format!("[{kind} {}]", attachment.filename));
    }
    for sticker in &message.sticker_items {
        parts.push(format!("[sticker {}]", sticker.name));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

#[cfg(test)]
mod tests {
    use serenity::json::json;

    use super::*;

    const ME: UserId = UserId::new(99);

    fn message(extra: &serenity::json::Value) -> Message {
        let mut base = json!({
            "id": "500",
            "channel_id": "300",
            "author": {"id": "1", "username": "haoran", "global_name": "Hao Ran", "discriminator": "0", "avatar": null},
            "content": "hey",
            "timestamp": "2026-11-12T07:00:00.000000+00:00",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0
        });
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serenity::json::from_value(base).unwrap()
    }

    fn her() -> serenity::json::Value {
        json!({"id": "99", "username": "weiwei", "global_name": "Weiwei", "discriminator": "0", "avatar": null, "bot": true})
    }

    #[test]
    fn a_direct_message_is_for_her() {
        let m = inbound(&message(&json!({})), ME, None).unwrap();
        assert_eq!(m.place.kind, PlaceKind::Direct);
        assert_eq!((m.place.id.as_str(), m.id.as_str()), ("300", "500"));
        assert_eq!(m.from.name, "Hao Ran");
        assert_eq!(m.from.handle.as_deref(), Some("haoran"));
        assert!(m.addressed && !m.from.automated);
    }

    #[test]
    fn in_a_server_only_mentions_and_replies_are_for_her() {
        let guild = json!({"guild_id": "700", "member": {"nick": "hao", "roles": [], "joined_at": "2026-01-01T00:00:00+00:00", "deaf": false, "mute": false}});
        let plain = inbound(&message(&guild), ME, Some("general".into())).unwrap();
        assert_eq!(plain.place.kind, PlaceKind::Group);
        assert_eq!(plain.place.name.as_deref(), Some("#general"));
        assert_eq!(plain.from.name, "hao");
        assert!(!plain.addressed);

        let mut mention = guild.clone();
        mention["content"] = json!("<@99> what do you think");
        mention["mentions"] = json!([her()]);
        let mention = inbound(&message(&mention), ME, None).unwrap();
        assert!(mention.addressed);
        assert_eq!(mention.text, "@Weiwei what do you think");

        let mut reply = guild;
        reply["type"] = json!(19);
        reply["referenced_message"] = json!({
            "id": "400", "channel_id": "300", "author": her(), "content": "hm",
            "timestamp": "2026-11-12T06:59:00.000000+00:00", "edited_timestamp": null,
            "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
            "attachments": [], "embeds": [], "pinned": false, "type": 0
        });
        let reply = inbound(&message(&reply), ME, None).unwrap();
        assert!(reply.addressed);
        assert_eq!(reply.reply_to.as_deref(), Some("400"));
    }

    #[test]
    fn attachments_are_named_and_noise_is_dropped() {
        let picture = message(&json!({"content": "", "attachments": [{
            "id": "1", "filename": "teahouse.jpg", "size": 10, "url": "https://x/y", "proxy_url": "https://x/y",
            "content_type": "image/jpeg"
        }]}));
        assert_eq!(
            inbound(&picture, ME, None).unwrap().text,
            "[image teahouse.jpg]"
        );
        // Her own message echoed back by the gateway.
        assert!(inbound(&message(&json!({"author": her()})), ME, None).is_none());
        // A member-joined system message.
        assert!(inbound(&message(&json!({"type": 7, "content": ""})), ME, None).is_none());
        assert!(inbound(&message(&json!({"content": "   "})), ME, None).is_none());
    }
}
