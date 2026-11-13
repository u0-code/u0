//! What she can recall, derived from the journal.
//!
//! Everything in here is a view. It can be deleted at any time and the
//! journal will rebuild it: places and what is unread in them, the people she
//! knows, notes, intentions, the canon of her own life, and a full-text index
//! for remembering things that are no longer in front of her.
//!
//! It is kept in two layers: what was said, per platform, and what she
//! remembers, in one place.
//!
//! ```text
//! derived/memory/<channel>.db   one platform's record: its places, accounts, messages,
//!                               read markers, reactions, posts and their counts,
//!                               what she saw in its feed
//! derived/memory/global.db      her memory, whole: notes, profiles, digests of
//!                               conversations, intentions, work, her days, canon,
//!                               reflections, pursuits, craft, practice
//! ```
//!
//! A memory is not split by platform. It carries where it was formed (a
//! `channel:id` key on the note, the profile, the digest, the intention) and
//! is found from anywhere. Which database an event goes to is decided by
//! `home`. A query about one place's messages reads only that platform's
//! database; the few that span every platform say so in their names and docs.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params, types::Value};
use serde::Serialize;

use crate::{
    Result,
    journal::{Entry, Event, Journal, Sources, parse_stamp, stamp},
    social::{self, PlaceKind},
};

/// Where the derived memory lives inside a ghost directory.
pub const MEMORY_DIR: &str = "derived/memory";
/// Where it lived before it was kept per platform. Removed when found.
const LEGACY_FILE: &str = "derived/memory.db";
/// The database for what belongs to no platform.
const GLOBAL: &str = "global";

/// Bump when the schema changes. A mismatch rebuilds from the journal.
const SCHEMA_VERSION: i64 = 10;

const WARMTH_MIN: i64 = -20;
const WARMTH_MAX: i64 = 20;

/// A place and how far she has read in it.
#[derive(Debug, Clone, Serialize)]
pub struct PlaceState {
    pub key: String,
    pub channel: String,
    pub id: String,
    pub kind: PlaceKind,
    pub name: Option<String>,
    pub read_upto: i64,
    pub last_in_seq: i64,
    pub last_in_at: Option<DateTime<Utc>>,
    pub last_out_seq: i64,
    pub last_out_at: Option<DateTime<Utc>>,
    pub unread: i64,
    pub unread_addressed: i64,
}

/// One message in a place, hers or someone else's.
#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub incoming: bool,
    pub person: Option<String>,
    pub name: String,
    pub platform_id: Option<String>,
    pub reply_to: Option<String>,
    pub addressed: bool,
    pub text: String,
}

/// Someone she has heard from.
#[derive(Debug, Clone, Serialize)]
pub struct Person {
    pub key: String,
    pub channel: String,
    pub id: String,
    pub name: String,
    pub handle: Option<String>,
    pub automated: bool,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
    /// Messages she has heard from them, anywhere.
    pub heard: i64,
    /// Sum of every `Felt` toward them, clamped.
    pub warmth: i64,
    /// She has left them.
    pub gone: bool,
    /// Key of the private place she shares with them, once one exists.
    pub direct: Option<String>,
    /// Distinct days on which she heard from them.
    pub active_days: i64,
    /// Distinct days on which she wrote to them privately without an answer since.
    pub unanswered_days: i64,
    /// When she last wrote to them privately.
    pub last_out_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub text: String,
    /// Where she learned it.
    pub place: Option<String>,
    /// What she had in front of her when she wrote it.
    pub sources: Option<Sources>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub fact: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Intention {
    pub seq: i64,
    pub due: DateTime<Utc>,
    pub what: String,
    pub about: Option<String>,
    pub place: Option<String>,
    /// She said she would come back with this.
    pub owed: bool,
}

/// Something found by searching her memory.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub seq: i64,
    pub at: Option<DateTime<Utc>>,
    pub kind: String,
    pub text: String,
    /// The place it was said or learned in, when it belongs to one.
    pub place: Option<String>,
}

/// Something she has taken on.
#[derive(Debug, Clone, Serialize)]
pub struct Undertaking {
    pub id: String,
    /// The seq of the entry that began it.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub brief: String,
    pub asked_by: Option<String>,
    pub place: Option<String>,
    /// `None` while it is still open.
    pub outcome: Option<String>,
    /// The latest account of it: the last sitting's, or how it ended.
    pub summary: Option<String>,
    pub sittings: i64,
    pub last_at: DateTime<Utc>,
    /// What she asked whoever it is for, and is waiting to hear.
    pub waiting: Option<String>,
    /// When she asked.
    pub asked_at: Option<DateTime<Utc>>,
}

/// A proof she is waiting for: `word`, from the account `claimed`.
#[derive(Debug, Clone, Serialize)]
pub struct Challenge {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub claimant: String,
    pub claimed: String,
    pub word: String,
}

/// What she kept of the earlier part of a conversation.
#[derive(Debug, Clone, Serialize)]
pub struct Digest {
    /// The newest message it stands for.
    pub upto: i64,
    pub at: DateTime<Utc>,
    pub text: String,
    /// Everything it stands for, when that was written down.
    pub sources: Option<Sources>,
}

/// Her standing account of someone, as last put in order.
#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    /// The seq of the entry that wrote it. Notes after this are newer.
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub shared: String,
    pub private: String,
    /// What it was put in order from, when that was written down.
    pub sources: Option<Sources>,
}

/// Something she has learned about her craft and not since replaced.
#[derive(Debug, Clone, Serialize)]
pub struct Lesson {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub lesson: String,
}

/// A public post of hers and the latest count of how it did.
#[derive(Debug, Clone, Serialize)]
pub struct Standing {
    pub id: String,
    pub at: DateTime<Utc>,
    /// What she posted, if her journal has it.
    pub text: Option<String>,
    pub likes: i64,
    pub replies: i64,
    pub reposts: i64,
    pub views: i64,
}

/// A place other than the one she is looking at, and what went on there.
#[derive(Debug, Clone, Serialize)]
pub struct Elsewhere {
    pub place: PlaceState,
    /// Messages that reached her there in the window.
    pub heard: i64,
    /// Messages she sent there in the window.
    pub said: i64,
    /// The newest line there.
    pub last: Line,
}

/// Which database an event belongs in.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Home {
    /// Her memory.
    Global,
    /// One platform's record.
    Channel(String),
}

/// The platform a `channel:id` key belongs to.
fn channel_of(key: &str) -> Option<&str> {
    social::split_key(key).map(|(channel, _)| channel)
}

/// Which database an event goes to. What was said and done on a platform,
/// and the accounts there, are that platform's record. Everything she makes
/// of it, and everything of her own, is her memory.
fn home(event: &Event) -> Home {
    match event {
        Event::Heard { channel, .. }
        | Event::Read { channel, .. }
        | Event::Said { channel, .. }
        | Event::Reacted { channel, .. }
        | Event::Posted { channel, .. }
        | Event::Saw { channel, .. }
        | Event::Measured { channel, .. } => Home::Channel(channel.clone()),
        // How she stands with an account, and whether she has left it, are
        // kept with the account.
        Event::Felt { toward: key, .. } | Event::Left { person: key, .. } => {
            channel_of(key).map_or(Home::Global, |c| Home::Channel(c.to_owned()))
        }
        Event::Noted { .. }
        | Event::Profiled { .. }
        | Event::Digested { .. }
        | Event::Intended { .. }
        | Event::Settled { .. }
        | Event::Undertook { .. }
        | Event::Worked { .. }
        | Event::Asked { .. }
        | Event::Added { .. }
        | Event::Concluded { .. }
        | Event::Born { .. }
        | Event::Dormant { .. }
        | Event::Woke
        | Event::Slept
        | Event::Planned { .. }
        | Event::Lived { .. }
        | Event::Decided { .. }
        | Event::Canon { .. }
        | Event::Reflected { .. }
        | Event::Learned { .. }
        | Event::Pursued { .. }
        | Event::Practised { .. }
        | Event::Challenged { .. }
        | Event::Linked { .. }
        // What she found out is hers, wherever she was when she did.
        | Event::LookedUp { .. } => Home::Global,
    }
}

/// The file a platform's database is kept in. A name that is not plain
/// lowercase letters, digits, `-` and `_`, or that is the global one's, is
/// written in hex so that no name can stand for another.
fn file_of(channel: &str) -> String {
    let plain = !channel.is_empty()
        && channel != GLOBAL
        && channel
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if plain {
        format!("{channel}.db")
    } else {
        let mut name = String::from("~");
        for b in channel.bytes() {
            let _ = write!(name, "{b:02x}");
        }
        name.push_str(".db");
        name
    }
}

/// The platform a database file is for, or `None` for one that is not a
/// platform's.
fn channel_of_file(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".db")?;
    if stem == GLOBAL {
        return None;
    }
    match stem.strip_prefix('~') {
        Some(hex) => {
            let bytes: Option<Vec<u8>> = (0..hex.len())
                .step_by(2)
                .map(|i| {
                    hex.get(i..i + 2)
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                })
                .collect();
            String::from_utf8(bytes?).ok()
        }
        None => Some(stem.to_owned()),
    }
}

/// Everything she can recall, kept per platform.
pub struct Memory {
    /// Where the databases are; `None` keeps them in memory.
    dir: Option<PathBuf>,
    global: Shard,
    channels: BTreeMap<String, Shard>,
}

impl Memory {
    /// Open the derived store under `dir`, creating or rebuilding it.
    pub fn open(dir: &Path) -> Result<Self> {
        let root = dir.join(MEMORY_DIR);
        // From before memory was kept per platform. It is only a view.
        for legacy in [
            LEGACY_FILE,
            "derived/memory.db-wal",
            "derived/memory.db-shm",
        ] {
            let path = dir.join(legacy);
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        std::fs::create_dir_all(&root)?;
        let global = root.join(format!("{GLOBAL}.db"));
        let version: i64 =
            Connection::open(&global)?.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION && version != 0 {
            // An older shape. It is only a view: throw all of it away.
            std::fs::remove_dir_all(&root)?;
            std::fs::create_dir_all(&root)?;
        }
        let mut channels = BTreeMap::new();
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(channel) = channel_of_file(&name) {
                channels.insert(channel, Shard::open(&entry.path())?);
            }
        }
        Ok(Self {
            global: Shard::open(&root.join(format!("{GLOBAL}.db")))?,
            dir: Some(root),
            channels,
        })
    }

