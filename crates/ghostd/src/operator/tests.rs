//! A console's operator speaking to her, and a console following her work
//! and her looks step by step, over the real protocol on this machine's
//! loopback, with her mind real and her brain scripted.

use std::time::Duration;

use ghost_core::journal::{Entry, Event};
use ghost_link::{Conn, Console};
use ghost_protocol::{Capability, Hello, PROTOCOL, PresenceHello, Role, RpcError, SayMode};

use super::*;
use crate::cast::tests::{Rig, Spec};

#[test]
fn the_console_channel_is_the_mind_s_operators_channel() {
    assert_eq!(CONSOLE, ghost_mind::CONSOLE);
}

#[test]
fn a_console_without_a_token_is_the_owner_s_only_from_this_machine() {
    let here: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    let there: SocketAddr = "192.0.2.7:5000".parse().unwrap();
    assert!(owns(None, here) && owns(Some("t"), here));
    assert!(!owns(None, there));
    // Holding the token is what makes it the owner's, from anywhere.
    assert!(owns(Some("t"), there));
}

/// The next entry a subscribed console is sent that `pick` takes, within
/// a while.
async fn next_where<T>(stream: &mut Console, pick: impl Fn(Event) -> Option<T>) -> T {
    let wait = async {
        loop {
            let value = stream.next_entry().await.unwrap().expect("the daemon went");
            let entry: Entry = serde_json::from_value(value).unwrap();
            if let Some(found) = pick(entry.event) {
                return found;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("nothing like it came")
}

/// What she said on the console channel, by place, as the journal a
/// console streams has it.
fn said_on_console(event: Event) -> Option<(String, String)> {
    match event {
        Event::Said {
            channel,
            place,
            text,
            ..
        } if channel == CONSOLE => Some((place, text)),
        _ => None,
    }
}

fn code_of(e: &anyhow::Error) -> Option<i64> {
    e.downcast_ref::<RpcError>().map(|e| e.code)
}

#[tokio::test]
async fn an_operator_at_a_console_speaks_to_her_and_reads_her_answer_in_the_journal_it_streams() {
    let rig = Rig::run(vec![Spec::new("lin")]).await;
    let (mut stream, _) = Console::connect(&rig.url, None, None).await.unwrap();
    stream.subscribe().await.unwrap();
    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("Fox".into()))
        .await
        .unwrap();

    let said = fox
        .say(None, "how are the patients, 5e1d", SayMode::Say)
        .await
        .unwrap();
    assert_eq!(said.place, "console:fox");
    assert_eq!(
        next_where(&mut stream, said_on_console).await,
        ("fox".to_owned(), "lin here".to_owned())
    );
    // It is a conversation with an operator of hers, kept like any other.
    // A console is a terminal: she answers what is said there at once, by
    // what the place is, and nothing said there is a directive.
    let thread = serde_json::to_string(&fox.thread(&said.place, 10).await.unwrap()).unwrap();
    assert!(
        thread.contains("5e1d") && thread.contains(&said.id),
        "{thread}"
    );

    // Marked as a directive or not, in a place of its own, it is the same.
    let other = fox
        .say(
            Some("desk-1".into()),
            "send me the rota",
            SayMode::Directive,
        )
        .await
        .unwrap();
    assert_eq!(other.place, "console:desk-1");
    assert_eq!(
        next_where(&mut stream, said_on_console).await,
        ("desk-1".to_owned(), "lin here".to_owned())
    );
    let events = rig.events(0).await;
    assert!(
        !events.iter().any(|e| matches!(e, Event::Directed { .. })),
        "{events:?}"
    );
}

#[tokio::test]
async fn an_operator_takes_a_line_back_and_the_conversation_stands_as_it_did_before_it() {
    let rig = Rig::run(vec![Spec::new("lin")]).await;
    let (mut stream, _) = Console::connect(&rig.url, None, None).await.unwrap();
    stream.subscribe().await.unwrap();
    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("Fox".into()))
        .await
        .unwrap();
    let said = fox
        .say(None, "how are the patients, 7a2e", SayMode::Say)
        .await
        .unwrap();
    assert_eq!(
        next_where(&mut stream, said_on_console).await,
        ("fox".to_owned(), "lin here".to_owned())
    );
    let kinds = |thread: &[serde_json::Value]| -> Vec<String> {
        thread
            .iter()
            .map(|e| e["event"]["kind"].as_str().unwrap_or("?").to_owned())
            .collect()
    };
    let thread = fox.thread(&said.place, 20).await.unwrap();
    let line = thread
        .iter()
        .find(|e| e["event"]["kind"] == "heard")
        .and_then(|e| e["seq"].as_i64())
        .unwrap();
    assert!(kinds(&thread).contains(&"said".to_owned()));

    // Only a line of theirs there can be taken back.
    let e = fox.rewind_place(None, line + 1).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_SUCH_LINE));
    let e = fox
        .rewind_place(Some("desk-1".into()), line)
        .await
        .unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_SUCH_LINE));
    let (mut nobody, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let e = nobody.rewind_place(None, line).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));

    assert_eq!(fox.rewind_place(None, line).await.unwrap(), 2);
    let (place, to) = next_where(&mut stream, |event| match event {
        Event::Rewound { place, to, .. } => Some((place, to)),
        _ => None,
    })
    .await;
    assert_eq!((place.as_str(), to), ("console:fox", line));
    // The journal keeps every word, and says the line was taken back.
    let thread = fox.thread(&said.place, 20).await.unwrap();
    assert!(
        kinds(&thread).ends_with(&["rewound".to_owned()]),
        "{thread:?}"
    );
    let rewound = thread.last().unwrap();
    assert_eq!(rewound["event"]["data"]["to"].as_i64(), Some(line));
}

