//! An undertaking's record: kept across handles and processes, loaded as
//! every earlier version wrote it, and never rewritten where it is meant
//! to be append-only.

use std::{fs, path::PathBuf};

use chrono::Utc;
use ghost_brain::{Item, Picture};
use ghost_core::media;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::*;
use crate::prompts::Prompts;

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
    });
    work.log(&serde_json::json!({"at": "2026-11-15T08:30:00+00:00", "step": 7}));
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
    first.update(|s| {
        s.plan.push(PlanStep {
            text: "look around".into(),
            status: Progress::Done,
            ..PlanStep::default()
        });
        s.notes.push(Note {
            at: Utc::now(),
            text: "python is 3.12".into(),
        });
    });
    first.save_transcript(&[Item::user("build the thing")]);
    first.remember(&[Item::user("build the thing"), Item::user("and test it")]);
    first.log(&serde_json::json!({"step": 1}));
    drop(first);

    let again = Undertaking::open(&db(&dir), "u1", Some("something else")).unwrap();
    let state = again.state();
    assert_eq!(state.brief, "build the thing", "the brief does not change");
    assert_eq!(state.plan_text(&Prompts::shipped()), "[x] 1. look around");
    assert_eq!(state.notes_text(1000), "- python is 3.12");
    assert_eq!(again.transcript().unwrap().len(), 1);
    assert_eq!(again.history().len(), 2);
    assert_eq!(again.logged(), [serde_json::json!({"step": 1})]);
}

#[test]
fn a_plan_saved_before_steps_had_checks_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    // As the state was written before steps had anything but a text
    // and a status.
    let state = r#"{"brief":"build the thing","plan":[
        {"text":"look around","status":"done"},
        {"text":"build it","status":"doing"}
    ],"notes":[],"finish":null,"sittings":1,"steps":4,
    "usage":{"input":0,"cached":0,"output":0,"thought":0},"seconds":1.0}"#;
    let conn = Connection::open(db(&dir)).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
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

#[test]
fn a_transcript_saved_before_tools_returned_pictures_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    // Exactly as an earlier version wrote it.
    let old = r#"[{"is":"user","text":"list the files"},{"is":"assistant","text":"","calls":[{"id":"c1","name":"shell","args":{"command":"ls"}}]},{"is":"result","id":"c1","name":"shell","output":"a.txt","failed":false}]"#;
    Connection::open(db(&dir))
        .unwrap()
        .execute(
            "INSERT INTO transcript (id, items) VALUES ('u1', ?1)",
            [old],
        )
        .unwrap();
    let items = u.transcript().expect("it loads");
    assert_eq!(items.len(), 3);
    assert_eq!(items[2], Item::result("c1", "shell", "a.txt", false));
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

fn saved_row(dir: &tempfile::TempDir) -> String {
    Connection::open(db(dir))
        .unwrap()
        .query_row("SELECT items FROM transcript", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_saved_transcript_names_a_picture_by_its_file_and_holds_no_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    let bytes = b"\x89PNG\r\n\x1a\n and the rest";
    // Never handed to `keep`: saving keeps it first.
    u.save_transcript(&[Item::user("look"), seen(bytes)]);
    let file = media::name(bytes, "image/png");
    let row = saved_row(&dir);
    let written: Value = serde_json::from_str(&row).unwrap();
    assert_eq!(
        written[1]["pictures"],
        json!([{"mime": "image/png", "file": file}])
    );
    assert!(!row.contains("\"data\""), "{row}");
    assert!(dir.path().join(media::MEDIA_DIR).join(&file).exists());
    // And it comes back whole.
    assert_eq!(u.transcript().unwrap()[1], seen(bytes));
}

#[test]
fn a_picture_whose_file_is_gone_comes_back_as_a_line_of_text() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    let bytes = b"GIF89a gone";
    u.save_transcript(&[seen(bytes)]);
    let file = media::name(bytes, "image/png");
    fs::remove_file(dir.path().join(media::MEDIA_DIR).join(&file)).unwrap();
    let items = u.transcript().expect("the rest still loads");
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
fn a_picture_saved_as_base64_still_loads_and_a_path_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let u = Undertaking::open(&db(&dir), "u1", Some("anything")).unwrap();
    fs::write(dir.path().join("outside.png"), "not to be read").unwrap();
    let row = r#"[{"is":"result","id":"c1","name":"fetch","output":"An image.","failed":false,"pictures":[{"mime":"image/png","data":"AQID"}]},
        {"is":"result","id":"c2","name":"fetch","output":"Another.","failed":false,"pictures":[{"mime":"image/png","file":"../outside.png"}]}]"#;
    Connection::open(db(&dir))
        .unwrap()
        .execute(
            "INSERT INTO transcript (id, items) VALUES ('u1', ?1)",
            [row],
        )
        .unwrap();
    let items = u.transcript().unwrap();
    assert_eq!(items[0], seen(&[1, 2, 3]));
    assert!(matches!(&items[1], Item::Result { pictures, output, .. }
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
    a.update(|s| s.steps = 7);
    a.save_transcript(&[Item::user("fence")]);
    a.remember(&[Item::user("fence")]);
    a.log(&serde_json::json!({"step": 1}));
    a.add("white").unwrap();

    assert_eq!(b.state().brief, "mend the gate");
    assert_eq!(b.state().steps, 0);
    assert!(b.transcript().is_none());
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
                    u.update(|s| s.steps += 1);
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
    let shown = state.notes_text(40);
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
        outside.state().task_text(),
        "find when it closes\n\nAdded later: weekends too\n\nAdded later: and whether it takes bookings"
    );
}

#[test]
fn an_inbox_written_by_schema_one_is_kept_and_what_is_in_it_still_joins_the_task() {
    let dir = tempfile::tempdir().unwrap();
    let state = serde_json::to_string(&State {
        brief: "find when it closes".into(),
        ..State::default()
    })
    .unwrap();
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
    conn.execute(
        "INSERT INTO undertakings (id, state) VALUES ('u1', ?1)",
        [&state],
    )
    .unwrap();
    drop(conn);

    let u = Undertaking::open(&db(&dir), "u1", None).unwrap();
    assert_eq!(
        u.take_inbox(),
        [Mail {
            kind: MailKind::Added,
            text: "weekends too".into()
        }]
    );
    assert_eq!(u.state().additions, ["weekends too"]);
    u.post(MailKind::Told, "from a part").unwrap();
    assert_eq!(u.take_inbox()[0].kind, MailKind::Told);
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
    second.update(|s| {
        s.finish = Some(Finish {
            outcome: Outcome::Finished,
            summary: "done".into(),
            files: vec![],
            lesson: None,
            at: Utc::now(),
        });
    });
    // A helper from before parts ran on their own is not one of them.
    Undertaking::open(&db(&dir), "main/3", Some("an old helper")).unwrap();
    let listed: Vec<(u64, String)> = parent
        .parts()
        .into_iter()
        .map(|(n, s)| (n, s.brief))
        .collect();
    assert_eq!(listed, [(1, "first".into()), (2, "second".into())]);
    assert_eq!(parent.part(2).unwrap().number(), Some(2));
    assert!(parent.part(3).is_err());

    assert!(parent.stop());
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
