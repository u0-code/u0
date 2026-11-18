use super::*;
use serde_json::json;

/// A ghost of whose steps nothing has been read.
static NOTHING_READ: std::sync::LazyLock<Following> = std::sync::LazyLock::new(Following::default);

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
    assert_eq!(all[0].parts[0].events.len(), 1, "its sitting is its own");
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

        name: "Ghost".into(),
        who: "Fox".into(),
        tz: chrono_tz::UTC,
        connected: true,
        abilities: None,
        expanded: &expanded,
        following: &NOTHING_READ,
        work: &[],
        whom: crate::answer::Whom::Solver,
        scroll: None,
        rendering: Rendering::Task,
        column: COLUMN,
        media: None,
        status: None,
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
fn a_transcript_names_who_said_what_and_when_and_counts_the_lines() {
    let journal = before_and_after();
    let expanded = HashSet::new();
    let place = place_key("s1");
    let (text, lines) = transcript(&view(&journal, &[], &place, &expanded));
    assert_eq!(lines, 4);
    let heads: Vec<&str> = text
        .lines()
        .filter(|l| l.contains(" · "))
        .map(|l| l.split(" · ").next().unwrap())
        .collect();
    assert_eq!(heads, ["Fox", "Ghost", "Fox", "Ghost"]);
    assert!(text.starts_with("Fox · "));
    assert!(text.contains("\nfree some disk\n\nGhost · "));
    assert!(text.ends_with("those too"), "no blank line at the end");
    let other = place_key("s3");
    assert_eq!(
        transcript(&view(&journal, &[], &other, &expanded)),
        (String::new(), 0)
    );
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
        name: "Ghost".into(),
        who: "Fox".into(),
        tz: chrono_tz::UTC,
        connected: true,
        abilities: None,
        expanded,
        following: &NOTHING_READ,
        work: &[],
        whom: crate::answer::Whom::Solver,
        scroll: None,
        rendering: Rendering::Task,
        column: COLUMN,
        media: None,
        status: None,
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

/// A look, numbered `n`, with one round at `at`, ended with the journal
/// entry `seq` or still going on.
fn a_look(n: u64, seq: Option<i64>, at: DateTime<Utc>) -> Look {
    let call = ghost_link::protocol::ToolCall {
        name: "shell".into(),
        args: json!({"command": "df -h"}),
        out: "Filesystem  Size".into(),
        cut: false,
        chars: 16,
        ms: 40,
        failed: false,
        pictures: 0,
    };
    Look {
        look: n,
        seq,
        running: seq.is_none(),
        rounds: vec![ghost_link::protocol::LookRound {
            at: at.to_rfc3339(),
            calls: vec![call],
            usage: None,
            context: None,
        }],
    }
}

#[test]
fn what_she_called_in_a_look_goes_above_the_answer_it_led_to() {
    let at = |s: i64| Utc::now() - chrono::Duration::seconds(600 - s);
    let heard = |seq, s, text: &str| Entry {
        at: at(s),
        ..entry(
            seq,
            "heard",
            json!({"channel": "console", "place": {"id": "s1", "kind": "direct"},
            "from": {"id": "fox", "name": "Fox"}, "text": text}),
        )
    };
    let opened = |seq, s| Entry {
        at: at(s),
        ..entry(
            seq,
            "opened",
            json!({"place": "console:s1", "url": "https://example.com"}),
        )
    };
    let journal = vec![
        heard(1, 0, "how full is the disk?"),
        // Opened in the look, and so one of its rows already.
        opened(10, 6),
        Entry {
            at: at(12),
            ..entry(
                2,
                "decided",
                json!({"about": "console:s1", "decision": "said something", "reason": ""}),
            )
        },
        Entry {
            at: at(14),
            ..entry(
                3,
                "said",
                json!({"channel": "console", "place": "s1", "text": "80% full"}),
            )
        },
        heard(4, 20, "stop that"),
        Entry {
            at: at(25),
            ..entry(
                5,
                "stopped",
                json!({"place": "console:s1", "by": "console:fox"}),
            )
        },
        // A look elsewhere, at the same moment, is not this thread's.
        Entry {
            at: at(25),
            ..entry(
                6,
                "decided",
                json!({"about": "qq:9", "decision": "nothing", "reason": ""}),
            )
        },
        opened(11, 40),
    ];
    let mut following = Following::default();
    following.looks(
        "console:s1",
        std::time::Instant::now(),
        Ok(vec![
            a_look(7, Some(2), at(5)),
            a_look(8, Some(5), at(22)),
            a_look(9, Some(6), at(22)),
        ]),
    );
    let expanded = HashSet::new();
    let place = place_key("s1");
    let view = View {
        following: &following,
        work: &[],
        whom: crate::answer::Whom::Solver,
        ..view(&journal, &[], &place, &expanded)
    };
    let shown: Vec<String> = items(&view)
        .into_iter()
        .map(|(_, _, item)| match item {
            Item::Said { m, .. } => m.text,
            Item::Look { look, from, to } => {
                format!("look {} for {}s", look.look, (to - from).num_seconds())
            }
            _ => "other".into(),
        })
        .collect();
    // Timed from what she was answering to the entry it ended with.
    assert_eq!(
        shown,
        [
            "how full is the disk?",
            "look 7 for 12s",
            "80% full",
            "stop that",
            "look 8 for 5s",
            "other"
        ]
    );
    assert!(!busy(&view), "a look that ended is her done with your line");
    assert_eq!(newest_look_end(&journal, &place), Some(5));
}

#[test]
fn a_look_is_timed_from_its_first_call_when_nothing_said_comes_before_it() {
    let first = Utc::now() - chrono::Duration::seconds(30);
    let look = a_look(1, None, first);
    let later = first + chrono::Duration::seconds(5);
    assert_eq!(look_started(&look, Some(later)), Some(first));
    let before = first - chrono::Duration::seconds(5);
    assert_eq!(look_started(&look, Some(before)), Some(before));
    assert_eq!(look_started(&look, None), Some(first));
}

#[test]
fn the_thread_asks_for_the_steps_of_every_work_and_of_the_open_parts_of_open_cards() {
    let journal = two_sessions();
    let place = place_key("s1");
    let mut expanded = HashSet::new();
    let works = |expanded: &HashSet<String>| wanted(&view(&journal, &[], &place, expanded)).works;
    // A folded work's steps are read too: its line says what it did.
    assert_eq!(works(&expanded), [("w1".to_owned(), true)]);
    expanded.insert("part:w1/2".to_owned());
    assert_eq!(
        works(&expanded),
        [("w1".to_owned(), true)],
        "a part shows only inside its open card"
    );
    expanded.insert("task:w1".to_owned());
    expanded.insert("part:w1/1".to_owned());
    assert_eq!(
        works(&expanded),
        [
            ("w1".to_owned(), true),
            ("w1/1".to_owned(), false),
            ("w1/2".to_owned(), true)
        ]
    );
    let w = wanted(&view(&journal, &[], &place, &expanded));
    assert_eq!((w.place.as_str(), w.thinking), ("console:s1", false));
}

#[test]
fn a_works_steps_and_what_its_journal_says_of_it_are_told_in_order() {
    let t0 = Utc::now() - chrono::Duration::minutes(10);
    let step = |n, secs: Option<i64>| ghost_link::protocol::LoggedStep {
        n,
        at: secs.map(|s| (t0 + chrono::Duration::seconds(s)).to_rfc3339()),
        said: String::new(),
        ms: None,
        calls: Vec::new(),
    };
    let told = Entry {
        at: t0 + chrono::Duration::seconds(15),
        ..entry(9, "reported", json!({"id": "w1", "text": "half done"}))
    };
    let steps = [step(1, Some(5)), step(2, None), step(3, Some(20))];
    let order: Vec<String> = cards::along(t0, &[&told], &steps)
        .into_iter()
        .map(|a| match a {
            cards::Along::Step(s) => format!("step {}", s.n),
            cards::Along::Event(e) => e.kind.clone(),
        })
        .collect();
    assert_eq!(order, ["step 1", "step 2", "reported", "step 3"]);
}

#[test]
fn a_line_taken_back_is_out_of_the_thread_with_everything_said_after_it() {
    let heard = |seq, text: &str| {
        entry(
            seq,
            "heard",
            json!({
                "channel": "console", "place": {"id": "s1", "kind": "direct"},
                "from": {"id": "fox", "name": "Fox"}, "id": format!("m{seq}"), "text": text,
            }),
        )
    };
    let said = |seq, text: &str| {
        entry(
            seq,
            "said",
            json!({"channel": "console", "place": "s1", "text": text}),
        )
    };
    let journal = vec![
        heard(1, "one"),
        said(2, "two"),
        heard(3, "three"),
        said(4, "four"),
        entry(
            5,
            "rewound",
            json!({"place": "console:s1", "by": "console:fox", "to": 3}),
        ),
        heard(6, "five"),
    ];
    let place = place_key("s1");
    let view = View {
        journal: &journal,
        pending: &[],
        stops: &[],
        place: &place,
        name: "Ghost".into(),
        who: "Fox".into(),
        tz: chrono_tz::UTC,
        connected: true,
        abilities: None,
        expanded: &std::collections::HashSet::default(),
        following: &NOTHING_READ,
        work: &[],
        whom: crate::answer::Whom::Solver,
        scroll: None,
        rendering: Rendering::Task,
        column: COLUMN,
        media: None,
        status: None,
    };
    let texts: Vec<String> = items(&view)
        .into_iter()
        .filter_map(|(_, _, item)| match item {
            Item::Said { m, .. } => Some(m.text),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["one", "two", "five"]);
    assert_eq!(taken_back(&journal, &place), [(3, 5)]);
}

#[test]
fn a_works_brief_shows_without_the_words_it_was_handed_and_its_title_is_its_first_line() {
    let brief = "统计每个 crate 的行数，排成表。\n\n步骤：\n1. 列出目录\n\nWhat Fox wrote, word for word:\n> 帮我看看";
    let journal = vec![entry(1, "undertook", json!({"id": "w", "brief": brief}))];
    let t = &tasks(&journal)[0];
    assert_eq!(
        t.brief_shown(),
        "统计每个 crate 的行数，排成表。\n\n步骤：\n1. 列出目录"
    );
    assert_eq!(t.title(), "统计每个 crate 的行数，排成表。");
    let plain = vec![entry(
        1,
        "undertook",
        json!({"id": "w", "brief": "  tidy up  "}),
    )];
    assert_eq!(tasks(&plain)[0].brief_shown(), "tidy up");
}

/// Something heard from you, or said by her, on the console in `s1`, `m`
/// minutes into the last hour.
fn console(seq: i64, m: i64, kind: &str, text: &str) -> Entry {
    let data = if kind == "heard" {
        json!({"channel": "console", "place": {"id": "s1", "kind": "direct"},
        "from": {"id": "fox", "name": "Fox"}, "text": text})
    } else {
        json!({"channel": "console", "place": "s1", "text": text})
    };
    Entry {
        at: Utc::now() - chrono::Duration::minutes(60 - m),
        ..entry(seq, kind, data)
    }
}

#[test]
fn a_line_sent_while_she_works_waits_to_be_taken_until_what_she_was_at_ends() {
    let expanded = HashSet::new();
    let place = place_key("s1");
    let mut journal = vec![
        console(1, 1, "heard", "tidy up"),
        console(2, 2, "heard", "and the caches"),
    ];
    let waiting = |journal: &[Entry]| {
        let items = items(&view(journal, &[], &place, &expanded));
        sent_while_working(&items, None)
            .into_iter()
            .filter_map(|i| match &items[i].2 {
                Item::Said { m, .. } => Some(m.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // The first line is what she is at; the second came while she was.
    assert_eq!(waiting(&journal), ["and the caches"]);
    // Once she has answered the first, she takes the second up.
    journal.push(console(3, 3, "said", "done"));
    assert!(waiting(&journal).is_empty());
    // A line of yours with nothing going on is only a line.
    journal.push(console(4, 4, "heard", "thanks"));
    assert!(waiting(&journal).is_empty());
    // One sent after a look she is in the middle of began waits for it.
    let items = items(&view(&journal, &[], &place, &expanded));
    let began = Utc::now() - chrono::Duration::minutes(57);
    assert_eq!(sent_while_working(&items, Some(began)), [items.len() - 1]);
}

#[test]
fn a_change_of_brain_shows_once_between_turns_from_the_first_brain_to_the_last() {
    let expanded = HashSet::new();
    let place = place_key("s1");
    let brain = |seq, m: i64, data: Value| Entry {
        at: Utc::now() - chrono::Duration::minutes(60 - m),
        ..entry(seq, "brain_changed", data)
    };
    let journal = vec![
        // The brain she was first started on is no change.
        brain(1, 0, json!({"think": "openai/deepseek-v4"})),
        // Nor is one before anything was said in this thread.
        brain(2, 0, json!({"think": "a/one", "was": "openai/deepseek-v4"})),
        console(3, 1, "heard", "hello"),
        brain(4, 2, json!({"think": "a/two", "was": "a/one"})),
        brain(5, 3, json!({"think": "a/three", "was": "a/two"})),
        console(6, 4, "said", "hi"),
    ];
    let brains: Vec<(String, String)> = items(&view(&journal, &[], &place, &expanded))
        .into_iter()
        .filter_map(|(_, _, item)| match item {
            Item::Brain { think, was } => Some((think, was)),
            _ => None,
        })
        .collect();
    assert_eq!(brains, [("a/three".to_owned(), "a/one".to_owned())]);
}

#[test]
fn her_note_of_an_earlier_conversation_is_in_that_thread_and_no_other() {
    let expanded = HashSet::new();
    let journal = vec![
        console(1, 1, "heard", "hello"),
        entry(
            2,
            "digested",
            json!({"place": "console:s1", "upto": 1, "text": "Fox said hello."}),
        ),
    ];
    let folded = |place: &str| {
        items(&view(&journal, &[], place, &expanded))
            .into_iter()
            .filter_map(|(_, _, item)| match item {
                Item::Folded { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(folded(&place_key("s1")), ["Fox said hello."]);
    assert!(folded(&place_key("s2")).is_empty());
}
