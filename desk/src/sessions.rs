//! Your sessions with the solver, as its core keeps them.
//!
//! A session is a place of its own on the console channel, and its core
//! keeps the list: what each is called, when it began, and which were set
//! aside, in her journal, so that every desk on that core lists the same
//! ones. The desk holds what the core last listed, and what it has done
//! itself since and the core has not listed yet, so that a new session,
//! a new name or one deleted shows at once and stays while the core
//! catches up. A new session nothing has been said in is the desk's
//! alone: the core keeps it from its first line.
//!
//! Before the core kept them, the desk kept them in `desk.json`. Those are
//! handed over once ([`handover`]): each is named on the core, so that it
//! is listed as the desk listed it, and any the core has that the desk no
//! longer did, because it was deleted here, is set aside on the core.
//! They leave `desk.json` when the core lists them so.

use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{DateTime, Utc};

/// What a session nothing has named goes by: its first line, as the core
/// cuts it.
const FIRST_LINE: usize = 48;
/// The longest name the core keeps.
const TITLE_MOST: usize = 80;

/// One conversation with the solver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
    /// When it began.
    pub at: DateTime<Utc>,
}

impl Session {
    /// A new session, by an id the desk proposes: `s` and the
    /// milliseconds, as ids have always been made, which its core takes;
    /// never the same twice in a run, however soon after another.
    pub fn new() -> Self {
        static LAST: AtomicI64 = AtomicI64::new(0);
        let now = Utc::now();
        let millis = now.timestamp_millis();
        let last = LAST.fetch_max(millis, Ordering::Relaxed);
        let millis = if last >= millis {
            LAST.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            millis
        };
        Self {
            id: format!("s{millis}"),
            title: String::new(),
            at: now,
        }
    }

    /// What the list calls it: its name, or nothing yet.
    pub fn label(&self) -> String {
        if self.title.is_empty() {
            "Untitled".to_owned()
        } else {
            self.title.clone()
        }
    }

    /// A session as its core lists it.
    pub fn listed(s: &ghost_link::protocol::Session) -> Self {
        Self {
            id: s.id.clone(),
            title: s.title.clone(),
            at: s.started.parse().unwrap_or(DateTime::<Utc>::MIN_UTC),
        }
    }
}

/// The sessions a search finds, by their place in the list: those whose
/// name has every word typed, in any case. An empty search finds all.
pub fn matching(sessions: &[Session], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    sessions
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            let label = s.label().to_lowercase();
            words.iter().all(|w| label.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .collect()
}

/// A name as its core keeps one: on one line, its spaces as one, cut at
/// the most a name is. Empty when nothing is left.
pub fn tidy(title: &str) -> String {
    let one = title.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(TITLE_MOST).collect()
}

/// What a session goes by once `text` is said in it first, as its core
/// will list it: the first line with anything on it, cut short.
fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .chars()
        .take(FIRST_LINE)
        .collect()
}

/// The sessions down the left, and the one on screen.
#[derive(Debug, Clone)]
pub struct Sessions {
    /// As the core last listed them, newest first, with what was done here
    /// since laid over them; before it has, those `desk.json` kept.
    listed: Vec<Session>,
    /// A new session nothing has been said in yet.
    draft: Option<Session>,
    /// The one on screen, by id.
    current: String,
    /// Begun or handed over here, and not yet listed by the core.
    begun: Vec<Session>,
    /// Named here, and not yet listed by that name.
    named: Vec<(String, String)>,
    /// Deleted here, and still listed by the core.
    gone: Vec<String>,
    /// Whether what `desk.json` kept has been handed over to the core in
    /// this run: it is sent once.
    handed: bool,
}

impl Sessions {
    /// The sessions to show until the core lists its own: those
    /// `desk.json` `kept`, if it still keeps any, and the one that was
    /// `open` last time on screen, or else the newest, or else a new one.
    pub fn new(kept: Vec<Session>, open: Option<String>) -> Self {
        let mut sessions = Self {
            current: String::new(),
            listed: kept,
            draft: None,
            begun: Vec::new(),
            named: Vec::new(),
            gone: Vec::new(),
            handed: false,
        };
        match open.or_else(|| sessions.listed.first().map(|s| s.id.clone())) {
            Some(id) => sessions.current = id,
            None => sessions.begin(),
        }
        sessions
    }

    /// Every session down the left, newest first: a new one first, then
    /// the rest.
    pub fn all(&self) -> Vec<Session> {
        self.draft.iter().chain(&self.listed).cloned().collect()
    }

    /// Whether `id` is one of them.
    pub fn has(&self, id: &str) -> bool {
        self.all().iter().any(|s| s.id == id)
    }

