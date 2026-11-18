//! Parts of the work handed out to helpers: how they run alongside their
//! parent, what they say to it, and how they end.

use super::*;

#[tokio::test]
async fn delegate_returns_at_once_and_the_parent_works_on_while_its_part_runs() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "count the lines of a.txt"})),
        // The part is under way now, held in its first step.
        calls("wait", json!({"seconds": 0.2})),
        calls("note", json!({"text": "the part is counting"})),
        calls("release", json!({})),
        calls("wait", json!({"seconds": 10})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "a.txt has one line"}),
        ),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "count the lines",
        vec![
            calls("hold", json!({})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "it has one line"}),
            ),
        ],
    );
    let (db, watched) = (state_db(dir.path()), brain.clone());
    with_parts(
        &mut worker,
        dir.path(),
        Some(Box::new(move || {
            // The parent has waited and noted while its part, begun
            // before, is in the middle of its first step.
            let part = Undertaking::open(&db, "main/1", None).unwrap();
            assert!(part.state().finish.is_none(), "the part had finished");
            assert_eq!(watched.turns_of("count the lines").len(), 1);
        })),
    );
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Finished);

    let turns = brain.turns_of(PARENT);
    // What came back at once was its number, and nothing of its work.
    let started = result_of(&turns[1], "delegate");
    assert_eq!(started, "Part 1 has started on it, alongside you.");
    // Its report reached the parent between steps, after the wait.
    let last = turns.last().unwrap();
    assert_eq!(result_of(last, "wait"), "Something arrived. It follows.");
    assert!(matches!(last.items.last(), Some(Item::User { text })
        if text.starts_with("Part 1 finished after 2 steps.") && text.ends_with("it has one line")));
    assert_eq!(
        part_record(dir.path(), 1).state().finish.unwrap().outcome,
        Outcome::Finished
    );
}

#[tokio::test]
async fn what_a_part_tells_reaches_its_parents_transcript_before_its_next_step_and_not_her() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "find the opening hours"})),
        calls("wait", json!({"seconds": 10})),
        calls("release", json!({})),
        calls("wait", json!({"seconds": 10})),
        calls("done", json!({"outcome": "finished", "summary": "nine"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "opening hours",
        vec![
            calls(
                "tell",
                json!({"text": "two sources say nine", "files": ["/w/hours.md"]}),
            ),
            calls("hold", json!({})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "nine on weekdays"}),
            ),
        ],
    );
    with_parts(&mut worker, dir.path(), Some(Box::new(|| {})));
    assert_eq!(worker.run().await.ended, Ended::Finished);

    let turns = brain.turns_of(PARENT);
    // The step after the first wait: the message is there, as a message
    // of its own that says whom it is from, and the part goes on.
    assert_eq!(
        result_of(&turns[2], "wait"),
        "Something arrived. It follows."
    );
    assert_eq!(
        messages(&turns[2]),
        [
            "A message from part 1, which goes on working:\n\ntwo sources say nine\n\nFiles it made: /w/hours.md"
        ]
    );
    // It is not something added to the task, and it is not what the
    // work tells whoever it is for: that is the parent's alone.
    let state = worker.undertaking.state();
    assert!(state.additions.is_empty());
    assert!(state.told.is_empty());
    assert!(
        messages(turns.last().unwrap())
            .last()
            .is_some_and(|m| m.starts_with("Part 1 finished"))
    );
}

#[tokio::test]
async fn wait_returns_when_a_part_ends_and_times_out_while_nothing_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "check the links"})),
        calls("wait", json!({"seconds": 0.3})),
        calls("release", json!({})),
        calls("wait", json!({"seconds": 30})),
        // Nothing is left running.
        calls("wait", json!({"seconds": 30})),
        calls("done", json!({"outcome": "finished", "summary": "checked"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "check the links",
        vec![
            calls("hold", json!({})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "all good"}),
            ),
        ],
    );
    with_parts(&mut worker, dir.path(), Some(Box::new(|| {})));
    let began = Instant::now();
    assert_eq!(worker.run().await.ended, Ended::Finished);
    // Neither of the long waits ran its course.
    assert!(began.elapsed().as_secs() < 10, "{:?}", began.elapsed());

    let turns = brain.turns_of(PARENT);
    assert_eq!(
        result_of(&turns[2], "wait"),
        "Nothing arrived in 0.3 seconds."
    );
    assert!(messages(&turns[2]).is_empty());
    assert_eq!(
        result_of(&turns[4], "wait"),
        "Something arrived. It follows."
    );
    assert!(result_of(&turns[5], "wait").starts_with("No part is running or queued"));
}