    /// A store that lives only as long as the value. For tests and tools.
    pub fn in_memory() -> Result<Self> {
        Ok(Self {
            dir: None,
            global: Shard::prepare(Connection::open_in_memory()?)?,
            channels: BTreeMap::new(),
        })
    }

    /// The platforms she has a memory of.
    pub fn channels(&self) -> impl Iterator<Item = &str> {
        self.channels.keys().map(String::as_str)
    }

    /// The seq of the last journal entry folded in.
    pub fn last_seq(&self) -> Result<i64> {
        self.global.last_seq()
    }

    /// Fold in every journal entry not yet seen. Returns how many.
    pub fn catch_up(&mut self, journal: &Journal) -> Result<u64> {
        let mut applied = 0;
        loop {
            let batch = journal.read(self.last_seq()? + 1, 500)?;
            if batch.is_empty() {
                return Ok(applied);
            }
            for entry in &batch {
                self.apply(entry)?;
                applied += 1;
            }
        }
    }

    /// Fold one journal entry into the database it belongs in. The global
    /// database records it last, so an entry interrupted half way is folded
    /// again on the next start, and a database that already has it skips it.
    pub fn apply(&mut self, entry: &Entry) -> Result<()> {
        match home(&entry.event) {
            Home::Global => self.global.fold(entry),
            Home::Channel(channel) => {
                self.channel_mut(&channel)?.fold(entry)?;
                self.global.mark(entry.seq)
            }
        }
    }

    /// A platform's database, made the first time something happens there.
    fn channel_mut(&mut self, channel: &str) -> Result<&mut Shard> {
        if !self.channels.contains_key(channel) {
            let shard = match &self.dir {
                Some(root) => Shard::open(&root.join(file_of(channel)))?,
                None => Shard::prepare(Connection::open_in_memory()?)?,
            };
            self.channels.insert(channel.to_owned(), shard);
        }
        Ok(self.channels.get_mut(channel).expect("inserted just above"))
    }

    /// The database a `channel:id` key's platform keeps, if it has one.
    fn shard_of(&self, key: &str) -> Option<&Shard> {
        channel_of(key).and_then(|c| self.channels.get(c))
    }

    // One platform's record: read from that platform's database.

    pub fn place(&self, key: &str) -> Result<Option<PlaceState>> {
        self.shard_of(key).map_or(Ok(None), |s| s.place(key))
    }

    /// The last `limit` messages in a place, oldest first.
    pub fn thread(&self, place: &str, limit: usize) -> Result<Vec<Line>> {
        self.shard_of(place)
            .map_or(Ok(Vec::new()), |s| s.thread(place, limit))
    }

    /// What she kept of the earlier part of a conversation, if she has.
    pub fn digest(&self, place: &str) -> Result<Option<Digest>> {
        self.global.digest(place)
    }

    /// The messages in a place after entry `after` and up to `upto`, oldest
    /// first, and no more than `limit` of them.
    pub fn thread_between(
        &self,
        place: &str,
        after: i64,
        upto: i64,
        limit: usize,
    ) -> Result<Vec<Line>> {
        self.shard_of(place).map_or(Ok(Vec::new()), |s| {
            s.thread_between(place, after, upto, limit)
        })
    }

    /// The messages on a platform from entry `from` to entry `upto`, each
    /// with the place it was in, oldest first. When there are more than
    /// `limit`, the newest are kept.
    pub fn lines_on(
        &self,
        channel: &str,
        from: i64,
        upto: i64,
        limit: usize,
    ) -> Result<Vec<(String, Line)>> {
        self.channels
            .get(channel)
            .map_or(Ok(Vec::new()), |s| s.lines_on(from, upto, limit))
    }

    /// The messages in a place around entry `seq`: up to `n` before it, the
    /// one at it, and up to `n` after, oldest first.
    pub fn around(&self, place: &str, seq: i64, n: usize) -> Result<Vec<Line>> {
        let Some(shard) = self.shard_of(place) else {
            return Ok(Vec::new());
        };
        let mut lines = shard.thread_before(place, seq, n + 1)?;
        lines.extend(shard.thread_between(place, seq, i64::MAX, n)?);
        Ok(lines)
    }

    /// The last `limit` messages in a place up to and including entry
    /// `upto`, oldest first.
    pub fn thread_before(&self, place: &str, upto: i64, limit: usize) -> Result<Vec<Line>> {
        self.shard_of(place)
            .map_or(Ok(Vec::new()), |s| s.thread_before(place, upto, limit))
    }

    /// How many messages a place holds after entry `after`.
    pub fn lines_after(&self, place: &str, after: i64) -> Result<i64> {
        self.shard_of(place)
            .map_or(Ok(0), |s| s.lines_after(place, after))
    }

    /// Whether a message with this channel id has already reached her here.
    pub fn knows_message(&self, place: &str, platform_id: &str) -> Result<bool> {
        self.shard_of(place)
            .map_or(Ok(false), |s| s.knows_message(place, platform_id))
    }

    /// Whether the message with this channel id in this place is one she sent.
    pub fn is_hers(&self, place: &str, platform_id: &str) -> Result<bool> {
        self.shard_of(place)
            .map_or(Ok(false), |s| s.is_hers(place, platform_id))
    }

    /// Whether she has already answered this message.
    pub fn replied_to(&self, place: &str, platform_id: &str) -> Result<bool> {
        self.shard_of(place)
            .map_or(Ok(false), |s| s.replied_to(place, platform_id))
    }

    /// How many messages she has sent in a place since `since`.
    pub fn said_since(&self, place: &str, since: DateTime<Utc>) -> Result<i64> {
        self.shard_of(place)
            .map_or(Ok(0), |s| s.said_since(place, since))
    }

    pub fn person(&self, key: &str) -> Result<Option<Person>> {
        self.shard_of(key).map_or(Ok(None), |s| s.person(key))
    }

    /// The person on the other side of a private place.
    pub fn person_of_direct(&self, place: &str) -> Result<Option<Person>> {
        self.shard_of(place)
            .map_or(Ok(None), |s| s.person_of_direct(place))
    }

    /// The people most recently heard in a place, most recent first.
    pub fn people_in(&self, place: &str, limit: usize) -> Result<Vec<Person>> {
        self.shard_of(place)
            .map_or(Ok(Vec::new()), |s| s.people_in(place, limit))
    }

    /// Notes about someone, oldest first.
    pub fn notes(&self, person: &str, limit: usize) -> Result<Vec<Note>> {
        self.global.notes(person, limit)
    }

    /// Her standing account of someone, if she has written one.
    pub fn profile(&self, person: &str) -> Result<Option<Profile>> {
        self.global.profile(person)
    }

    /// Notes about someone written after entry `after`, oldest first.
    pub fn notes_after(&self, person: &str, after: i64, limit: usize) -> Result<Vec<Note>> {
        self.global.notes_after(person, after, limit)
    }

    /// Follower counts on a channel as they were reported, oldest first.
    pub fn followers(&self, channel: &str, limit: usize) -> Result<Vec<(DateTime<Utc>, i64)>> {
        self.channels
            .get(channel)
            .map_or(Ok(Vec::new()), |s| s.followers(channel, limit))
    }

    /// Her measured posts on a channel, newest first.
    pub fn standings(&self, channel: &str, limit: usize) -> Result<Vec<Standing>> {
        self.channels
            .get(channel)
            .map_or(Ok(Vec::new()), |s| s.standings(channel, limit))
    }

    // Her memory: read from the global database, whichever platform a
    // memory was formed on.

    pub fn canon(&self, limit: usize) -> Result<Vec<Fact>> {
        self.global.canon(limit)
    }

    /// What she has learned about her craft and still holds, oldest first.
    pub fn craft(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.global.craft(limit)
    }

    /// What she is after over months and has not let go of, oldest first.
    pub fn pursuits(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.global.pursuits(limit)
    }

    /// What doing things has taught her about doing things, oldest first.
    pub fn practice(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.global.practice(limit)
    }

    pub fn challenges_to(&self, claimed: &str, since: DateTime<Utc>) -> Result<Vec<Challenge>> {
        self.global.challenges_to(claimed, since)
    }

    pub fn linked(&self, key: &str) -> Result<Vec<String>> {
        self.global.linked(key)
    }

    // Across every platform. Each of these reads every database.