    /// The id of the one on screen.
    pub fn current_id(&self) -> &str {
        &self.current
    }

    /// The one on screen. There is always one: until the list has it, one
    /// with nothing known of it but its id, whose thread is in her journal
    /// all the same.
    pub fn current(&self) -> Session {
        self.all()
            .into_iter()
            .find(|s| s.id == self.current)
            .unwrap_or_else(|| Session {
                id: self.current.clone(),
                title: String::new(),
                at: Utc::now(),
            })
    }

    /// Show the session `id`, if it is one. Says whether it was.
    pub fn choose(&mut self, id: &str) -> bool {
        let found = self.has(id);
        if found {
            id.clone_into(&mut self.current);
        }
        found
    }

    /// Show a new session: the one nothing has been said in yet, if there
    /// is one, rather than another.
    pub fn begin(&mut self) {
        let draft = self.draft.get_or_insert_with(Session::new);
        self.current = draft.id.clone();
    }

    /// `text` was said in the session on screen. A new one is now one the
    /// core keeps, by its first line, as the core will list it.
    pub fn said(&mut self, text: &str) {
        if let Some(draft) = self.draft.take_if(|d| d.id == self.current) {
            let session = Session {
                title: first_line(text),
                ..draft
            };
            self.begun.push(session.clone());
            self.listed.insert(0, session);
        }
    }

    /// Call the session `id` `title`. Returns the name as its core will
    /// keep it, to be sent to it; `None` when there is nothing to call it
    /// or no such session.
    pub fn rename(&mut self, id: &str, title: &str) -> Option<String> {
        let title = tidy(title);
        if title.is_empty() {
            return None;
        }
        // A new session given a name is kept by the core from its name.
        if let Some(draft) = self.draft.take_if(|d| d.id == id) {
            self.begun.push(draft.clone());
            self.listed.insert(0, draft);
        }
        let session = self.listed.iter_mut().find(|s| s.id == id)?;
        session.title.clone_from(&title);
        if let Some(begun) = self.begun.iter_mut().find(|s| s.id == id) {
            begun.title.clone_from(&title);
        }
        self.named.retain(|(n, _)| n != id);
        self.named.push((id.to_owned(), title.clone()));
        Some(title)
    }

    /// Take the session `id` off the list, and show the one after it, or
    /// before it, or a new one. Returns whether its core has to be told:
    /// a new session nothing was said in is not the core's.
    pub fn remove(&mut self, id: &str) -> bool {
        let all = self.all();
        let Some(at) = all.iter().position(|s| s.id == id) else {
            return false;
        };
        let tell = if self.draft.take_if(|d| d.id == id).is_some() {
            false
        } else {
            self.listed.retain(|s| s.id != id);
            self.begun.retain(|s| s.id != id);
            self.gone.push(id.to_owned());
            true
        };
        if self.current == id {
            let rest = self.all();
            match rest.get(at).or_else(|| rest.get(at.wrapping_sub(1))) {
                Some(next) => self.current.clone_from(&next.id),
                None => self.begin(),
            }
        }
        tell
    }

    /// Whether it is time to hand over what `desk.json` kept: the first
    /// time it is asked in a run, and never again in it.
    pub fn handing_over(&mut self) -> bool {
        !std::mem::replace(&mut self.handed, true)
    }

    /// Hand over to the core the session `session` from `desk.json`: it
    /// is listed as it is now until the core lists it.
    pub fn hand_over(&mut self, session: &Session) {
        if !self.begun.iter().any(|s| s.id == session.id) {
            self.begun.push(session.clone());
        }
        self.named.retain(|(n, _)| *n != session.id);
        self.named.push((session.id.clone(), tidy(&session.title)));
    }

    /// Set aside on the core the session `id`, which `desk.json` no longer
    /// kept: it is not listed again.
    pub fn set_aside(&mut self, id: &str) {
        self.gone.push(id.to_owned());
    }

    /// The core could not do what was asked about the session `id`: what
    /// the core lists of it is what stands.
    pub fn refused(&mut self, id: &str) {
        self.named.retain(|(n, _)| n != id);
        self.gone.retain(|g| g != id);
        self.begun.retain(|s| s.id != id);
    }

