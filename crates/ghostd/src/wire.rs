//! Translation between the wire and the mind.
//!
//! The protocol's types and the journal's types look alike on purpose and are
//! separate on purpose: the journal is a forever format, the protocol is
//! versioned. This is the one place they meet.

use ghost_core::social;
use ghost_mind::{Act, Acted, Can, Feed, Incoming};
use ghost_protocol as wire;

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
    }
}

pub fn acted(a: wire::Acted) -> Acted {
    Acted {
        id: a.id,
        items: a.items.into_iter().map(incoming).collect(),
    }
}
