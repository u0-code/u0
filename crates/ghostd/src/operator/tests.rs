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
