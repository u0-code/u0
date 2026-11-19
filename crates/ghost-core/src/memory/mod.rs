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
use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::{
    Error, Result,
    journal::{Entry, Journal, Sources, parse_stamp, stamp},
    social,
};

mod belongings;
mod canon;
mod craft;
mod digests;
mod installed;
mod intentions;
mod links;
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
pub use links::Challenge;
pub use messages::{Aside, Line, Share, Unprompted};
pub use notes::{Note, Profile};
pub use people::{Feeling, Person};
pub use places::{Elsewhere, PlaceState, Talked};
pub use public::Standing;
pub use recall::Hit;
pub use sessions::{Session, TITLE_CHARS};
pub use skills::Skill;
pub use work::{Undertaking, WorkReport};

/// Where the derived memory lives inside a ghost directory.
pub const MEMORY_FILE: &str = "derived/memory.db";
/// Where it was kept when each platform had a database of its own. Removed
/// when found.
const OLD_DIR: &str = "derived/memory";

/// Bump when the schema changes. A mismatch rebuilds from the journal.
const SCHEMA_VERSION: i64 = 24;

/// How far the journal has been folded in.
const META: &str =
    "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);";

/// Everything she can recall, in one database.
pub struct Memory {
    conn: Connection,
}

impl Memory {
    /// Open the derived store under `dir`, creating or rebuilding it.
    pub fn open(dir: &Path) -> Result<Self> {
        // From when each platform had a file of its own. It is only a view.
        let old = dir.join(OLD_DIR);
        if old.is_dir() {
            std::fs::remove_dir_all(old)?;
        }
        let path = dir.join(MEMORY_FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let version: i64 =
            Connection::open(&path)?.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION && version != 0 {
            // An older shape. It is only a view: throw all of it away.
            discard(dir)?;
        }
        let memory = Self::prepare(Connection::open(&path)?)?;
        if memory.folded()? == memory.last_seq()? {
            return Ok(memory);
        }
        // Her journal's seqs run 1, 2, 3 without a gap, so a store that
        // folded each entry once has folded exactly `last_seq` of them.
        // Fewer means one may have been passed over: by a build from before
        // entries were folded strictly in order, whose failed fold the next
        // one stepped past, or by such a build opening this store since.
        // Which entries is not known, so all of them are folded again.
        drop(memory);
        discard(dir)?;
        Self::prepare(Connection::open(&path)?)
    }

    /// The store under `dir` as it was last written, for looking at from
    /// another process while she lives. It is opened read-only, so that
    /// nothing here can rebuild or change it under the body that keeps it.
    /// `None` if there is none yet, or it is of a shape this build does
    /// not read.
    pub fn glance(dir: &Path) -> Result<Option<Self>> {
        let path = dir.join(MEMORY_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        Ok((version == SCHEMA_VERSION).then_some(Self { conn }))
    }

    /// A store that lives only as long as the value. For tests and tools.
    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        // It can be rebuilt from her journal, so it does not need to survive
        // a power cut whole: what NORMAL may lose is the last few folds, and
        // `catch_up` folds them again. WAL lets a reader in another process
        // look while she writes, instead of holding her writes up.
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        for tables in [
            META,
            places::SCHEMA,
            messages::SCHEMA,
            people::SCHEMA,
            notes::SCHEMA,
            digests::SCHEMA,
            intentions::SCHEMA,
            canon::SCHEMA,
            work::SCHEMA,
            craft::SCHEMA,
            public::SCHEMA,
            links::SCHEMA,
            belongings::SCHEMA,
            installed::SCHEMA,
            skills::SCHEMA,
            threads::SCHEMA,
            sessions::SCHEMA,
            recall::SCHEMA,
        ] {
            conn.execute_batch(tables)?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// The seq of the last journal entry folded in.
    pub fn last_seq(&self) -> Result<i64> {
        self.meta("last_seq")
    }

    /// How many journal entries have been folded in.
    fn folded(&self) -> Result<i64> {
        self.meta("folded")
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
            fold(&tx, entry, &stamp(&entry.at)).map_err(|e| unfolded(entry, e.to_string()))?;
        }
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('last_seq', ?1), ('folded', ?2)
             ON CONFLICT(key) DO UPDATE SET value =
                 CASE key WHEN 'last_seq' THEN excluded.value ELSE value + excluded.value END",
            [last.seq, as_i64(entries.len())],
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

/// Every feature's fold of one entry. `at` is the entry's time as it is kept.
fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    places::fold(tx, entry, at)?;
    messages::fold(tx, entry, at)?;
    people::fold(tx, entry, at)?;
    notes::fold(tx, entry, at)?;
    digests::fold(tx, entry, at)?;
    intentions::fold(tx, entry, at)?;
    canon::fold(tx, entry, at)?;
    work::fold(tx, entry, at)?;
    craft::fold(tx, entry, at)?;
    public::fold(tx, entry, at)?;
    links::fold(tx, entry, at)?;
    belongings::fold(tx, entry, at)?;
    installed::fold(tx, entry, at)?;
    skills::fold(tx, entry, at)?;
    sessions::fold(tx, entry, at)?;
    // After the people, so that a note about someone finds their place.
    threads::fold(tx, entry, at)?;
    recall::fold(tx, entry, at)
}

/// The platform a `channel:id` key belongs to.
fn channel_of(key: &str) -> Option<&str> {
    social::split_key(key).map(|(channel, _)| channel)
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
        MEMORY_FILE, Memory, OLD_DIR, SCHEMA_VERSION,
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
        let mut m = Memory::open(dir.path()).unwrap();
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
        let mut m = Memory::open(dir.path()).unwrap();
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
        let mut m = Memory::open(dir.path()).unwrap();
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
        let mut m = Memory::open(dir.path()).unwrap();
        assert_eq!(m.catch_up(&j).unwrap(), 0);
        assert_eq!(m.places().unwrap().len(), 2);
        assert_eq!(m.canon(5).unwrap().len(), 1);
    }

    #[test]
    fn the_store_from_before_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        // A database per platform, as they were kept.
        let old = dir.path().join(OLD_DIR);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("global.db"), b"old").unwrap();
        std::fs::write(old.join("qq.db"), b"old").unwrap();
        // And a file where the store is now, in an older shape.
        let path = dir.path().join(MEMORY_FILE);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE places (key TEXT PRIMARY KEY);")
                .unwrap();
            conn.pragma_update(None, "user_version", 10).unwrap();
        }

        let mut j = Journal::in_memory();
        let mut m = Memory::open(dir.path()).unwrap();
        assert!(!old.exists());
        let version: i64 = Connection::open(&path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(m.last_seq().unwrap(), 0);
        m.apply(
            &j.append(heard("a", PlaceKind::Direct, "a", "1", "hi", true))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(m.place("t:a").unwrap().unwrap().unread, 1);
    }
}
