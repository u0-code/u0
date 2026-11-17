//! The desk's copy of a journal: what it has seen of her, by seq.
//!
//! Everything the desk shows of a ghost is read from this copy, so what
//! it holds decides what a session can show. Every source adds to it and
//! nothing replaces it: the tail read at the first connection, the stream
//! after it, what was written while the daemon was out of reach, and the
//! entries of a place read when its thread is first shown. An entry is
//! held once, whichever way it came.
//!
//! Two kinds of entry are held. The *tail* is one unbroken stretch that
//! ends at the newest entry the stream has brought, the part the desk keeps
//! up to date and keeps a copy of between runs; it is trimmed from its
//! oldest end when it grows past [`MOST`]. Below it lie the entries read
//! for a place, which stay as long as the desk runs: there are at most a
//! few hundred for each thread opened.

use std::ops::Deref;

use crate::model::Entry;

/// How many entries of the tail are held: four times what the first
/// connection reads, so that a session's day stays on screen.
pub const MOST: usize = crate::link::TAIL as usize * 4;

#[derive(Debug, Default)]
pub struct Held {
    /// Oldest first, each seq once.
    entries: Vec<Entry>,
    /// The seq the tail begins at; everything below it was read for a
    /// place. Zero until a tail has come.
    tail_from: i64,
}

impl Deref for Held {
    type Target = [Entry];

    fn deref(&self) -> &[Entry] {
        &self.entries
    }
}

impl Held {
    /// What was kept of the journal last time: all of it the tail.
    pub fn kept(mut entries: Vec<Entry>) -> Self {
        entries.sort_by_key(|e| e.seq);
        entries.dedup_by_key(|e| e.seq);
        let tail_from = entries.first().map_or(0, |e| e.seq);
        Self { entries, tail_from }
    }

    /// The newest seq held; zero when nothing is.
    pub fn newest(&self) -> i64 {
        self.entries.last().map_or(0, |e| e.seq)
    }

    /// The tail: the stretch that ends at the newest entry and is kept up
    /// to date.
    pub fn tail(&self) -> &[Entry] {
        &self.entries[self.tail_start()..]
    }

    fn tail_start(&self) -> usize {
        self.entries.partition_point(|e| e.seq < self.tail_from)
    }

    /// Take in a stretch of the journal that ends at its newest entry:
    /// the tail at a first connection, or what was written since the
    /// newest held. Joins the tail when it reaches it; otherwise it is the
    /// tail from now on, and what was held before it is kept as a place's.
    /// Gives back the entries that were new.
    pub fn stretch(&mut self, entries: Vec<Entry>) -> Vec<Entry> {
        let newest = self.newest();
        if let Some(first) = entries.iter().map(|e| e.seq).min()
            && (newest == 0 || first > newest + 1)
        {
            self.tail_from = first;
        }
        let new = self.merge(entries);
        self.trim();
        new
    }

    /// Take in the entries of one place, however old. Gives back the
    /// entries that were new.
    pub fn place(&mut self, entries: Vec<Entry>) -> Vec<Entry> {
        self.merge(entries)
    }

    /// Hold each of `entries` that is not held yet, in order of seq.
    fn merge(&mut self, entries: Vec<Entry>) -> Vec<Entry> {
        let mut new = Vec::new();
        for entry in entries {
            if let Err(at) = self.entries.binary_search_by_key(&entry.seq, |e| e.seq) {
                self.entries.insert(at, entry.clone());
                new.push(entry);
            }
        }
        new
    }

    /// Let the oldest of the tail go once it holds more than [`MOST`].
    fn trim(&mut self) {
        let start = self.tail_start();
        let over = (self.entries.len() - start).saturating_sub(MOST);
        if over > 0 {
            self.entries.drain(start..start + over);
            self.tail_from = self.entries[start].seq;
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn entry(seq: i64) -> Entry {
        Entry {
            seq,
            at: Utc::now(),
            kind: "said".into(),
            data: serde_json::Value::Null,
        }
    }

    fn entries(seqs: impl IntoIterator<Item = i64>) -> Vec<Entry> {
        seqs.into_iter().map(entry).collect()
    }

    fn seqs(held: &[Entry]) -> Vec<i64> {
        held.iter().map(|e| e.seq).collect()
    }

    #[test]
    fn an_entry_is_held_once_whichever_way_it_came() {
        let mut held = Held::kept(entries([5, 6]));
        assert_eq!(seqs(&held.stretch(entries([6, 7, 8]))), [7, 8]);
        assert_eq!(seqs(&held.place(entries([2, 5, 7]))), [2]);
        assert_eq!(seqs(&held), [2, 5, 6, 7, 8]);
        assert_eq!(held.newest(), 8);
        assert_eq!(seqs(held.tail()), [5, 6, 7, 8]);
    }

    #[test]
    fn what_was_written_since_joins_the_tail_and_a_tail_too_far_on_starts_it_again() {
        let mut held = Held::kept(entries([10, 11]));
        held.stretch(entries([12, 13]));
        assert_eq!(seqs(held.tail()), [10, 11, 12, 13]);
        // The daemon was gone so long that only its tail was read: what was
        // held before stays, as a place's entries do, but is not the tail.
        held.stretch(entries([40, 41]));
        assert_eq!(seqs(held.tail()), [40, 41]);
        assert_eq!(seqs(&held), [10, 11, 12, 13, 40, 41]);
    }

    #[test]
    fn the_tail_is_trimmed_from_its_oldest_end_and_a_places_entries_are_not() {
        let mut held = Held::default();
        held.place(entries([1, 2, 3]));
        let most = i64::try_from(MOST).unwrap();
        held.stretch(entries(100..100 + most));
        assert_eq!(held.tail().len(), MOST);
        held.stretch(entries([100 + most]));
        assert_eq!(held.tail().len(), MOST);
        assert_eq!(held.tail()[0].seq, 101);
        assert_eq!(&seqs(&held)[..3], [1, 2, 3]);
    }

    #[test]
    fn nothing_held_takes_the_first_tail_whole() {
        let mut held = Held::default();
        assert_eq!(held.newest(), 0);
        held.stretch(entries([3, 4]));
        assert_eq!(seqs(held.tail()), [3, 4]);
        held.stretch(entries([5]));
        assert_eq!(seqs(held.tail()), [3, 4, 5]);
    }
}