#[tokio::test]
async fn a_console_says_who_is_at_it_before_it_speaks_and_has_something_to_say() {
    let rig = Rig::run(vec![Spec::new("lin")]).await;
    let (mut nobody, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let e = nobody.say(None, "hello", SayMode::Say).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("fox".into()))
        .await
        .unwrap();
    let e = fox.say(None, "   ", SayMode::Say).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
    assert!(rig.heard(0).await.is_empty());
}

#[tokio::test]
async fn a_console_with_the_daemon_s_token_speaks_as_its_owner() {
    let token = "made-up-for-this-test";
    let rig = Rig::guarded(vec![Spec::new("lin")], Some(token)).await;
    let (mut fox, _) = Console::connect_as(&rig.url, Some(token.into()), None, Some("fox".into()))
        .await
        .unwrap();
    fox.say(None, "evening, 3c9a", SayMode::Say).await.unwrap();
    rig.until_heard(0, "evening, 3c9a").await;
    // Admitted, it is the console's terminal she answers on.
    rig.until(
        0,
        "her answer at the console",
        |e| matches!(e, Event::Said { channel, .. } if channel == CONSOLE),
    )
    .await;
}

#[tokio::test]
async fn no_presence_can_take_the_console_s_channel() {
    let rig = Rig::run(vec![Spec::new("lin")]).await;
    let hello = Hello {
        protocol: PROTOCOL,
        role: Role::Presence,
        token: None,
        ghost: None,
        presence: Some(PresenceHello {
            channel: CONSOLE.into(),
            me: None,
            can: vec![Capability::Send],
            allows: ghost_protocol::Allows::default(),
        }),
        who: None,
    };
    let e = Conn::open(&rig.url, &hello).await.err().unwrap();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
}

/// Her work ended because Fox, who runs her, stopped it.
fn stopped_by_fox(event: &Event) -> bool {
    matches!(event, Event::Concluded { outcome, summary, .. }
        if outcome == "dropped" && summary.starts_with("Fox, who runs you, stopped it"))
}

