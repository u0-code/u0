//! What the desk follows: each ghost it watches on a daemon, kept up to
//! date from her link, with what has been read of her steps for the
//! thread on screen; and the character opened from the left, with the
//! place her thread shows.

use std::{collections::HashSet, time::Instant};

use chrono::Utc;
use chrono_tz::Tz;
use ghost_link::protocol::{CONSOLE, Work};

use crate::{
    cache,
    daemons::Host,
    follow::{Following, Read},
    held::Held,
    link::{self, Link, Order},
    media,
    model::{Entry, LinkState, Message, Reach, Status, Stop, Update},
    thread,
};

/// One ghost the desk follows on one daemon: the solver, or a character
/// opened from the left.
pub struct Watched {
    pub(super) conn: Link,
    /// What to call her until the daemon says.
    fallback: String,
    pub link: LinkState,
    /// How the link has gone: since when, and why it is down.
    pub reach: Reach,
    pub card: Option<ghost_link::protocol::GhostCard>,
    pub status: Option<Status>,

    /// What has been seen of her journal, oldest first: its tail, kept up
    /// to date, and the places read for the threads shown. Every thread is
    /// read from it.
    pub journal: Held,
    /// The places whose entries were read, once each, when their thread
    /// was first shown.
    read_places: HashSet<String>,
    /// What you said that the journal has not shown yet.
    pub pending: Vec<Message>,
    /// Her work and what ended lately, as the daemon keeps it.
    pub work: Vec<Work>,
    /// Her sessions as her core last listed them, until the desk takes
    /// them in.
    pub sessions: Option<Vec<ghost_link::protocol::Session>>,
    /// The sessions, by id, whose rename or deletion her core did not
    /// carry out, until the desk takes them in.
    pub unsessioned: Vec<String>,
    /// Why the last thing you did to her could not be done, until the desk
    /// has said so.
    pub refused: Option<String>,
    /// The stops asked for from the desk since it began following her.
    pub stops: Vec<Stop>,
    /// What has been read of her steps: her looks at the places shown, and
    /// the work whose cards were opened.
    pub following: Following,
    /// Keeps a copy of the journal's tail and the work on this machine,
    /// for the first screen next time.
    keeper: cache::Keeper,
    /// Her clock's zone as it was kept, until her daemon says.
    kept_tz: Option<Tz>,
    /// Her core's id as it was kept, until her daemon says.
    kept_core: Option<String>,
    /// How her files are asked of her core, but for its id.
    media: media::Source,
}

/// What a thread draws of her images with while her core has not said
/// which core it is.
const NO_CORE_YET: &str = "Waiting for her core";
/// Her core's id, which her images are kept here by: as her daemon said
/// it in `status`, or, until it has, as it was `kept` last time. Without
/// one, why her images cannot be shown, in a line.
fn core_id<'a>(status: Option<&'a Status>, kept: Option<&'a str>) -> Result<&'a str, &'static str> {
    let id = match status {
        Some(status) => status.core.as_ref().map(|c| c.id.as_str()),
        None => kept,
    };
    id.filter(|id| !id.is_empty()).ok_or(NO_CORE_YET)
}

impl Watched {
    /// Start following, with what was seen of this daemon last time on
    /// screen until it answers.
    pub(super) fn start(cfg: link::Config, fallback: &str) -> Self {
        let kept = cache::load(&cfg.url, cfg.ghost.as_deref());
        let keeper = cache::Keeper::new(&cfg.url, cfg.ghost.as_deref());
        let journal = Held::kept(kept.journal);
        let media = media::Source {
            core: String::new(),
            url: cfg.url.clone(),
            token: cfg.token.clone(),
            ghost: cfg.ghost.clone(),
        };
        Self {
            kept_core: kept.core,
            media,
            conn: Link::watch(cfg, journal.newest()),
            fallback: fallback.to_owned(),
            link: LinkState::Connecting,
            reach: Reach::default(),
            card: None,
            status: None,
            journal,
            read_places: HashSet::new(),
            pending: Vec::new(),
            work: kept.work,
            sessions: None,
            unsessioned: Vec::new(),
            refused: None,
            stops: Vec::new(),
            following: Following::default(),
            keeper,
            kept_tz: kept.timezone.and_then(|z| z.parse().ok()),
        }
    }