#[tokio::test]
async fn waiting_again_on_a_part_still_at_it_is_not_taken_for_being_stuck() {
    let dir = tempfile::tempdir().unwrap();
    let mut script = vec![calls("delegate", json!({"task": "build the docs"}))];
    script.extend((0..4).map(|_| calls("wait", json!({"seconds": 0.05}))));
    script.extend([
        calls("release", json!({})),
        calls("wait", json!({"seconds": 10})),
        calls("done", json!({"outcome": "finished", "summary": "built"})),
    ]);
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "build the docs",
        vec![
            calls("hold", json!({})),
            calls("done", json!({"outcome": "finished", "summary": "built"})),
        ],
    );
    with_parts(&mut worker, dir.path(), Some(Box::new(|| {})));
    assert_eq!(worker.run().await.ended, Ended::Finished);
    let last = brain.turns_of(PARENT).pop().unwrap();
    let timed_out = last
        .items
        .iter()
        .filter(|item| matches!(item, Item::Result { output, .. } if output.starts_with("Nothing arrived")))
        .count();
    assert_eq!(timed_out, 4);
    assert!(
        !messages(&last)
            .iter()
            .any(|m| m.contains("same call three times"))
    );
}

#[tokio::test]
async fn steering_a_running_part_reaches_it_before_its_next_step() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "paint the gate"})),
        calls("wait", json!({"seconds": 0.2})),
        calls("steer", json!({"part": 1, "text": "use the blue paint"})),
        calls("release", json!({})),
        calls("wait", json!({"seconds": 10})),
        calls("done", json!({"outcome": "finished", "summary": "blue"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "paint the gate",
        vec![
            calls("hold", json!({})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "painted blue"}),
            ),
        ],
    );
    with_parts(&mut worker, dir.path(), Some(Box::new(|| {})));
    assert_eq!(worker.run().await.ended, Ended::Finished);

    let parent = brain.turns_of(PARENT);
    assert_eq!(
        result_of(&parent[3], "steer"),
        "Passed on to part 1. It reads it before its next step."
    );
    let part = brain.turns_of("paint the gate");
    assert_eq!(part.len(), 2);
    // Begun before it was steered, and told before the step after.
    assert!(!part[0].items.iter().any(|item| matches!(item,
        Item::User { text } if text.contains("blue"))));
    assert!(matches!(part[1].items.last(), Some(Item::User { text })
        if text.contains("use the blue paint") && text.contains("does not replace it")));
    // The parent's own task is unchanged.
    assert!(worker.undertaking.state().additions.is_empty());
}

#[tokio::test]
async fn a_stopped_part_ends_as_dropped_after_the_step_it_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "write the long report"})),
        calls("wait", json!({"seconds": 0.2})),
        calls("stop", json!({"part": "part 1"})),
        calls("release", json!({})),
        calls("wait", json!({"seconds": 10})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "not needed"}),
        ),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "write the long report",
        vec![
            calls("hold", json!({})),
            calls("write", json!({"path": "late.txt", "content": "too late"})),
        ],
    );
    with_parts(&mut worker, dir.path(), Some(Box::new(|| {})));
    assert_eq!(worker.run().await.ended, Ended::Finished);

    let state = part_record(dir.path(), 1).state();
    assert_eq!(state.finish.unwrap().outcome, Outcome::Dropped);
    assert_eq!(state.steps, 1, "nothing after the step it was on");
    assert!(!dir.path().join("late.txt").exists());
    assert_eq!(brain.turns_of("write the long report").len(), 1);
    let last = brain.turns_of(PARENT).pop().unwrap();
    assert!(
        messages(&last)
            .iter()
            .any(|m| m.starts_with("Part 1 was stopped after 1 steps."))
    );
}

