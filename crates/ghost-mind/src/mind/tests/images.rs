//! Images: seeing one she is sent, keeping it with her note, letting it go,
//! and sending it on as itself.

use super::*;

/// A private message holding one image and nothing else.
fn picture_from(from: &str, id: &str, url: &str) -> Incoming {
    let mut m = dm(from, id, "[image]");
    m.images = vec![url.to_owned()];
    m
}

fn viewed(rig: &Rig) -> Vec<(i64, u32, String, Option<String>)> {
    rig.mind
        .ghost
        .journal()
        .of_kinds_after(&["viewed"], 0)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Viewed {
                message,
                image,
                what,
                file,
            } => Some((message, image, what, file)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_image_she_is_sent_is_described_once_and_she_sees_the_description() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.tick.see();
    let (url, hits) = image_host("image/png", PIXEL).await;
    rig.tick
        .then("A cartoon cat lying flat on the floor, captioned 躺平. People send it to say they have given up, half joking.");
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"same"}]}"#);
    rig.hear(picture_from("wen", "1", &url), t0);
    // She glances at it as it comes in, before she has read the conversation.
    rig.pump(t0).await;
    assert_eq!(rig.think.calls(), 0);
    rig.until(|r| r.body.sent().len() == 1).await;

    // Looked at once, by the brain that sees, with the image itself.
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(rig.tick.calls(), 1);
    {
        let seen = rig.tick.seen.lock().unwrap();
        let shown = &seen[0].messages[0].images;
        assert_eq!(shown.len(), 1);
        assert_eq!(
            (shown[0].mime.as_str(), &shown[0].bytes[..]),
            ("image/png", PIXEL)
        );
    }
    let heard = rig
        .mind
        .ghost
        .journal()
        .last_of(&["heard"])
        .unwrap()
        .unwrap();
    assert!(matches!(&heard.event, Event::Heard { images, .. } if *images == [url.clone()]));
    let views = viewed(&rig);
    assert_eq!(views.len(), 1);
    let (message, image, what, file) = &views[0];
    assert_eq!((*message, *image), (heard.seq, 0));
    assert!(what.contains("躺平"));

    // The bytes are kept under her directory, named by what they are.
    let file = file.as_deref().expect("the image was kept");
    assert_eq!(file, ghost_core::media::name(PIXEL, "image/png"));
    let kept = rig.mind.ghost.dir().join("media").join(file);
    assert_eq!(std::fs::read(kept).unwrap(), PIXEL);

    // When she read the conversation, she read what the image was.
    let prompt = format!("{:?}", rig.think.seen.lock().unwrap()[0].messages);
    assert!(
        prompt.contains("[image: A cartoon cat lying flat on the floor, captioned 躺平."),
        "{prompt}"
    );
}

#[tokio::test]
async fn a_brain_that_cannot_see_leaves_the_placeholder_and_fetches_nothing() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let (url, hits) = image_host("image/png", PIXEL).await;
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"say","text":"?"}]}"#);
    rig.hear(picture_from("wen", "1", &url), t0);
    rig.until(|r| r.body.sent().len() == 1).await;

    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(rig.tick.calls(), 0);
    assert!(viewed(&rig).is_empty());
    let prompt = format!("{:?}", rig.think.seen.lock().unwrap()[0].messages);
    assert!(
        prompt.contains("[image]") && !prompt.contains("[image:"),
        "{prompt}"
    );
}

#[tokio::test]
async fn a_link_that_does_not_hold_an_image_leaves_the_placeholder() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    rig.tick.see();
    let (url, hits) = image_host("text/html", b"<html>expired</html>").await;
    rig.hear(picture_from("wen", "1", &url), t0);
    rig.pump(t0).await;

    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    // Nothing to describe, so no brain was asked and nothing was written.
    assert_eq!(rig.tick.calls(), 0);
    assert!(viewed(&rig).is_empty());
    assert_eq!(
        rig.mind.ghost.memory().thread("t:wen", 1).unwrap()[0].text,
        "[image]"
    );
    assert!(!rig.mind.ghost.dir().join("media").exists());
}

#[tokio::test]
async fn looking_at_images_spends_from_the_hourly_budget() {
    let t0 = at(14, 0);
    let mut rig = Rig::new(
        MindConfig {
            live: false,
            browse_minutes: [0, 0],
            calls_per_hour: 1,
            ..MindConfig::default()
        },
        t0,
    );
    rig.tick.see();
    let (url, hits) = image_host("image/png", PIXEL).await;
    rig.tick.then("A single pixel.");
    rig.hear(picture_from("wen", "1", &url), t0);
    rig.pump(t0).await;
    rig.hear(picture_from("mei", "2", &url), t0);
    rig.pump(t0).await;
    // The first image used the hour's one call; the second waits as `[image]`.
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(viewed(&rig).len(), 1);
}

