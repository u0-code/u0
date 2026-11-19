//! What she can recall, derived from the journal.
//!
//! Everything in here is a view. It can be deleted at any time and the
//! journal will rebuild it: places and what is unread in them, the people she
//! knows, notes, intentions, the canon of her own life, the sessions a console
//! lists, and a full-text index for remembering things that are no longer in
//! front of her.
//!
//! It is one database, `derived/memory.db`, and each thing she can recall is
//! kept by a file of its own here: its rows, its tables, how journal events
//! change them, and how they are read.
//!
//! What was said and done on a platform, and the accounts there, are that
//! platform's record: those rows carry the platform in a `channel` column.
//! Everything she makes of it, and everything of her own, is her memory,
//! whole: it carries no platform, only where it was formed (a `channel:id`
//! key on the note, the profile, the digest, the intention), and is found
//! from anywhere. A query about one place's messages reads only that place's;
//! the few that span every platform say so in their names and docs.

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    Error, Result,
    journal::{Entry, Journal, Sources, parse_stamp, stamp, time_at},
    social,
};

mod belongings;
mod canon;
mod craft;
mod digests;
mod installed;
mod intentions;
mod messages;
mod notes;
mod people;
mod places;
mod public;
mod recall;
mod sessions;
mod skills;
mod threads;
mod work;

pub use belongings::Belonging;
pub use canon::Fact;
pub use craft::Lesson;
pub use digests::Digest;
pub use installed::Installed;
pub use intentions::Intention;
pub use messages::{Aside, Line, Share, Unprompted};
pub use notes::{Note, Profile};
pub use people::{Feeling, Person};
pub use places::{Elsewhere, PlaceState, Talked};
pub use public::Standing;
pub use recall::Hit;
pub use sessions::Session;
pub use skills::Skill;
pub use work::{Undertaking, WorkReport};

/// Where the derived memory lives inside a ghost directory.
pub const MEMORY_FILE: &str = "derived/memory.db";

/// How the store says which version of each feature it holds. Bumped only
/// if that changes: the whole store is then thrown away and rebuilt. A
/// change to one feature bumps that feature's `VERSION` instead.
const LAYOUT: i64 = 26;

/// How far the journal has been folded in (`last_seq`), and the version of
/// each feature the store holds (`version:<name>`).
const META: &str =
    "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);";

/// One thing she can recall, kept by a file of its own here.
///
/// When a feature's `VERSION` is not the one the store holds, its tables
/// are dropped on open and it is folded again from the whole journal,
/// while every other feature keeps what it has. A feature that reads
/// another's tables as it folds is rebuilt with it, whichever of the two
/// changed: alone, it would read the other's tables as they are now
/// rather than as they were at each entry, or leave what it once read
/// from them as an older version wrote it.
struct Feature {
    name: &'static str,
    version: i64,
    schema: &'static str,
    /// The features whose tables its fold reads.
    reads: &'static [&'static str],
    fold: fn(&Transaction<'_>, &Entry, &str) -> Result<()>,
}

macro_rules! feature {
    ($name:ident $(, reads $($read:ident),+)?) => {
        Feature {
            name: stringify!($name),
            version: $name::VERSION,
            schema: $name::SCHEMA,
            // Each by its file, so that a name that is not one does not
            // build.
            reads: &[$($({
                let _ = $read::VERSION;
                stringify!($read)
            }),+)?],
            fold: $name::fold,
        }
    };
}

/// Every feature, in the order each entry is folded into them.
const FEATURES: [Feature; 16] = [
    // First, so that it reads a line, and what was said in a place, as they
    // were before this entry changes them.
    feature!(recall, reads messages, people, work, belongings),
    feature!(places),
    feature!(messages),
    feature!(people),
    // Whether a note was learned in private is read from its place.
    feature!(notes, reads places),
    feature!(digests),
    feature!(intentions),
    feature!(canon),
    feature!(work),
    feature!(craft),
    feature!(public),
    feature!(belongings),
    feature!(installed),
    feature!(skills),
    feature!(sessions),
    // After the people, so that a note about someone finds their place.
    feature!(threads, reads people),
];

/// Everything she can recall, in one database.
pub struct Memory {
    conn: Connection,
}