#[tokio::test]
async fn parts_beyond_the_cap_queue_and_start_when_one_ends() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        both(
            calls("delegate", json!({"task": "count the files"})),
            calls("delegate", json!({"task": "name the largest file"})),
        ),
        calls("parts", json!({})),
        calls("release", json!({})),
        calls("wait", json!({"seconds": 10})),
        calls("wait", json!({"seconds": 10})),
        calls("done", json!({"outcome": "finished", "summary": "both"})),
    ];
    let limits = Limits {
        parts: 1,
        ..Limits::default()
    };
    let (mut worker, brain) = worker(dir.path(), script, limits);
    brain.part(
        "count the files",
        vec![
            calls("hold", json!({})),
            calls("done", json!({"outcome": "finished", "summary": "three"})),
        ],
    );
    brain.part(
        "name the largest file",
        vec![calls(
            "done",
            json!({"outcome": "finished", "summary": "big.bin"}),
        )],
    );
    with_parts(&mut worker, dir.path(), Some(Box::new(|| {})));
    assert_eq!(worker.run().await.ended, Ended::Finished);

    let turns = brain.turns_of(PARENT);
    let answers: Vec<&str> = turns[1]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Result { output, .. } => Some(output.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        answers,
        [
            "Part 1 has started on it, alongside you.",
            "Part 2 is queued, because as many parts as may run at once (1) already are. It starts when one of them ends."
        ]
    );
    let listed = result_of(&turns[2], "parts");
    assert!(
        listed.contains("- part 1 (running, 0 steps), given \"count the files\": "),
        "{listed}"
    );
    assert!(
        listed.contains("- part 2 (queued, 0 steps), given \"name the largest file\": "),
        "{listed}"
    );
    // The second began only once the first had ended.
    let (_, first_ended) = brain.span_of("count the files");
    let (second_began, _) = brain.span_of("name the largest file");
    assert!(second_began > first_ended);
    for n in [1, 2] {
        assert_eq!(
            part_record(dir.path(), n).state().finish.unwrap().outcome,
            Outcome::Finished
        );
    }
}

#[tokio::test]
async fn an_unfinished_part_resumes_at_its_parents_next_sitting() {
    let dir = tempfile::tempdir().unwrap();
    // The first sitting hands out a part and ends while the part is in
    // the middle of a step.
    let script = vec![
        calls("delegate", json!({"task": "survey the old files"})),
        calls("wait", json!({"seconds": 0.2})),
    ];
    let limits = Limits {
        max_steps: 2,
        ..Limits::default()
    };
    let (mut first, brain) = worker(dir.path(), script, limits);
    brain.part(
        "survey the old files",
        vec![
            calls("note", json!({"text": "halfway through the survey"})),
            calls("shell", json!({"command": "sleep 1"})),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "too soon"}),
            ),
        ],
    );
    with_parts(&mut first, dir.path(), None);
    assert_eq!(first.run().await.ended, Ended::OutOfSteps);
    // It stopped after the step it was on, and has not ended.
    let state = part_record(dir.path(), 1).state();
    assert!(state.finish.is_none());
    assert_eq!(state.steps, 2);

    let script = vec![
        calls("wait", json!({"seconds": 10})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "surveyed"}),
        ),
    ];
    let (mut second, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "survey the old files",
        vec![calls(
            "done",
            json!({"outcome": "finished", "summary": "surveyed it all"}),
        )],
    );
    with_parts(&mut second, dir.path(), None);
    assert_eq!(second.run().await.ended, Ended::Finished);
    // It went on from where it was, not from the start.
    let part = brain.turns_of("survey the old files");
    assert_eq!(part.len(), 1);
    assert!(part[0].items.iter().any(|item| matches!(item,
        Item::Assistant { calls, .. } if calls.iter().any(|c| c.args["text"] == "halfway through the survey"))));
    let state = part_record(dir.path(), 1).state();
    assert_eq!(state.steps, 3);
    assert_eq!(state.finish.unwrap().summary, "surveyed it all");
    assert!(
        messages(brain.turns_of(PARENT).last().unwrap())
            .iter()
            .any(|m| m.starts_with("Part 1 finished after 3 steps."))
    );
}

