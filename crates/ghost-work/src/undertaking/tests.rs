//! An undertaking's record: kept across handles and processes, loaded as
//! every earlier version wrote it, and never rewritten where it is meant
//! to be append-only.

use std::{fs, path::PathBuf};

use chrono::Utc;
use ghost_brain::{Item, Picture};
use ghost_core::media;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::{
    db::{SCHEMA, SCHEMA_VERSION},
    *,
};
use crate::{prompts::Prompts, steps::Line};

/// A step that said `said`, taken at `at`.
fn line(at: &str, said: &str) -> Line {
    Line {
        at: at.into(),
        step: 1,
        said: said.into(),
        calls: Vec::new(),
        asked: None,
    }
}

impl Undertaking {
    /// The words of what was taken from the inbox, whatever their kind.
    fn taken(&self) -> Vec<String> {
        self.take_inbox().into_iter().map(|m| m.text).collect()
    }
}

fn db(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join(WORK_DB)
}

#[test]
fn work_under_way_can_be_looked_at_from_outside_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let work = Undertaking::open(&db(&dir), "u1", Some("build the thing")).unwrap();
    work.update(|s| {
        s.steps = 7;
        s.sittings = 2;
        s.waiting = Some("which colour?".into());
    })
    .unwrap();
    work.log(&line("2026-11-15T08:30:00+00:00", ""));
    let part = work.open_part("one half").unwrap();
    drop(part);
    Undertaking::open(&db(&dir), "u2", Some("another")).unwrap();
    drop(work);

    let before = fs::read(db(&dir)).unwrap();
    let seen = glance(&db(&dir)).unwrap();
    assert_eq!(fs::read(db(&dir)).unwrap(), before, "looking wrote nothing");
    assert_eq!(seen.len(), 2, "a part is counted under its whole: {seen:?}");
    assert_eq!(seen[0].id, "u1");
    assert_eq!(
        (seen[0].steps, seen[0].sittings, seen[0].parts_open),
        (7, 2, 1)
    );
    assert_eq!(seen[0].waiting.as_deref(), Some("which colour?"));
    assert_eq!(
        seen[0].last_step.map(|at| at.to_rfc3339()),
        Some("2026-11-15T08:30:00+00:00".to_owned())
    );
    assert_eq!(seen[1].last_step, None);
}

#[test]
fn an_undertaking_is_picked_up_from_the_database_after_its_handle_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let first = Undertaking::open(&db(&dir), "u1", Some("build the thing")).unwrap();
    first
        .update(|s| {
            s.plan.push(PlanStep {
                text: "look around".into(),
                status: Progress::Done,
                ..PlanStep::default()
            });
            s.notes.push(Note {
                at: Utc::now(),
                text: "python is 3.12".into(),
            });
        })
        .unwrap();
    first
        .save(
            &crate::Prompts::shipped(),
            &[Item::user("build the thing")],
            0,
        )
        .unwrap();
    first.remember(&[Item::user("and test it")]);
    first.log(&line("2026-11-15T08:30:00+00:00", "looked"));
    drop(first);

    let again = Undertaking::open(&db(&dir), "u1", Some("something else")).unwrap();
    let state = again.state();
    assert_eq!(state.brief, "build the thing", "the brief does not change");
    assert_eq!(state.plan_text(&Prompts::shipped()), "[x] 1. look around");
    assert_eq!(
        state.notes_text(&crate::Prompts::shipped(), 1000),
        "- python is 3.12"
    );
    assert_eq!(
        again.transcript(&crate::Prompts::shipped()).unwrap().len(),
        1
    );
    assert_eq!(again.history().len(), 2);
    assert_eq!(
        again.logged(),
        [line("2026-11-15T08:30:00+00:00", "looked")]
    );
}

