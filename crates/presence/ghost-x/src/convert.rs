//! X's posts and direct messages, turned into Ghost's.
//!
//! A public thread is one place: `tweet:<conversation id>`. A direct message
//! conversation is another: `dm:<conversation id>`.

use ghost_link::protocol::{Actor, Inbound, Measure, Place, PlaceKind, PostStats};
use serde_json::Value;

/// What each lookup asks X to include, so posts arrive with their authors.
pub const TWEET_FIELDS: &[(&str, &str)] = &[
    ("expansions", "author_id"),
    (
        "tweet.fields",
        "conversation_id,author_id,referenced_tweets,note_tweet",
    ),
    ("user.fields", "username,name"),
];

/// Where a place id points.
#[derive(Debug, PartialEq, Eq)]
pub enum Target<'a> {
    /// A public thread, by conversation id.
    Thread(&'a str),
    /// A direct message conversation.
    Dm(&'a str),
}

pub fn target(place: &str) -> Option<Target<'_>> {
    match place.split_once(':')? {
        ("tweet", id) if !id.is_empty() => Some(Target::Thread(id)),
        ("dm", id) if !id.is_empty() => Some(Target::Dm(id)),
        _ => None,
    }
}

fn author(page: &Value, id: &str) -> Actor {
    let user = page["includes"]["users"]
        .as_array()
        .and_then(|users| users.iter().find(|u| u["id"] == id));
    let handle = user.and_then(|u| u["username"].as_str()).map(str::to_owned);
    Actor {
        id: id.to_owned(),
        name: user
            .and_then(|u| u["name"].as_str())
            .map(str::to_owned)
            .or_else(|| handle.clone())
            .unwrap_or_else(|| id.to_owned()),
        handle,
        automated: false,
    }
}

/// The posts on a page of results, oldest first, leaving out her own.
pub fn posts(page: &Value, me: &str, addressed: bool) -> Vec<Inbound> {
    let mut out: Vec<Inbound> = page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tweet| {
            let id = tweet["id"].as_str()?;
            let author_id = tweet["author_id"].as_str()?;
            if author_id == me {
                return None;
            }
            // Long posts carry their full text separately.
            let text = tweet["note_tweet"]["text"]
                .as_str()
                .or_else(|| tweet["text"].as_str())?
                .trim();
            if text.is_empty() {
                return None;
            }
            let conversation = tweet["conversation_id"].as_str().unwrap_or(id);
            let reply_to = tweet["referenced_tweets"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r["type"] == "replied_to")
                .and_then(|r| r["id"].as_str())
                .map(str::to_owned);
            Some(Inbound {
                place: Place {
                    id: format!("tweet:{conversation}"),
                    kind: PlaceKind::Public,
                    name: None,
                },
                from: author(page, author_id),
                id: id.to_owned(),
                text: text.to_owned(),
                reply_to,
                addressed,
                images: Vec::new(),
            })
        })
        .collect();
    out.reverse();
    out
}

/// The direct messages on a page of events, oldest first, leaving out her own.
pub fn direct_messages(page: &Value, me: &str) -> Vec<Inbound> {
    let mut out: Vec<Inbound> = page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|event| {
            if event["event_type"]
                .as_str()
                .is_some_and(|t| t != "MessageCreate")
            {
                return None;
            }
            let sender = event["sender_id"].as_str()?;
            let text = event["text"].as_str()?.trim();
            if sender == me || text.is_empty() {
                return None;
            }
            Some(Inbound {
                place: Place {
                    id: format!("dm:{}", event["dm_conversation_id"].as_str()?),
                    kind: PlaceKind::Direct,
                    name: None,
                },
                from: author(page, sender),
                id: event["id"].as_str()?.to_owned(),
                text: text.to_owned(),
                reply_to: None,
                addressed: true,
                images: Vec::new(),
            })
        })
        .collect();
    out.reverse();
    out
}

/// The numerically largest id on a page. X's ids grow with time.
pub fn newest(page: &Value) -> Option<u64> {
    page["data"]
        .as_array()?
        .iter()
        .filter_map(|item| item["id"].as_str()?.parse().ok())
        .max()
}

/// How many characters fit in one post. X counts most CJK characters double.
pub fn post_limit(text: &str) -> usize {
    let wide = text
        .chars()
        .any(|c| matches!(u32::from(c), 0x1100..=0x11FF | 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF));
    if wide { 140 } else { 280 }
}

