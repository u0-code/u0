use super::*;
use serde_json::json;

fn entry(seq: i64, kind: &str, data: Value) -> Entry {
    Entry {
        seq,
        at: Utc::now(),
        kind: kind.to_owned(),
        data,
    }
}

#[test]
fn a_task_waits_on_you_from_its_question_until_its_answer() {
    let mut journal = vec![
        entry(1, "undertook", json!({"id": "a", "brief": "tidy"})),
        entry(2, "asked", json!({"id": "a", "question": "which folder?"})),
    ];
    assert_eq!(tasks(&journal)[0].state(), State::Waiting);
    journal.push(entry(3, "added", json!({"id": "a", "text": "Downloads"})));
    assert_eq!(tasks(&journal)[0].state(), State::Running);
    journal.push(entry(
        4,
        "concluded",
        json!({"id": "a", "outcome": "gave_up", "summary": ""}),
    ));
    assert_eq!(tasks(&journal)[0].state(), State::Blocked);
}

#[test]
fn parts_are_kept_under_the_task_that_handed_them_out() {
    let journal = vec![
        entry(1, "undertook", json!({"id": "a", "brief": "tidy"})),
        entry(2, "undertook", json!({"id": "a/1", "brief": "photos"})),
        entry(
            3,
            "worked",
            json!({"id": "a/1", "summary": "sorted", "steps": 4}),
        ),
        entry(4, "undertook", json!({"id": "b", "brief": "other"})),
    ];
    let all = tasks(&journal);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].parts.len(), 1);
    assert_eq!(all[0].parts[0].steps(), 4);
}

/// Two sessions' worth of journal, as the daemon writes it: work taken
/// on in `s1` with two parts, one done; work in `s2`; a file reported
/// and one kept by `s1`'s work, one kept and let go, one kept by `s2`.
fn two_sessions() -> Vec<Entry> {
    vec![
        entry(
            1,
            "undertook",
            json!({"id": "w1", "brief": "tidy downloads", "place": "desk:s1"}),
        ),
        entry(2, "undertook", json!({"id": "w1/1", "brief": "images"})),
        entry(3, "undertook", json!({"id": "w1/2", "brief": "archives"})),
        entry(
            4,
            "concluded",
            json!({"id": "w1/1", "outcome": "finished", "summary": "done"}),
        ),
        entry(
            5,
            "undertook",
            json!({"id": "w2", "brief": "other", "place": "desk:s2"}),
        ),
        entry(
            6,
            "reported",
            json!({"id": "w1", "text": "listed", "files": ["/home/x/list.md"]}),
        ),
        entry(
            7,
            "kept",
            json!({"file": "abc.png", "note": "a chart", "from": "w1/2"}),
        ),
        entry(
            8,
            "kept",
            json!({"file": "old.png", "note": "gone", "from": "w1"}),
        ),
        entry(9, "let_go", json!({"kept": 8})),
        entry(
            10,
            "kept",
            json!({"file": "theirs.png", "note": "", "from": "w2"}),
        ),
        entry(
            11,
            "reported",
            json!({"id": "w1", "text": "again", "files": ["/home/x/list.md"]}),
        ),
    ]
}

#[test]
fn a_session_holds_only_the_work_taken_on_in_its_place() {
    let journal = two_sessions();
    let s1 = session_tasks(&journal, "s1");
    assert_eq!(s1.len(), 1);
    assert_eq!(s1[0].id, "w1");
    assert_eq!(session_tasks(&journal, "s2")[0].id, "w2");
    assert!(session_tasks(&journal, "s3").is_empty());
}

#[test]
fn the_sidebar_nests_every_part_of_open_work_and_none_of_finished_work() {
    let mut journal = two_sessions();
    let s1 = session_tasks(&journal, "s1");
    let parts: Vec<&str> = open_parts(&s1).iter().map(|p| p.brief.as_str()).collect();
    assert_eq!(parts, ["images", "archives"]);
    assert_eq!(open_parts(&s1)[0].state(), State::Finished);
    assert_eq!(session_state(&s1), Some(State::Running));

    journal.push(entry(
        12,
        "concluded",
        json!({"id": "w1", "outcome": "finished", "summary": ""}),
    ));
    let s1 = session_tasks(&journal, "s1");
    assert!(open_parts(&s1).is_empty());
    assert_eq!(session_state(&s1), None);
}