#[test]
fn a_plan_whose_steps_have_only_a_text_and_a_status_loads() {
    let dir = tempfile::tempdir().unwrap();
    // As a step with no check, no order and no proof is saved.
    let state = r#"{"brief":"build the thing","plan":[
        {"text":"look around","status":"done"},
        {"text":"build it","status":"doing"}
    ],"notes":[],"finish":null,"sittings":1,"steps":4,
    "usage":{"input":0,"cached":0,"output":0,"thought":0},"seconds":1.0}"#;
    let conn = Connection::open(db(&dir)).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .unwrap();
    conn.execute(
        "INSERT INTO undertakings (id, state) VALUES ('u1', ?1)",
        [state],
    )
    .unwrap();
    drop(conn);

    let state = Undertaking::open(&db(&dir), "u1", None).unwrap().state();
    assert_eq!(state.brief, "build the thing");
    assert_eq!(
        state.plan[1],
        PlanStep {
            text: "build it".into(),
            status: Progress::Doing,
            ..PlanStep::default()
        }
    );
    assert_eq!(
        state.plan_text(&Prompts::shipped()),
        "[x] 1. look around\n[>] 2. build it"
    );
}

#[test]
fn a_part_is_numbered_on_from_the_parts_on_record_and_only_its_own_parent_counts() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Undertaking::open(&db(&dir), "main", Some("the whole")).unwrap();
    assert_eq!(parent.open_part("first").unwrap().id(), "main/1");
    // A sibling whose id merely begins the same way is not a part.
    let other = Undertaking::open(&db(&dir), "mainline", Some("other")).unwrap();
    other.open_part("elsewhere").unwrap();
    drop(parent);

    // A new handle, as in a later sitting, counts from the database.
    let parent = Undertaking::open(&db(&dir), "main", None).unwrap();
    let second = parent.open_part("second").unwrap();
    assert_eq!(second.id(), "main/2");
    assert_eq!(second.state().brief, "second");
    assert!(parent.open_part("  ").is_err(), "a part needs a brief");
}

/// The rows of the transcript of `u1`: what each holds of its own, and
/// which row of the history it is instead.
fn transcript_rows(dir: &tempfile::TempDir) -> Vec<(Option<String>, Option<i64>)> {
    Connection::open(db(dir))
        .unwrap()
        .prepare("SELECT item, history FROM transcript_items WHERE id = 'u1' ORDER BY n")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn a_saved_transcript_points_into_the_history_and_is_written_again_only_where_it_changed() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    let items = vec![Item::user("task"), Item::user("a"), Item::user("b")];
    u.save(&crate::Prompts::shipped(), &items, 0).unwrap();
    assert_eq!(
        transcript_rows(&dir),
        [(None, Some(1)), (None, Some(2)), (None, Some(3))],
        "each item is the history's, and only pointed to"
    );

    // An earlier item changed, as when a picture is shed, and a new one.
    let mut more = items.clone();
    more[1] = Item::user("a, shortened");
    more.push(Item::user("c"));
    u.save(&crate::Prompts::shipped(), &more, 3).unwrap();
    let rows = transcript_rows(&dir);
    assert_eq!(rows[0], (None, Some(1)), "what did not change stays");
    assert!(rows[1].0.as_deref().unwrap().contains("a, shortened"));
    assert_eq!(rows[2], (None, Some(3)));
    assert_eq!(rows[3], (None, Some(4)));
    assert_eq!(u.history().len(), 4);
    let again = Undertaking::open(&db(&dir), "u1", None).unwrap();
    assert_eq!(again.transcript(&crate::Prompts::shipped()).unwrap(), more);

    // Folded down, the rows past its end go.
    let folded = vec![Item::user("task, and where it got to")];
    again.save(&crate::Prompts::shipped(), &folded, 1).unwrap();
    assert_eq!(u.transcript(&crate::Prompts::shipped()).unwrap(), folded);
}

fn seen(bytes: &[u8]) -> Item {
    Item::Result {
        id: "c1".into(),
        name: "fetch".into(),
        output: "An image.".into(),
        failed: false,
        pictures: vec![Picture {
            mime: "image/png".into(),
            bytes: bytes.to_vec(),
        }],
    }
}

