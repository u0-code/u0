//! A console's operator speaking to her, over the real protocol on this
//! machine's loopback, with her mind real and her brain scripted.

use std::time::Duration;

use ghost_core::journal::{Entry, Event};
use ghost_link::{Conn, Console};
use ghost_protocol::{Capability, Hello, PROTOCOL, PresenceHello, Role, RpcError};

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
    let thread = serde_json::to_string(&fox.thread(&said.place, 10).await.unwrap()).unwrap();
    assert!(
        thread.contains("5e1d") && thread.contains(&said.id),
        "{thread}"
    );
    assert!(!rig.journal(0).await.contains("\"directed\""));

    // Asked for a directive, in a place of its own, it is one.
    let directed = fox
        .say(
            Some("desk-1".into()),
            "send me the rota",
            SayMode::Directive,
        )
        .await
        .unwrap();
    assert_eq!(directed.place, "console:desk-1");
    let (from, text) = next_where(&mut stream, |event| match event {
        Event::Directed { from, text, .. } => Some((from, text)),
        _ => None,
    })
    .await;
    assert_eq!(
        (from.as_str(), text.as_str()),
        ("console:fox", "send me the rota")
    );
    assert_eq!(
        next_where(&mut stream, said_on_console).await,
        ("desk-1".to_owned(), "lin here".to_owned())
    );
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
    assert!(!rig.journal(0).await.contains("\"heard\""));
}

#[tokio::test]
async fn a_console_with_the_daemon_s_token_speaks_as_its_owner() {
    let token = "made-up-for-this-test";
    let rig = Rig::guarded(vec![Spec::new("lin")], Some(token)).await;
    let (mut fox, _) = Console::connect_as(&rig.url, Some(token.into()), None, Some("fox".into()))
        .await
        .unwrap();
    fox.say(None, "evening, 3c9a", SayMode::Directive)
        .await
        .unwrap();
    rig.until_written(0, "3c9a").await;
    rig.until_written(0, "\"directed\"").await;
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
        }),
        who: None,
    };
    let e = Conn::open(&rig.url, &hello).await.err().unwrap();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
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
    let (id, text, by) = {
        let journal = rig.journal(0).await;
        let entries: Vec<Entry> = serde_json::from_str(&journal).unwrap();
        entries
            .into_iter()
            .find_map(|e| match e.event {
                Event::Added { id, text, by } => Some((id, text, by)),
                _ => None,
            })
            .expect("the answer is in her journal")
    };
    assert_eq!((id.as_str(), text.as_str()), ("w1", "next week, 7d4f"));
    assert_eq!(by.as_deref(), Some("console:fox"));
    assert_eq!(fox.work().await.unwrap()[0].question, None);

    fox.stop("w1").await.unwrap();
    rig.until_written(0, "Fox, who runs you, stopped it").await;
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
    assert!(!rig.journal(0).await.contains("\"added\""));
    assert_eq!(
        fox.work().await.unwrap()[0].state,
        ghost_protocol::WorkState::Waiting
    );
}