#[tokio::test]
async fn the_work_is_not_called_finished_while_a_part_has_not_ended() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "check the spelling"})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "all of it"}),
        ),
        calls("wait", json!({"seconds": 10})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "all of it, checked"}),
        ),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "check the spelling",
        vec![calls(
            "done",
            json!({"outcome": "finished", "summary": "no mistakes"}),
        )],
    );
    with_parts(&mut worker, dir.path(), None);
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Finished);
    assert_eq!(report.summary, "all of it, checked");

    let turns = brain.turns_of(PARENT);
    let refused = turns[2]
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            Item::Result {
                name,
                output,
                failed,
                ..
            } if name == "done" => Some((output, *failed)),
            _ => None,
        })
        .unwrap();
    assert!(refused.1);
    assert!(
        refused
            .0
            .starts_with("Not recorded. These parts have not ended: 1."),
        "{}",
        refused.0
    );
    assert!(refused.0.contains("`wait`") && refused.0.contains("`stop`"));
}

#[tokio::test]
async fn a_part_handed_a_step_marks_it_done_with_the_first_line_of_its_report() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls(
            "plan",
            json!({"steps": [
                {"text": "count the lines of a.txt", "status": "todo",
                 "check": "wc -l a.txt"},
                {"text": "say how many", "status": "todo", "after": [1]},
            ]}),
        ),
        calls(
            "delegate",
            json!({"task": "count the lines of a.txt", "step": 1}),
        ),
        calls("wait", json!({"seconds": 10})),
        calls(
            "plan",
            json!({"steps": [
                {"text": "count the lines of a.txt", "status": "done"},
                {"text": "say how many", "status": "done"},
            ]}),
        ),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "one line"}),
        ),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "count the lines",
        vec![calls(
            "done",
            json!({"outcome": "finished", "summary": "wc -l a.txt says 1\nIt has one line."}),
        )],
    );
    with_parts(&mut worker, dir.path(), None);
    assert_eq!(worker.run().await.ended, Ended::Finished);

    let turns = brain.turns_of(PARENT);
    assert_eq!(
        result_of(&turns[2], "delegate"),
        "Part 1 has started on it, alongside you. It is on step 1 of your plan, which is \
         marked done with its report when it finishes, and failed if it does not."
    );
    // The whole report still reaches the parent, and says what became
    // of the step.
    let report = messages(&turns[3])
        .into_iter()
        .find(|m| m.starts_with("Part 1 finished"))
        .unwrap();
    assert!(
        report.contains("wc -l a.txt says 1\nIt has one line."),
        "{report}"
    );
    assert!(report.ends_with("Step 1 of your plan, which it was given, is now marked done, with the first line of this report as its result."), "{report}");
    // Marked done by the part's report, it kept that result when the
    // plan was written again without one.
    let step = &worker.undertaking.state().plan[0];
    assert_eq!(step.status, Progress::Done);
    assert_eq!(step.part, Some(1));
    assert_eq!(
        step.result.as_deref(),
        Some("Part 1 finished: wc -l a.txt says 1")
    );
}

#[tokio::test]
async fn a_part_that_gives_up_on_its_step_marks_it_failed_and_a_step_not_ready_is_not_handed_out() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls(
            "plan",
            json!({"steps": ["find the mirror", {"text": "fetch from it", "status": "todo", "after": [1]}]}),
        ),
        calls(
            "delegate",
            json!({"task": "fetch from the mirror", "step": 2}),
        ),
        calls(
            "delegate",
            json!({"task": "find the mirror", "step": "step 1"}),
        ),
        calls("wait", json!({"seconds": 10})),
        calls("done", json!({"outcome": "finished", "summary": "no"})),
        calls(
            "done",
            json!({"outcome": "gave_up", "summary": "no mirror"}),
        ),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "find the mirror",
        vec![calls(
            "done",
            json!({"outcome": "gave_up", "summary": "Every mirror listed is gone.\nTried three."}),
        )],
    );
    with_parts(&mut worker, dir.path(), None);
    assert_eq!(worker.run().await.ended, Ended::GaveUp);

    let turns = brain.turns_of(PARENT);
    assert_eq!(
        result_of(&turns[2], "delegate"),
        "Step 2 cannot be started yet: it is after steps that are not done (1)."
    );
    // Only the part that could start was handed out.
    assert_eq!(worker.undertaking.parts().len(), 1);
    let step = &worker.undertaking.state().plan[0];
    assert_eq!(step.status, Progress::Failed);
    assert_eq!(
        step.result.as_deref(),
        Some("Part 1 gave up: Every mirror listed is gone.")
    );
    // A failed step is open: the work is not called finished over it.
    assert!(result_of(&turns[5], "done").starts_with(
        "Not recorded. These steps of the plan are neither done nor dropped:\n[!] 1. find the mirror"
    ));
}