/// The `n`th item of the transcript as its row holds it.
fn saved_row(dir: &tempfile::TempDir, n: i64) -> String {
    Connection::open(db(dir))
        .unwrap()
        .query_row(
            "SELECT item FROM transcript_items WHERE n = ?1",
            [n],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_saved_transcript_names_a_picture_by_its_file_and_holds_no_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    let bytes = b"\x89PNG\r\n\x1a\n and the rest";
    // Never handed to `keep`: saving keeps it first.
    u.save(
        &crate::Prompts::shipped(),
        &[Item::user("look"), seen(bytes)],
        0,
    )
    .unwrap();
    let file = media::name(bytes, "image/png");
    let row = saved_row(&dir, 2);
    let written: Value = serde_json::from_str(&row).unwrap();
    assert_eq!(
        written["pictures"],
        json!([{"mime": "image/png", "file": file}])
    );
    assert!(!row.contains("\"data\""), "{row}");
    assert!(dir.path().join(media::MEDIA_DIR).join(&file).exists());
    // And it comes back whole.
    assert_eq!(
        u.transcript(&crate::Prompts::shipped()).unwrap()[1],
        seen(bytes)
    );
}

#[test]
fn a_picture_whose_file_is_gone_comes_back_as_a_line_of_text() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    let bytes = b"GIF89a gone";
    u.save(&crate::Prompts::shipped(), &[seen(bytes)], 0)
        .unwrap();
    let file = media::name(bytes, "image/png");
    fs::remove_file(dir.path().join(media::MEDIA_DIR).join(&file)).unwrap();
    let items = u
        .transcript(&crate::Prompts::shipped())
        .expect("the rest still loads");
    assert_eq!(
        items,
        [Item::result(
            "c1",
            "fetch",
            format!("An image.\n[The image {file} is no longer on disk, so it cannot be shown.]"),
            false
        )]
    );
}

#[test]
fn a_picture_named_by_a_path_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    fs::write(dir.path().join("outside.png"), "not to be read").unwrap();
    let row = r#"{"is":"result","id":"c2","name":"fetch","output":"Another.","failed":false,"pictures":[{"mime":"image/png","file":"../outside.png"}]}"#;
    Connection::open(db(&dir))
        .unwrap()
        .execute(
            "INSERT INTO transcript_items (id, n, item) VALUES ('u1', 1, ?1)",
            [row],
        )
        .unwrap();
    let items = u.transcript(&crate::Prompts::shipped()).unwrap();
    assert!(matches!(&items[0], Item::Result { pictures, output, .. }
        if pictures.is_empty() && output.contains("no longer on disk")));
}

#[test]
fn the_history_names_a_picture_and_does_not_hold_it() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    let picture = Picture {
        mime: "image/png".into(),
        bytes: b"\x89PNG not really".to_vec(),
    };
    let kept = u.keep(std::slice::from_ref(&picture));
    let file = media::name(&picture.bytes, &picture.mime);
    assert_eq!(kept, std::slice::from_ref(&file));
    assert!(dir.path().join(media::MEDIA_DIR).join(&file).exists());
    u.remember(&[Item::Result {
        id: "c1".into(),
        name: "fetch".into(),
        output: "An image.".into(),
        failed: false,
        pictures: vec![picture],
    }]);
    let conn = Connection::open(db(&dir)).unwrap();
    let written: String = conn
        .query_row("SELECT item FROM history", [], |row| row.get(0))
        .unwrap();
    assert!(!written.contains("pictures"), "{written}");
    assert_eq!(
        u.history(),
        [Item::result(
            "c1",
            "fetch",
            format!("An image.\n[image: {file}]"),
            false
        )]
    );
}