    /// The sessions as the core lists them now, newest first. What was
    /// done here and the core does not show yet is laid over them; what it
    /// shows is no longer waited for. If the one on screen is no longer
    /// among them, a new one is shown.
    pub fn take(&mut self, core: Vec<Session>) {
        let find = |id: &str| core.iter().find(|s| s.id == id);
        self.begun.retain(|b| find(&b.id).is_none());
        self.named
            .retain(|(id, title)| find(id).is_none_or(|s| tidy(&s.title) != *title));
        self.gone.retain(|id| find(id).is_some());
        if self.draft.as_ref().is_some_and(|d| find(&d.id).is_some()) {
            self.draft = None;
        }
        let mut listed: Vec<Session> = self.begun.clone();
        listed.extend(core.into_iter().filter(|s| !self.gone.contains(&s.id)));
        listed.sort_by(|a, b| b.at.cmp(&a.at));
        for s in &mut listed {
            if let Some((_, title)) = self.named.iter().find(|(id, _)| *id == s.id) {
                s.title.clone_from(title);
            }
        }
        self.listed = listed;
        if !self.has(&self.current.clone()) {
            self.begin();
        }
    }
}

/// What handing the sessions `desk.json` kept over to the core still
/// takes, once the core has listed what it has.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Handover {
    /// The sessions to name on the core: those it does not list as the
    /// desk did.
    pub name: Vec<Session>,
    /// The sessions to set aside on the core, by id: those it lists that
    /// began before the handover did and the desk no longer kept, since
    /// they were deleted here.
    pub set_aside: Vec<String>,
}

impl Handover {
    /// Whether nothing is left to do: the core lists them as the desk did,
    /// and `desk.json` need not keep them.
    pub fn done(&self) -> bool {
        self.name.is_empty() && self.set_aside.is_empty()
    }
}