/// A picture is sent to her and she has looked at it: the message is
/// heard, its image is on disk, and what she saw is in her journal, as if
/// a brain that sees had glanced at it. Returns the message's seq and the
/// file.
fn sent_and_seen(
    rig: &mut Rig,
    from: &str,
    id: &str,
    what: &str,
    now: DateTime<Utc>,
) -> (i64, String) {
    // Nobody here can see, so nothing is fetched from the link.
    rig.hear(picture_from(from, id, "https://example.invalid/a.png"), now);
    let message = rig.mind.ghost.journal().last_seq().unwrap();
    let file = ghost_core::media::store(rig.mind.ghost.dir(), PIXEL, "image/png").unwrap();
    rig.mind
        .record(Event::Viewed {
            message,
            image: 0,
            what: what.into(),
            file: Some(file.clone()),
        })
        .unwrap();
    (message, file)
}

#[tokio::test]
async fn a_viewed_image_is_kept_with_her_note_and_found_again_by_recall() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    let (message, file) = sent_and_seen(
        &mut rig,
        "wen",
        "1",
        "A heron standing in the rain, looking cross about it.",
        t0,
    );
    rig.think.then_turn(
        "",
        &[
            (
                "keep",
                serde_json::json!({"message": message, "note": "The cross heron. For grey mornings when nothing goes right."}),
            ),
            ("say", serde_json::json!({"text": "ha"})),
        ],
    );
    rig.until(|r| r.body.sent().len() == 1).await;
    let held = kept(&rig);
    assert_eq!(held.len(), 1);
    let (seq, kept_file, note, from) = &held[0];
    assert_eq!(kept_file, &file);
    assert!(note.starts_with("The cross heron."));
    assert_eq!(from.as_deref(), Some("t:wen"));
    assert!(rig.decisions().iter().any(|d| d.contains("kept something")));

    // Hours later, somewhere else, it comes back by what her note says,
    // with the number she can refer to it by and the file it is.
    let later = t0 + Duration::hours(3);
    rig.think.then_turn(
        "",
        &[("recall", serde_json::json!({"query": "grey mornings"}))],
    );
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "hold on"}))]);
    rig.hear(dm("mei", "2", "rough morning"), later);
    rig.until(|r| r.body.sent().len() == 2).await;
    let (found, failed) = last_result(&rig, "recall");
    assert!(!failed);
    assert!(
        found.contains(&format!("[#{seq}]"))
            && found.contains(&format!("you kept an image ({file}): The cross heron.")),
        "{found}"
    );
}

#[tokio::test]
async fn keeping_the_same_image_again_replaces_the_note() {
    let t0 = at(14, 0);
    let mut rig = Rig::quiet(t0);
    let (message, file) = sent_and_seen(&mut rig, "wen", "1", "A heron in the rain.", t0);
    // Answering in JSON, she keeps it, and thinks better of the note.
    rig.think.then(&format!(
        r#"{{"thought":"","actions":[
            {{"do":"keep","message":"m{message}","note":"A heron."}},
            {{"do":"keep","message":"m{message}","image":1,"note":"The cross heron, for grey mornings."}}]}}"#
    ));
    rig.until(|r| r.kinds().contains(&"decided")).await;
    assert_eq!(kept(&rig).len(), 2);
    let held = rig.mind.ghost.memory().belongings(5).unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(
        (held[0].file.as_str(), held[0].note.as_str()),
        (file.as_str(), "The cross heron, for grey mornings.")
    );
}

#[tokio::test]
async fn something_let_go_is_no_longer_found() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    let (message, _) = sent_and_seen(&mut rig, "wen", "1", "A heron in the rain.", t0);
    rig.think.then_turn(
        "",
        &[(
            "keep",
            serde_json::json!({"message": message, "note": "The cross heron, for grey mornings."}),
        )],
    );
    rig.until(|r| r.kinds().contains(&"kept")).await;
    let seq = kept(&rig)[0].0;

    let later = t0 + Duration::hours(1);
    // A number she holds nothing under is put back to her; the right one
    // lets it go.
    rig.think
        .then_turn("", &[("let_go", serde_json::json!({"thing": seq + 100}))]);
    rig.think
        .then_turn("", &[("let_go", serde_json::json!({"thing": seq}))]);
    rig.hear(dm("wen", "2", "do you still have that heron?"), later);
    rig.until(|r| r.kinds().contains(&"let_go")).await;
    assert_eq!(rig.kinds().iter().filter(|k| **k == "let_go").count(), 1);
    assert!(rig.mind.ghost.memory().belongings(5).unwrap().is_empty());
    assert!(
        !rig.mind
            .ghost
            .memory()
            .search("t", "grey mornings", 5)
            .unwrap()
            .iter()
            .any(|h| h.kind == "kept")
    );
}