#[test]
fn history_refuses_rewriting() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    u.remember(&[Item::user("first"), Item::user("second")]);
    let conn = Connection::open(db(&dir)).unwrap();
    assert!(
        conn.execute("UPDATE history SET item = '\"changed\"'", [])
            .is_err()
    );
    assert!(conn.execute("DELETE FROM history", []).is_err());
    u.remember(&[Item::user("third")]);
    assert_eq!(u.history().len(), 3, "and it is still added to");
}

#[test]
fn two_undertakings_in_one_database_do_not_see_each_others_state() {
    let dir = tempfile::tempdir().unwrap();
    let a = Undertaking::open(&db(&dir), "a", Some("paint the fence")).unwrap();
    let b = Undertaking::open(&db(&dir), "b", Some("mend the gate")).unwrap();
    a.update(|s| s.steps = 7).unwrap();
    a.save(&crate::Prompts::shipped(), &[Item::user("fence")], 0)
        .unwrap();
    a.log(&line("2026-11-15T08:30:00+00:00", ""));
    a.add("white").unwrap();

    assert_eq!(b.state().brief, "mend the gate");
    assert_eq!(b.state().steps, 0);
    assert!(b.transcript(&crate::Prompts::shipped()).is_none());
    assert!(b.history().is_empty());
    assert!(b.logged().is_empty());
    assert!(b.taken().is_empty(), "what was added to a is not b's");
    assert_eq!(a.taken(), ["white"]);
}

#[test]
fn changes_made_through_different_handles_at_once_are_all_kept() {
    let dir = tempfile::tempdir().unwrap();
    Undertaking::open(&db(&dir), "u1", Some("count")).unwrap();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let path = db(&dir);
            std::thread::spawn(move || {
                let u = Undertaking::open(&path, "u1", None).unwrap();
                for _ in 0..25 {
                    u.update(|s| s.steps += 1).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let u = Undertaking::open(&db(&dir), "u1", None).unwrap();
    assert_eq!(u.state().steps, 100);
}

#[test]
fn items_added_from_another_connection_are_taken_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    Undertaking::open(&db(&dir), "u1", Some("find when it closes")).unwrap();
    let adders: Vec<_> = (0..3)
        .map(|who| {
            let path = db(&dir);
            std::thread::spawn(move || {
                let u = Undertaking::open(&path, "u1", None).unwrap();
                for i in 0..20 {
                    u.add(&format!("{who}-{i}")).unwrap();
                }
            })
        })
        .collect();
    // Two takers racing the adders and each other.
    let takers: Vec<_> = (0..2)
        .map(|_| {
            let path = db(&dir);
            std::thread::spawn(move || {
                let u = Undertaking::open(&path, "u1", None).unwrap();
                let mut got = Vec::new();
                for _ in 0..200 {
                    got.extend(u.taken());
                }
                got
            })
        })
        .collect();
    for t in adders {
        t.join().unwrap();
    }
    let mut taken: Vec<String> = takers.into_iter().flat_map(|t| t.join().unwrap()).collect();
    let last = Undertaking::open(&db(&dir), "u1", None).unwrap();
    taken.extend(last.taken());

    let mut sorted = taken.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), taken.len(), "nothing was taken twice");
    assert_eq!(taken.len(), 60, "nothing was lost");
    let mut additions = last.state().additions;
    additions.sort();
    assert_eq!(additions, sorted, "each became part of the task once");
}

#[test]
fn only_the_newest_notes_are_shown_when_there_are_many() {
    let mut state = State::default();
    for i in 0..10 {
        state.notes.push(Note {
            at: Utc::now(),
            text: format!("fact number {i}"),
        });
    }
    let shown = state.notes_text(&crate::Prompts::shipped(), 40);
    assert_eq!(
        shown,
        "(8 older notes are not shown)\n- fact number 8\n- fact number 9"
    );
}