/// Before her mind starts: she took something on, and it stopped to ask.
fn waiting_on_an_answer(dir: &std::path::Path) {
    let mut ghost = ghost_core::Ghost::open(dir).unwrap();
    ghost
        .record(Event::Undertook {
            id: "w1".into(),
            brief: "Find the rota.".into(),
            asked_by: None,
            place: None,
            directed: false,
            dir: None,
        })
        .unwrap();
    ghost
        .record(Event::Asked {
            id: "w1".into(),
            question: "which week?".into(),
        })
        .unwrap();
}

#[tokio::test]
async fn an_operator_at_a_console_answers_what_her_work_asked_and_stops_it() {
    let mut spec = Spec::new("lin");
    spec.before = Some(waiting_on_an_answer);
    let rig = Rig::run(vec![spec]).await;
    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("Fox".into()))
        .await
        .unwrap();
    let work = fox.work().await.unwrap();
    assert_eq!(work[0].id, "w1");
    assert_eq!(work[0].state, ghost_protocol::WorkState::Waiting);
    assert_eq!(work[0].question.as_deref(), Some("which week?"));

    fox.answer("w1", "next week, 7d4f").await.unwrap();
    let (id, text, by) = rig
        .events(0)
        .await
        .into_iter()
        .find_map(|e| match e {
            Event::Added { id, text, by } => Some((id, text, by)),
            _ => None,
        })
        .expect("the answer is in her journal");
    assert_eq!((id.as_str(), text.as_str()), ("w1", "next week, 7d4f"));
    assert_eq!(by.as_deref(), Some("console:fox"));
    assert_eq!(fox.work().await.unwrap()[0].question, None);

    fox.stop("w1").await.unwrap();
    rig.until(0, "the end of her work", stopped_by_fox).await;
    // It is over: neither can be done to it again.
    let e = fox.stop("w1").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_SUCH_WORK));
    let e = fox.answer("w1", "too late").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_SUCH_WORK));
}

#[tokio::test]
async fn only_an_operator_who_says_who_they_are_answers_her_work_and_only_work_she_has() {
    let mut spec = Spec::new("lin");
    spec.before = Some(waiting_on_an_answer);
    let rig = Rig::run(vec![spec]).await;
    let (mut nobody, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let e = nobody.answer("w1", "next week").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
    let e = nobody.stop("w1").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));

    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("fox".into()))
        .await
        .unwrap();
    let e = fox.answer("w9", "next week").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_SUCH_WORK));
    let e = fox.answer("w1", "  ").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
    let events = rig.events(0).await;
    assert!(!events.iter().any(|e| matches!(e, Event::Added { .. })));
    assert_eq!(
        fox.work().await.unwrap()[0].state,
        ghost_protocol::WorkState::Waiting
    );
}

/// Before her mind starts: a piece of work with steps logged by its own
/// loop and by a command-line agent's server of tools, a part with a step
/// of its own, and a look at a conversation that kept a round.
fn worked_and_looked(dir: &std::path::Path) {
    use ghost_work::steps::call;
    use serde_json::json;
    let db = dir.join(ghost_work::WORK_DB);
    let work = ghost_work::Undertaking::open(&db, "w1", Some("Find the rota.")).unwrap();
    for n in 1..=3 {
        work.log(&json!({
            "at": format!("2026-11-16T08:00:0{n}+00:00"),
            "said": format!("step {n}"),
            "model_ms": 40,
            "calls": [call("shell", &json!({"command": format!("echo {n}")}), &format!("{n}\n"), false, 0, 5)],
            "read": 1000 * n,
            "cached": 600,
            "wrote": 50,
            "context": {"system": 900, "tools": 1200, "task": 40, "messages": 30 * n, "results": 10 * n},
            "window": 1_048_576,
            "folds_at": 838_860,
        }));
    }
    let mut line = call("read", &json!({"path": "rota.md"}), "mon: wen", false, 0, 2);
    line["via"] = json!("mcp");
    work.log(&line);
    let part = work.open_part("Check the weekend.").unwrap();
    part.update(|state| state.brain = Some("openai/deepseek-chat".into()))
        .unwrap();
    part.log(&json!({"said": "looking at the weekend", "calls": []}));
    let handed = work.open_part("Check the holidays.").unwrap();
    handed
        .update(|state| state.agent = Some("codex".into()))
        .unwrap();

    let looks = ghost_work::looks::Looks::open(&db).unwrap();
    let look = looks.begin("t:wen", chrono::Utc::now()).unwrap();
    looks
        .round(
            look,
            &json!({
                "at": "2026-11-16T08:10:00+00:00",
                "calls": [call("search", &json!({"query": "tea house hours"}), "closes at ten", false, 0, 300)],
            }),
        )
        .unwrap();
    looks.end(look, 7).unwrap();
}