#[tokio::test]
async fn giving_the_work_up_ends_its_parts_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "rebuild the index"})),
        calls("wait", json!({"seconds": 0.2})),
        calls(
            "done",
            json!({"outcome": "gave_up", "summary": "the disk is gone"}),
        ),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "rebuild the index",
        vec![
            calls("shell", json!({"command": "sleep 0.5"})),
            calls("write", json!({"path": "late.txt", "content": "too late"})),
        ],
    );
    with_parts(&mut worker, dir.path(), None);
    assert_eq!(worker.run().await.ended, Ended::GaveUp);
    let state = part_record(dir.path(), 1).state();
    assert_eq!(state.finish.unwrap().outcome, Outcome::Dropped);
    assert!(!dir.path().join("late.txt").exists());
}

#[tokio::test]
async fn what_a_part_spends_is_counted_in_its_parents_totals() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "list the tests"})),
        calls("wait", json!({"seconds": 10})),
        calls("done", json!({"outcome": "finished", "summary": "listed"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "list the tests",
        vec![
            calls("note", json!({"text": "there are twelve"})),
            calls("done", json!({"outcome": "finished", "summary": "twelve"})),
        ],
    );
    with_parts(&mut worker, dir.path(), None);
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Finished);
    assert_eq!(report.steps, 3, "the part's steps are its own");

    let own: u64 = worker
        .undertaking
        .logged()
        .iter()
        .map(|line| line["read"].as_u64().unwrap())
        .sum();
    let part = part_record(dir.path(), 1).state().usage;
    assert!(part.input >= 200 && part.output == 20);
    assert_eq!(report.usage.input, own + part.input);
    assert_eq!(report.usage.output, 30 + part.output);
}