#[test]
fn what_is_added_from_outside_becomes_part_of_the_task() {
    let dir = tempfile::tempdir().unwrap();
    let worker = Undertaking::open(&db(&dir), "u1", Some("find when it closes")).unwrap();
    // Someone else, with their own handle on the same database.
    let outside = Undertaking::open(&db(&dir), "u1", None).unwrap();
    outside.add("weekends too").unwrap();
    outside.add("and whether it takes bookings").unwrap();
    assert_eq!(
        worker.taken(),
        ["weekends too", "and whether it takes bookings"]
    );
    assert!(worker.taken().is_empty(), "taken once");
    assert_eq!(
        outside.state().task_text(&crate::Prompts::shipped()),
        "find when it closes\n\nAdded later: weekends too\n\nAdded later: and whether it takes bookings"
    );
}

#[test]
fn a_database_of_another_schema_is_refused_and_left_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    // As schema 1 left it, with a line not yet taken.
    let conn = Connection::open(db(&dir)).unwrap();
    conn.execute_batch(
        "CREATE TABLE undertakings (id TEXT PRIMARY KEY, state TEXT NOT NULL);
         CREATE TABLE inbox (
             id TEXT NOT NULL, n INTEGER NOT NULL, text TEXT NOT NULL,
             taken INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (id, n));
         INSERT INTO inbox (id, n, text) VALUES ('u1', 1, 'weekends too');
         PRAGMA user_version = 1;",
    )
    .unwrap();
    drop(conn);

    let refused = Undertaking::open(&db(&dir), "u1", Some("anything"))
        .err()
        .expect("schema 1 is not read");
    assert!(refused.to_string().contains("schema 1"), "{refused}");
    let conn = Connection::open(db(&dir)).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let columns: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('inbox')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((version, columns), (1, 4), "nothing in it was changed");
}

#[test]
fn what_a_part_says_to_its_parent_does_not_join_the_parents_task() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Undertaking::open(&db(&dir), "main", Some("find when it closes")).unwrap();
    parent.add("weekends too").unwrap();
    parent
        .post(MailKind::Told, "part 1 says two sources agree")
        .unwrap();
    parent
        .post(MailKind::Asked, "part 1 asks which year")
        .unwrap();
    parent.post(MailKind::Ended, "part 1 finished").unwrap();
    assert_eq!(parent.pending(), 4);
    let kinds: Vec<MailKind> = parent.take_inbox().iter().map(|m| m.kind).collect();
    assert_eq!(
        kinds,
        [
            MailKind::Added,
            MailKind::Told,
            MailKind::Asked,
            MailKind::Ended
        ]
    );
    assert_eq!(parent.pending(), 0);
    assert_eq!(parent.state().additions, ["weekends too"]);
}

#[test]
fn the_parts_of_a_work_are_listed_by_number_and_end_when_it_is_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Undertaking::open(&db(&dir), "main", Some("the whole")).unwrap();
    let first = parent.open_part("first").unwrap();
    let second = parent.open_part("second").unwrap();
    second
        .update(|s| {
            s.finish = Some(Finish {
                outcome: Outcome::Finished,
                summary: "done".into(),
                files: vec![],
                lesson: None,
                at: Utc::now(),
            });
        })
        .unwrap();
    // One whose id only looks like a part's, with no parent, is not one.
    Undertaking::open(&db(&dir), "main/3", Some("not a part")).unwrap();
    let listed: Vec<(u64, String)> = parent
        .parts()
        .into_iter()
        .map(|(n, s)| (n, s.brief))
        .collect();
    assert_eq!(listed, [(1, "first".into()), (2, "second".into())]);
    assert_eq!(parent.part(2).unwrap().number(), Some(2));
    assert!(parent.part(3).is_err());

    assert!(parent.stop().unwrap());
    assert_eq!(first.state().finish.unwrap().outcome, Outcome::Dropped);
    assert_eq!(
        second.state().finish.unwrap().outcome,
        Outcome::Finished,
        "what had ended stays as it ended"
    );
}

#[test]
fn nothing_starts_without_a_brief() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Undertaking::open(&db(&dir), "u1", None).is_err());
}