#[tokio::test]
async fn keeping_an_image_that_was_not_viewed_is_refused_without_an_event() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    // An image she has seen, in another conversation, which she has read
    // and left.
    let (elsewhere, _) = sent_and_seen(&mut rig, "mei", "1", "A heron.", t0);
    rig.until(|r| {
        r.mind
            .ghost
            .memory()
            .place("t:mei")
            .unwrap()
            .unwrap()
            .unread
            == 0
    })
    .await;

    // Here, an image was sent and not looked at: nobody here can see.
    rig.hear(
        picture_from("wen", "2", "https://example.invalid/a.png"),
        t0 + Duration::hours(1),
    );
    let message = rig.mind.ghost.journal().last_seq().unwrap();
    rig.think.then_turn(
        "",
        &[
            (
                "keep",
                serde_json::json!({"message": message, "note": "whatever that was"}),
            ),
            (
                "keep",
                serde_json::json!({"message": elsewhere, "note": "the heron"}),
            ),
        ],
    );
    rig.think.then_turn(
        "",
        &[("say", serde_json::json!({"text": "it didn't load"}))],
    );
    rig.until(|r| r.body.sent().len() == 1).await;

    // Each refusal is put back to her, and nothing is written.
    let refused: Vec<(String, bool)> = rig
        .think
        .turns
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .items
        .iter()
        .filter_map(|item| match item {
            ghost_brain::Item::Result {
                name,
                output,
                failed,
                ..
            } if name == "keep" => Some((output.clone(), *failed)),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 2);
    assert!(
        refused[0].1 && refused[0].0.contains("have not seen"),
        "{refused:?}"
    );
    assert!(
        refused[1].1
            && refused[1]
                .0
                .contains(&format!("no message m{elsewhere} in this conversation")),
        "{refused:?}"
    );
    assert!(kept(&rig).is_empty());
}

/// Wait until she has read everything in `key`.
async fn read_through(rig: &mut Rig, key: &str) {
    rig.until(|r| {
        r.mind
            .ghost
            .memory()
            .place(key)
            .unwrap()
            .is_some_and(|p| p.unread == 0)
    })
    .await;
}

#[tokio::test]
async fn a_kept_image_is_sent_by_its_number_or_its_file_name_and_arrives_as_an_image() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    takes_files(&mut rig, t0);
    let (_, file) = sent_and_seen(&mut rig, "wen", "1", "A heron in the rain.", t0);
    read_through(&mut rig, "t:wen").await;
    rig.mind
        .record(Event::Kept {
            file: file.clone(),
            note: "The cross heron. For grey mornings.".into(),
            from: Some("t:wen".into()),
        })
        .unwrap();
    let seq = rig.mind.ghost.journal().last_seq().unwrap();

    // Hours later, somewhere else, by the number recall shows it with...
    let later = t0 + Duration::hours(3);
    rig.think.then_turn(
        "",
        &[(
            "send_file",
            serde_json::json!({"file": format!("#{seq}"), "text": "this one"}),
        )],
    );
    rig.hear(dm("mei", "2", "rough morning"), later);
    rig.until(|r| files_sent(r).len() == 1).await;
    // ...and by its name, on its own, without a word.
    rig.think
        .then_turn("", &[("send_file", serde_json::json!({"file": file}))]);
    rig.hear(dm("mei", "3", "again"), later + Duration::hours(1));
    rig.until(|r| files_sent(r).len() == 2).await;

    let sent = files_sent(&rig);
    assert_eq!(
        sent[0],
        (file.clone(), PIXEL.to_vec(), Some("this one".into()))
    );
    assert_eq!(sent[1], (file.clone(), PIXEL.to_vec(), None));
    assert_eq!(
        ghost_core::media::sniff(&sent[0].1),
        Some("image/png"),
        "it arrives as the image itself"
    );
    // In her own lines, what she sent reads as what it was, and which file.
    let said = said_with_files(&rig);
    let shown = "[image: The cross heron. For grey mornings.]";
    assert!(said.contains(&(format!("{shown} this one"), Some(file.clone()))));
    assert!(said.contains(&(shown.to_owned(), Some(file.clone()))));
    let thread = rig.mind.ghost.memory().thread("t:mei", 5).unwrap();
    assert!(thread.iter().any(|l| !l.incoming && l.text == shown));
}

