//! Keeping the transcript within the window: folding, handoffs, and the
//! budget it is folded at.

use super::*;

#[tokio::test]
async fn a_full_context_is_folded_into_a_handoff() {
    let dir = tempfile::tempdir().unwrap();
    let big = "x".repeat(9000);
    // Enough output to pass the ceiling, which is set above what the
    // instructions and the tools already take.
    let mut script: Vec<Step> = (0..3)
        .map(|i| calls("shell", json!({"command": format!("echo {big}{i}")})))
        .collect();
    script.push(calls(
        "done",
        json!({"outcome": "blocked", "summary": "enough"}),
    ));
    let limits = Limits {
        context_tokens: Some(8000),
        ..Limits::default()
    };
    let (worker, brain) = worker(dir.path(), script, limits);
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Blocked);
    assert_eq!(report.sittings, 2);

    let seen = brain.seen.lock().unwrap();
    let at = seen
        .iter()
        .position(|t| matches!(t.items.last(), Some(Item::User { text }) if text.contains("about to be cleared")))
        .expect("a handoff was asked for");
    // Asked with the tools of every other step, and with the transcript
    // as it was: nothing in front of the request has changed, so the
    // provider reads it from its cache.
    let names = |t: &Turn| t.tools.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&seen[at]), names(&seen[0]));
    assert!(seen[at].items.starts_with(&seen[at - 1].items));
    // What it had seen last was too large to carry over, so the sitting
    // after it starts from the record alone.
    let fresh = &seen[at + 1];
    assert_eq!(fresh.items.len(), 1);
    assert!(matches!(&fresh.items[0], Item::User { text }
        if text.contains("the file goes in hi.txt") && text.contains("Do not start over")));
    // Work that has outlived a context is told how to finish when it
    // is picked up, not caught at the end.
    assert!(matches!(&fresh.items[0], Item::User { text }
        if text.contains("treat that as unproven")));
}

#[tokio::test]
async fn the_last_stretch_survives_a_cleared_context() {
    let dir = tempfile::tempdir().unwrap();
    // Many small steps: nothing in them is long enough to trim or drop.
    let line = "y".repeat(300);
    let mut script: Vec<Step> = (0..60)
        .map(|i| calls("shell", json!({"command": format!("echo {i} {line}")})))
        .collect();
    script.push(calls(
        "done",
        json!({"outcome": "blocked", "summary": "enough"}),
    ));
    let limits = Limits {
        context_tokens: Some(8000),
        max_steps: 80,
        ..Limits::default()
    };
    let (worker, brain) = worker(dir.path(), script, limits);
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Blocked);
    assert!(report.sittings >= 2);

    let seen = brain.seen.lock().unwrap();
    let at = seen
        .iter()
        .position(|t| matches!(t.items.last(), Some(Item::User { text }) if text.contains("about to be cleared")))
        .expect("a handoff was asked for");
    let before = &seen[at].items[..seen[at].items.len() - 1];
    let fresh = &seen[at + 1].items;
    assert!(matches!(&fresh[0], Item::User { text }
        if text.contains("the file goes in hi.txt") && text.contains("exactly as it happened")));
    // After the opening comes the end of what it was doing, unchanged,
    // beginning with something it did and not with a result cut loose.
    let tail = &fresh[1..];
    assert!(tail.len() >= 4, "{} items carried over", tail.len());
    assert!(matches!(tail[0], Item::Assistant { .. }));
    assert_eq!(tail, &before[before.len() - tail.len()..]);
    // And it is a small part of what there was.
    assert!(tail.len() < before.len() / 2);

    // Nothing was lost: the history holds every step, in order.
    let commands = worker
        .undertaking
        .history()
        .iter()
        .filter(|item| matches!(item, Item::Assistant { calls, .. }
            if calls.iter().any(|c| c.args["command"].as_str().is_some_and(|c| c.starts_with("echo ")))))
        .count();
    assert_eq!(commands, 60);
}