#[test]
fn a_session_shows_waiting_before_running() {
    let journal = vec![
        entry(
            1,
            "undertook",
            json!({"id": "a", "brief": "x", "place": "desk:s"}),
        ),
        entry(
            2,
            "undertook",
            json!({"id": "b", "brief": "y", "place": "desk:s"}),
        ),
        entry(3, "asked", json!({"id": "b", "question": "which?"})),
    ];
    assert_eq!(
        session_state(&session_tasks(&journal, "s")),
        Some(State::Waiting)
    );
}

#[test]
fn outputs_are_the_files_this_sessions_work_handed_over_each_once() {
    let journal = two_sessions();
    let out = outputs(&journal, "s1");
    let files: Vec<&str> = out.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, ["/home/x/list.md", "abc.png"], "{out:?}");
    assert_eq!(out[0].name(), "list.md");
    assert_eq!(out[0].kind(), "md · handed over");
    assert_eq!(out[1].kind(), "png · kept");
    assert_eq!(outputs(&journal, "s2")[0].file, "theirs.png");
    assert!(outputs(&journal, "s3").is_empty());
}

#[test]
fn a_long_list_shows_five_until_the_rest_are_asked_for() {
    assert_eq!(shown(3, 5, false), (3, 0));
    assert_eq!(shown(5, 5, false), (5, 0));
    assert_eq!(shown(8, 5, false), (5, 3));
    assert_eq!(shown(8, 5, true), (8, 0));
}

#[test]
fn her_places_are_listed_newest_first_each_once_and_called_by_name() {
    let at = |m: i64| Utc::now() - chrono::Duration::minutes(60 - m);
    let said = |seq, m, kind: &str, data| Entry {
        at: at(m),
        ..entry(seq, kind, data)
    };
    let journal = vec![
        said(
            1,
            1,
            "heard",
            json!({"channel": "qq", "place": {"id": "g7", "kind": "group", "name": "the clinic"},
            "from": {"id": "u1", "name": "Wen"}, "text": "anyone?"}),
        ),
        said(
            2,
            2,
            "heard",
            json!({"channel": "qq", "place": {"id": "u2", "kind": "direct"},
            "from": {"id": "u2", "name": "Lin"}, "text": "hi"}),
        ),
        said(
            3,
            3,
            "said",
            json!({"channel": "qq", "place": "g7", "text": "here"}),
        ),
        said(
            4,
            4,
            "said",
            json!({"channel": "x", "place": "home", "text": "a post"}),
        ),
    ];
    let spots = places(&journal);
    let keys: Vec<&str> = spots.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["x:home", "qq:g7", "qq:u2"]);
    let labels: Vec<&str> = spots.iter().map(|s| s.label.as_str()).collect();
    // A group by its name, not by whoever spoke last; a private place
    // by who it is with; a place nobody named by its id.
    assert_eq!(labels, ["home · x", "the clinic · qq", "Lin · qq"]);
    // Only what was said on the console channel settles a line you sent.
    assert!(journal.iter().all(|e| console_message(e).is_none()));
}

/// A session's lines as the desk said them before it spoke through the
/// console, then after; with another session beside it.
fn before_and_after() -> Vec<Entry> {
    let at = |m: i64| Utc::now() - chrono::Duration::minutes(60 - m);
    let heard = |seq, m, channel: &str, place: &str, text: &str| Entry {
        at: at(m),
        ..entry(
            seq,
            "heard",
            json!({"channel": channel, "place": {"id": place, "kind": "direct"},
            "from": {"id": "fox", "name": "Fox"}, "text": text}),
        )
    };
    let said = |seq, m, channel: &str, place: &str, text: &str| Entry {
        at: at(m),
        ..entry(
            seq,
            "said",
            json!({"channel": channel, "place": place, "text": text}),
        )
    };
    vec![
        heard(1, 1, "desk", "s1", "free some disk"),
        Entry {
            at: at(2),
            ..entry(
                2,
                "undertook",
                json!({"id": "w1", "brief": "free disk", "place": "desk:s1"}),
            )
        },
        said(3, 3, "desk", "s1", "done: 4 GB"),
        heard(4, 4, "console", "s2", "another session"),
        heard(5, 5, "console", "s1", "and the caches?"),
        said(6, 6, "console", "s1", "those too"),
    ]
}