impl Memory {
    /// Open the derived store under `dir`, creating it, or rebuilding from
    /// `journal` what in it is not of this build's shape.
    pub fn open(dir: &Path, journal: &Journal) -> Result<Self> {
        let path = dir.join(MEMORY_FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let layout: i64 =
            Connection::open(&path)?.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if layout != LAYOUT && layout != 0 {
            // Kept in another way. It is only a view: throw all of it away.
            discard(dir)?;
        }
        let mut memory = Self::prepare(Connection::open(&path)?)?;
        memory.refold(journal)?;
        Ok(memory)
    }

    /// The store under `dir` as it was last written, for looking at from
    /// another process while she lives. It is opened read-only, so that
    /// nothing here can rebuild or change it under the body that keeps it.
    /// `None` if there is none yet, or any of it is of a shape this build
    /// does not read, or is still being rebuilt.
    pub fn glance(dir: &Path) -> Result<Option<Self>> {
        let path = dir.join(MEMORY_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let layout: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let memory = Self { conn };
        Ok((layout == LAYOUT && memory.stale()?.is_empty()).then_some(memory))
    }

    /// A store that lives only as long as the value. For tests and tools.
    pub fn in_memory() -> Result<Self> {
        let mut memory = Self::prepare(Connection::open_in_memory()?)?;
        // Nothing has been folded into it, so every feature is whole.
        let tx = memory.conn.transaction()?;
        held(&tx, &FEATURES.iter().collect::<Vec<_>>())?;
        tx.commit()?;
        Ok(memory)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        // It can be rebuilt from her journal, so it does not need to survive
        // a power cut whole: what NORMAL may lose is the last few folds, and
        // `catch_up` folds them again. WAL lets a reader in another process
        // look while she writes, instead of holding her writes up.
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(META)?;
        let memory = Self { conn };
        // What is of another version is dropped, and made again empty;
        // `refold` fills it.
        for feature in memory.stale()? {
            for table in tables(feature)? {
                memory
                    .conn
                    .execute_batch(&format!("DROP TABLE IF EXISTS \"{table}\";"))?;
            }
        }
        for feature in &FEATURES {
            memory.conn.execute_batch(feature.schema)?;
        }
        memory.conn.pragma_update(None, "user_version", LAYOUT)?;
        Ok(memory)
    }

    /// The features this store does not hold as this build keeps them,
    /// with every feature they read or that reads them, in fold order.
    fn stale(&self) -> Result<Vec<&'static Feature>> {
        let mut stale: Vec<&str> = Vec::new();
        for feature in &FEATURES {
            if self.meta(&format!("version:{}", feature.name))? != feature.version {
                stale.push(feature.name);
            }
        }
        // With whatever reads one of them or is read by one, until no more
        // is added.
        let touch =
            |f: &Feature, g: &Feature| f.reads.contains(&g.name) || g.reads.contains(&f.name);
        loop {
            let more: Vec<&str> = FEATURES
                .iter()
                .filter(|f| {
                    !stale.contains(&f.name)
                        && FEATURES
                            .iter()
                            .any(|g| stale.contains(&g.name) && touch(f, g))
                })
                .map(|f| f.name)
                .collect();
            if more.is_empty() {
                break;
            }
            stale.extend(more);
        }
        Ok(FEATURES
            .iter()
            .filter(|f| stale.contains(&f.name))
            .collect())
    }

    /// Fold the journal, as far as the rest of memory has, into each
    /// feature that is stale, 500 entries to a transaction. Its version
    /// is written with the last of them, so a rebuild cut short is begun
    /// again on the next open rather than taken for whole.
    fn refold(&mut self, journal: &Journal) -> Result<()> {
        let features = self.stale()?;
        if features.is_empty() {
            return Ok(());
        }
        let upto = self.last_seq()?;
        let mut from = 1;
        loop {
            let mut batch = if from > upto {
                Vec::new()
            } else {
                journal.read(from, BATCH)?
            };
            batch.retain(|e| e.seq <= upto);
            let done = batch.last().is_none_or(|e| e.seq >= upto);
            let tx = self.conn.transaction()?;
            for entry in &batch {
                let at = stamp(&entry.at);
                for feature in &features {
                    (feature.fold)(&tx, entry, &at).map_err(|e| unfolded(entry, e.to_string()))?;
                }
            }
            if done {
                held(&tx, &features)?;
            }
            tx.commit()?;
            match batch.last() {
                Some(last) if !done => from = last.seq + 1,
                _ => return Ok(()),
            }
        }
    }

    /// The seq of the last journal entry folded in.
    pub fn last_seq(&self) -> Result<i64> {
        self.meta("last_seq")
    }

    fn meta(&self, key: &str) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }

    /// Fold in every journal entry not yet seen, in order, from the one
    /// after the last folded. Returns how many.
    ///
    /// They are folded 500 to a transaction: a rebuild then commits a few
    /// hundred times, not once an entry.
    pub fn catch_up(&mut self, journal: &Journal) -> Result<u64> {
        let mut applied = 0;
        loop {
            let batch = journal.read(self.last_seq()? + 1, BATCH)?;
            self.fold_in(&batch)?;
            applied += batch.len() as u64;
            if batch.len() < BATCH {
                return Ok(applied);
            }
        }
    }

    /// Fold one journal entry in, unless it already has been. Every feature
    /// folds it in one transaction with the note of how far the journal has
    /// been read, so an entry is kept whole or not at all, and only once.
    ///
    /// Entries are folded in the order of the journal and none is passed
    /// over: one that comes after an entry not yet folded is refused, so a
    /// fold that failed is tried again by [`Memory::catch_up`] rather than
    /// left behind.
    pub fn apply(&mut self, entry: &Entry) -> Result<()> {
        let last = self.last_seq()?;
        if entry.seq <= last {
            return Ok(());
        }
        if entry.seq != last + 1 {
            return Err(unfolded(
                entry,
                format!("it comes after {last}, not yet folded"),
            ));
        }
        self.fold_in(std::slice::from_ref(entry))
    }

    /// Fold `entries`, which follow on from the last folded and from each
    /// other, in one transaction. If any fails, none is kept, and the error
    /// names the one that failed.
    fn fold_in(&mut self, entries: &[Entry]) -> Result<()> {
        let (Some(first), Some(last)) = (entries.first(), entries.last()) else {
            return Ok(());
        };
        let tx = self
            .conn
            .transaction()
            .map_err(|e| unfolded(first, e.to_string()))?;
        for entry in entries {
            let at = stamp(&entry.at);
            for feature in &FEATURES {
                (feature.fold)(&tx, entry, &at).map_err(|e| unfolded(entry, e.to_string()))?;
            }
        }
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('last_seq', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [last.seq],
        )
        .and_then(|_| tx.commit())
        .map_err(|e| unfolded(first, e.to_string()))
    }
}

/// How many entries `catch_up` folds in one transaction.
const BATCH: usize = 500;

/// Memory stopped before `entry`, for `reason`.
fn unfolded(entry: &Entry, reason: String) -> Error {
    Error::Unfolded {
        seq: entry.seq,
        kind: entry.event.kind(),
        reason,
    }
}

/// Throw the store under `dir` away. It is only a view.
fn discard(dir: &Path) -> Result<()> {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let file = dir.join(format!("{MEMORY_FILE}{suffix}"));
        if file.exists() {
            std::fs::remove_file(file)?;
        }
    }
    Ok(())
}

/// Note that the store holds `features` as this build keeps them.
fn held(tx: &Transaction<'_>, features: &[&Feature]) -> Result<()> {
    for feature in features {
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![format!("version:{}", feature.name), feature.version],
        )?;
    }
    Ok(())
}

/// The tables `feature` makes, as `SQLite` lists those its schema creates.
/// Its indexes, and the tables behind a full-text index, go with them.
fn tables(feature: &Feature) -> Result<Vec<String>> {
    let scratch = Connection::open_in_memory()?;
    scratch.execute_batch(feature.schema)?;
    let mut stmt = scratch.prepare(
        "SELECT name FROM pragma_table_list
         WHERE schema = 'main' AND type IN ('table', 'virtual') AND name NOT LIKE 'sqlite_%'",
    )?;
    let names = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(names)
}

/// The platform a `channel:id` key belongs to.
fn channel_of(key: &str) -> Option<&str> {
    social::split_key(key).map(|(channel, _)| channel)
}

fn as_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn opt_time_at(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let text: Option<String> = row.get(idx)?;
    Ok(text.and_then(|t| parse_stamp(&t).ok()))
}

/// Sources as the four columns they are kept in: `src_from`, `src_upto`,
/// `since`, `until`.
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

/// How the tests here build a memory: from events, as the journal would
/// hand them over.
#[cfg(test)]
mod testing {
    use super::Memory;
    use crate::{
        journal::{Event, Journal},
        social::{Actor, Place, PlaceKind},
    };