#[tokio::test]
async fn a_console_follows_her_work_step_by_step_and_each_part_by_its_own_id() {
    let mut spec = Spec::new("lin");
    spec.before = Some(worked_and_looked);
    let rig = Rig::run(vec![spec]).await;
    let (mut watcher, _) = Console::connect(&rig.url, None, None).await.unwrap();

    let all = watcher.work_steps("w1", None, None).await.unwrap();
    assert_eq!(
        all.steps.iter().map(|s| s.n).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(!all.more);
    let first = &all.steps[0];
    assert_eq!((first.said.as_str(), first.ms), ("step 1", Some(40)));
    assert_eq!(first.calls[0].name, "shell");
    assert_eq!(first.calls[0].args["command"], "echo 1");
    assert_eq!(first.calls[0].out, "1\n");
    // A call a command-line agent made is a step of its own.
    let agent = &all.steps[3];
    assert_eq!(agent.said, "");
    assert_eq!(agent.calls[0].args["path"], "rota.md");
    assert_eq!(agent.calls[0].out, "mon: wen");
    assert_eq!(all.parts.len(), 2);
    assert_eq!(all.parts[0].id, "w1/1");
    assert_eq!(all.parts[0].brief, "Check the weekend.");
    assert_eq!(all.parts[0].state, ghost_protocol::WorkState::Open);
    // What fills the worker's context is its newest step's, though a call
    // a command-line agent made came after it.
    let context = all.context.expect("what the worker was given");
    assert_eq!(context.n, 3);
    assert_eq!(
        context.usage.map(|u| (u.input, u.cached)),
        Some((3000, 600))
    );
    assert_eq!(context.parts["results"], 30);
    assert_eq!(
        (context.window, context.folds_at),
        (Some(1_048_576), Some(838_860))
    );

    let next = watcher.work_steps("w1", Some(1), Some(2)).await.unwrap();
    assert_eq!(next.steps.iter().map(|s| s.n).collect::<Vec<_>>(), [2, 3]);
    assert!(next.more);
    assert_eq!(
        next.context.map(|c| c.n),
        Some(3),
        "whatever steps were asked for"
    );
    let part = watcher.work_steps("w1/1", None, None).await.unwrap();
    assert_eq!(part.steps[0].said, "looking at the weekend");
    assert!(part.parts.is_empty());

    let e = watcher.work_steps("w9", None, None).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_SUCH_WORK));
}

#[tokio::test]
async fn a_console_is_told_which_brain_or_agent_steps_each_part() {
    let mut spec = Spec::new("lin");
    spec.before = Some(worked_and_looked);
    let rig = Rig::run(vec![spec]).await;
    let (mut watcher, _) = Console::connect(&rig.url, None, None).await.unwrap();

    let all = watcher.work_steps("w1", None, None).await.unwrap();
    let brains: Vec<(&str, Option<&str>)> = all
        .parts
        .iter()
        .map(|p| (p.id.as_str(), p.brain.as_deref()))
        .collect();
    // The first was stepped by her own model, the second handed to an
    // agent installed beside her.
    assert_eq!(
        brains,
        [
            ("w1/1", Some("openai/deepseek-chat")),
            ("w1/2", Some("codex"))
        ]
    );
}

