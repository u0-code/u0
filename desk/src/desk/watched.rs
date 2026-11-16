//! What the desk follows: each ghost it watches on a daemon, kept up to
//! date from her link, and the character opened from the left, with the
//! place her thread shows.

use chrono::Utc;
use chrono_tz::Tz;
use ghost_link::protocol::Work;

use crate::{
    daemons::Host,
    link::{self, Link},
    model::{Entry, LinkState, Message, PersonView, Reach, Status, Update},
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
    pub people: Vec<PersonView>,
    /// Its journal's tail, oldest first. Every thread is read from it.
    pub journal: Vec<Entry>,
    /// What was said on the desk that the journal has not shown yet: your
    /// lines as you type them, its answers as they arrive.
    pub pending: Vec<Message>,
    /// The session it is typing in, if any.
    pub typing: Option<String>,
    pub desk_online: bool,
    /// Her work and what ended lately, as the daemon keeps it.
    pub work: Vec<Work>,
}

impl Watched {
    pub(super) fn start(cfg: link::Config, fallback: &str) -> Self {
        Self {
            conn: Link::watch(cfg),
            fallback: fallback.to_owned(),
            link: LinkState::Connecting,
            reach: Reach::default(),
            card: None,
            status: None,
            people: Vec::new(),
            journal: Vec::new(),
            pending: Vec::new(),
            typing: None,
            desk_online: false,
            work: Vec::new(),
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
        changed
    }

    /// Say `text` to her in the place `place` on the desk channel, and
    /// show it until her journal does.
    pub(super) fn say(&mut self, place: String, text: String) {
        self.pending.push(Message {
            place: place.clone(),
            hers: false,
            text: text.clone(),
            at: Utc::now(),
        });
        self.conn.say(place, text);
    }

    /// Try its daemon again now, instead of after the wait.
    pub fn retry(&self) {
        self.conn.retry();
    }

    pub fn tz(&self) -> Tz {
        self.status.as_ref().map_or(chrono_tz::UTC, |s| s.tz)
    }

    /// How it stands with you: the person record for you on the desk.
    pub fn you(&self, who: &str) -> Option<&PersonView> {
        let key = link::person_key(who);
        self.people.iter().find(|p| p.key == key)
    }

    fn take(&mut self, update: Update) {
        match update {
            Update::Link(state) => {
                if state != LinkState::Connected {
                    self.desk_online = false;
                }
                self.reach.note(&self.link, &state);
                self.link = state;
            }
            Update::Welcome(card) => self.card = Some(card),
            Update::Status(status) => self.status = Some(*status),
            Update::People(people) => self.people = people,
            Update::Journal(entries) => {
                self.journal = entries;
                // Each line the journal holds settles one shown before it,
                // so the same words sent twice still show twice.
                for written in self.journal.iter().filter_map(thread::desk_message) {
                    if let Some(i) = self.pending.iter().position(|m| m.written_as(&written)) {
                        self.pending.remove(i);
                    }
                }
            }
            Update::Entry(entry) => {
                if self.journal.last().is_none_or(|last| last.seq < entry.seq) {
                    if let Some(m) = thread::desk_message(&entry)
                        && let Some(i) = self.pending.iter().position(|p| p.written_as(&m))
                    {
                        self.pending.remove(i);
                    }
                    if self.journal.len() >= link::TAIL as usize * 4 {
                        self.journal.remove(0);
                    }
                    self.journal.push(entry);
                }
            }
            Update::HerSay { place, text } => {
                self.typing = None;
                self.pending.push(Message {
                    place,
                    hers: true,
                    text,
                    at: Utc::now(),
                });
            }
            Update::HerTyping { place } => self.typing = Some(place),
            Update::DeskOnline(on) => self.desk_online = on,
            Update::Work(work) => self.work = work,
            Update::Roster(_) => {}
        }
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
}

impl Open {
    /// The place her thread shows: the one chosen, or else the one
    /// something was last said in, or else yours with her on the desk.
    pub fn place(&self, who: &str) -> String {
        self.place
            .clone()
            .or_else(|| {
                thread::places(&self.her.journal)
                    .first()
                    .map(|s| s.key.clone())
            })
            .unwrap_or_else(|| link::person_key(who))
    }

    /// The places to pick from, the most recent first, with yours with her
    /// on the desk among them even before you have said anything.
    pub fn places(&self, who: &str) -> Vec<thread::Spot> {
        let mut places = thread::places(&self.her.journal);
        let yours = link::person_key(who);
        if !places.iter().any(|p| p.key == yours) {
            places.push(thread::Spot {
                key: yours,
                label: format!("{who} · {}", link::CHANNEL),
                at: chrono::DateTime::<Utc>::MIN_UTC,
            });
        }
        places
    }
}