    pub fn heard(
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
            images: Vec::new(),
        }
    }

    pub fn said(place: &str, text: &str) -> Event {
        Event::Said {
            channel: "t".into(),
            place: place.into(),
            id: None,
            text: text.into(),
            reply_to: None,
            file: None,
        }
    }

    /// The same event, heard on another platform.
    pub fn on(channel: &str, event: Event) -> Event {
        match event {
            Event::Heard {
                place,
                from,
                id,
                text,
                reply_to,
                addressed,
                images,
                ..
            } => Event::Heard {
                channel: channel.into(),
                place,
                from,
                id,
                text,
                reply_to,
                addressed,
                images,
            },
            other => other,
        }
    }

    /// How many instructions SQLite runs for `read`: what a read costs,
    /// counted rather than timed, so that it does not depend on how busy
    /// the machine is.
    pub fn steps<T>(m: &mut Memory, read: impl FnOnce(&Memory) -> T) -> (T, u64) {
        use std::sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        };
        let count = Arc::new(AtomicU64::new(0));
        let counter = count.clone();
        m.conn
            .progress_handler(
                1,
                Some(move || {
                    counter.fetch_add(1, Ordering::Relaxed);
                    false
                }),
            )
            .unwrap();
        let out = read(m);
        m.conn.progress_handler(0, None::<fn() -> bool>).unwrap();
        (out, count.load(Ordering::Relaxed))
    }

    pub fn fold(events: Vec<Event>) -> (Journal, Memory) {
        let mut j = Journal::in_memory();
        let mut m = Memory::in_memory().unwrap();
        for e in events {
            let entry = j.append(e).unwrap();
            m.apply(&entry).unwrap();
        }
        (j, m)
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{
        FEATURES, LAYOUT, MEMORY_FILE, Memory, tables,
        testing::{fold, heard, on, said},
    };
    use crate::{
        journal::{Event, Journal},
        social::PlaceKind,
    };

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
    fn an_entry_folded_twice_is_kept_once() {
        let mut j = Journal::in_memory();
        let mut m = Memory::in_memory().unwrap();
        let entry = j
            .append(heard("a", PlaceKind::Direct, "a", "1", "hi", true))
            .unwrap();
        m.apply(&entry).unwrap();
        m.apply(&entry).unwrap();
        m.catch_up(&j).unwrap();
        assert_eq!(m.thread("t:a", 5).unwrap().len(), 1);
        assert_eq!(m.person("t:a").unwrap().unwrap().heard, 1);
        assert_eq!(m.search("t", "hi", 5).unwrap().len(), 1);
        assert_eq!(m.last_seq().unwrap(), 1);
    }

    #[test]
    fn an_entry_is_not_folded_past_one_that_was_not() {
        let mut j = Journal::in_memory();
        let mut m = Memory::in_memory().unwrap();
        let first = j
            .append(heard("a", PlaceKind::Direct, "a", "1", "hi", true))
            .unwrap();
        let second = j.append(said("a", "hey")).unwrap();
        assert!(matches!(
            m.apply(&second),
            Err(crate::Error::Unfolded {
                seq: 2,
                kind: "said",
                ..
            })
        ));
        assert_eq!(m.last_seq().unwrap(), 0);
        assert_eq!(m.catch_up(&j).unwrap(), 2);
        m.apply(&first).unwrap();
        assert_eq!(m.thread("t:a", 10).unwrap().len(), 2);
    }

    #[test]
    fn a_reader_in_another_process_does_not_hold_up_her_writes() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::in_memory();
        let mut m = Memory::open(dir.path(), &j).unwrap();
        m.apply(&j.append(said("a", "one")).unwrap()).unwrap();

        // Looking, in the middle of a read, the way `ghostd status` does.
        let look = Memory::glance(dir.path()).unwrap().unwrap();
        look.conn
            .execute_batch("BEGIN; SELECT count(*) FROM messages;")
            .unwrap();
        let begun = std::time::Instant::now();
        m.apply(&j.append(said("a", "two")).unwrap()).unwrap();
        // Behind a rollback journal this waited out the busy timeout, five
        // seconds, and failed.
        assert!(begun.elapsed() < std::time::Duration::from_secs(1));
        // The reader goes on seeing what was there when it began.
        assert_eq!(look.last_seq().unwrap(), 1);
        look.conn.execute_batch("COMMIT").unwrap();
        assert_eq!(look.last_seq().unwrap(), 2);
    }

    #[test]
    fn her_memory_can_be_looked_at_when_nobody_has_it_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::in_memory();
        let mut m = Memory::open(dir.path(), &j).unwrap();
        m.apply(&j.append(said("a", "one")).unwrap()).unwrap();
        drop(m);
        let look = Memory::glance(dir.path()).unwrap().unwrap();
        assert_eq!(look.last_seq().unwrap(), 1);
        assert_eq!(look.thread("t:a", 5).unwrap().len(), 1);
    }

    #[test]
    fn a_batch_that_fails_keeps_none_of_it_and_names_the_entry() {
        let mut j = Journal::in_memory();
        let mut m = Memory::in_memory().unwrap();
        for i in 0..5 {
            j.append(Event::Canon {
                fact: format!("fact {i}"),
                source: "life".into(),
            })
            .unwrap();
        }
        m.conn
            .execute_batch(
                "CREATE TRIGGER refuse BEFORE INSERT ON canon WHEN NEW.fact = 'fact 3'
                 BEGIN SELECT RAISE(ABORT, 'refused'); END;",
            )
            .unwrap();
        assert!(matches!(
            m.catch_up(&j),
            Err(crate::Error::Unfolded {
                seq: 4,
                kind: "canon",
                ..
            })
        ));
        assert_eq!(m.last_seq().unwrap(), 0);
        assert!(m.canon(10).unwrap().is_empty());

        m.conn.execute_batch("DROP TRIGGER refuse").unwrap();
        assert_eq!(m.catch_up(&j).unwrap(), 5);
        assert_eq!(m.canon(10).unwrap().len(), 5);
    }

    #[test]
    fn every_platform_and_her_memory_are_kept_in_one_file_that_outlives_her_body() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::in_memory();
        let mut m = Memory::open(dir.path(), &j).unwrap();
        for event in [
            on(
                "telegram",
                heard("a", PlaceKind::Direct, "a", "1", "on telegram", true),
            ),
            on(
                "qq",
                heard("b", PlaceKind::Group, "b", "2", "chatter on qq", false),
            ),
            Event::Canon {
                fact: "lives near the old gate".into(),
                source: "life".into(),
            },
        ] {
            m.apply(&j.append(event).unwrap()).unwrap();
        }
        let derived: Vec<String> = std::fs::read_dir(dir.path().join("derived"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| {
                std::path::Path::new(n)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("db"))
            })
            .collect();
        assert_eq!(derived, ["memory.db"]);

        // Opened again, it is all still there, and nothing is folded twice.
        drop(m);
        let mut m = Memory::open(dir.path(), &j).unwrap();
        assert_eq!(m.catch_up(&j).unwrap(), 0);
        assert_eq!(m.places().unwrap().len(), 2);
        assert_eq!(m.canon(5).unwrap().len(), 1);
    }

    #[test]
    fn a_store_kept_in_another_layout_is_rebuilt_whole() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the store is, in another shape.
        let path = dir.path().join(MEMORY_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE places (key TEXT PRIMARY KEY);")
                .unwrap();
            conn.pragma_update(None, "user_version", 10).unwrap();
        }

        let mut j = Journal::in_memory();
        let mut m = Memory::open(dir.path(), &j).unwrap();
        let version: i64 = Connection::open(&path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, LAYOUT);
        assert_eq!(m.last_seq().unwrap(), 0);
        m.apply(
            &j.append(heard("a", PlaceKind::Direct, "a", "1", "hi", true))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(m.place("t:a").unwrap().unwrap().unread, 1);
    }

    /// A life with something in every feature that reads another's tables,
    /// and in a few that do not.
    fn lived() -> Journal {
        let mut j = Journal::in_memory();
        for event in [
            heard(
                "a",
                PlaceKind::Direct,
                "a",
                "1",
                "the lantern [image]",
                true,
            ),
            heard("g", PlaceKind::Group, "b", "2", "茶馆 at dusk", false),
            said("a", "东边那家茶馆要拆了"),
            Event::Viewed {
                message: 1,
                image: 0,
                what: "a paper lantern".into(),
                file: None,
            },
            Event::Noted {
                sources: None,
                about: "t:a".into(),
                text: "keeps a lantern".into(),
                place: Some("t:a".into()),
            },
            Event::Felt {
                toward: "t:b".into(),
                delta: 2,
                why: "kind".into(),
            },
            Event::Canon {
                fact: "lives near the old gate".into(),
                source: "life".into(),
            },
            Event::Undertook {
                id: "w1".into(),
                brief: "Draw the east gate.".into(),
                asked_by: Some("t:a".into()),
                place: Some("t:a".into()),
                directed: false,
                dir: None,
            },
            Event::Stopped {
                place: "t:a".into(),
                by: "t:a".into(),
                looking: true,
                unsaid: Vec::new(),
                works: vec!["w1".into()],
            },
            heard("g", PlaceKind::Group, "a", "3", "anyone here", false),
        ] {
            j.append(event).unwrap();
        }
        j
    }

    /// Every row of every feature's tables, and what a few searches find
    /// (the full-text index keeps no rows to compare), to set two stores
    /// side by side.
    fn contents(m: &Memory) -> Vec<String> {
        let mut out = Vec::new();
        for feature in &FEATURES {
            for table in tables(feature).unwrap() {
                if table == "recall" {
                    continue;
                }
                let mut stmt = m.conn.prepare(&format!("SELECT * FROM {table}")).unwrap();
                let n = stmt.column_count();
                let mut rows: Vec<String> = stmt
                    .query_map([], |r| {
                        (0..n)
                            .map(|i| r.get::<_, rusqlite::types::Value>(i))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })
                    .unwrap()
                    .map(|row| format!("{table} {:?}", row.unwrap()))
                    .collect();
                rows.sort();
                out.extend(rows);
            }
        }
        for query in ["lantern", "茶馆", "east gate", "you"] {
            out.push(format!("{query}: {:?}", m.search("t", query, 10).unwrap()));
        }
        out
    }

    fn rebuilt_whole(j: &Journal) -> Vec<String> {
        let mut m = Memory::in_memory().unwrap();
        m.catch_up(j).unwrap();
        contents(&m)
    }

    #[test]
    fn a_feature_of_another_version_is_rebuilt_alone_and_ends_as_a_whole_rebuild_would() {
        let dir = tempfile::tempdir().unwrap();
        let j = lived();
        let mut m = Memory::open(dir.path(), &j).unwrap();
        m.catch_up(&j).unwrap();
        // The canon as an older build kept it, and a mark in the places
        // that a rebuild of them would wipe out.
        m.conn
            .execute_batch(
                "UPDATE meta SET value = 0 WHERE key = 'version:canon';
                 DELETE FROM canon;
                 UPDATE places SET name = 'marked' WHERE key = 't:g';",
            )
            .unwrap();
        drop(m);
        // Until it is rebuilt, it is not looked at.
        assert!(Memory::glance(dir.path()).unwrap().is_none());

        let mut m = Memory::open(dir.path(), &j).unwrap();
        assert_eq!(m.canon(5).unwrap().len(), 1);
        assert_eq!(
            m.place("t:g").unwrap().unwrap().name.as_deref(),
            Some("marked")
        );
        assert_eq!(m.catch_up(&j).unwrap(), 0);
        assert!(Memory::glance(dir.path()).unwrap().is_some());

        m.conn
            .execute_batch("UPDATE places SET name = NULL WHERE key = 't:g'")
            .unwrap();
        assert_eq!(contents(&m), rebuilt_whole(&j));
    }

    #[test]
    fn a_feature_is_rebuilt_with_those_it_reads_and_those_that_read_it() {
        let dir = tempfile::tempdir().unwrap();
        let j = lived();
        let mut m = Memory::open(dir.path(), &j).unwrap();
        m.catch_up(&j).unwrap();
        // The people as an older build kept them. What she recalls reads
        // them (who stopped her), and reads the messages; the threads read
        // them too. None of those may keep what was made from the old.
        m.conn
            .execute_batch(
                "UPDATE meta SET value = 0 WHERE key = 'version:people';
                 UPDATE people SET name = 'OLD';
                 DELETE FROM messages;
                 DELETE FROM thread_of;
                 DELETE FROM remembered;
                 UPDATE places SET name = 'marked' WHERE key = 't:g';",
            )
            .unwrap();
        drop(m);

        let m = Memory::open(dir.path(), &j).unwrap();
        // The places neither read the people nor are read by them.
        assert_eq!(
            m.place("t:g").unwrap().unwrap().name.as_deref(),
            Some("marked")
        );
        m.conn
            .execute_batch("UPDATE places SET name = NULL WHERE key = 't:g'")
            .unwrap();
        assert_eq!(contents(&m), rebuilt_whole(&j));
        let stopped = m.search("t", "stopped", 5).unwrap();
        assert!(stopped[0].text.starts_with("A stopped you"), "{stopped:?}");
    }
}