#[test]
fn a_database_from_a_newer_version_is_not_written_to() {
    let dir = tempfile::tempdir().unwrap();
    Connection::open(db(&dir))
        .unwrap()
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(Undertaking::open(&db(&dir), "u1", Some("anything")).is_err());
}

#[test]
fn a_part_is_done_in_the_folder_its_work_is_done_in() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(WORK_DB);
    let work = Undertaking::open(&db, "w", Some("count the lines")).unwrap();
    work.update(|s| s.dir = Some("/home/her/Ghost/2026-11-18/count-w".into()))
        .unwrap();
    let part = work.open_part("count ghost-core").unwrap();
    assert_eq!(
        part.state().dir.as_deref(),
        Some("/home/her/Ghost/2026-11-18/count-w")
    );
}

#[test]
fn a_change_that_could_not_be_written_is_not_kept_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let work = Undertaking::open(&db(&dir), "u1", Some("build the thing")).unwrap();
    work.update(|s| s.steps = 1).unwrap();
    // Something on disk refuses every change to the record from here on.
    Connection::open(db(&dir))
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER no_more BEFORE UPDATE ON undertakings
             BEGIN SELECT RAISE(ABORT, 'the disk is full'); END;",
        )
        .unwrap();

    let written = work.update(|s| s.steps = 2);
    assert!(
        written
            .unwrap_err()
            .to_string()
            .contains("the disk is full"),
        "the error is handed back"
    );
    assert_eq!(work.state().steps, 1, "the handle shows what is on disk");
    assert!(work.stop().is_err(), "nor is an end taken as written");
    assert!(work.state().finish.is_none());
}

#[test]
fn work_on_record_is_opened_and_looked_at_while_a_worker_holds_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let work = Undertaking::open(&db(&dir), "u1", Some("build the thing")).unwrap();
    work.update(|s| s.steps = 3).unwrap();
    let records = Records::open(&db(&dir)).unwrap();
    // A worker in the middle of a write.
    let worker = Connection::open(db(&dir)).unwrap();
    worker.execute_batch("BEGIN IMMEDIATE").unwrap();

    let started = std::time::Instant::now();
    let again = Undertaking::open(&db(&dir), "u1", None).unwrap();
    assert_eq!(again.state().steps, 3);
    let seen = records.look(&["u1".into(), "u2".into()]).unwrap();
    assert_eq!(
        seen.len(),
        1,
        "work with no record is not looked into being"
    );
    assert_eq!(seen[0].state.steps, 3);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "nothing waited for the lock"
    );
    worker.execute_batch("COMMIT").unwrap();
    assert!(records.look(&["u2".into()]).unwrap().is_empty());
    assert_eq!(glance(&db(&dir)).unwrap().len(), 1);
}

#[test]
fn a_record_a_newer_build_wrote_is_refused_rather_than_written_back_without_what_it_added() {
    let dir = tempfile::tempdir().unwrap();
    let work = Undertaking::open(&db(&dir), "u1", Some("build the thing")).unwrap();
    work.update(|s| {
        s.plan.push(PlanStep {
            text: "start".into(),
            ..PlanStep::default()
        });
    })
    .unwrap();
    let conn = Connection::open(db(&dir)).unwrap();
    let saved = |conn: &Connection| -> String {
        conn.query_row("SELECT state FROM undertakings WHERE id = 'u1'", [], |r| {
            r.get(0)
        })
        .unwrap()
    };
    // A newer build added a field to a step of the plan.
    let mut newer: Value = serde_json::from_str(&saved(&conn)).unwrap();
    newer["plan"][0]["weight"] = json!(3);
    let newer = newer.to_string();
    conn.execute(
        "UPDATE undertakings SET state = ?1 WHERE id = 'u1'",
        [&newer],
    )
    .unwrap();

    let refused = work.update(|s| s.steps = 9).unwrap_err().to_string();
    assert!(
        refused.contains("plan[0].weight"),
        "it says what: {refused}"
    );
    assert!(Undertaking::open(&db(&dir), "u1", None).is_err());
    assert_eq!(
        saved(&conn),
        newer,
        "the record is as the newer build left it"
    );
}

