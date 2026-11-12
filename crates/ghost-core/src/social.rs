//! The vocabulary of being somewhere with someone.
//!
//! These types are part of the journal format, so they change slowly. The
//! wire protocol has its own copies; the daemon converts between the two.

use serde::{Deserialize, Serialize};

/// Someone on a channel, as that channel knows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    /// Stable identifier on the channel.
    pub id: String,
    /// What they are called there.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// The channel marks this account as a bot or another ghost.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// A private conversation with one person.
    Direct,
    /// A room with several people.
    Group,
    /// A public thread anyone can read.
    Public,
}

impl PlaceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Group => "group",
            Self::Public => "public",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "direct" => Self::Direct,
            "group" => Self::Group,
            _ => Self::Public,
        }
    }
}

/// Somewhere a conversation happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub kind: PlaceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// The key of a person or a place across all channels: `channel:id`.
pub fn key(channel: &str, id: &str) -> String {
    format!("{channel}:{id}")
}

/// Split a key back into `(channel, id)`.
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_even_with_colons_in_ids() {
        let k = key("onebot", "group:42");
        assert_eq!(split_key(&k), Some(("onebot", "group:42")));
    }

    #[test]
    fn actor_omits_defaults() {
        let a = Actor {
            id: "1".into(),
            name: "a".into(),
            handle: None,
            automated: false,
        };
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"id":"1","name":"a"}"#
        );
    }
}