#[tokio::test]
async fn a_part_starts_from_its_task_alone_and_only_its_report_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "needle-in-a-file").unwrap();
    let script = vec![
        calls("delegate", json!({"task": "say what is in a.txt"})),
        calls("wait", json!({"seconds": 10})),
        calls("done", json!({"outcome": "blocked", "summary": "enough"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "say what is in a.txt",
        vec![
            calls("shell", json!({"command": "cat a.txt"})),
            calls(
                "done",
                json!({"outcome": "gave_up", "summary": "it holds one word about a needle"}),
            ),
        ],
    );
    with_parts(&mut worker, dir.path(), None);
    assert_eq!(worker.run().await.ended, Ended::Blocked);

    let part = brain.turns_of("say what is in a.txt");
    // It started from the task it was given and nothing else, knowing
    // it is a part, and with nothing to hand out parts of its own.
    assert!(matches!(&part[0].items[..],
        [Item::User { text }] if text.contains("say what is in a.txt")));
    assert!(
        part[0]
            .system
            .contains("You are doing one part of a larger piece of work.")
    );
    let names: Vec<&str> = part[0].tools.iter().map(|t| t.name.as_str()).collect();
    for name in ["delegate", "wait", "parts", "steer", "stop"] {
        assert!(!names.contains(&name), "a part has {name}");
    }
    assert!(names.contains(&"tell") && names.contains(&"ask"));
    // What it read stayed with it. Its report came back.
    let last = brain.turns_of(PARENT).pop().unwrap();
    assert!(
        messages(&last)
            .iter()
            .any(|m| m.starts_with("Part 1 gave up after 2 steps.")
                && m.contains("one word about a needle"))
    );
    assert!(!last.items.iter().any(|item| matches!(item,
        Item::Result { output, .. } if output.contains("needle-in-a-file"))));
    assert_eq!(
        part_record(dir.path(), 1).state().brief,
        "say what is in a.txt"
    );
}

#[tokio::test]
async fn a_part_handed_out_in_a_later_sitting_is_new_work_not_one_already_finished() {
    let dir = tempfile::tempdir().unwrap();
    // The first sitting hands out one part, which finishes, and then
    // runs out of steps.
    let script = vec![
        calls("delegate", json!({"task": "count the files"})),
        calls("wait", json!({"seconds": 10})),
    ];
    let limits = Limits {
        max_steps: 2,
        ..Limits::default()
    };
    let (mut first, brain) = worker(dir.path(), script, limits);
    brain.part(
        "count the files",
        vec![calls(
            "done",
            json!({"outcome": "finished", "summary": "there are three files"}),
        )],
    );
    with_parts(&mut first, dir.path(), None);
    assert_eq!(first.run().await.ended, Ended::OutOfSteps);

    // The second sitting hands out another.
    let script = vec![
        calls("delegate", json!({"task": "name the largest file"})),
        calls("wait", json!({"seconds": 10})),
        calls("done", json!({"outcome": "blocked", "summary": "enough"})),
    ];
    let (mut second, brain) = worker(dir.path(), script, Limits::default());
    brain.part(
        "name the largest file",
        vec![calls(
            "done",
            json!({"outcome": "finished", "summary": "the largest is big.bin"}),
        )],
    );
    with_parts(&mut second, dir.path(), None);
    assert_eq!(second.run().await.ended, Ended::Blocked);

    let turns = brain.turns_of(PARENT);
    assert_eq!(
        result_of(&turns[1], "delegate"),
        "Part 2 has started on it, alongside you."
    );
    // The new part did work: it was asked, and it was asked the new task.
    assert_eq!(brain.turns_of("name the largest file").len(), 1);
    assert!(brain.turns_of("count the files").is_empty());
    let reports: Vec<&str> = messages(turns.last().unwrap())
        .into_iter()
        .filter(|m| m.starts_with("Part "))
        .collect();
    assert!(
        reports
            .iter()
            .any(|m| m.starts_with("Part 1 finished") && m.contains("three files"))
    );
    assert!(
        reports
            .iter()
            .any(|m| m.starts_with("Part 2 finished") && m.contains("big.bin"))
    );
    // Both are on record, each under its own id.
    assert_eq!(part_record(dir.path(), 1).state().brief, "count the files");
    assert_eq!(
        part_record(dir.path(), 2).state().brief,
        "name the largest file"
    );
}

/// An installed agent standing in for Claude Code: a script in `dir` that
/// writes each command line it was run with to `runs.txt` there, says it
/// started session `s1`, runs one command and answers. Resumed, it answers
/// with what it was given.
fn fake_claude(dir: &std::path::Path) -> crate::agents::Agent {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("claude");
    let runs = dir.join("runs.txt");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
echo "$@" >> '{runs}'
echo '{{"type":"system","subtype":"init","session_id":"s1"}}'
echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"make test"}}}}]}}}}'
case "$*" in
  *--resume*) echo '{{"type":"result","result":"Also did: '"$2"'","is_error":false,"usage":{{}}}}' ;;
  *) echo '{{"type":"result","result":"Tests pass: 12.","is_error":false,"usage":{{}}}}' ;;
esac
"#,
            runs = runs.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    crate::agents::Agent {
        name: "claude-code".into(),
        cli: ghost_brain::CliAgent {
            kind: ghost_brain::CliKind::Claude,
            program: script.display().to_string(),
            model: None,
            args: Vec::new(),
        },
        dir: dir.to_path_buf(),
    }
}

