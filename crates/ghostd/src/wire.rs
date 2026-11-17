//! Translation between the wire and the mind.
//!
//! The protocol's types and the journal's types look alike on purpose and are
//! separate on purpose: the journal is a forever format, the protocol is
//! versioned. This is the one place they meet.

use base64::Engine as _;
use ghost_core::social;
use ghost_mind::{Act, Acted, Can, Feed, Incoming, Kind};
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
        images: m.images.into_iter().map(|i| i.url).collect(),
    }
}

/// What sort of place a presence says its platform is, as her mind takes
/// it. No presence can make a terminal of itself: only the console is one.
pub fn kind(k: wire::PresenceKind) -> Kind {
    match k {
        wire::PresenceKind::Public => Kind::Public,
        wire::PresenceKind::Chat | wire::PresenceKind::Other => Kind::Chat,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_presence_can_make_a_terminal_of_itself() {
        assert_eq!(kind(wire::PresenceKind::Public), Kind::Public);
        assert_eq!(kind(wire::PresenceKind::Chat), Kind::Chat);
        assert_eq!(kind(wire::PresenceKind::Other), Kind::Chat);
    }
}
