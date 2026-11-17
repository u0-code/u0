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
    /// Links to the images in it, in the order of their `[image]`
    /// placeholders in `text`.
    pub images: Vec<String>,
}

/// What a channel is like, as far as what she does there goes: whether
/// whoever writes is waiting on her, and what the platform allows her. Each
/// channel says it when it comes online; the console's are the daemon's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelFacts {
    /// Whoever writes here runs her and is waiting on her answer: the
    /// console. She looks and answers at once, whatever her day, and what
    /// she does only among people (putting off, reacting, leaving) is not
    /// offered.
    pub waited_on: bool,
    /// How long after a message reaches her in a private chat she can still
    /// answer it, where the platform limits it.
    pub answer_direct: Option<std::time::Duration>,
    /// The same in a group.
    pub answer_group: Option<std::time::Duration>,
    /// Whether she may write to someone who has not just written to her.
    pub writes_first: bool,
}

impl ChannelFacts {
    /// The console: waited on, with nothing it does not allow.
    pub fn console() -> Self {
        Self {
            waited_on: true,
            ..Self::default()
        }
    }

    /// How long she has to answer what reaches her in a place of `kind`,
    /// where the platform limits it.
    pub fn answer_within(
        &self,
        kind: ghost_core::social::PlaceKind,
    ) -> Option<std::time::Duration> {
        match kind {
            ghost_core::social::PlaceKind::Direct => self.answer_direct,
            _ => self.answer_group,
        }
    }
}

impl Default for ChannelFacts {
    /// A chat that allows her anything a person may do.
    fn default() -> Self {
        Self {
            waited_on: false,
            answer_direct: None,
            answer_group: None,
            writes_first: true,
        }
    }
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
    /// Report how her posts are doing and how many follow her.
    Metrics,
    /// Take a file and deliver it.
    Files,
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
    /// Ask how her recent posts are doing.
    Measure,
    /// Hand over a file.
    File {
        place: String,
        name: String,
        data: Vec<u8>,
        text: Option<String>,
    },
}

/// What a channel reports about her public standing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Measure {
    pub followers: Option<u64>,
    pub posts: Vec<ghost_core::journal::PostStats>,
}

/// What came of an act.
#[derive(Debug, Clone, Default)]
pub struct Acted {
    /// The channel's id for whatever was created, if anything was.
    pub id: Option<String>,
    /// What a `Fetch` brought back.
    pub items: Vec<Incoming>,
    /// What a `Measure` brought back.
    pub measure: Option<Measure>,
}

#[async_trait]
pub trait Body: Send + Sync + 'static {
    /// Carry out `act` on `channel`. The error is a human-readable reason.
    async fn act(&self, channel: &str, act: Act) -> Result<Acted, String>;
}