/// Her standing on X from two reads: her own user object and her recent
/// posts, each with `public_metrics`.
pub fn measure(me: &Value, posts: &Value) -> Measure {
    let count = |v: &Value| v.as_u64().unwrap_or(0);
    Measure {
        followers: me["data"]["public_metrics"]["followers_count"].as_u64(),
        posts: posts["data"]
            .as_array()
            .map(|posts| {
                posts
                    .iter()
                    .filter_map(|post| {
                        let m = &post["public_metrics"];
                        Some(PostStats {
                            id: post["id"].as_str()?.to_owned(),
                            likes: count(&m["like_count"]),
                            replies: count(&m["reply_count"]),
                            // A quote is a repost with something added.
                            reposts: count(&m["retweet_count"]) + count(&m["quote_count"]),
                            views: count(&m["impression_count"]),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn standing_is_read_from_public_metrics() {
        let me = json!({"data": {"id": "99", "public_metrics": {"followers_count": 412, "following_count": 80}}});
        let posts = json!({"data": [
            {"id": "2001", "text": "the door on kuixinglou street", "public_metrics":
                {"like_count": 31, "reply_count": 4, "retweet_count": 5, "quote_count": 2, "impression_count": 2900}},
            {"id": "2000", "text": "no metrics on this one"},
            {"text": "no id"},
        ]});
        let m = measure(&me, &posts);
        assert_eq!(m.followers, Some(412));
        assert_eq!(m.posts.len(), 2);
        assert_eq!(
            m.posts[0],
            PostStats {
                id: "2001".into(),
                likes: 31,
                replies: 4,
                reposts: 7,
                views: 2900
            }
        );
        assert_eq!(m.posts[1].likes, 0);
        assert_eq!(measure(&json!({}), &json!({})), Measure::default());
    }

    fn page() -> Value {
        json!({
            "data": [
                {"id": "1903", "author_id": "7", "conversation_id": "1900", "text": "@weiwei_ghost what about the old teahouse?",
                 "referenced_tweets": [{"type": "replied_to", "id": "1901"}]},
                {"id": "1902", "author_id": "99", "conversation_id": "1900", "text": "her own post"},
                {"id": "1899", "author_id": "8", "conversation_id": "1899", "text": "short",
                 "note_tweet": {"text": "the full text of a long post"}}
            ],
            "includes": {"users": [
                {"id": "7", "name": "Mei", "username": "mei_cd"},
                {"id": "8", "name": "Old Wang", "username": "laowang"}
            ]},
            "meta": {"newest_id": "1903"}
        })
    }

    #[test]
    fn posts_arrive_oldest_first_with_authors_and_threads() {
        let posts = posts(&page(), "99", true);
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].id, "1899");
        assert_eq!(posts[0].text, "the full text of a long post");
        assert_eq!(posts[0].place.id, "tweet:1899");
        let mention = &posts[1];
        assert_eq!(mention.place.id, "tweet:1900");
        assert_eq!(mention.place.kind, PlaceKind::Public);
        assert_eq!(mention.from.name, "Mei");
        assert_eq!(mention.from.handle.as_deref(), Some("mei_cd"));
        assert_eq!(mention.reply_to.as_deref(), Some("1901"));
        assert!(mention.addressed);
        assert_eq!(newest(&page()), Some(1903));
        assert_eq!(newest(&json!({"meta": {"result_count": 0}})), None);
    }

    #[test]
    fn direct_messages_become_private_places() {
        let page = json!({
            "data": [
                {"id": "52", "event_type": "MessageCreate", "sender_id": "99", "dm_conversation_id": "7-99", "text": "hers"},
                {"id": "51", "event_type": "MessageCreate", "sender_id": "7", "dm_conversation_id": "7-99", "text": "are you around"},
                {"id": "50", "event_type": "ParticipantsJoin", "sender_id": "7", "dm_conversation_id": "7-99"}
            ],
            "includes": {"users": [{"id": "7", "name": "Mei", "username": "mei_cd"}]}
        });
        let messages = direct_messages(&page, "99");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].place.id, "dm:7-99");
        assert_eq!(messages[0].place.kind, PlaceKind::Direct);
        assert!(messages[0].addressed);
    }

    #[test]
    fn places_and_limits() {
        assert_eq!(target("tweet:1900"), Some(Target::Thread("1900")));
        assert_eq!(target("dm:7-99"), Some(Target::Dm("7-99")));
        assert_eq!(target("tweet:"), None);
        assert_eq!(target("elsewhere"), None);
        assert_eq!(post_limit("plain latin text"), 280);
        assert_eq!(post_limit("今天的光线很平"), 140);
    }
}