/// What is left of handing over the sessions `kept` in `desk.json`, now
/// that the core lists `core`. The handover began at `since`: a session
/// the core began after it was begun by this desk, or another, and is not
/// one deleted here. A session kept with no name had nothing said in it,
/// and is nothing to hand over.
pub fn handover(kept: &[Session], core: &[Session], since: DateTime<Utc>) -> Handover {
    let name = kept
        .iter()
        .filter(|k| !tidy(&k.title).is_empty())
        .filter(|k| {
            core.iter()
                .find(|c| c.id == k.id)
                .is_none_or(|c| tidy(&c.title) != tidy(&k.title))
        })
        .cloned()
        .collect();
    let set_aside = core
        .iter()
        .filter(|c| c.at < since && !kept.iter().any(|k| k.id == c.id))
        .map(|c| c.id.clone())
        .collect();
    Handover { name, set_aside }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 18, 9, minute, 0).unwrap()
    }

    fn session(id: &str, title: &str, minute: u32) -> Session {
        Session {
            id: id.to_owned(),
            title: title.to_owned(),
            at: at(minute),
        }
    }

    fn named(title: &str) -> Session {
        Session {
            title: title.to_owned(),
            ..Session::new()
        }
    }

    fn ids(sessions: &Sessions) -> Vec<String> {
        sessions.all().into_iter().map(|s| s.id).collect()
    }

    #[test]
    fn a_search_finds_sessions_by_every_word_of_their_name_in_any_case() {
        let sessions = [
            named("Free some disk space"),
            named("Summarise the commits"),
            named(""),
            named("Clear the Disk cache"),
        ];
        assert_eq!(matching(&sessions, "disk"), [0, 3]);
        assert_eq!(matching(&sessions, "DISK clear"), [3]);
        assert_eq!(matching(&sessions, "nothing like it"), Vec::<usize>::new());
        assert_eq!(matching(&sessions, "  "), [0, 1, 2, 3]);
        assert_eq!(
            matching(&sessions, "untitled"),
            [2],
            "an untitled session goes by its label"
        );
    }

    #[test]
    fn a_new_session_is_the_desk_s_until_something_is_said_in_it() {
        let mut s = Sessions::new(Vec::new(), None);
        let draft = s.current_id().to_owned();
        assert_eq!(ids(&s), std::slice::from_ref(&draft));
        // Asked for again, the same one is shown rather than another.
        s.begin();
        assert_eq!(ids(&s), std::slice::from_ref(&draft));

        // Said in, it goes by its first line at once, and stays while the
        // core has not listed it yet.
        s.said("\n  free some disk space on this machine, the big caches\nthen tell me");
        assert_eq!(
            s.current().title,
            "free some disk space on this machine, the big ca"
        );
        s.take(vec![session("s1", "older", 1)]);
        assert_eq!(ids(&s), [draft.clone(), "s1".to_owned()]);
        assert_eq!(s.current_id(), draft);

        // Listed by the core, it is the core's.
        let mut theirs = s.current();
        theirs.title = "free some disk space".to_owned();
        s.take(vec![theirs, session("s1", "older", 1)]);
        assert_eq!(s.current().title, "free some disk space");
        s.take(vec![session("s1", "older", 1)]);
        assert!(!s.has(&draft), "once the core has it, the core says");
    }

    #[test]
    fn a_name_given_here_holds_until_the_core_lists_it_so() {
        let mut s = Sessions::new(Vec::new(), None);
        s.take(vec![session("s1", "free some disk", 1)]);
        assert_eq!(
            s.rename("s1", "  Disk \n space "),
            Some("Disk space".into())
        );
        assert_eq!(s.rename("s1", " \n "), None);
        assert_eq!(s.rename("s9", "nothing"), None);
        // A list from before the core took the name does not undo it.
        s.take(vec![session("s1", "free some disk", 1)]);
        assert_eq!(s.all()[1].title, "Disk space");
        s.take(vec![session("s1", "Disk space", 1)]);
        // Once it has, what the core says stands, a rename elsewhere too.
        s.take(vec![session("s1", "Renamed elsewhere", 1)]);
        assert_eq!(s.all()[1].title, "Renamed elsewhere");

        // Refused, the core's name stands again.
        s.rename("s1", "Mine");
        s.refused("s1");
        s.take(vec![session("s1", "Renamed elsewhere", 1)]);
        assert_eq!(s.all()[1].title, "Renamed elsewhere");
    }

    #[test]
    fn a_session_deleted_here_stays_off_the_list_and_the_next_one_is_shown() {
        let core = || {
            vec![
                session("s3", "three", 3),
                session("s2", "two", 2),
                session("s1", "one", 1),
            ]
        };
        let mut s = Sessions::new(Vec::new(), Some("s2".into()));
        s.take(core());
        assert_eq!(s.current_id(), "s2");
        assert!(s.remove("s2"), "the core is told");
        assert_eq!(s.current_id(), "s1", "the one after it");
        // Still listed by a core that has not set it aside yet, it is not
        // shown.
        s.take(core());
        assert!(!s.has("s2"));
        assert!(s.remove("s1"));
        assert_eq!(s.current_id(), "s3", "the last goes back to the one before");

        // A new one nothing was said in is only the desk's to drop.
        s.begin();
        let draft = s.current_id().to_owned();
        assert!(!s.remove(&draft));
        assert_eq!(s.current_id(), "s3");
        assert!(s.remove("s3"));
        assert_eq!(
            ids(&s),
            [s.current_id().to_owned()],
            "a new one, when none is left"
        );
    }

    #[test]
    fn the_session_open_last_time_is_shown_and_one_the_core_lost_gives_way_to_a_new_one() {
        let mut s = Sessions::new(Vec::new(), Some("s2".into()));
        assert_eq!(
            s.current().id,
            "s2",
            "its thread shows before the list does"
        );
        s.take(vec![session("s2", "two", 2), session("s1", "one", 1)]);
        assert_eq!(s.current().title, "two");

        let mut s = Sessions::new(Vec::new(), Some("s7".into()));
        s.take(vec![session("s1", "one", 1)]);
        assert_eq!(s.current().title, "");
        assert_ne!(s.current_id(), "s7");
    }

    #[test]
    fn what_desk_json_kept_is_handed_over_once_and_nothing_is_lost() {
        let since = at(30);
        let kept = vec![
            session("s4", "", 20),
            session("s3", "Summarise the commits", 10),
            session("s2", "free some disk space\nplease", 5),
        ];
        // The core has s2 by its first line, s1 that was deleted here,
        // s9 begun since by another desk, and not s3, which was said in on
        // the old desk channel.
        let core = vec![
            session("s9", "begun since", 31),
            session("s2", "free some disk space please", 5),
            session("s1", "deleted here", 2),
        ];
        let left = handover(&kept, &core, since);
        assert_eq!(
            left.name.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["s3"],
            "the untitled one had nothing said in it; s2 is listed as it was"
        );
        assert_eq!(left.set_aside, ["s1"]);
        assert!(!left.done());

        // While the core catches up, the list is the desk's as it was.
        let mut s = Sessions::new(kept.clone(), Some("s2".into()));
        for session in &left.name {
            s.hand_over(session);
        }
        for id in &left.set_aside {
            s.set_aside(id);
        }
        s.take(core.clone());
        let titles: Vec<(String, String)> = s.all().into_iter().map(|s| (s.id, s.title)).collect();
        assert_eq!(
            titles,
            [
                ("s9".to_owned(), "begun since".to_owned()),
                ("s3".to_owned(), "Summarise the commits".to_owned()),
                ("s2".to_owned(), "free some disk space please".to_owned()),
            ]
        );

        // Once it has, nothing is left to send, and desk.json lets go.
        let after = vec![
            session("s9", "begun since", 31),
            session("s3", "Summarise the commits", 31),
            session("s2", "free some disk space please", 5),
        ];
        assert!(handover(&kept, &after, since).done());
    }
}