    pub fn name(&self) -> String {
        self.status
            .as_ref()
            .map(|s| s.name.clone())
            .or_else(|| self.card.as_ref().map(|c| c.name.clone()))
            .unwrap_or_else(|| self.fallback.clone())
    }

    /// Take in everything its link has learned. Says whether anything did.
    pub(super) fn pump(&mut self) -> bool {
        let mut changed = false;
        while let Ok(update) = self.conn.rx.try_recv() {
            self.take(update);
            changed = true;
        }
        let (journal, work, status) = (&self.journal, &self.work, &self.status);
        let kept_core = self.kept_core.as_deref();
        self.keeper.settle(|| cache::Cached {
            journal: journal.tail().to_vec(),
            work: work.clone(),
            timezone: status.as_ref().map(|s| s.tz.name().to_owned()),
            core: core_id(status.as_ref(), kept_core).ok().map(str::to_owned),
        });
        changed
    }

    /// Where her images are asked for, or, as a thread says it, why they
    /// cannot be yet.
    pub fn media(&self) -> Result<media::Source, &'static str> {
        let id = core_id(self.status.as_ref(), self.kept_core.as_deref())?;
        Ok(media::Source {
            core: id.to_owned(),
            ..self.media.clone()
        })
    }

    /// Say `text` to her in the place `place` on the console channel, and
    /// show it until her journal does.
    pub(super) fn say(&mut self, place: String, text: String) {
        self.pending.push(Message {
            place: place.clone(),
            hers: false,
            text: text.clone(),
            at: Utc::now(),
        });
        self.conn.order(Order::Say { place, text });
    }

    /// Answer or stop a piece of her work, as an operator of hers.
    pub(crate) fn order(&self, order: Order) {
        self.conn.order(order);
    }

    /// Ask her daemon for what the thread on screen would show and does
    /// not have yet: the place's own entries, once, when it is first
    /// shown, and her steps as `follow.rs` decides; nothing while her
    /// daemon cannot be reached.
    pub(super) fn follow(&mut self, wanted: &thread::Wanted) {
        if self.link != LinkState::Connected {
            return;
        }
        if self.read_places.insert(wanted.place.clone()) {
            self.conn.read(Read::Place {
                place: wanted.place.clone(),
            });
        }
        let now = Instant::now();
        let place = self
            .following
            .place(&wanted.place, now, wanted.thinking, wanted.newest_end);
        for read in place.into_iter().chain(
            wanted
                .works
                .iter()
                .filter_map(|(work, open)| self.following.work(work, now, *open))
                .collect::<Vec<_>>(),
        ) {
            self.conn.read(read);
        }
    }

    /// Ask her daemon what filled the context of `work`, the work whose
    /// context the right column shows, as `follow.rs` decides; `open` is
    /// whether it is still going.
    pub(super) fn follow_context(&mut self, work: &str, open: bool) {
        if self.link != LinkState::Connected {
            return;
        }
        if let Some(read) = self.following.context(work, Instant::now(), open) {
            self.conn.read(read);
        }
    }

    /// Try its daemon again now, instead of after the wait.
    pub fn retry(&self) {
        self.conn.retry();
    }

    pub fn tz(&self) -> Tz {
        self.status
            .as_ref()
            .map(|s| s.tz)
            .or(self.kept_tz)
            .unwrap_or(chrono_tz::UTC)
    }

    fn take(&mut self, update: Update) {
        match update {
            Update::Link(state) => {
                // Nothing asked on a connection that went away will be
                // answered.
                if state != LinkState::Connected {
                    self.following.lost();
                }
                self.reach.note(&self.link, &state);
                self.link = state;
            }
            Update::Looks { place, looks } => self.following.looks(&place, Instant::now(), looks),
            Update::Steps { work, log } => self.following.steps(&work, Instant::now(), log),
            Update::Context { work, context } => {
                self.following.took_context(&work, Instant::now(), context);
            }
            Update::Welcome(card) => self.card = Some(card),
            Update::Status(status) => {
                self.status = Some(*status);
                self.keeper.changed();
            }
            Update::Journal(entries) => {
                let new = self.journal.stretch(entries);
                self.hold(&new);
            }
            Update::Entry(entry) => {
                let new = self.journal.stretch(vec![entry]);
                self.hold(&new);
            }
            Update::Place(entries) => {
                let new = self.journal.place(entries);
                self.hold(&new);
            }
            Update::Refused { order, why } => {
                if let Some(id) = order.session() {
                    self.unsessioned.push(id.to_owned());
                }
                // A line that was not said is not left waiting for her.
                if let Order::Say { place, text, .. } = order
                    && let Some(i) = self
                        .pending
                        .iter()
                        .rposition(|m| m.place == place && m.text == text && !m.hers)
                {
                    self.pending.remove(i);
                }
                self.refused = Some(why);
            }
            Update::Stopped { place, stopped } => self.stops.push(Stop {
                place,
                at: Utc::now(),
                stopped,
            }),
            Update::Work(work) => {
                self.work = work;
                self.keeper.changed();
            }
            Update::Sessions(sessions) => self.sessions = Some(sessions),
            Update::Roster(_) | Update::Attention { .. } | Update::Ordered { .. } => {}
        }
    }
}