#[tokio::test]
async fn an_image_seen_in_this_conversation_can_be_sent_on() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    takes_files(&mut rig, t0);
    let (message, file) = sent_and_seen(&mut rig, "wen", "1", "A heron in the rain.", t0);
    rig.think.then_turn(
        "",
        &[(
            "send_file",
            serde_json::json!({"message": message, "image": 1}),
        )],
    );
    rig.until(|r| files_sent(r).len() == 1).await;
    assert_eq!(files_sent(&rig)[0], (file.clone(), PIXEL.to_vec(), None));
    assert_eq!(
        said_with_files(&rig),
        [("[image: A heron in the rain.]".to_owned(), Some(file))]
    );
}

#[tokio::test]
async fn an_image_seen_in_another_conversation_cannot_be_sent() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    takes_files(&mut rig, t0);
    let (elsewhere, _) = sent_and_seen(&mut rig, "mei", "1", "A heron.", t0);
    read_through(&mut rig, "t:mei").await;

    rig.think.then_turn(
        "",
        &[("send_file", serde_json::json!({"message": elsewhere}))],
    );
    rig.think
        .then_turn("", &[("say", serde_json::json!({"text": "never mind"}))]);
    rig.hear(dm("wen", "2", "show me"), t0 + Duration::hours(1));
    rig.until(|r| r.body.sent().len() == 1).await;
    let (why, failed) = last_result(&rig, "send_file");
    assert!(failed);
    assert!(
        why.contains(&format!("no message m{elsewhere} in this conversation"))
            && why.contains("Nothing was sent"),
        "{why}"
    );
    assert!(files_sent(&rig).is_empty());
}

#[tokio::test]
async fn a_file_she_does_not_have_is_refused_with_a_reason_and_nothing_is_sent() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    takes_files(&mut rig, t0);
    // Her journal is beside `media/`; a path is not a name in it.
    for (i, wanted) in ["sunset.png", "#999", "../journal.db"].iter().enumerate() {
        rig.think
            .then_turn("", &[("send_file", serde_json::json!({"file": wanted}))]);
        rig.think
            .then_turn("", &[("say", serde_json::json!({"text": "hm"}))]);
        rig.hear(
            dm("wen", &i.to_string(), "send it"),
            t0 + Duration::hours(i64::try_from(i).unwrap()),
        );
        rig.until(|r| r.body.sent().len() == i + 1).await;
        let (why, failed) = last_result(&rig, "send_file");
        assert!(failed, "{wanted}");
        let reason = if *wanted == "#999" {
            "You hold nothing numbered 999.".to_owned()
        } else {
            format!("You have nothing called {wanted} to hand over here.")
        };
        assert!(why.starts_with(&reason), "{why}");
    }
    assert!(files_sent(&rig).is_empty());

    // Answering in JSON there is no second turn: the refusal is written
    // down as what she decided, and nothing is sent.
    let mut rig = Rig::quiet(t0);
    takes_files(&mut rig, t0);
    rig.think
        .then(r#"{"thought":"","actions":[{"do":"send_file","file":"sunset.png"}]}"#);
    rig.hear(dm("wen", "1", "send it"), t0);
    rig.until(|r| r.decisions().iter().any(|d| d == "did not send a file"))
        .await;
    assert!(
        rig.reasons()
            .iter()
            .any(|r| r.starts_with("You have nothing called sunset.png"))
    );
    assert!(files_sent(&rig).is_empty());
}

#[tokio::test]
async fn send_file_is_offered_with_no_machine_where_the_presence_can_take_files() {
    let t0 = at(14, 0);
    let mut rig = Rig::with_tools(t0);
    assert!(rig.mind.workshop.machine.is_none());
    let offered = |rig: &Rig| {
        rig.think
            .turns
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .tools
            .iter()
            .any(|t| t.name == "send_file")
    };
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| !r.think.turns.lock().unwrap().is_empty())
        .await;
    assert!(!offered(&rig), "this channel cannot take a file");

    takes_files(&mut rig, t0 + Duration::hours(1));
    rig.hear(dm("wen", "2", "hi again"), t0 + Duration::hours(1));
    rig.until(|r| r.think.turns.lock().unwrap().len() == 2)
        .await;
    assert!(offered(&rig));

    // Answering in JSON, it is among what she can do.
    let mut rig = Rig::quiet(t0);
    takes_files(&mut rig, t0);
    rig.hear(dm("wen", "1", "hi"), t0);
    rig.until(|r| r.think.calls() == 1).await;
    assert!(rig.think.last_prompt().contains(r#""do":"send_file""#));
}