    /// The message with this journal seq, wherever it was.
    pub fn line(&self, seq: i64) -> Result<Option<(String, Line)>> {
        for shard in self.channels.values() {
            if let Some(found) = shard.line(seq)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// Every place on every platform, most recently active first.
    pub fn places(&self) -> Result<Vec<PlaceState>> {
        let mut all = Vec::new();
        for shard in self.channels.values() {
            all.extend(shard.places()?);
        }
        all.sort_by_key(|p| std::cmp::Reverse(p.last_in_seq.max(p.last_out_seq)));
        Ok(all)
    }

    /// Every place on every platform with something she has not looked at.
    pub fn places_unread(&self) -> Result<Vec<PlaceState>> {
        let mut all = Vec::new();
        for shard in self.channels.values() {
            all.extend(shard.places_unread()?);
        }
        Ok(all)
    }

    /// Everyone heard from since `since`, on every platform, most recent first.
    pub fn people_since(&self, since: DateTime<Utc>) -> Result<Vec<Person>> {
        let mut all = Vec::new();
        for shard in self.channels.values() {
            all.extend(shard.people_since(since)?);
        }
        all.sort_by_key(|p| std::cmp::Reverse(p.last_at));
        Ok(all)
    }

    /// Everyone she knows, on every platform, most recent first.
    pub fn people(&self) -> Result<Vec<Person>> {
        let mut all = Vec::new();
        for shard in self.channels.values() {
            all.extend(shard.people()?);
        }
        all.sort_by_key(|p| std::cmp::Reverse(p.last_at));
        Ok(all)
    }

    /// What she means to do and has not done, soonest first.
    pub fn open_intentions(&self) -> Result<Vec<Intention>> {
        self.global.open_intentions()
    }

    /// What she has taken on, most recently touched first.
    pub fn undertakings(&self, open: bool, limit: usize) -> Result<Vec<Undertaking>> {
        self.global.undertakings(open, limit)
    }

    pub fn undertaking(&self, id: &str) -> Result<Option<Undertaking>> {
        self.global.undertaking(id)
    }

    /// How many undertakings were begun at or after `since`.
    pub fn undertaken_since(&self, since: DateTime<Utc>) -> Result<i64> {
        self.global.undertaken_since(since)
    }

    /// Places on the same platform as `except`, other than it, where
    /// something was said since `since`, most recent first.
    pub fn elsewhere(
        &self,
        since: DateTime<Utc>,
        except: &str,
        limit: usize,
    ) -> Result<Vec<Elsewhere>> {
        self.shard_of(except)
            .map_or(Ok(Vec::new()), |s| s.elsewhere(since, except, limit))
    }

    /// Search her memory, and the messages on platform `channel`, oldest
    /// first. Words of three letters or more are matched as words
    /// and the best of them kept; shorter ones (most Chinese) are matched
    /// as text and the newest kept.
    pub fn search(&self, channel: &str, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let mut ranked: Vec<(f64, Hit)> = Vec::new();
        let mut scanned: Vec<Hit> = Vec::new();
        let shards = std::iter::once(&self.global).chain(self.channels.get(channel));
        for shard in shards {
            let (r, s) = shard.search(query, limit)?;
            ranked.extend(r);
            scanned.extend(s);
        }
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        scanned.sort_by_key(|h| std::cmp::Reverse(h.seq));
        let mut hits: Vec<Hit> = ranked.into_iter().take(limit).map(|(_, h)| h).collect();
        for hit in scanned.into_iter().take(limit) {
            if !hits.iter().any(|h| h.seq == hit.seq) {
                hits.push(hit);
            }
        }
        hits.sort_by_key(|h| h.seq);
        Ok(hits)
    }
}

/// One database: the global one, or one platform's. Every table exists in
/// every one; what is in them depends on which events were folded in.
struct Shard {
    conn: Connection,
}

impl Shard {
    fn open(path: &Path) -> Result<Self> {
        Self::prepare(Connection::open(path)?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// The seq of the last journal entry folded in.
    pub fn last_seq(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'last_seq'", [], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(0))
    }

    /// Note that entry `seq` has been seen, when nothing of it is kept here.
    fn mark(&mut self, seq: i64) -> Result<()> {
        if seq > self.last_seq()? {
            self.conn.execute(
                "INSERT INTO meta (key, value) VALUES ('last_seq', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [seq],
            )?;
        }
        Ok(())
    }

    /// Fold one journal entry into this database's views, unless it already
    /// has it.
    fn fold(&mut self, entry: &Entry) -> Result<()> {
        if entry.seq <= self.last_seq()? {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        let at = stamp(&entry.at);
        let seq = entry.seq;
        match &entry.event {
            Event::Heard {
                channel,
                place,
                from,
                id,
                text,
                reply_to,
                addressed,
            } => {
                let place_key = social::key(channel, &place.id);
                let person_key = social::key(channel, &from.id);
                tx.execute(
                    "INSERT INTO places (key, channel, id, kind, name, last_in_seq, last_in_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(key) DO UPDATE SET kind = ?4, name = COALESCE(?5, name),
                         last_in_seq = ?6, last_in_at = ?7",
                    params![
                        place_key,
                        channel,
                        place.id,
                        place.kind.as_str(),
                        place.name,
                        seq,
                        at
                    ],
                )?;
                let direct = (place.kind == PlaceKind::Direct).then_some(&place_key);
                tx.execute(
                    "INSERT INTO people (key, channel, id, name, handle, automated, first_at, last_at, heard, direct)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1, ?8)
                     ON CONFLICT(key) DO UPDATE SET name = ?4, handle = COALESCE(?5, handle),
                         automated = ?6, last_at = ?7, heard = heard + 1,
                         direct = COALESCE(?8, direct)",
                    params![person_key, channel, from.id, from.name, from.handle, from.automated, at, direct],
                )?;
                tx.execute(
                    "INSERT INTO messages (seq, at, place, dir, person, name, platform_id, reply_to, addressed, text)
                     VALUES (?1, ?2, ?3, 'in', ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![seq, at, place_key, person_key, from.name, id, reply_to, addressed, text],
                )?;
                remember(
                    &tx,
                    seq,
                    &at,
                    "heard",
                    &format!("{}: {text}", from.name),
                    Some(&place_key),
                )?;
            }
            Event::Said {
                channel,
                place,
                id,
                text,
                reply_to,
            } => {
                let place_key = social::key(channel, place);
                tx.execute(
                    "INSERT INTO places (key, channel, id, kind, last_out_seq, last_out_at)
                     VALUES (?1, ?2, ?3, 'public', ?4, ?5)
                     ON CONFLICT(key) DO UPDATE SET last_out_seq = ?4, last_out_at = ?5",
                    params![place_key, channel, place, seq, at],
                )?;
                tx.execute(
                    "INSERT INTO messages (seq, at, place, dir, name, platform_id, reply_to, text)
                     VALUES (?1, ?2, ?3, 'out', 'you', ?4, ?5, ?6)",
                    params![seq, at, place_key, id, reply_to, text],
                )?;
                remember(
                    &tx,
                    seq,
                    &at,
                    "said",
                    &format!("you: {text}"),
                    Some(&place_key),
                )?;
            }
            Event::Read {
                channel,
                place,
                upto,
            } => {
                tx.execute(
                    "UPDATE places SET read_upto = MAX(read_upto, ?2) WHERE key = ?1",
                    params![social::key(channel, place), upto],
                )?;
            }
            Event::Felt { toward, delta, .. } => {
                tx.execute(
                    "UPDATE people SET warmth = MAX(?3, MIN(?4, warmth + ?2)) WHERE key = ?1",
                    params![toward, i64::from(*delta), WARMTH_MIN, WARMTH_MAX],
                )?;
            }
            Event::Noted {
                about,
                text,
                place,
                sources,
            } => {
                let (from, upto, since, until) = columns(sources.as_ref());
                tx.execute(
                    "INSERT INTO notes (seq, at, person, text, place, src_from, src_upto, since, until)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![seq, at, about, text, place, from, upto, since, until],
                )?;
                remember(&tx, seq, &at, "noted", text, place.as_deref())?;
            }
            Event::Left { person, .. } => {
                tx.execute("UPDATE people SET gone = 1 WHERE key = ?1", [person])?;
            }
            Event::Intended {
                due,
                what,
                about,
                place,
                owed,
            } => {
                tx.execute(
                    "INSERT INTO intentions (seq, due, what, about, place, owed) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![seq, stamp(due), what, about, place, owed],
                )?;
            }
            Event::Settled { intention, .. } => {
                tx.execute(
                    "UPDATE intentions SET settled = 1 WHERE seq = ?1",
                    [intention],
                )?;
            }
            Event::Canon { fact, source } => {
                tx.execute(
                    "INSERT INTO canon (seq, at, fact, source) VALUES (?1, ?2, ?3, ?4)",
                    params![seq, at, fact, source],
                )?;
                remember(&tx, seq, &at, "canon", fact, None)?;
            }
            Event::Lived { text, .. } => remember(&tx, seq, &at, "lived", text, None)?,
            Event::Reflected { text, .. } => remember(&tx, seq, &at, "reflected", text, None)?,
            Event::Posted { channel, id, text } => {
                if let Some(id) = id {
                    tx.execute(
                        "INSERT INTO posts (channel, id, seq, at, text) VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(channel, id) DO UPDATE SET text = ?5",
                        params![channel, id, seq, at, text],
                    )?;
                }
                remember(&tx, seq, &at, "posted", text, None)?;
            }
            Event::Saw { text, .. } => remember(&tx, seq, &at, "saw", text, None)?,
            Event::Undertook {
                id,
                brief,
                asked_by,
                place,
            } => {
                tx.execute(
                    "INSERT OR IGNORE INTO undertakings (id, seq, at, brief, asked_by, place, last_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?3)",
                    params![id, seq, at, brief, asked_by, place],
                )?;
                remember(&tx, seq, &at, "undertook", brief, place.as_deref())?;
            }
            Event::Worked { id, summary, .. } => {
                tx.execute(
                    "UPDATE undertakings SET sittings = sittings + 1, summary = ?2, last_at = ?3 WHERE id = ?1",
                    params![id, summary, at],
                )?;
            }
            Event::Asked { id, question } => {
                tx.execute(
                    "UPDATE undertakings SET waiting = ?2, asked_at = ?3, last_at = ?3 WHERE id = ?1",
                    params![id, question, at],
                )?;
            }
            Event::Added { id, text } => {
                // Whatever she was waiting to hear, this is it.
                tx.execute(
                    "UPDATE undertakings SET waiting = NULL, asked_at = NULL, last_at = ?2 WHERE id = ?1",
                    params![id, at],
                )?;
                remember(&tx, seq, &at, "added", text, None)?;
            }
            Event::Concluded {
                id,
                outcome,
                summary,
            } => {
                tx.execute(
                    "UPDATE undertakings SET outcome = ?2, summary = ?3, last_at = ?4,
                     waiting = NULL, asked_at = NULL WHERE id = ?1",
                    params![id, outcome, summary, at],
                )?;
                remember(&tx, seq, &at, "concluded", summary, None)?;
            }
            Event::Measured {
                channel,
                followers,
                posts,
            } => {
                if let Some(count) = followers {
                    tx.execute(
                        "INSERT INTO followers (seq, at, channel, count) VALUES (?1, ?2, ?3, ?4)",
                        params![seq, at, channel, i64::try_from(*count).unwrap_or(i64::MAX)],
                    )?;
                }
                let n = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
                for post in posts {
                    tx.execute(
                        "INSERT INTO posts (channel, id, seq, at, likes, replies, reposts, views, measured)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
                         ON CONFLICT(channel, id) DO UPDATE SET
                             likes = ?5, replies = ?6, reposts = ?7, views = ?8, measured = 1",
                        params![channel, post.id, seq, at, n(post.likes), n(post.replies), n(post.reposts), n(post.views)],
                    )?;
                }
            }
            Event::Challenged {
                claimant,
                claimed,
                word,
            } => {
                tx.execute(
                    "INSERT INTO challenges (seq, at, claimant, claimed, word) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![seq, at, claimant, claimed, word],
                )?;
            }
            Event::Linked { one, other } => {
                tx.execute(
                    "INSERT INTO links (seq, one, other) VALUES (?1, ?2, ?3)",
                    params![seq, one, other],
                )?;
                tx.execute(
                    "UPDATE challenges SET met = 1
                     WHERE (claimant = ?1 AND claimed = ?2) OR (claimant = ?2 AND claimed = ?1)",
                    params![one, other],
                )?;
            }
            Event::Profiled {
                person,
                shared,
                private,
                sources,
            } => {
                let (from, upto, since, until) = columns(sources.as_ref());
                tx.execute(
                    "INSERT INTO profiles (person, seq, at, shared, private, src_from, src_upto, since, until)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(person) DO UPDATE SET seq = ?2, at = ?3, shared = ?4, private = ?5,
                       src_from = ?6, src_upto = ?7, since = ?8, until = ?9",
                    params![person, seq, at, shared, private, from, upto, since, until],
                )?;
            }
            Event::Digested {
                place,
                upto,
                text,
                sources,
            } => {
                let (from, src_upto, since, until) = columns(sources.as_ref());
                tx.execute(
                    "INSERT INTO digests (place, upto, at, text, src_from, src_upto, since, until)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(place) DO UPDATE SET upto = ?2, at = ?3, text = ?4,
                       src_from = ?5, src_upto = ?6, since = ?7, until = ?8",
                    params![place, upto, at, text, from, src_upto, since, until],
                )?;
            }
            Event::Pursued { what, replaces } => {
                if let Some(old) = replaces {
                    tx.execute("UPDATE pursuits SET retired = 1 WHERE seq = ?1", [old])?;
                }
                if !what.trim().is_empty() {
                    tx.execute(
                        "INSERT INTO pursuits (seq, at, lesson) VALUES (?1, ?2, ?3)",
                        params![seq, at, what],
                    )?;
                    remember(&tx, seq, &at, "pursued", what, None)?;
                }
            }
            Event::Practised { lesson, replaces } => {
                if let Some(old) = replaces {
                    tx.execute("UPDATE practice SET retired = 1 WHERE seq = ?1", [old])?;
                }
                tx.execute(
                    "INSERT INTO practice (seq, at, lesson) VALUES (?1, ?2, ?3)",
                    params![seq, at, lesson],
                )?;
                remember(&tx, seq, &at, "practised", lesson, None)?;
            }
            Event::LookedUp { query, found, .. } => {
                // Kept without a place: it came from the web, not from
                // anyone she was talking with, so it is nobody's secret.
                let text = format!("you looked up \"{query}\": {found}");
                remember(&tx, seq, &at, "looked_up", &text, None)?;
            }
            Event::Learned { lesson, replaces } => {
                if let Some(old) = replaces {
                    tx.execute("UPDATE craft SET retired = 1 WHERE seq = ?1", [old])?;
                }
                tx.execute(
                    "INSERT INTO craft (seq, at, lesson) VALUES (?1, ?2, ?3)",
                    params![seq, at, lesson],
                )?;
                remember(&tx, seq, &at, "learned", lesson, None)?;
            }
            Event::Born { .. }
            | Event::Dormant { .. }
            | Event::Woke
            | Event::Slept
            | Event::Planned { .. }
            | Event::Reacted { .. }
            | Event::Decided { .. } => {}
        }
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('last_seq', ?1)
             ON CONFLICT(key) DO UPDATE SET value = ?1",
            [seq],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn place(&self, key: &str) -> Result<Option<PlaceState>> {
        Ok(self
            .conn
            .query_row(
                &format!("{PLACE_SELECT} WHERE p.key = ?1"),
                [key],
                row_to_place,
            )
            .optional()?)
    }

    /// Every place she has been in, most recently active first.
    pub fn places(&self) -> Result<Vec<PlaceState>> {
        let mut stmt = self.conn.prepare(&format!(
            "{PLACE_SELECT} ORDER BY MAX(p.last_in_seq, p.last_out_seq) DESC"
        ))?;
        let rows = stmt.query_map([], row_to_place)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every place with something she has not looked at.
    pub fn places_unread(&self) -> Result<Vec<PlaceState>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{PLACE_SELECT} WHERE p.last_in_seq > p.read_upto"))?;
        let rows = stmt.query_map([], row_to_place)?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|p| p.unread > 0)
            .collect())
    }

    /// The last `limit` messages in a place, oldest first.
    pub fn thread(&self, place: &str, limit: usize) -> Result<Vec<Line>> {
        let mut stmt = self.conn.prepare(&format!(
            "{LINE_SELECT} WHERE place = ?1 ORDER BY seq DESC LIMIT ?2"
        ))?;
        let mut lines = stmt
            .query_map(params![place, as_i64(limit)], row_to_line)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lines.reverse();
        Ok(lines)
    }

    /// What she kept of the earlier part of a conversation, if she has.
    pub fn digest(&self, place: &str) -> Result<Option<Digest>> {
        Ok(self
            .conn
            .query_row(
                "SELECT upto, at, text, src_from, src_upto, since, until FROM digests WHERE place = ?1",
                [place],
                |r| {
                    Ok(Digest {
                        upto: r.get(0)?,
                        at: time_at(r, 1)?,
                        text: r.get(2)?,
                        sources: sources_at(r, 3)?,
                    })
                },
            )
            .optional()?)
    }

    /// The messages in a place after entry `after` and up to `upto`, oldest
    /// first, and no more than `limit` of them.
    pub fn thread_between(
        &self,
        place: &str,
        after: i64,
        upto: i64,
        limit: usize,
    ) -> Result<Vec<Line>> {
        let mut stmt = self.conn.prepare(&format!(
            "{LINE_SELECT} WHERE place = ?1 AND seq > ?2 AND seq <= ?3 ORDER BY seq LIMIT ?4"
        ))?;
        Ok(stmt
            .query_map(params![place, after, upto, as_i64(limit)], row_to_line)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The last `limit` messages in a place up to and including entry
    /// `upto`, oldest first.
    pub fn thread_before(&self, place: &str, upto: i64, limit: usize) -> Result<Vec<Line>> {
        let mut stmt = self.conn.prepare(&format!(
            "{LINE_SELECT} WHERE place = ?1 AND seq <= ?2 ORDER BY seq DESC LIMIT ?3"
        ))?;
        let mut lines = stmt
            .query_map(params![place, upto, as_i64(limit)], row_to_line)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lines.reverse();
        Ok(lines)
    }

    /// Every place's messages from entry `from` to `upto`, the newest
    /// `limit` of them, oldest first.
    pub fn lines_on(&self, from: i64, upto: i64, limit: usize) -> Result<Vec<(String, Line)>> {
        let mut stmt = self.conn.prepare(
            "SELECT place, seq, at, dir, person, name, platform_id, reply_to, addressed, text
             FROM messages WHERE seq >= ?1 AND seq <= ?2 ORDER BY seq DESC LIMIT ?3",
        )?;
        let mut lines = stmt
            .query_map(params![from, upto, as_i64(limit)], |r| {
                Ok((r.get(0)?, line_at(r, 1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lines.reverse();
        Ok(lines)
    }

    /// How many messages a place holds after entry `after`.
    pub fn lines_after(&self, place: &str, after: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE place = ?1 AND seq > ?2",
            params![place, after],
            |r| r.get(0),
        )?)
    }

    pub fn line(&self, seq: i64) -> Result<Option<(String, Line)>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT place, {LINE_COLUMNS} FROM messages WHERE seq = ?1"),
                [seq],
                |r| Ok((r.get(0)?, line_at(r, 1)?)),
            )
            .optional()?)
    }

    /// Whether a message with this channel id has already reached her here.
    pub fn knows_message(&self, place: &str, platform_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM messages WHERE place = ?1 AND platform_id = ?2 LIMIT 1",
                params![place, platform_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Whether the message with this channel id in this place is one she sent.
    pub fn is_hers(&self, place: &str, platform_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM messages WHERE place = ?1 AND platform_id = ?2 AND dir = 'out' LIMIT 1",
                params![place, platform_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Whether she has already replied to the message with this channel id.
    pub fn replied_to(&self, place: &str, platform_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM messages WHERE place = ?1 AND reply_to = ?2 AND dir = 'out' LIMIT 1",
                params![place, platform_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// How many messages she has sent in a place since `since`.
    pub fn said_since(&self, place: &str, since: DateTime<Utc>) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE place = ?1 AND dir = 'out' AND at >= ?2",
            params![place, stamp(&since)],
            |r| r.get(0),
        )?)
    }

    pub fn person(&self, key: &str) -> Result<Option<Person>> {
        let base = self
            .conn
            .query_row(
                "SELECT key, channel, id, name, handle, automated, first_at, last_at, heard, warmth, gone, direct
                 FROM people WHERE key = ?1",
                [key],
                |r| {
                    Ok(Person {
                        key: r.get(0)?,
                        channel: r.get(1)?,
                        id: r.get(2)?,
                        name: r.get(3)?,
                        handle: r.get(4)?,
                        automated: r.get(5)?,
                        first_at: time_at(r, 6)?,
                        last_at: time_at(r, 7)?,
                        heard: r.get(8)?,
                        warmth: r.get(9)?,
                        gone: r.get(10)?,
                        direct: r.get(11)?,
                        active_days: 0,
                        unanswered_days: 0,
                        last_out_at: None,
                    })
                },
            )
            .optional()?;
        let Some(mut person) = base else {
            return Ok(None);
        };
        person.active_days = self.conn.query_row(
            "SELECT COUNT(DISTINCT substr(at, 1, 10)) FROM messages WHERE person = ?1",
            [key],
            |r| r.get(0),
        )?;
        if let Some(direct) = &person.direct {
            let (days, last): (i64, Option<String>) = self.conn.query_row(
                "SELECT COUNT(DISTINCT substr(m.at, 1, 10)), MAX(m.at) FROM messages m
                 JOIN places p ON p.key = m.place
                 WHERE m.place = ?1 AND m.dir = 'out' AND m.seq > p.last_in_seq",
                [direct],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            person.unanswered_days = days;
            person.last_out_at = match last {
                Some(at) => parse_stamp(&at).ok(),
                None => self
                    .conn
                    .query_row(
                        "SELECT last_out_at FROM places WHERE key = ?1",
                        [direct],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten()
                    .and_then(|at| parse_stamp(&at).ok()),
            };
        }
        Ok(Some(person))
    }

    /// The person on the other side of a private place.
    pub fn person_of_direct(&self, place: &str) -> Result<Option<Person>> {
        let key: Option<String> = self
            .conn
            .query_row(
                "SELECT key FROM people WHERE direct = ?1 LIMIT 1",
                [place],
                |r| r.get(0),
            )
            .optional()?;
        match key {
            Some(key) => self.person(&key),
            None => Ok(None),
        }
    }

    /// The people most recently heard in a place, most recent first.
    pub fn people_in(&self, place: &str, limit: usize) -> Result<Vec<Person>> {
        let mut stmt = self.conn.prepare(
            "SELECT person FROM messages WHERE place = ?1 AND dir = 'in' AND person IS NOT NULL
             GROUP BY person ORDER BY MAX(seq) DESC LIMIT ?2",
        )?;
        let keys = stmt
            .query_map(params![place, as_i64(limit)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.load_people(&keys)
    }

    /// Everyone she heard from since `since`, most recent first.
    pub fn people_since(&self, since: DateTime<Utc>) -> Result<Vec<Person>> {
        let mut stmt = self.conn.prepare(
            "SELECT person FROM messages WHERE dir = 'in' AND person IS NOT NULL AND at >= ?1
             GROUP BY person ORDER BY MAX(seq) DESC",
        )?;
        let keys = stmt
            .query_map([stamp(&since)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.load_people(&keys)
    }

    /// Everyone she knows, most recently heard first.
    pub fn people(&self) -> Result<Vec<Person>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM people ORDER BY last_at DESC")?;
        let keys = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.load_people(&keys)
    }

    fn load_people(&self, keys: &[String]) -> Result<Vec<Person>> {
        let mut people = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(p) = self.person(key)? {
                people.push(p);
            }
        }
        Ok(people)
    }

    /// The newest `limit` notes about a person, oldest first.
    pub fn notes(&self, person: &str, limit: usize) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, text, place, src_from, src_upto, since, until
             FROM notes WHERE person = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let mut notes = stmt
            .query_map(params![person, as_i64(limit)], |r| {
                Ok(Note {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    text: r.get(2)?,
                    place: r.get(3)?,
                    sources: sources_at(r, 4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        notes.reverse();
        Ok(notes)
    }

    /// The newest `limit` facts about her own life, oldest first.
    pub fn canon(&self, limit: usize) -> Result<Vec<Fact>> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, at, fact, source FROM canon ORDER BY seq DESC LIMIT ?1")?;
        let mut facts = stmt
            .query_map([as_i64(limit)], |r| {
                Ok(Fact {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    fact: r.get(2)?,
                    source: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        facts.reverse();
        Ok(facts)
    }

    /// Intentions not yet settled, soonest first.
    pub fn open_intentions(&self) -> Result<Vec<Intention>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, due, what, about, place, owed FROM intentions WHERE settled = 0 ORDER BY due ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Intention {
                seq: r.get(0)?,
                due: time_at(r, 1)?,
                what: r.get(2)?,
                about: r.get(3)?,
                place: r.get(4)?,
                owed: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Search everything she has heard, said, lived and noted.
    ///
    /// Terms of three or more characters go through the trigram index; shorter
    /// ones (common in Chinese) fall back to a substring scan.
    /// The proofs she is still waiting for from the account `claimed`,
    /// asked for at or after `since`.
    pub fn challenges_to(&self, claimed: &str, since: DateTime<Utc>) -> Result<Vec<Challenge>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, claimant, claimed, word FROM challenges
             WHERE claimed = ?1 AND met = 0 AND at >= ?2 ORDER BY seq",
        )?;
        Ok(stmt
            .query_map(params![claimed, stamp(&since)], |r| {
                Ok(Challenge {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    claimant: r.get(2)?,
                    claimed: r.get(3)?,
                    word: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The other accounts shown to belong to the same person as `key`.
    pub fn linked(&self, key: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT one, other FROM links")?;
        let pairs = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Links are few; follow them until nothing new turns up.
        let mut same = vec![key.to_owned()];
        let mut grew = true;
        while grew {
            grew = false;
            for (one, other) in &pairs {
                for (here, there) in [(one, other), (other, one)] {
                    if same.contains(here) && !same.contains(there) {
                        same.push(there.clone());
                        grew = true;
                    }
                }
            }
        }
        same.remove(0);
        Ok(same)
    }

    /// Her standing account of someone, if she has put one in order.
    pub fn profile(&self, person: &str) -> Result<Option<Profile>> {
        Ok(self
            .conn
            .query_row(
                "SELECT seq, at, shared, private, src_from, src_upto, since, until
                 FROM profiles WHERE person = ?1",
                [person],
                |r| {
                    Ok(Profile {
                        seq: r.get(0)?,
                        at: time_at(r, 1)?,
                        shared: r.get(2)?,
                        private: r.get(3)?,
                        sources: sources_at(r, 4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Notes about someone written after entry `after`, oldest first.
    pub fn notes_after(&self, person: &str, after: i64, limit: usize) -> Result<Vec<Note>> {
        let mut notes = self.notes(person, limit)?;
        notes.retain(|n| n.seq > after);
        Ok(notes)
    }

    /// What she has taken on, most recently touched first. `open` leaves out
    /// what has ended.
    pub fn undertakings(&self, open: bool, limit: usize) -> Result<Vec<Undertaking>> {
        let only = if open { "WHERE outcome IS NULL" } else { "" };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {UNDERTAKING_COLUMNS} FROM undertakings {only} ORDER BY last_at DESC LIMIT ?1"
        ))?;
        Ok(stmt
            .query_map([as_i64(limit)], row_to_undertaking)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn undertaking(&self, id: &str) -> Result<Option<Undertaking>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {UNDERTAKING_COLUMNS} FROM undertakings WHERE id = ?1"),
                [id],
                row_to_undertaking,
            )
            .optional()?)
    }

    /// How many undertakings were begun at or after `since`.
    pub fn undertaken_since(&self, since: DateTime<Utc>) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM undertakings WHERE at >= ?1",
            [stamp(&since)],
            |r| r.get(0),
        )?)
    }

    /// What she has learned about her craft and still holds, oldest first.
    pub fn craft(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.held("craft", limit)
    }

    /// What she is after over months and has not let go of, oldest first.
    pub fn pursuits(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.held("pursuits", limit)
    }

    /// What doing things has taught her about doing things, oldest first.
    pub fn practice(&self, limit: usize) -> Result<Vec<Lesson>> {
        self.held("practice", limit)
    }

    /// The newest entries of one of the tables of things she holds until
    /// she replaces them, oldest first.
    fn held(&self, table: &'static str, limit: usize) -> Result<Vec<Lesson>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT seq, at, lesson FROM {table} WHERE retired = 0 ORDER BY seq DESC LIMIT ?1"
        ))?;
        let mut lessons = stmt
            .query_map([as_i64(limit)], |r| {
                Ok(Lesson {
                    seq: r.get(0)?,
                    at: time_at(r, 1)?,
                    lesson: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lessons.reverse();
        Ok(lessons)
    }

    /// Follower counts on a channel as they were reported, oldest first.
    pub fn followers(&self, channel: &str, limit: usize) -> Result<Vec<(DateTime<Utc>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT at, count FROM followers WHERE channel = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let mut counts = stmt
            .query_map(params![channel, as_i64(limit)], |r| {
                Ok((time_at(r, 0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        counts.reverse();
        Ok(counts)
    }

    /// Her measured posts on a channel, newest first.
    pub fn standings(&self, channel: &str, limit: usize) -> Result<Vec<Standing>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, at, text, likes, replies, reposts, views FROM posts
             WHERE channel = ?1 AND measured = 1 ORDER BY seq DESC LIMIT ?2",
        )?;
        Ok(stmt
            .query_map(params![channel, as_i64(limit)], |r| {
                Ok(Standing {
                    id: r.get(0)?,
                    at: time_at(r, 1)?,
                    text: r.get(2)?,
                    likes: r.get(3)?,
                    replies: r.get(4)?,
                    reposts: r.get(5)?,
                    views: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The places, other than `except`, where something passed since
    /// `since`: most recently active first.
    pub fn elsewhere(
        &self,
        since: DateTime<Utc>,
        except: &str,
        limit: usize,
    ) -> Result<Vec<Elsewhere>> {
        let mut stmt = self.conn.prepare(
            "SELECT place, SUM(dir = 'in'), SUM(dir = 'out'), MAX(seq) FROM messages
             WHERE at >= ?1 AND place != ?2 GROUP BY place ORDER BY MAX(seq) DESC LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![stamp(&since), except, as_i64(limit)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (key, heard, said, last) in rows {
            let (Some(place), Some((_, last))) = (self.place(&key)?, self.line(last)?) else {
                continue;
            };
            out.push(Elsewhere {
                place,
                heard,
                said,
                last,
            });
        }
        Ok(out)
    }

    /// Hits for words of three letters or more, with their rank (lower is
    /// better), and hits for shorter ones, newest first.
    #[allow(clippy::type_complexity)]
    fn search(&self, query: &str, limit: usize) -> Result<(Vec<(f64, Hit)>, Vec<Hit>)> {
        let terms: Vec<&str> = query.split_whitespace().collect();
        let mut ranked = Vec::new();
        let mut scanned = Vec::new();
        if terms.is_empty() {
            return Ok((ranked, scanned));
        }
        let limit = as_i64(limit);
        let (long, short): (Vec<&str>, Vec<&str>) =
            terms.iter().partition(|t| t.chars().count() >= 3);
        if !long.is_empty() {
            let phrases = long
                .iter()
                .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(" OR ");
            let mut stmt = self.conn.prepare(
                "SELECT seq, at, kind, text, place, bm25(recall) FROM recall WHERE recall MATCH ?1 ORDER BY rank LIMIT ?2",
            )?;
            ranked = stmt
                .query_map(params![phrases, limit], |r| {
                    Ok((r.get::<_, f64>(5)?, row_to_hit(r)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        if !short.is_empty() {
            let clause = short
                .iter()
                .map(|_| "text LIKE ? ESCAPE '\\'")
                .collect::<Vec<_>>()
                .join(" OR ");
            let patterns = short.iter().map(|t| {
                let escaped = t
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                format!("%{escaped}%")
            });
            let mut stmt = self.conn.prepare(&format!(
                "SELECT seq, at, kind, text, place FROM recall WHERE {clause} ORDER BY rowid DESC LIMIT {limit}"
            ))?;
            scanned = stmt
                .query_map(rusqlite::params_from_iter(patterns), row_to_hit)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        Ok((ranked, scanned))
    }
}

fn remember(
    tx: &rusqlite::Transaction<'_>,
    seq: i64,
    at: &str,
    kind: &str,
    text: &str,
    place: Option<&str>,
) -> Result<()> {
    tx.execute(
        "INSERT INTO recall (text, kind, seq, at, place) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![text, kind, seq, at, place],
    )?;
    Ok(())
}

fn as_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn time_at(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<DateTime<Utc>> {
    let text: String = row.get(idx)?;
    parse_stamp(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Sources as the four columns they are kept in.
fn columns(
    sources: Option<&Sources>,
) -> (Option<i64>, Option<i64>, Option<String>, Option<String>) {
    match sources {
        Some(s) => (
            Some(s.from),
            Some(s.upto),
            Some(stamp(&s.since)),
            Some(stamp(&s.until)),
        ),
        None => (None, None, None, None),
    }
}

/// Sources read back from four columns starting at `idx`.
fn sources_at(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<Sources>> {
    let from: Option<i64> = row.get(idx)?;
    let upto: Option<i64> = row.get(idx + 1)?;
    let since = opt_time_at(row, idx + 2)?;
    let until = opt_time_at(row, idx + 3)?;
    Ok(match (from, upto, since, until) {
        (Some(from), Some(upto), Some(since), Some(until)) => Some(Sources {
            from,
            upto,
            since,
            until,
        }),
        _ => None,
    })
}

fn opt_time_at(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let text: Option<String> = row.get(idx)?;
    Ok(text.and_then(|t| parse_stamp(&t).ok()))
}

const PLACE_SELECT: &str = "
SELECT p.key, p.channel, p.id, p.kind, p.name, p.read_upto,
       p.last_in_seq, p.last_in_at, p.last_out_seq, p.last_out_at,
       (SELECT COUNT(*) FROM messages m
         WHERE m.place = p.key AND m.dir = 'in' AND m.seq > p.read_upto),
       (SELECT COUNT(*) FROM messages m
         WHERE m.place = p.key AND m.dir = 'in' AND m.seq > p.read_upto AND m.addressed = 1)
FROM places p";

fn row_to_place(r: &rusqlite::Row<'_>) -> rusqlite::Result<PlaceState> {
    let kind: String = r.get(3)?;
    Ok(PlaceState {
        key: r.get(0)?,
        channel: r.get(1)?,
        id: r.get(2)?,
        kind: PlaceKind::parse(&kind),
        name: r.get(4)?,
        read_upto: r.get(5)?,
        last_in_seq: r.get(6)?,
        last_in_at: opt_time_at(r, 7)?,
        last_out_seq: r.get(8)?,
        last_out_at: opt_time_at(r, 9)?,
        unread: r.get(10)?,
        unread_addressed: r.get(11)?,
    })
}

const LINE_COLUMNS: &str = "seq, at, dir, person, name, platform_id, reply_to, addressed, text";
const LINE_SELECT: &str =
    "SELECT seq, at, dir, person, name, platform_id, reply_to, addressed, text FROM messages";

fn row_to_line(r: &rusqlite::Row<'_>) -> rusqlite::Result<Line> {
    line_at(r, 0)
}

fn line_at(r: &rusqlite::Row<'_>, o: usize) -> rusqlite::Result<Line> {
    let dir: String = r.get(o + 2)?;
    Ok(Line {
        seq: r.get(o)?,
        at: time_at(r, o + 1)?,
        incoming: dir == "in",
        person: r.get(o + 3)?,
        name: r.get(o + 4)?,
        platform_id: r.get(o + 5)?,
        reply_to: r.get(o + 6)?,
        addressed: r.get(o + 7)?,
        text: r.get(o + 8)?,
    })
}

fn row_to_hit(r: &rusqlite::Row<'_>) -> rusqlite::Result<Hit> {
    // FTS5 does not promise to keep column types, so accept either.
    let seq = match r.get::<_, Value>(0)? {
        Value::Integer(i) => i,
        Value::Text(t) => t.parse().unwrap_or(0),
        _ => 0,
    };
    let at: Option<String> = r.get(1)?;
    Ok(Hit {
        seq,
        at: at.and_then(|t| parse_stamp(&t).ok()),
        kind: r.get(2)?,
        text: r.get(3)?,
        place: r.get(4)?,
    })
}

const UNDERTAKING_COLUMNS: &str =
    "id, seq, at, brief, asked_by, place, outcome, summary, sittings, last_at, waiting, asked_at";

fn row_to_undertaking(r: &rusqlite::Row<'_>) -> rusqlite::Result<Undertaking> {
    Ok(Undertaking {
        id: r.get(0)?,
        seq: r.get(1)?,
        at: time_at(r, 2)?,
        brief: r.get(3)?,
        asked_by: r.get(4)?,
        place: r.get(5)?,
        outcome: r.get(6)?,
        summary: r.get(7)?,
        sittings: r.get(8)?,
        last_at: time_at(r, 9)?,
        waiting: r.get(10)?,
        asked_at: opt_time_at(r, 11)?,
    })
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS places (
    key TEXT PRIMARY KEY, channel TEXT NOT NULL, id TEXT NOT NULL,
    kind TEXT NOT NULL, name TEXT,
    read_upto INTEGER NOT NULL DEFAULT 0,
    last_in_seq INTEGER NOT NULL DEFAULT 0, last_in_at TEXT,
    last_out_seq INTEGER NOT NULL DEFAULT 0, last_out_at TEXT
);
CREATE TABLE IF NOT EXISTS people (
    key TEXT PRIMARY KEY, channel TEXT NOT NULL, id TEXT NOT NULL,
    name TEXT NOT NULL, handle TEXT, automated INTEGER NOT NULL DEFAULT 0,
    first_at TEXT NOT NULL, last_at TEXT NOT NULL,
    heard INTEGER NOT NULL DEFAULT 0, warmth INTEGER NOT NULL DEFAULT 0,
    gone INTEGER NOT NULL DEFAULT 0, direct TEXT
);
CREATE TABLE IF NOT EXISTS messages (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, place TEXT NOT NULL, dir TEXT NOT NULL,
    person TEXT, name TEXT NOT NULL, platform_id TEXT, reply_to TEXT,
    addressed INTEGER NOT NULL DEFAULT 0, text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_place ON messages (place, seq);
CREATE INDEX IF NOT EXISTS messages_person ON messages (person, seq);
CREATE INDEX IF NOT EXISTS messages_platform ON messages (place, platform_id);
CREATE TABLE IF NOT EXISTS notes (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, person TEXT NOT NULL, text TEXT NOT NULL,
    place TEXT, src_from INTEGER, src_upto INTEGER, since TEXT, until TEXT
);
CREATE INDEX IF NOT EXISTS notes_person ON notes (person, seq);
CREATE TABLE IF NOT EXISTS intentions (
    seq INTEGER PRIMARY KEY, due TEXT NOT NULL, what TEXT NOT NULL,
    about TEXT, place TEXT, settled INTEGER NOT NULL DEFAULT 0,
    owed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS canon (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, fact TEXT NOT NULL, source TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS undertakings (
    id TEXT PRIMARY KEY, seq INTEGER NOT NULL, at TEXT NOT NULL, brief TEXT NOT NULL,
    asked_by TEXT, place TEXT, outcome TEXT, summary TEXT,
    sittings INTEGER NOT NULL DEFAULT 0, last_at TEXT NOT NULL,
    waiting TEXT, asked_at TEXT
);
CREATE TABLE IF NOT EXISTS challenges (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, claimant TEXT NOT NULL,
    claimed TEXT NOT NULL, word TEXT NOT NULL, met INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS links (
    seq INTEGER PRIMARY KEY, one TEXT NOT NULL, other TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS digests (
    place TEXT PRIMARY KEY, upto INTEGER NOT NULL, at TEXT NOT NULL, text TEXT NOT NULL,
    src_from INTEGER, src_upto INTEGER, since TEXT, until TEXT
);
CREATE TABLE IF NOT EXISTS profiles (
    person TEXT PRIMARY KEY, seq INTEGER NOT NULL, at TEXT NOT NULL,
    shared TEXT NOT NULL, private TEXT NOT NULL,
    src_from INTEGER, src_upto INTEGER, since TEXT, until TEXT
);
CREATE TABLE IF NOT EXISTS craft (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, lesson TEXT NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS pursuits (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, lesson TEXT NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS practice (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, lesson TEXT NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS followers (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, channel TEXT NOT NULL, count INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS posts (
    channel TEXT NOT NULL, id TEXT NOT NULL, seq INTEGER NOT NULL, at TEXT NOT NULL,
    text TEXT, likes INTEGER NOT NULL DEFAULT 0, replies INTEGER NOT NULL DEFAULT 0,
    reposts INTEGER NOT NULL DEFAULT 0, views INTEGER NOT NULL DEFAULT 0,
    measured INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (channel, id)
);
CREATE VIRTUAL TABLE IF NOT EXISTS recall USING fts5(
    text, kind UNINDEXED, seq UNINDEXED, at UNINDEXED, place UNINDEXED, tokenize = 'trigram'
);
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::{Actor, Place};

    fn heard(
        place: &str,
        kind: PlaceKind,
        from: &str,
        id: &str,
        text: &str,
        addressed: bool,
    ) -> Event {
        Event::Heard {
            channel: "t".into(),
            place: Place {
                id: place.into(),
                kind,
                name: None,
            },
            from: Actor {
                id: from.into(),
                name: from.to_uppercase(),
                handle: None,
                automated: false,
            },
            id: id.into(),
            text: text.into(),
            reply_to: None,
            addressed,
        }
    }

    fn said(place: &str, text: &str) -> Event {
        Event::Said {
            channel: "t".into(),
            place: place.into(),
            id: None,
            text: text.into(),
            reply_to: None,
        }
    }

    fn fold(events: Vec<Event>) -> (Journal, Memory) {
        let mut j = Journal::in_memory();
        let mut m = Memory::in_memory().unwrap();
        for e in events {
            let entry = j.append(e).unwrap();
            m.apply(&entry).unwrap();
        }
        (j, m)
    }

    #[test]
    fn unread_follows_read_markers() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            heard("a", PlaceKind::Direct, "a", "2", "there?", true),
            Event::Read {
                channel: "t".into(),
                place: "a".into(),
                upto: 2,
            },
            said("a", "yes"),
            heard("a", PlaceKind::Direct, "a", "3", "good", true),
        ]);
        let p = m.place("t:a").unwrap().unwrap();
        assert_eq!((p.unread, p.read_upto, p.last_out_seq), (1, 2, 4));
        assert_eq!(m.places_unread().unwrap().len(), 1);
        assert!(m.knows_message("t:a", "2").unwrap());
        assert!(!m.knows_message("t:a", "9").unwrap());
        let thread = m.thread("t:a", 10).unwrap();
        assert_eq!(thread.len(), 4);
        assert!(!thread[2].incoming);
    }

    #[test]
    fn people_accumulate() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            heard("g", PlaceKind::Group, "a", "2", "hello group", false),
            heard("g", PlaceKind::Group, "b", "3", "hey", false),
            Event::Felt {
                toward: "t:a".into(),
                delta: 2,
                why: "kind".into(),
            },
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "designs buildings".into(),
                place: Some("t:a".into()),
            },
            said("a", "hello"),
        ]);
        let a = m.person("t:a").unwrap().unwrap();
        assert_eq!((a.heard, a.warmth, a.active_days), (2, 2, 1));
        assert_eq!(a.direct.as_deref(), Some("t:a"));
        assert_eq!(a.unanswered_days, 1);
        assert_eq!(m.person_of_direct("t:a").unwrap().unwrap().key, "t:a");
        assert_eq!(m.people_in("t:g", 5).unwrap().len(), 2);
        assert_eq!(m.notes("t:a", 5).unwrap()[0].text, "designs buildings");
        let p = m.place("t:g").unwrap().unwrap();
        assert_eq!((p.unread, p.unread_addressed), (2, 0));
    }

    #[test]
    fn intentions_open_and_settle() {
        let due = Utc::now();
        let (_, m) = fold(vec![
            Event::Intended {
                due,
                what: "ask about the interview".into(),
                about: Some("t:a".into()),
                place: None,
                owed: false,
            },
            Event::Intended {
                due,
                what: "second".into(),
                about: None,
                place: None,
                owed: false,
            },
            Event::Settled {
                intention: 1,
                outcome: "asked".into(),
            },
        ]);
        let open = m.open_intentions().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].what, "second");
    }

    #[test]
    fn search_finds_latin_and_short_chinese() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "my interview is on friday",
                true,
            ),
            said("a", "东边那家茶馆要拆了"),
            Event::Canon {
                fact: "她养了一只叫豆包的猫".into(),
                source: "life".into(),
            },
        ]);
        assert_eq!(m.search("t", "interview", 5).unwrap().len(), 1);
        // Two characters: below the trigram minimum, found by scan.
        let tea = m.search("t", "茶馆", 5).unwrap();
        assert_eq!(tea.len(), 1);
        assert_eq!(tea[0].kind, "said");
        assert_eq!(m.search("t", "豆包 interview", 5).unwrap().len(), 2);
        assert!(m.search("t", "nothing here", 5).unwrap().is_empty());
        assert!(m.search("t", "100%_", 5).unwrap().is_empty());
    }

    #[test]
    fn catch_up_rebuilds_from_the_journal() {
        let (j, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            said("a", "hey"),
        ]);
        let mut fresh = Memory::in_memory().unwrap();
        assert_eq!(fresh.catch_up(&j).unwrap(), 2);
        assert_eq!(fresh.last_seq().unwrap(), m.last_seq().unwrap());
        assert_eq!(fresh.thread("t:a", 10).unwrap().len(), 2);
        assert_eq!(fresh.catch_up(&j).unwrap(), 0);
    }

    #[test]
    fn what_was_learned_keeps_where_it_was_learned() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "my sister is moving to Kunming",
                true,
            ),
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "sister is moving to Kunming".into(),
                place: Some("t:a".into()),
            },
        ]);
        assert_eq!(m.notes("t:a", 5).unwrap()[0].place.as_deref(), Some("t:a"));
        let hits = m.search("t", "Kunming", 5).unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|h| h.place.as_deref() == Some("t:a")));
    }

    #[test]
    fn an_undertaking_is_followed_from_start_to_end() {
        let (_, m) = fold(vec![
            Event::Undertook {
                id: "u1".into(),
                brief: "find out when the old tea house closes".into(),
                asked_by: Some("t:a".into()),
                place: Some("t:a".into()),
            },
            Event::Worked {
                id: "u1".into(),
                summary: "two sources so far".into(),
                steps: 9,
            },
        ]);
        let open = m.undertakings(true, 5).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(
            (open[0].sittings, open[0].summary.as_deref()),
            (1, Some("two sources so far"))
        );

        let (_, m) = fold(vec![
            Event::Undertook {
                id: "u1".into(),
                brief: "b".into(),
                asked_by: None,
                place: None,
            },
            Event::Concluded {
                id: "u1".into(),
                outcome: "finished".into(),
                summary: "it closes at nine".into(),
            },
        ]);
        assert!(m.undertakings(true, 5).unwrap().is_empty());
        let done = m.undertaking("u1").unwrap().unwrap();
        assert_eq!(done.outcome.as_deref(), Some("finished"));
        assert_eq!(m.search("t", "closes at nine", 5).unwrap().len(), 1);
    }

    #[test]
    fn a_lesson_replaces_the_one_she_changed_her_mind_about() {
        let (_, m) = fold(vec![
            Event::Learned {
                lesson: "long threads do well".into(),
                replaces: None,
            },
            Event::Learned {
                lesson: "short posts about one street do better than threads".into(),
                replaces: Some(1),
            },
        ]);
        let held = m.craft(10).unwrap();
        assert_eq!(held.len(), 1);
        assert!(held[0].lesson.starts_with("short posts"));
    }

    #[test]
    fn posts_carry_their_latest_counts() {
        let (_, m) = fold(vec![
            Event::Posted {
                channel: "x".into(),
                id: Some("p1".into()),
                text: "the door on Kuixinglou street".into(),
            },
            Event::Measured {
                channel: "x".into(),
                followers: Some(40),
                posts: vec![crate::journal::PostStats {
                    id: "p1".into(),
                    likes: 3,
                    ..Default::default()
                }],
            },
            Event::Measured {
                channel: "x".into(),
                followers: Some(52),
                posts: vec![crate::journal::PostStats {
                    id: "p1".into(),
                    likes: 11,
                    replies: 2,
                    views: 900,
                    ..Default::default()
                }],
            },
        ]);
        let posts = m.standings("x", 10).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(
            (posts[0].likes, posts[0].replies, posts[0].views),
            (11, 2, 900)
        );
        assert_eq!(
            posts[0].text.as_deref(),
            Some("the door on Kuixinglou street")
        );
        let counts: Vec<i64> = m
            .followers("x", 10)
            .unwrap()
            .into_iter()
            .map(|c| c.1)
            .collect();
        assert_eq!(counts, [40, 52]);
    }

    #[test]
    fn elsewhere_is_everywhere_but_here() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "are you around", true),
            heard(
                "g",
                PlaceKind::Group,
                "b",
                "2",
                "anyone seen the new line 30",
                false,
            ),
            said("g", "not yet"),
        ]);
        let since = Utc::now() - chrono::Duration::hours(1);
        let other = m.elsewhere(since, "t:a", 5).unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].place.key, "t:g");
        assert_eq!((other[0].heard, other[0].said), (1, 1));
        assert_eq!(other[0].last.text, "not yet");
    }

    #[test]
    fn a_conversation_sees_its_own_platform_and_the_whole_memory() {
        let (_, m) = fold(vec![
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "the lantern festival is friday",
                true,
            ),
            on(
                "qq",
                heard(
                    "b",
                    PlaceKind::Direct,
                    "b",
                    "2",
                    "the lantern shop closed",
                    true,
                ),
            ),
            Event::Noted {
                sources: None,
                about: "qq:b".into(),
                text: "runs the lantern stall".into(),
                place: Some("qq:b".into()),
            },
        ]);
        let texts = |hits: Vec<Hit>| hits.into_iter().map(|h| h.text).collect::<Vec<_>>();
        // From Telegram: Telegram's messages, and every memory.
        let from_t = texts(m.search("t", "lantern", 10).unwrap());
        assert!(from_t.iter().any(|t| t.contains("festival is friday")));
        assert!(from_t.iter().any(|t| t == "runs the lantern stall"));
        assert!(!from_t.iter().any(|t| t.contains("shop closed")));
        // From QQ: the other way round for the messages, the same memory.
        let from_qq = texts(m.search("qq", "lantern", 10).unwrap());
        assert!(from_qq.iter().any(|t| t.contains("shop closed")));
        assert!(from_qq.iter().any(|t| t == "runs the lantern stall"));
        assert!(!from_qq.iter().any(|t| t.contains("festival is friday")));
        // What else is going on is what is going on on this platform.
        let since = Utc::now() - chrono::Duration::hours(1);
        assert!(
            m.elsewhere(since, "t:x", 5)
                .unwrap()
                .iter()
                .all(|e| e.place.channel == "t")
        );
        assert_eq!(m.elsewhere(since, "qq:x", 5).unwrap().len(), 1);
    }

    #[test]
    fn what_she_is_after_and_what_work_taught_her_are_held_until_replaced() {
        let (journal, m) = fold(vec![
            Event::Pursued {
                what: "draw every tea house in the city".into(),
                replaces: None,
            },
            Event::Practised {
                lesson: "check the opening hours on the day, pages go stale".into(),
                replaces: None,
            },
        ]);
        let first = m.pursuits(5).unwrap()[0].seq;
        assert_eq!(m.practice(5).unwrap().len(), 1);
        drop((journal, m));
        let (_, m) = fold(vec![
            Event::Pursued {
                what: "draw every tea house in the city".into(),
                replaces: None,
            },
            Event::Pursued {
                what: "draw the ones that are closing first".into(),
                replaces: Some(first),
            },
            Event::Pursued {
                what: "learn to bind a book".into(),
                replaces: None,
            },
        ]);
        let held: Vec<String> = m
            .pursuits(5)
            .unwrap()
            .into_iter()
            .map(|p| p.lesson)
            .collect();
        assert_eq!(
            held,
            [
                "draw the ones that are closing first",
                "learn to bind a book"
            ]
        );
        // Letting one go: nothing in its place.
        let last = m.pursuits(5).unwrap()[1].seq;
        let (_, m) = fold(vec![
            Event::Pursued {
                what: "a".into(),
                replaces: None,
            },
            Event::Pursued {
                what: String::new(),
                replaces: Some(1),
            },
        ]);
        let _ = last;
        assert!(m.pursuits(5).unwrap().is_empty());
    }

    fn on(channel: &str, event: Event) -> Event {
        match event {
            Event::Heard {
                place,
                from,
                id,
                text,
                reply_to,
                addressed,
                ..
            } => Event::Heard {
                channel: channel.into(),
                place,
                from,
                id,
                text,
                reply_to,
                addressed,
            },
            other => other,
        }
    }

    #[test]
    fn each_platform_is_kept_in_a_database_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::in_memory();
        let mut m = Memory::open(dir.path()).unwrap();
        for event in [
            on(
                "telegram",
                heard(
                    "a",
                    PlaceKind::Direct,
                    "a",
                    "1",
                    "a secret on telegram",
                    true,
                ),
            ),
            on(
                "qq",
                heard("b", PlaceKind::Group, "b", "2", "chatter on qq", false),
            ),
            Event::Canon {
                fact: "lives near the old gate".into(),
                source: "life".into(),
            },
            Event::Noted {
                sources: None,
                about: "telegram:a".into(),
                text: "is moving in spring".into(),
                place: Some("telegram:a".into()),
            },
        ] {
            m.apply(&j.append(event).unwrap()).unwrap();
        }
        let root = dir.path().join(MEMORY_DIR);
        let mut files: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| {
                std::path::Path::new(n)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("db"))
            })
            .collect();
        files.sort();
        assert_eq!(files, ["global.db", "qq.db", "telegram.db"]);

        // What was said on one platform is in that platform's file and no other.
        let holds = |file: &str, needle: &str| {
            let conn = Connection::open(root.join(file)).unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM recall WHERE text LIKE ?1",
                [format!("%{needle}%")],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
                > 0
        };
        // The messages are the platform's record, in its file and no other.
        assert!(holds("telegram.db", "a secret on telegram"));
        assert!(!holds("qq.db", "a secret on telegram"));
        assert!(!holds("global.db", "a secret on telegram"));
        assert!(holds("qq.db", "chatter on qq"));
        // What she remembers is in her memory, whole, and says where it was
        // formed.
        assert!(holds("global.db", "is moving in spring"));
        assert!(!holds("telegram.db", "is moving in spring"));
        assert!(holds("global.db", "lives near the old gate"));
        assert_eq!(
            m.notes("telegram:a", 5).unwrap()[0].place.as_deref(),
            Some("telegram:a")
        );

        // Asking about a place or a person reads its own platform's.
        assert_eq!(m.thread("telegram:a", 5).unwrap().len(), 1);
        assert_eq!(m.notes("telegram:a", 5).unwrap().len(), 1);
        assert!(m.thread("qq:a", 5).unwrap().is_empty());
        assert!(m.place("nowhere:a").unwrap().is_none());

        // Opened again, it is all still there, and nothing is folded twice.
        drop(m);
        let mut m = Memory::open(dir.path()).unwrap();
        assert_eq!(m.catch_up(&j).unwrap(), 0);
        assert_eq!(m.places().unwrap().len(), 2);
        assert_eq!(m.canon(5).unwrap().len(), 1);
    }

    #[test]
    fn an_entry_folded_twice_is_kept_once() {
        // A platform's database has it, the global one has not recorded it:
        // what an interrupted fold leaves behind. Folding it again changes
        // nothing.
        let mut j = Journal::in_memory();
        let mut m = Memory::in_memory().unwrap();
        let entry = j
            .append(heard("a", PlaceKind::Direct, "a", "1", "hi", true))
            .unwrap();
        m.channel_mut("t").unwrap().fold(&entry).unwrap();
        assert_eq!(m.last_seq().unwrap(), 0);
        m.catch_up(&j).unwrap();
        assert_eq!(m.thread("t:a", 5).unwrap().len(), 1);
        assert_eq!(m.last_seq().unwrap(), 1);
    }

    #[test]
    fn work_is_remembered_whole_and_says_where_it_was_asked_for() {
        let (_, m) = fold(vec![
            Event::Undertook {
                id: "u1".into(),
                brief: "find the hours".into(),
                asked_by: Some("t:a".into()),
                place: Some("t:a".into()),
            },
            Event::Undertook {
                id: "u2".into(),
                brief: "draw the gate".into(),
                asked_by: None,
                place: None,
            },
            Event::Concluded {
                id: "u1".into(),
                outcome: "finished".into(),
                summary: "nine on weekdays".into(),
            },
        ]);
        assert_eq!(
            m.undertaking("u1").unwrap().unwrap().outcome.as_deref(),
            Some("finished")
        );
        assert!(m.undertaking("u2").unwrap().unwrap().outcome.is_none());
        assert_eq!(m.undertakings(false, 10).unwrap().len(), 2);
        assert!(m.global.undertaking("u1").unwrap().is_some());
        assert_eq!(
            m.undertaking("u1").unwrap().unwrap().place.as_deref(),
            Some("t:a")
        );
    }

    #[test]
    fn a_platform_name_names_one_file_and_comes_back_from_it() {
        for name in ["qq", "x", "telegram", "Telegram", "global", "a/b", "群", ""] {
            let file = file_of(name);
            assert!(!file.contains('/'), "{file}");
            assert_eq!(channel_of_file(&file).as_deref(), Some(name), "{file}");
        }
        assert_eq!(file_of("qq"), "qq.db");
        assert_eq!(channel_of_file("global.db"), None);
    }

    #[test]
    fn the_store_from_before_platforms_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("derived")).unwrap();
        std::fs::write(dir.path().join(LEGACY_FILE), b"old").unwrap();
        Memory::open(dir.path()).unwrap();
        assert!(!dir.path().join(LEGACY_FILE).exists());
        assert!(dir.path().join(MEMORY_DIR).join("global.db").exists());
    }

    #[test]
    fn a_digest_replaces_the_one_before() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "one", true),
            heard("a", PlaceKind::Direct, "a", "2", "two", true),
            heard("a", PlaceKind::Direct, "a", "3", "three", true),
            Event::Digested {
                sources: None,
                place: "t:a".into(),
                upto: 1,
                text: "said one".into(),
            },
            Event::Digested {
                sources: None,
                place: "t:a".into(),
                upto: 2,
                text: "said one, then two".into(),
            },
        ]);
        let digest = m.digest("t:a").unwrap().unwrap();
        assert_eq!(
            (digest.upto, digest.text.as_str()),
            (2, "said one, then two")
        );
        assert!(m.digest("t:b").unwrap().is_none());
        assert_eq!(m.lines_after("t:a", digest.upto).unwrap(), 1);
        let between = m.thread_between("t:a", 0, 2, 10).unwrap();
        assert_eq!(between.len(), 2);
        assert_eq!(between[1].text, "two");
    }

    #[test]
    fn a_profile_replaces_the_one_before_and_dates_the_notes() {
        let (_, m) = fold(vec![
            heard("a", PlaceKind::Direct, "a", "1", "hi", true),
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "designs buildings".into(),
                place: None,
            },
            Event::Profiled {
                sources: None,
                person: "t:a".into(),
                shared: "An architect.".into(),
                private: String::new(),
            },
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "is leaving the institute".into(),
                place: Some("t:a".into()),
            },
            Event::Profiled {
                sources: None,
                person: "t:a".into(),
                shared: "An architect, careful with words.".into(),
                private: "Leaving the institute; has told nobody.".into(),
            },
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "likes the plane trees".into(),
                place: None,
            },
        ]);
        let profile = m.profile("t:a").unwrap().unwrap();
        assert_eq!(profile.shared, "An architect, careful with words.");
        assert!(profile.private.starts_with("Leaving"));
        let since: Vec<String> = m
            .notes_after("t:a", profile.seq, 10)
            .unwrap()
            .into_iter()
            .map(|n| n.text)
            .collect();
        assert_eq!(since, ["likes the plane trees"]);
        assert!(m.profile("t:nobody").unwrap().is_none());
    }

    #[test]
    fn what_a_memory_was_made_from_is_kept_with_it() {
        let then = Utc::now() - chrono::Duration::days(3);
        let sources = Sources {
            from: 2,
            upto: 9,
            since: then,
            until: then + chrono::Duration::hours(2),
        };
        let (_, m) = fold(vec![
            Event::Noted {
                about: "t:a".into(),
                text: "is leaving the institute".into(),
                place: Some("t:a".into()),
                sources: Some(sources),
            },
            Event::Profiled {
                person: "t:a".into(),
                shared: "An architect.".into(),
                private: String::new(),
                sources: Some(sources),
            },
            Event::Digested {
                place: "t:a".into(),
                upto: 9,
                text: "Talk of the institute.".into(),
                sources: Some(sources),
            },
            Event::Noted {
                about: "t:a".into(),
                text: "likes the plane trees".into(),
                place: None,
                sources: None,
            },
        ]);
        let kept = |s: Option<Sources>| s.map(|s| (s.from, s.upto, s.since.timestamp()));
        let want = Some((2, 9, then.timestamp()));
        let notes = m.notes("t:a", 10).unwrap();
        assert_eq!(kept(notes[0].sources), want);
        assert_eq!(
            notes[1].sources, None,
            "a note from before sources were kept"
        );
        assert_eq!(kept(m.profile("t:a").unwrap().unwrap().sources), want);
        assert_eq!(kept(m.digest("t:a").unwrap().unwrap().sources), want);
    }

    #[test]
    fn a_proof_is_waited_for_until_it_is_met() {
        let challenged = Event::Challenged {
            claimant: "d:wen".into(),
            claimed: "t:wen".into(),
            word: "heron-42".into(),
        };
        let (_, m) = fold(vec![challenged.clone()]);
        let since = Utc::now() - chrono::Duration::hours(1);
        assert_eq!(m.challenges_to("t:wen", since).unwrap()[0].word, "heron-42");
        assert!(m.challenges_to("d:wen", since).unwrap().is_empty());
        assert!(m.linked("t:wen").unwrap().is_empty());

        let (_, m) = fold(vec![
            challenged,
            Event::Linked {
                one: "d:wen".into(),
                other: "t:wen".into(),
            },
            Event::Linked {
                one: "t:wen".into(),
                other: "x:wen".into(),
            },
        ]);
        assert!(m.challenges_to("t:wen", since).unwrap().is_empty(), "met");
        let mut same = m.linked("d:wen").unwrap();
        same.sort();
        assert_eq!(same, ["t:wen", "x:wen"], "links carry across");
    }
}