#[tokio::test]
async fn a_part_handed_to_an_installed_agent_is_done_by_it_step_by_step_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls(
            "delegate",
            json!({"task": "make the tests pass", "agent": "claude-code"}),
        ),
        calls("wait", json!({"seconds": 10})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "the tests pass"}),
        ),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    with_parts(&mut worker, dir.path(), None);
    worker.agents = vec![fake_claude(dir.path())];
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Finished);

    // The agent is offered by name, and only the one there is.
    let turns = brain.turns_of(PARENT);
    let delegate = turns[0]
        .tools
        .iter()
        .find(|t| t.name == "delegate")
        .unwrap();
    assert_eq!(
        delegate.parameters["properties"]["agent"]["enum"],
        json!(["claude-code"])
    );
    // It was given the task, and its answer is the part's report.
    let runs = std::fs::read_to_string(dir.path().join("runs.txt")).unwrap();
    assert!(runs.contains("make the tests pass"), "{runs}");
    let part = part_record(dir.path(), 1);
    let state = part.state();
    assert_eq!(state.agent.as_deref(), Some("claude-code"));
    assert_eq!(state.session.as_deref(), Some("s1"));
    assert_eq!(state.finish.unwrap().summary, "Tests pass: 12.");
    assert!(
        messages(turns.last().unwrap())
            .iter()
            .any(|m| m.starts_with("Part 1 finished") && m.ends_with("Tests pass: 12."))
    );
    // What it did is the part's steps, for whoever follows the work.
    let logged = part.logged();
    assert_eq!(logged[0]["calls"][0]["name"], "Bash");
    assert_eq!(logged[0]["calls"][0]["args"]["command"], "make test");
}

#[tokio::test]
async fn what_is_passed_on_to_an_agents_part_is_given_to_it_in_the_same_session() {
    let dir = tempfile::tempdir().unwrap();
    let parent = Undertaking::open(&state_db(dir.path()), "main", Some("build it")).unwrap();
    let part = parent.open_part("make the tests pass").unwrap();
    // Passed on before it next stops.
    part.add("and update the changelog").unwrap();
    let agent = fake_claude(dir.path());
    let ended = crate::agents::run(
        &agent,
        &part,
        "make the tests pass",
        "go on",
        &AtomicBool::new(false),
        std::time::Instant::now() + std::time::Duration::from_secs(30),
    )
    .await;
    assert_eq!(ended, Ok(Ended::Finished));
    let runs = std::fs::read_to_string(dir.path().join("runs.txt")).unwrap();
    let runs: Vec<&str> = runs.lines().collect();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(runs[1].contains("--resume s1") && runs[1].contains("and update the changelog"));
    assert_eq!(
        part.state().finish.unwrap().summary,
        "Also did: and update the changelog"
    );
}

