//! Translation between the wire and the mind.
//!
//! The protocol's types and the journal's types look alike on purpose and are
//! separate on purpose: the journal is a forever format, the protocol is
//! versioned. This is the one place they meet.

use base64::Engine as _;
use ghost_core::social;
use ghost_mind::{Act, Acted, Can, ChannelFacts, Feed, Incoming};
use ghost_protocol as wire;

/// An entry of her journal as the protocol carries it: what happened, as
/// her journal keeps it, under its kind.
pub fn entry(e: &ghost_core::journal::Entry) -> wire::Entry {
    let data = match serde_json::to_value(&e.event) {
        Ok(serde_json::Value::Object(mut event)) => event.remove("data"),
        _ => None,
    };
    wire::Entry {
        seq: e.seq,
        at: e.at,
        kind: e.event.kind().to_owned(),
        data: data.unwrap_or_default(),
    }
}

/// What happened in an entry the protocol carried, as her journal keeps
/// it; an error for a kind or a shape this build does not know.
pub fn event(e: wire::Entry) -> serde_json::Result<ghost_core::journal::Event> {
    let event = serde_json::Map::from_iter([
        ("kind".to_owned(), serde_json::Value::String(e.kind)),
        ("data".to_owned(), e.data),
    ]);
    serde_json::from_value(serde_json::Value::Object(event))
}

pub fn actor(a: wire::Actor) -> social::Actor {
    social::Actor {
        id: a.id,
        name: a.name,
        handle: a.handle,
        automated: a.automated,
    }
}

fn place(p: wire::Place) -> social::Place {
    social::Place {
        id: p.id,
        kind: match p.kind {
            wire::PlaceKind::Direct => social::PlaceKind::Direct,
            wire::PlaceKind::Group => social::PlaceKind::Group,
            wire::PlaceKind::Public => social::PlaceKind::Public,
        },
        name: p.name,
    }
}

pub fn incoming(m: wire::Inbound) -> Incoming {
    Incoming {
        place: place(m.place),
        from: actor(m.from),
        id: m.id,
        text: m.text,
        reply_to: m.reply_to,
        addressed: m.addressed,
        images: m.images.into_iter().map(|i| i.url).collect(),
    }
}

/// What a presence says its platform allows her, as her mind takes it. No
/// presence is waited on the way the console is.
pub fn facts(allows: wire::Allows) -> ChannelFacts {
    let secs = |s: Option<u64>| s.map(std::time::Duration::from_secs);
    ChannelFacts {
        waited_on: false,
        answer_direct: secs(allows.answer_direct_secs),
        answer_group: secs(allows.answer_group_secs),
        writes_first: allows.writes_first,
    }
}

pub fn can(c: wire::Capability) -> Can {
    match c {
        wire::Capability::Send => Can::Send,
        wire::Capability::Typing => Can::Typing,
        wire::Capability::React => Can::React,
        wire::Capability::Read => Can::Read,
        wire::Capability::Post => Can::Post,
        wire::Capability::Feed => Can::Feed,
        wire::Capability::Metrics => Can::Metrics,
        wire::Capability::Files => Can::Files,
    }
}

pub fn action(act: Act) -> wire::Action {
    match act {
        Act::Send {
            place,
            text,
            reply_to,
        } => wire::Action::Send {
            place,
            text,
            reply_to,
        },
        Act::Typing { place } => wire::Action::Typing { place },
        Act::React { place, to, emoji } => wire::Action::React { place, to, emoji },
        Act::Read { place, upto } => wire::Action::Read { place, upto },
        Act::Post { text } => wire::Action::Post { text },
        Act::Fetch { feed, limit } => wire::Action::Fetch {
            feed: match feed {
                Feed::Timeline => wire::Feed::Timeline,
                Feed::Mentions => wire::Feed::Mentions,
            },
            limit,
        },
        Act::Measure => wire::Action::Measure,
        Act::File {
            place,
            name,
            data,
            text,
        } => wire::Action::File {
            place,
            name,
            data: base64::engine::general_purpose::STANDARD.encode(data),
            text,
        },
    }
}

pub fn acted(a: wire::Acted) -> Acted {
    Acted {
        id: a.id,
        items: a.items.into_iter().map(incoming).collect(),
        measure: a.measure.map(|m| ghost_mind::Measure {
            followers: m.followers,
            posts: m
                .posts
                .into_iter()
                .map(|p| ghost_core::journal::PostStats {
                    id: p.id,
                    likes: p.likes,
                    replies: p.replies,
                    reposts: p.reposts,
                    views: p.views,
                })
                .collect(),
        }),
    }
}