#[test]
fn a_field_this_build_does_not_have_that_holds_nothing_does_not_stop_it() {
    let dir = tempfile::tempdir().unwrap();
    let work = Undertaking::open(&db(&dir), "u1", Some("build the thing")).unwrap();
    Connection::open(db(&dir))
        .unwrap()
        .execute(
            "UPDATE undertakings
             SET state = json_set(state, '$.gone', json('null'), '$.none', json('[]'))",
            [],
        )
        .unwrap();
    work.update(|s| s.steps = 2).unwrap();
    assert_eq!(work.state().steps, 2);
}

#[test]
fn a_database_of_another_schema_is_not_read_either() {
    let dir = tempfile::tempdir().unwrap();
    Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    Connection::open(db(&dir))
        .unwrap()
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    let refused = Records::open(&db(&dir)).err().unwrap().to_string();
    assert!(refused.contains("schema"), "{refused}");
}

#[test]
fn work_that_has_ended_keeps_its_history_and_not_its_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    let items = vec![Item::user("task"), Item::user("a"), Item::user("b")];
    u.save(&crate::Prompts::shipped(), &items, 0).unwrap();
    assert_eq!(transcript_rows(&dir).len(), 3);

    // Ended by its own last step, which still adds to the history.
    let mut last = items.clone();
    last.push(Item::user("done"));
    u.step(
        &crate::Prompts::shipped(),
        |s| {
            s.finish = Some(Finish {
                outcome: Outcome::Finished,
                summary: "done".into(),
                files: Vec::new(),
                lesson: None,
                at: Utc::now(),
            });
        },
        &line("2026-11-15T08:30:00+00:00", "done"),
        &last,
        3,
    )
    .unwrap();
    assert!(transcript_rows(&dir).is_empty());
    assert_eq!(u.history(), last, "everything it held is in its history");
    // A sitting stopping after it saves nothing back.
    u.save(&crate::Prompts::shipped(), &last, 4).unwrap();
    assert!(transcript_rows(&dir).is_empty());
    assert!(u.transcript(&crate::Prompts::shipped()).is_none());
    assert_eq!(u.history().len(), 4);
}

#[test]
fn work_ended_from_outside_lets_its_transcript_go() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    u.save(
        &crate::Prompts::shipped(),
        &[Item::user("task"), Item::user("a")],
        0,
    )
    .unwrap();
    let other = Undertaking::open(&db(&dir), "u1", None).unwrap();
    assert!(other.stop().unwrap());
    assert!(transcript_rows(&dir).is_empty());
    assert_eq!(u.history().len(), 2);
}

#[test]
fn room_freed_in_a_new_database_is_given_back_to_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    let pages = |dir: &tempfile::TempDir| -> (i64, i64) {
        let conn = Connection::open(db(dir)).unwrap();
        (
            conn.pragma_query_value(None, "page_count", |r| r.get(0))
                .unwrap(),
            conn.pragma_query_value(None, "freelist_count", |r| r.get(0))
                .unwrap(),
        )
    };
    let before = pages(&dir).0;
    // A long transcript of items the history does not have word for word.
    let long: Vec<Item> = (0..200)
        .map(|n| Item::user(format!("{n} {}", "x".repeat(2000))))
        .collect();
    u.save(&crate::Prompts::shipped(), &long, 200).unwrap();
    let grown = pages(&dir).0;
    assert!(grown > before + 50, "{before} pages, then {grown}");
    u.stop().unwrap();
    let (after, free) = pages(&dir);
    assert_eq!(free, 0, "no page is kept free inside the file");
    assert!(after < grown - 50, "{grown} pages, then {after}");
}