#[test]
fn a_session_is_one_thread_across_the_old_desk_channel_and_the_console() {
    let journal = before_and_after();
    let expanded = HashSet::new();
    let pending = [Message {
        place: "s1".into(),
        hers: false,
        text: "thanks".into(),
        at: Utc::now(),
    }];
    let place = place_key("s1");
    let view = View {
        journal: &journal,
        pending: &pending,
        stops: &[],
        place: &place,

        name: "小幽灵".into(),
        who: "Fox".into(),
        tz: chrono_tz::UTC,
        connected: true,
        abilities: None,
        expanded: &expanded,
    };
    let said: Vec<(String, bool)> = items(&view)
        .into_iter()
        .filter_map(|(_, _, item)| match item {
            Item::Said { m, mine, .. } => Some((m.text, mine)),
            _ => None,
        })
        .collect();
    let said: Vec<(&str, bool)> = said.iter().map(|(t, m)| (t.as_str(), *m)).collect();
    assert_eq!(
        said,
        [
            ("free some disk", true),
            ("done: 4 GB", false),
            ("and the caches?", true),
            ("those too", false),
            ("thanks", true),
        ]
    );
    // Its work taken on before is still its own, and nobody else's.
    let ids = |s: &str| -> Vec<String> {
        session_tasks(&journal, s)
            .iter()
            .map(|t| t.id.clone())
            .collect()
    };
    assert_eq!(ids("s1"), ["w1"]);
    assert!(ids("s2").is_empty());
    assert_eq!(task_place(&journal, "w1").as_deref(), Some("desk:s1"));
}

#[test]
fn a_place_said_in_on_both_channels_is_listed_once_on_the_console() {
    let spots = places(&before_and_after());
    let keys: Vec<&str> = spots.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["console:s1", "console:s2"]);
    assert_eq!(spots[0].label, "Fox · console");
    assert_eq!(thread_key("desk:fox"), "console:fox");
    assert_eq!(thread_key("qq:desk"), "qq:desk", "only the channel moves");
}

fn view<'a>(
    journal: &'a [Entry],
    stops: &'a [crate::model::Stop],
    place: &'a str,
    expanded: &'a HashSet<String>,
) -> View<'a> {
    View {
        journal,
        pending: &[],
        stops,
        place,
        name: "小幽灵".into(),
        who: "Fox".into(),
        tz: chrono_tz::UTC,
        connected: true,
        abilities: None,
        expanded,
    }
}

#[test]
fn she_is_busy_after_your_line_until_she_answers_fails_or_is_stopped() {
    let at = |m: i64| Utc::now() - chrono::Duration::minutes(60 - m);
    let mut journal = vec![Entry {
        at: at(1),
        ..entry(
            1,
            "heard",
            json!({"channel": "console", "place": {"id": "s1", "kind": "direct"},
            "from": {"id": "fox", "name": "Fox"}, "text": "tidy up"}),
        )
    }];
    let expanded = HashSet::new();
    let place = place_key("s1");
    assert!(busy(&view(&journal, &[], &place, &expanded)));
    // A stop asked for after it ends the wait.
    let stops = [crate::model::Stop {
        place: "s1".into(),
        at: at(2),
        stopped: true,
    }];
    assert!(!busy(&view(&journal, &stops, &place, &expanded)));
    // So does a look that could not think, in that place only.
    journal.push(Entry {
        at: at(3),
        ..entry(
            2,
            "decided",
            json!({"about": "console:s1", "decision": "nothing",
            "reason": "could not think: claude: 401 OAuth access token is invalid · Please run /login"}),
        )
    });
    let shown = view(&journal, &[], &place, &expanded);
    assert!(!busy(&shown));
    let failed: Vec<String> = items(&shown)
        .into_iter()
        .filter_map(|(_, _, i)| match i {
            Item::Failed(why) => Some(why),
            _ => None,
        })
        .collect();
    assert_eq!(
        failed,
        ["claude: 401 OAuth access token is invalid · Please run /login"]
    );
    let other = place_key("s2");
    assert!(items(&view(&journal, &[], &other, &expanded)).is_empty());
}

#[test]
fn a_failure_to_think_says_how_to_sign_in_or_else_its_first_line() {
    assert_eq!(
        could_not("claude: 401 OAuth access token is invalid · Please run /login"),
        "Couldn't think: Claude Code is not logged in — run `claude` in a terminal and /login"
    );
    assert_eq!(
        could_not("openai: HTTP 401 Unauthorized"),
        "Couldn't think: the brain did not accept its key or its login"
    );
    assert_eq!(
        could_not("request timed out\nafter 120s"),
        "Couldn't think: request timed out"
    );
}