#[tokio::test]
async fn a_part_cannot_be_handed_to_an_agent_that_is_not_here() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls("delegate", json!({"task": "fix it", "agent": "codex"})),
        calls("done", json!({"outcome": "gave_up", "summary": "no"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    with_parts(&mut worker, dir.path(), None);
    worker.agents = vec![fake_claude(dir.path())];
    worker.run().await;
    let turns = brain.turns_of(PARENT);
    assert_eq!(
        result_of(&turns[1], "delegate"),
        "There is no agent called codex here."
    );
    assert!(Undertaking::open(&state_db(dir.path()), "main/1", None).is_err());
}

/// An agent standing in for Claude Code that is not logged in: it answers
/// every run with the error Claude Code gives then.
fn logged_out_claude(dir: &std::path::Path) -> crate::agents::Agent {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("claude-out");
    std::fs::write(
        &script,
        r#"#!/bin/sh
echo '{"type":"system","subtype":"init","session_id":"s9"}'
echo '{"type":"result","result":"API Error: 401 {\"type\":\"error\",\"error\":{\"type\":\"authentication_error\"}} · Please run /login","is_error":true,"usage":{}}'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    crate::agents::Agent {
        name: "claude-code".into(),
        cli: ghost_brain::CliAgent {
            kind: ghost_brain::CliKind::Claude,
            program: script.display().to_string(),
            model: None,
            args: Vec::new(),
        },
        dir: dir.to_path_buf(),
    }
}

#[tokio::test]
async fn an_agent_that_is_not_logged_in_is_reported_with_the_fix_and_handed_nothing_more() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![
        calls(
            "delegate",
            json!({"task": "write it", "agent": "claude-code"}),
        ),
        calls("wait", json!({"seconds": 10})),
        calls(
            "delegate",
            json!({"task": "write it again", "agent": "claude-code"}),
        ),
        calls("done", json!({"outcome": "blocked", "summary": "no agent"})),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    with_parts(&mut worker, dir.path(), None);
    worker.agents = vec![logged_out_claude(dir.path())];
    worker.run().await;
    let turns = brain.turns_of(PARENT);
    // Its report says what is wrong and the command that fixes it, and
    // that the fix is not the work's to make.
    let report = messages(&turns[2])
        .into_iter()
        .find(|m| m.starts_with("Part 1 failed"))
        .unwrap()
        .to_owned();
    assert!(report.contains("Claude Code is not logged in"), "{report}");
    assert!(report.contains("claude setup-token"), "{report}");
    assert!(
        report.contains("do not install, upgrade or log in"),
        "{report}"
    );
    // Nothing more is handed to it in this sitting.
    let again = result_of(&turns[3], "delegate");
    assert!(again.contains("Claude Code is not logged in"), "{again}");
    assert!(Undertaking::open(&state_db(dir.path()), "main/2", None).is_err());
}

#[tokio::test]
async fn a_part_of_work_in_a_repository_works_in_a_checkout_of_its_own_brought_back_when_it_finishes()
 {
    let dir = tempfile::tempdir().unwrap();
    // The work's folder: a repository, with a file not committed yet.
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&folder)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(folder.join("notes.txt"), "from the work\n").unwrap();
    let stray = folder.join("stray.txt").display().to_string();
    let script = vec![
        calls("delegate", json!({"task": "write the summary"})),
        calls("wait", json!({"seconds": 10})),
        calls(
            "done",
            json!({"outcome": "finished", "summary": "summarised"}),
        ),
    ];
    let (mut worker, brain) = worker(dir.path(), script, Limits::default());
    worker
        .undertaking
        .update(|s| s.dir = Some(folder.display().to_string()));
    brain.part(
        "write the summary",
        vec![
            // Into the work's folder: refused.
            calls("write", json!({"path": stray, "content": "x"})),
            // Where it is: its own checkout, which has the work's file.
            calls("shell", json!({"command": "cat notes.txt"})),
            calls(
                "write",
                json!({"path": "summary.md", "content": "# Summary\n"}),
            ),
            calls(
                "done",
                json!({"outcome": "finished", "summary": "wrote summary.md"}),
            ),
        ],
    );
    // Each worker's machine is in its own folder, as her mind gives it.
    let (root, words) = (dir.path().to_path_buf(), worker.prompts.clone());
    worker.helpers = Some(Arc::new(move |undertaking: &Arc<Undertaking>| {
        let base = Local::new(&root);
        let machine: Arc<dyn crate::Machine> = match undertaking.state().dir {
            Some(d) => crate::Machine::at(&base, std::path::Path::new(&d)).unwrap(),
            None => Arc::new(base),
        };
        tools::standard(machine, undertaking, &words)
    }));
    let report = worker.run().await;
    assert_eq!(report.ended, Ended::Finished);

    let part = brain.turns_of("write the summary");
    assert!(
        result_of(&part[1], "write").contains("is in the work's folder"),
        "{}",
        result_of(&part[1], "write")
    );
    assert!(result_of(&part[2], "shell").contains("\nfrom the work\n"));
    // What it made is in the work's folder, and nothing else of it is.
    assert_eq!(
        std::fs::read_to_string(folder.join("summary.md")).unwrap(),
        "# Summary\n"
    );
    assert!(!folder.join("stray.txt").exists());
    assert!(!dir.path().join("worktrees").join("main-1").exists());
    let told = brain.turns_of(PARENT);
    let said = told.last().unwrap().items.iter().any(|item| {
        matches!(item, Item::User { text }
            if text.contains("What it changed is now in your folder: summary.md."))
    });
    assert!(said, "its report says what came back");
}