#[tokio::test]
async fn a_console_reads_what_she_called_while_she_looked_at_a_place() {
    let mut spec = Spec::new("lin");
    spec.before = Some(worked_and_looked);
    let rig = Rig::run(vec![spec]).await;
    let (mut watcher, _) = Console::connect(&rig.url, None, None).await.unwrap();

    let looks = watcher.look_steps("t:wen", None).await.unwrap();
    assert_eq!(looks.len(), 1);
    let look = &looks[0];
    assert_eq!((look.seq, look.running), (Some(7), false));
    let search = &look.rounds[0].calls[0];
    assert_eq!(search.name, "search");
    assert_eq!(search.args["query"], "tea house hours");
    assert_eq!((search.out.as_str(), search.ms), ("closes at ten", 300));
    // Nothing ended after the entry it ended with, and nothing is going
    // on, there or anywhere else.
    assert!(
        watcher
            .look_steps("t:wen", Some(7))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(watcher.look_steps("t:mei", None).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_console_lists_what_waits_on_her_operators_and_answering_it_takes_it_off() {
    let mut spec = Spec::new("lin");
    spec.before = Some(waiting_on_an_answer);
    let rig = Rig::run(vec![spec]).await;
    // Any console may look; it needs to be no one in particular.
    let (mut watcher, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let waiting = watcher.attention().await.unwrap();
    assert_eq!(waiting.len(), 1);
    let question = &waiting[0];
    assert_eq!(question.kind, ghost_protocol::AttentionKind::Question);
    assert_eq!(question.id, format!("asked-{}", question.seq));
    assert_eq!(question.work.as_deref(), Some("w1"));
    assert_eq!(question.brief.as_deref(), Some("Find the rota."));
    assert_eq!(question.text, "which week?");
    // Asked again, it is the same item.
    assert_eq!(watcher.attention().await.unwrap(), waiting);

    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("fox".into()))
        .await
        .unwrap();
    fox.answer("w1", "next week").await.unwrap();
    assert!(watcher.attention().await.unwrap().is_empty());
}

#[tokio::test]
async fn stopping_her_takes_an_operator_who_says_who_they_are_and_stops_nothing_when_she_is_idle() {
    let rig = Rig::run(vec![Spec::new("lin")]).await;
    let (mut nobody, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let e = nobody.stop_place(None).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));

    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("Fox".into()))
        .await
        .unwrap();
    assert!(!fox.stop_place(None).await.unwrap());
    assert!(!fox.stop_place(Some("desk-1".into())).await.unwrap());
    let events = rig.events(0).await;
    assert!(!events.iter().any(|e| matches!(e, Event::Stopped { .. })));
}

/// Before her mind starts: Fox directed her, from his place at a console,
/// to take something on, and it is still open.
fn directed_from_the_console(dir: &std::path::Path) {
    let mut ghost = ghost_core::Ghost::open(dir).unwrap();
    ghost
        .record(Event::Undertook {
            id: "w1".into(),
            brief: "Draw the east gate.".into(),
            asked_by: Some("console:fox".into()),
            place: Some("console:fox".into()),
            directed: true,
            dir: None,
        })
        .unwrap();
}

#[tokio::test]
async fn an_operator_who_stops_her_in_their_place_leaves_the_work_they_directed_going() {
    let mut spec = Spec::new("lin");
    spec.before = Some(directed_from_the_console);
    let rig = Rig::run(vec![spec]).await;
    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("Fox".into()))
        .await
        .unwrap();
    // With no look going on there, there is nothing to stop: the work she
    // took on is stopped from its own card, not with her.
    assert!(!fox.stop_place(Some("desk-1".into())).await.unwrap());
    assert!(!fox.stop_place(None).await.unwrap());
    let events = rig.events(0).await;
    assert!(!events.iter().any(|e| matches!(e, Event::Stopped { .. })));
    let work = fox.work().await.unwrap();
    assert_eq!(work[0].state, ghost_protocol::WorkState::Open);
    fox.stop("w1").await.unwrap();
    rig.until(0, "the end of her work", stopped_by_fox).await;
}
