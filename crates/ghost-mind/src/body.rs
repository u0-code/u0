//! How the mind touches the outside world.
//!
//! A [`Body`] is whatever can carry out an [`Act`] on a named channel. The
//! daemon implements it over connected presence adapters; tests implement it
//! with a recorder.

use async_trait::async_trait;
use ghost_core::social::{Actor, Place};
use serde::Serialize;

/// A message that reached her on some channel.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub place: Place,
    pub from: Actor,
    /// The channel's own id for the message.
    pub id: String,
    pub text: String,
    pub reply_to: Option<String>,
    /// Meant for her: a direct message, a mention, or a reply to her.
    pub addressed: bool,
}

/// What a channel is able to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Can {
    Send,
    Typing,
    React,
    Read,
    Post,
    Feed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    Timeline,
    Mentions,
}

/// Something she does on a channel.
#[derive(Debug, Clone)]
pub enum Act {
    Send {
        place: String,
        text: String,
        reply_to: Option<String>,
    },
    Typing {
        place: String,
    },
    React {
        place: String,
        to: String,
        emoji: String,
    },
    Read {
        place: String,
        upto: Option<String>,
    },
    Post {
        text: String,
    },
    Fetch {
        feed: Feed,
        limit: u32,
    },
}

/// What came of an act.
#[derive(Debug, Clone, Default)]
pub struct Acted {
    /// The channel's id for whatever was created, if anything was.
    pub id: Option<String>,
    /// What a `Fetch` brought back.
    pub items: Vec<Incoming>,
}

#[async_trait]
pub trait Body: Send + Sync + 'static {
    /// Carry out `act` on `channel`. The error is a human-readable reason.
    async fn act(&self, channel: &str, act: Act) -> Result<Acted, String>;
}
