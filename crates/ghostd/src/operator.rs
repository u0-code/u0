//! What a console may do because it is the daemon's owner's.
//!
//! The rule, once: **a console the daemon admits, from where the daemon
//! allows it to be, is its owner's, and whoever is at it is an operator of
//! every character the daemon runs.** It is admitted because it holds the
//! daemon's token, or because the daemon has none; and a daemon with no
//! token listens on this machine alone (`[listen]` refuses anything else),
//! so a console without one is also only taken as the owner's when it
//! connected from this machine. Nobody is listed in `ghostd.toml` for it:
//! holding the token is the listing.
//!
//! An operator at a console speaks to her on the channel
//! [`CONSOLE`], which is the daemon's own. No
//! presence can register it. What she says there is acted on by the daemon
//! itself, which has nowhere to carry it: it is written in her journal as
//! everything she says is, and that is where a console reads it, from the
//! journal it already streams. Nothing is sent to any connection, so it
//! reaches every console, now or later, and none needs to be connected.

use std::{
    net::SocketAddr,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use ghost_core::social::{self, Actor, Place, PlaceKind};
use ghost_mind::{Act, Acted, Can, Incoming};
use ghost_protocol::{CONSOLE, ConsoleSaid, ConsoleSay, Frame, SayMode, code};

use crate::cast::Character;

#[cfg(test)]
mod tests;

/// What she can do on the console channel: say something, and be seen to
/// be about to.
pub(crate) const CAN: [Can; 2] = [Can::Send, Can::Typing];

/// Whether a console admitted with `token` configured, connecting from
/// `peer`, is the daemon's owner's: see the rule above.
pub(crate) fn owns(token: Option<&str>, peer: SocketAddr) -> bool {
    token.is_some() || peer.ip().is_loopback()
}

/// She does `act` on the console channel. There is nobody to hand it to:
/// what she says is in her journal once her mind has it back, and that is
/// how it reaches a console.
pub(crate) fn act(act: &Act) -> Result<Acted, String> {
    match act {
        Act::Send { .. } => Ok(Acted {
            id: Some(fresh("said")),
            ..Acted::default()
        }),
        Act::Typing { .. } => Ok(Acted::default()),
        _ => Err("a console takes only what she says".to_owned()),
    }
}

/// `console.say`, request `id`, from a console that `owner` says is the
/// owner's, with `at_console` the name its hello gave: tell `her` what
/// they said, as an operator of hers, and answer where it was said.
pub(crate) async fn say(
    id: u64,
    frame: &Frame,
    owner: bool,
    at_console: Option<&str>,
    her: &Character,
) -> Frame {
    if !owner {
        return Frame::err(
            id,
            code::UNAUTHORIZED,
            "only the daemon's owner speaks to her from a console: give the daemon's token",
        );
    }
    let said = match frame.params::<ConsoleSay>() {
        Ok(said) => said,
        Err(e) => return Frame::err(id, e.code, e.message),
    };
    let Some(name) = at_console.map(str::trim).filter(|n| !n.is_empty()) else {
        return Frame::err(
            id,
            code::INVALID_PARAMS,
            "say who is at the console, with `who` in its hello, before speaking to her",
        );
    };
    if said.text.trim().is_empty() {
        return Frame::err(
            id,
            code::INVALID_PARAMS,
            "there is nothing in `text` to say",
        );
    }
    let person = name.to_lowercase();
    let place = said
        .place
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or(&person)
        .to_owned();
    let message = fresh("console");
    her.hear(
        CONSOLE,
        Incoming {
            place: Place {
                id: place.clone(),
                kind: PlaceKind::Direct,
                name: None,
            },
            from: Actor {
                id: person,
                name: name.to_owned(),
                handle: None,
                automated: false,
            },
            id: message.clone(),
            text: said.text,
            reply_to: None,
            addressed: true,
            images: Vec::new(),
            directive: said.mode == SayMode::Directive,
        },
    )
    .await;
    Frame::ok(
        id,
        &ConsoleSaid {
            place: social::key(CONSOLE, &place),
            id: message,
        },
    )
}

/// An id no message on the console channel has had: her memory drops a
/// message whose id it has seen in the same place, so one must never come
/// round again, across restarts too.
fn fresh(kind: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("{kind}-{millis}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}
