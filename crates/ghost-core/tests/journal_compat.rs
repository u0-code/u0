//! The journal is a forever format.
//!
//! What a ghost has lived through is written once and read for as long as
//! she exists, by whatever version of this code is running her then. So an
//! event may gain a variant, or a field that is optional, and nothing else:
//! no field removed, none renamed, none made to mean something new.
//!
//! `fixtures/events.jsonl` holds entries exactly as they have been written,
//! one per line. **Its lines are never edited and never removed.** A new
//! kind of event adds a line; a new optional field adds a line that has it,
//! beside the old line that does not. If a change to an event makes one of
//! these tests fail, the change is wrong, not the fixture.

use std::collections::BTreeSet;

use ghost_core::{
    Ghost,
    journal::{Event, Journal},
};
use serde_json::Value;

const WRITTEN: &str = include_str!("fixtures/events.jsonl");

const CERTIFICATE: &str = r#"
name = "Lin"
pronouns = "she/her"
born = "2026-11-01"
temperament = "quiet"
voice = "short"
origin = "made"

[home]
city = "Chengdu"
country = "CN"
timezone = "Asia/Shanghai"

[rhythm]
wake = "08:30"
sleep = "00:30"
"#;

fn written() -> impl Iterator<Item = (usize, &'static str)> {
    WRITTEN
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(at, line)| (at + 1, line))
}

#[test]
fn what_was_written_is_still_read() {
    for (at, line) in written() {
        let event: Event = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line {at} is no longer read: {e}\n{line}"));
        let as_written: Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            event.kind(),
            as_written["kind"],
            "line {at} is read as another kind of event"
        );
    }
}

#[test]
fn nothing_written_is_dropped_on_the_way_back_out() {
    // Reading an entry and writing it again keeps every field it had. New
    // optional fields may appear with their defaults; nothing may go.
    fn within(old: &Value, new: &Value, path: &str) {
        match (old, new) {
            (Value::Object(old), Value::Object(new)) => {
                for (key, value) in old {
                    let there = new
                        .get(key)
                        .unwrap_or_else(|| panic!("{path}.{key} was written and is not kept"));
                    within(value, there, &format!("{path}.{key}"));
                }
            }
            (Value::Array(old), Value::Array(new)) => {
                assert_eq!(old.len(), new.len(), "{path} changed length");
                for (i, (a, b)) in old.iter().zip(new).enumerate() {
                    within(a, b, &format!("{path}[{i}]"));
                }
            }
            // A time may come back in another spelling of the same instant.
            (Value::String(a), Value::String(b)) if a != b => {
                let instant = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok();
                assert!(
                    instant(a).is_some() && instant(a) == instant(b),
                    "{path} was {a:?} and is now {b:?}"
                );
            }
            (a, b) => assert_eq!(a, b, "{path} changed"),
        }
    }
    for (at, line) in written() {
        let event: Event = serde_json::from_str(line).unwrap();
        let old: Value = serde_json::from_str(line).unwrap();
        let new = serde_json::to_value(&event).unwrap();
        within(&old, &new, &format!("line {at}"));
    }
}

#[test]
fn every_kind_of_event_has_an_entry_as_it_is_written() {
    let have: BTreeSet<String> = written()
        .map(|(_, line)| {
            let entry: Value = serde_json::from_str(line).unwrap();
            entry["kind"].as_str().unwrap().to_owned()
        })
        .collect();
    let missing: Vec<&str> = Event::KINDS
        .iter()
        .copied()
        .filter(|kind| !have.contains(*kind))
        .collect();
    assert!(
        missing.is_empty(),
        "no entry in tests/fixtures/events.jsonl for: {missing:?}. \
         Add a line for each, as it is written today."
    );
}

#[test]
fn a_life_written_by_earlier_versions_still_makes_a_memory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ghost.toml"), CERTIFICATE).unwrap();
    // Born the way earlier versions did it, with one entry, so that the seqs
    // the fixture refers to ("replaces") are the seqs it was written with.
    {
        let mut journal = Journal::create(&dir.path().join("journal.db")).unwrap();
        let identity = ghost_core::identity::read_certificate(dir.path()).unwrap();
        journal
            .append(Event::Born {
                name: "Lin".into(),
                seal: identity.seal().clone(),
            })
            .unwrap();
    }
    let mut ghost = Ghost::open(dir.path()).unwrap();
    let mut lived = 0;
    for (at, line) in written() {
        let event: Event = serde_json::from_str(line).unwrap();
        // She is born once, and already has been.
        if matches!(event, Event::Born { .. }) {
            continue;
        }
        ghost
            .record(event)
            .unwrap_or_else(|e| panic!("line {at} can no longer be lived through: {e}\n{line}"));
        lived += 1;
    }
    // The chain holds, and what she remembers can be rebuilt from it.
    assert_eq!(ghost.journal().verify().unwrap(), lived + 1);
    drop(ghost);
    let again = Ghost::open(dir.path()).unwrap();
    assert!(again.memory().place("t:wen").unwrap().is_some());
    assert!(again.memory().digest("t:wen").unwrap().is_some());
    assert_eq!(again.memory().practice(5).unwrap().len(), 1);
}
