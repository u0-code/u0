//! Sessions kept by her daemon, over the real protocol on this machine's
//! loopback, with her mind real and her brain scripted.

use ghost_core::journal::Event;
use ghost_link::Console;
use ghost_protocol::RpcError;

use super::*;
use crate::cast::tests::{Rig, Spec};

fn code_of(e: &anyhow::Error) -> Option<i64> {
    e.downcast_ref::<RpcError>().map(|e| e.code)
}

fn listed(sessions: &[Session]) -> Vec<(&str, &str)> {
    sessions
        .iter()
        .map(|s| (s.id.as_str(), s.title.as_str()))
        .collect()
}

#[tokio::test]
async fn every_console_lists_the_sessions_she_keeps_by_the_names_an_operator_gives_them() {
    let rig = Rig::run(vec![Spec::new("lin")]).await;
    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("Fox".into()))
        .await
        .unwrap();
    // A console that says nobody is at it reads the same list.
    let (mut other, _) = Console::connect(&rig.url, None, None).await.unwrap();
    assert!(other.sessions().await.unwrap().is_empty());

    // A session is kept from its first line, and goes by it.
    fox.say(Some("s1".into()), "free some disk space\nplease")
        .await
        .unwrap();
    assert_eq!(
        listed(&other.sessions().await.unwrap()),
        [("s1", "free some disk space")]
    );

    // Named, it goes by its name on every console.
    fox.rename_session("s1", "  Disk\n space ").await.unwrap();
    assert_eq!(
        listed(&other.sessions().await.unwrap()),
        [("s1", "Disk space")]
    );

    // Begun with a name, it is listed before anything is said in it; an
    // id already taken is not given again.
    let imported = fox
        .new_session(Some("s0".into()), Some("Summarise the commits".into()))
        .await
        .unwrap();
    assert_eq!(imported.id, "s0");
    assert_eq!(imported.title, "Summarise the commits");
    let fresh = fox.new_session(Some("s1".into()), None).await.unwrap();
    assert!(fresh.id != "s1" && fresh.id.starts_with('s'), "{fresh:?}");
    let ids: Vec<String> = other
        .sessions()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, ["s0", "s1"], "one begun with no name is not kept yet");

    // Set aside, it leaves the list; what was said there stays.
    fox.delete_session("s1").await.unwrap();
    assert_eq!(
        listed(&other.sessions().await.unwrap()),
        [("s0", "Summarise the commits")]
    );
    let e = fox.delete_session("s1").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_SUCH_SESSION));
    assert!(
        rig.heard(0)
            .await
            .iter()
            .any(|text| text.contains("free some disk space"))
    );
    let events = rig.events(0).await;
    assert!(events.iter().any(|e| matches!(e,
        Event::SessionSetAside { place, by } if place == "console:s1" && by == "console:fox")));
    assert!(events.iter().any(|e| matches!(e,
        Event::SessionNamed { place, title, .. } if place == "console:s1" && title == "Disk space")));

    // Said in again, it is back.
    fox.say(Some("s1".into()), "and the logs").await.unwrap();
    assert_eq!(other.sessions().await.unwrap().len(), 2);
}

#[tokio::test]
async fn only_an_operator_who_says_who_they_are_begins_renames_or_sets_aside_a_session() {
    let rig = Rig::run(vec![Spec::new("lin")]).await;
    let (mut nobody, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let e = nobody
        .new_session(None, Some("x".into()))
        .await
        .unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
    let e = nobody.rename_session("s1", "x").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));

    // A console that is not the owner's is refused whoever is at it, and
    // may still read the list.
    let her = &rig.cast[0];
    for (method, params) in [
        (method::SESSION_NEW, serde_json::json!({"title": "x"})),
        (
            method::SESSION_RENAME,
            serde_json::json!({"id": "s1", "title": "x"}),
        ),
        (method::SESSION_DELETE, serde_json::json!({"id": "s1"})),
    ] {
        let frame = Frame::request(1, method, &params);
        let reply = answer(1, &frame, false, Some("fox"), her).await;
        assert_eq!(
            reply.error.map(|e| e.code),
            Some(code::UNAUTHORIZED),
            "{method}"
        );
    }
    let list = Frame::request(1, method::SESSIONS, &());
    assert!(answer(1, &list, false, None, her).await.error.is_none());

    let (mut fox, _) = Console::connect_as(&rig.url, None, None, Some("fox".into()))
        .await
        .unwrap();
    let e = fox.rename_session("s1", " \n ").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
    let e = fox.rename_session(" s1", "x").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS));
    assert!(
        !rig.events(0)
            .await
            .iter()
            .any(|e| matches!(e, Event::SessionNamed { .. }))
    );
}

#[test]
fn a_new_session_takes_the_id_proposed_only_when_it_is_free_and_fit() {
    let taken = ["s100".to_owned(), "s101".to_owned()];
    assert_eq!(fresh(Some("desk-7"), &taken, 100), "desk-7");
    assert_eq!(fresh(Some("s100"), &taken, 100), "s102");
    assert_eq!(fresh(Some(" padded"), &taken, 5), "s5");
    assert_eq!(fresh(Some("a\u{7}b"), &taken, 5), "s5");
    assert_eq!(fresh(None, &[], 7), "s7");
    let long = "x".repeat(ID_MOST + 1);
    assert_eq!(fresh(Some(&long), &[], 7), "s7");
}