impl Watched {
    /// Entries newly held: each line among them settles one shown before
    /// her journal had it, so the same words sent twice still show twice.
    fn hold(&mut self, new: &[Entry]) {
        if new.is_empty() {
            return;
        }
        for written in new.iter().filter_map(thread::console_message) {
            if let Some(i) = self.pending.iter().position(|m| m.written_as(&written)) {
                self.pending.remove(i);
            }
        }
        self.keeper.changed();
    }
}

/// A character opened from the left: who, on which daemon, the link that
/// follows her, and the place her thread shows.
pub struct Open {
    pub host: Host,
    pub id: String,
    pub url: String,
    pub her: Watched,
    /// The place on screen, by key, once one is chosen.
    pub place: Option<String>,
    /// A piece of her work she was opened for, from Attention: until a
    /// place is chosen, her thread shows the place it was taken on in, once
    /// her journal is in.
    pub focus: Option<String>,
}

impl Open {
    /// The place her thread shows: the one chosen, or else the one the
    /// work she was opened for was taken on in, or else the one something
    /// was last said in, or else yours with her.
    pub fn place(&self, who: &str) -> String {
        self.place
            .clone()
            .or_else(|| {
                let work = self.focus.as_deref()?;
                thread::task_place(&self.her.journal, work)
            })
            .or_else(|| {
                thread::places(&self.her.journal)
                    .first()
                    .map(|s| s.key.clone())
            })
            .unwrap_or_else(|| yours(who))
    }

    /// The places to pick from, the most recent first, with yours with her
    /// among them even before you have said anything.
    pub fn places(&self, who: &str) -> Vec<thread::Spot> {
        let mut places = thread::places(&self.her.journal);
        let yours = yours(who);
        if !places.iter().any(|p| p.key == yours) {
            places.push(thread::Spot {
                key: yours,
                label: format!("{who} · {CONSOLE}"),
                at: chrono::DateTime::<Utc>::MIN_UTC,
            });
        }
        places
    }
}

/// The key of your own place with a character: on the console channel,
/// named after you, as the daemon names it when no place is given.
pub fn yours(who: &str) -> String {
    thread::place_key(&who.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn her_images_are_kept_by_the_core_id_her_daemon_says_or_the_one_kept_until_it_does() {
        let said = Status::from_value(&serde_json::json!({
            "core": {"id": "5f0c", "id_file": "/srv/ghost/core-id"},
        }));
        assert_eq!(core_id(Some(&said), Some("77aa")), Ok("5f0c"));
        assert_eq!(core_id(None, Some("77aa")), Ok("77aa"), "before it says");
        assert_eq!(core_id(None, None), Err(NO_CORE_YET));
    }
}