#[test]
fn long_output_is_trimmed_before_anything_is_dropped() {
    let result = |output: String| Item::result("a", "shell", output, false);
    let mut items = vec![
        Item::user("task"),
        result(format!("START{}END", "z".repeat(30_000))),
        result("short".into()),
        result("w".repeat(30_000)),
    ];
    let freed = prune(&mut items, 1);
    assert!(freed > 20_000);
    let Item::Result { output, .. } = &items[1] else {
        panic!()
    };
    assert!(output.starts_with("START") && output.ends_with("END"));
    assert!(output.contains("cut from the middle"));
    assert!(output.chars().count() < PRUNE_OVER);
    assert!(matches!(&items[2], Item::Result { output, .. } if output == "short"));
    // The newest is what the worker is looking at: left whole.
    assert!(matches!(&items[3], Item::Result { output, .. } if output.len() == 30_000));
    // Trimming what is already trimmed changes nothing.
    assert_eq!(prune(&mut items, 1), 0);
}

#[tokio::test]
async fn the_budget_follows_the_window_and_stays_under_a_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, brain) = worker(dir.path(), vec![], Limits::default());
    assert_eq!(worker.budget(), UNKNOWN_WINDOW);
    brain.window.store(1_048_576, Ordering::Relaxed);
    assert_eq!(worker.budget(), 838_860);
    brain.window.store(128_000, Ordering::Relaxed);
    assert_eq!(worker.budget(), 102_400);
    // A small window keeps room to answer in.
    brain.window.store(32_000, Ordering::Relaxed);
    assert_eq!(worker.budget(), 32_000 - 8192 - 4000);
    // What a step may write is what the model is known to manage, up
    // to a ceiling; little, for a model nothing is known about.
    assert_eq!(worker.max_output(), 8192);
    brain.max_output.store(393_216, Ordering::Relaxed);
    assert_eq!(worker.max_output(), 32_000);
    brain.max_output.store(0, Ordering::Relaxed);
    worker.limits.context_tokens = Some(50_000);
    assert_eq!(worker.budget(), 50_000);
    worker
        .undertaking
        .update(|s| s.refused_at = Some(40_000))
        .unwrap();
    assert_eq!(worker.budget(), 28_000);
}

#[tokio::test]
async fn a_refused_context_is_folded_and_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let some = "v".repeat(3000);
    let mut script: Vec<Step> = (0..10)
        .map(|i| calls("shell", json!({"command": format!("echo {i} {some}")})))
        .collect();
    script.push(calls(
        "done",
        json!({"outcome": "blocked", "summary": "enough"}),
    ));
    // It thinks it has room for 60,000 tokens. The model takes 6,000.
    let (worker, brain) = worker(dir.path(), script, Limits::default());
    brain.refuses_over.store(6000, Ordering::Relaxed);
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Blocked);
    let refused = worker.undertaking.state().refused_at.expect("remembered");
    // (The scripted brain writes a long call and reports ten tokens for
    // it, so the worker's count runs a little under the brain's.)
    assert!(refused > 4000 && refused < 12_000, "{refused}");
    assert!(worker.budget() < 6000);
    // Every request it went on to make was one the model would take.
    assert_eq!(report.steps, 11);
}

#[tokio::test]
async fn the_newest_step_says_what_its_request_was_made_of_and_the_room_it_had() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("shell", json!({"command": "echo hello there"})),
        calls("done", json!({"outcome": "finished", "summary": "said"})),
    ];
    let (worker, brain) = worker(dir.path(), script, Limits::default());
    brain.window.store(200_000, Ordering::Relaxed);
    worker.run().await;

    let logged = crate::undertaking::steps(&state_db(dir.path()), "main", 0, 100)
        .unwrap()
        .unwrap();
    let context = logged.context.expect("the newest step says");
    assert_eq!(context.n, 2, "the second step, not the first");
    assert_eq!(context.window, Some(200_000));
    assert!(context.folds_at.is_some_and(|f| f < 200_000));
    let seen = brain.seen.lock().unwrap();
    let last = seen.last().unwrap();
    // The model read what it was shown: the provider's count comes back
    // with the parts it can be shared out among.
    assert_eq!(
        context.usage.input,
        (chars(&last.items) / 3).max(100) as u64
    );
    let part = |name: &str| context.parts.get(name).copied().unwrap_or(0);
    let n = |s: &str| s.chars().count() as u64;
    assert_eq!(part("system"), n(&last.system));
    assert!(part("tools") > 0);
    assert!(matches!(&last.items[0], Item::User { text } if part("task") == n(text)));
    assert!(part("messages") > n("echo hello there"), "what it called");
    assert!(part("results") >= n("hello there"), "what came back");
}
