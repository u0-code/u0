//! An operator reading and changing the daemon's brains over the real
//! protocol on this machine's loopback: a home of its own in a temporary
//! directory, minds real and brains scripted, and no request leaving the
//! machine.

use std::sync::atomic::Ordering;

use ghost_link::Console;
use ghost_protocol::{BrainAsk, BrainSave, RpcError};

use super::*;
use crate::cast::tests::{Rig, Spec};

/// Key variables nobody sets, so that a test never finds a real key.
const MAIN_KEY: &str = "GHOST_TEST_BRAINS_MAIN_KEY_UNSET";
const SECOND_KEY: &str = "GHOST_TEST_BRAINS_SECOND_KEY_UNSET";

fn home_file() -> String {
    format!(
        "# how it thinks\n[brain]\nthink = \"main\"\ntick = \"main\"\n\n\
         # the API\n[providers.main]\nkind = \"openai\"\nmodel = \"m1\" # fast\n\
         base_url = \"https://api.example.invalid/v1\"\napi_key_env = \"{MAIN_KEY}\"\n\n\
         # signed in\n[providers.codex]\nkind = \"codex\"\nprogram = \"/no/such/codex\"\n"
    )
}

/// A home with that configuration in it, and its brains held as a daemon
/// running it holds them.
fn home() -> (tempfile::TempDir, Held) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(home::CONFIG);
    std::fs::write(&path, home_file()).unwrap();
    let cfg = Config::parse(&home_file()).unwrap();
    let held = Held::new(Arc::new(cfg), Some(path), Some(dir.path().to_path_buf()));
    (dir, held)
}

fn code_of(e: &anyhow::Error) -> Option<i64> {
    e.downcast_ref::<RpcError>().map(|e| e.code)
}

fn fields(pairs: &[(&str, Json)]) -> serde_json::Map<String, Json> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

#[tokio::test]
async fn a_console_lists_the_brains_with_whether_each_has_a_key_and_never_the_key() {
    let (_dir, held) = home();
    let rig = Rig::holding(vec![Spec::new("lin")], None, held).await;
    let (mut console, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let list = console.brains().await.unwrap();
    assert!(list.changeable);
    assert_eq!(
        (list.think.as_deref(), list.tick.as_deref()),
        (Some("main"), Some("main"))
    );
    let main = list.brains.iter().find(|b| b.name == "main").unwrap();
    assert_eq!((main.kind.as_str(), main.model.as_str()), ("openai", "m1"));
    assert_eq!(main.key_env.as_deref(), Some(MAIN_KEY));
    assert!(!main.key_set);
    assert_eq!(main.blocker, Some(format!("{MAIN_KEY} is not set")));
    let codex = list.brains.iter().find(|b| b.name == "codex").unwrap();
    assert_eq!((codex.key_env.as_deref(), codex.key_set), (None, false));
    assert_eq!(codex.program.as_deref(), Some("/no/such/codex"));
    assert!(codex.blocker.as_deref().unwrap().contains("not found"));
    // Each is said to be the preset it is, where it is one, and the
    // presets to add another from come with them.
    assert_eq!(codex.preset.as_deref(), Some("codex"));
    assert_eq!(main.preset, None, "an API on a host no preset is on");
    let deepseek = list.presets.iter().find(|p| p.id == "deepseek").unwrap();
    assert_eq!(
        Json::Object(deepseek.provider.clone()),
        serde_json::json!({"kind": "openai", "model": "deepseek-flash",
            "base_url": "https://api.deepseek.com", "api_key_env": "DEEPSEEK_API_KEY",
            "json_mode": true})
    );
}

#[tokio::test]
async fn a_brain_saved_with_its_key_keeps_the_key_in_env_and_the_comments_in_the_file() {
    let (dir, held) = home();
    let rig = Rig::holding(vec![Spec::new("lin")], None, held).await;
    let (mut console, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let save = BrainSave {
        was: None,
        name: "second".into(),
        set: fields(&[
            ("kind", "openai".into()),
            ("model", "m2".into()),
            ("base_url", "https://other.example.invalid/v1".into()),
            ("api_key_env", SECOND_KEY.into()),
            ("json_mode", true.into()),
        ]),
        clear: Vec::new(),
        key: Some("sk-test-not-a-real-key".into()),
    };
    let list = console.save_brain(&save).await.unwrap();
    let second = list.brains.iter().find(|b| b.name == "second").unwrap();
    assert!(second.key_set, "the key given is the one it has now");
    assert_eq!(second.blocker, None);

    let text = std::fs::read_to_string(dir.path().join(home::CONFIG)).unwrap();
    assert!(text.starts_with(&home_file()), "{text}");
    assert!(
        text.ends_with(&format!(
            "[providers.second]\nkind = \"openai\"\nmodel = \"m2\"\n\
             base_url = \"https://other.example.invalid/v1\"\napi_key_env = \"{SECOND_KEY}\"\n\
             json_mode = true\n"
        )),
        "{text}"
    );
    assert!(!text.contains("sk-test"), "no key in the configuration");
    let env = std::fs::read_to_string(dir.path().join(home::ENV)).unwrap();
    assert_eq!(env, format!("{SECOND_KEY}='sk-test-not-a-real-key'\n"));
    // Nobody ran on it, so nobody was started again.
    assert_eq!(rig.made[0].load(Ordering::SeqCst), 1);

    // Nor is a key taken where one is not written into the configuration.
    let mut sneaked = save.clone();
    sneaked.was = Some("second".into());
    sneaked.key = None;
    sneaked.set = fields(&[("api_key", "sk-test-in-the-file".into())]);
    let e = console.save_brain(&sneaked).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS), "{e:#}");
    let text = std::fs::read_to_string(dir.path().join(home::CONFIG)).unwrap();
    assert!(!text.contains("sk-test"), "{text}");
}

#[tokio::test]
async fn a_change_to_the_brain_in_use_starts_her_mind_again_and_one_that_would_not_run_is_refused()
{
    let (dir, held) = home();
    let rig = Rig::holding(vec![Spec::new("lin")], None, held).await;
    let (mut console, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let save = BrainSave {
        was: Some("main".into()),
        name: "main".into(),
        set: fields(&[("model", "m1-newer".into())]),
        ..BrainSave::default()
    };
    let list = console.save_brain(&save).await.unwrap();
    assert_eq!(
        list.brains.iter().find(|b| b.name == "main").unwrap().model,
        "m1-newer"
    );
    for _ in 0..200 {
        if rig.made[0].load(Ordering::SeqCst) >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(rig.made[0].load(Ordering::SeqCst), 2, "made again, once");
    let text = std::fs::read_to_string(dir.path().join(home::CONFIG)).unwrap();
    assert!(text.contains("model = \"m1-newer\" # fast"), "{text}");

    // A kind ghostd does not know would not run: nothing is written.
    let wrong = BrainSave {
        was: Some("main".into()),
        name: "main".into(),
        set: fields(&[("kind", "telepathy".into())]),
        ..BrainSave::default()
    };
    let e = console.save_brain(&wrong).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS), "{e:#}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(home::CONFIG)).unwrap(),
        text
    );
}

#[tokio::test]
async fn a_brain_that_cannot_think_is_not_used_and_the_one_in_use_is_not_removed() {
    let (dir, held) = home();
    let rig = Rig::holding(vec![Spec::new("lin")], None, held).await;
    let (mut console, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let before = std::fs::read_to_string(dir.path().join(home::CONFIG)).unwrap();

    // Its program is not there: it was asked, and could not answer.
    let e = console.use_brain("codex", None).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::BRAIN_FAILED), "{e:#}");
    let e = console.use_brain("nobody", None).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_SUCH_BRAIN), "{e:#}");
    // The one in use, on another model: its key is not set.
    let e = console
        .use_brain("main", Some("m-other".into()))
        .await
        .unwrap_err();
    assert_eq!(code_of(&e), Some(code::NO_KEY), "{e:#}");
    assert!(format!("{e:#}").contains(MAIN_KEY), "{e:#}");
    let e = console.remove_brain("main").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::INVALID_PARAMS), "{e:#}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(home::CONFIG)).unwrap(),
        before,
        "nothing refused was written"
    );
    assert_eq!(rig.made[0].load(Ordering::SeqCst), 1);

    let list = console.remove_brain("codex").await.unwrap();
    assert_eq!(
        list.brains
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
        ["main"]
    );
    let text = std::fs::read_to_string(dir.path().join(home::CONFIG)).unwrap();
    assert!(!text.contains("codex") && text.starts_with("# how it thinks\n"));
}

#[tokio::test]
async fn models_are_asked_with_the_key_the_daemon_holds_and_a_draft_is_laid_over_what_is_saved() {
    let (_dir, held) = home();
    let ask = |name: Option<&str>, provider: Option<serde_json::Map<String, Json>>| BrainAsk {
        name: name.map(str::to_owned),
        provider,
        model: None,
    };
    // No key, no request.
    let e = held.models(&ask(Some("main"), None)).await.unwrap_err();
    assert_eq!(e.0, code::NO_KEY);
    // A command-line agent's are read from it where it is installed, and
    // one that is not installed has none.
    let e = held.models(&ask(Some("codex"), None)).await.unwrap_err();
    assert_eq!(e.0, code::BRAIN_FAILED);
    assert!(e.1.contains("not found"), "{}", e.1);
    // What is being written is laid over what is saved: the variable
    // changed, the rest is the saved brain's.
    let draft = fields(&[("api_key_env", SECOND_KEY.into())]);
    let p = held.provider(&ask(Some("main"), Some(draft))).unwrap();
    let ProviderConfig::Openai {
        model, api_key_env, ..
    } = p
    else {
        panic!("{p:?}")
    };
    assert_eq!((model.as_str(), api_key_env.as_str()), ("m1", SECOND_KEY));
    let e = held.provider(&ask(None, None)).unwrap_err();
    assert_eq!(e.0, code::INVALID_PARAMS);
    let e = held.provider(&ask(Some("nobody"), None)).unwrap_err();
    assert_eq!(e.0, code::NO_SUCH_BRAIN);
}

#[tokio::test]
async fn only_the_daemons_owner_changes_or_asks_its_brains_and_anyone_admitted_lists_them() {
    let (dir, held) = home();
    let cast = Cast(Vec::new());
    let save = BrainSave {
        name: "x".into(),
        set: fields(&[("kind", "codex".into())]),
        ..BrainSave::default()
    };
    for (asked, params) in [
        (method::BRAIN_SAVE, serde_json::to_value(&save).unwrap()),
        (method::BRAIN_REMOVE, serde_json::json!({"name": "codex"})),
        (method::BRAIN_USE, serde_json::json!({"name": "codex"})),
        (method::BRAIN_MODELS, serde_json::json!({"name": "main"})),
        (method::BRAIN_CHECK, serde_json::json!({"name": "main"})),
    ] {
        let frame = Frame::request(1, asked, &params);
        let reply = answer(1, &frame, false, &held, &cast).await;
        assert_eq!(
            reply.error.map(|e| e.code),
            Some(code::UNAUTHORIZED),
            "{asked}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join(home::CONFIG)).unwrap(),
        home_file()
    );
    let listed = answer(
        1,
        &Frame::request(1, method::BRAINS, &()),
        false,
        &held,
        &cast,
    )
    .await;
    assert!(listed.result.is_some(), "{listed:?}");
}

#[tokio::test]
async fn a_daemon_run_without_a_home_lists_its_brains_and_changes_none() {
    let cfg = Config::parse(&home_file()).unwrap();
    let held = Held::new(Arc::new(cfg), None, None);
    let rig = Rig::holding(vec![Spec::new("lin")], None, held).await;
    let (mut console, _) = Console::connect(&rig.url, None, None).await.unwrap();
    let list = console.brains().await.unwrap();
    assert!(!list.changeable);
    assert_eq!(list.brains.len(), 2);
    let e = console.remove_brain("codex").await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::UNCHANGEABLE), "{e:#}");
    let e = console.use_brain("codex", None).await.unwrap_err();
    assert_eq!(code_of(&e), Some(code::UNCHANGEABLE), "{e:#}");
}

#[test]
fn a_models_listing_gives_its_ids_and_a_refusal_its_servers_own_words() {
    let openai = r#"{"object":"list","data":[{"id":"deepseek-flash"},{"id":"deepseek-v4-pro"}]}"#;
    assert_eq!(
        parse_models(openai).unwrap(),
        ["deepseek-flash", "deepseek-v4-pro"]
    );
    let gemini = r#"{"data":[{"id":"models/gemini-3.8-flash"}]}"#;
    assert_eq!(parse_models(gemini).unwrap(), ["gemini-3.8-flash"]);
    assert!(parse_models(r#"{"models":[]}"#).is_err());
    assert_eq!(
        error_line(
            401,
            r#"{"error":{"message":"Incorrect API key provided: sk-…","type":"invalid_request_error"}}"#
        ),
        "401 · Incorrect API key provided: sk-…"
    );
    assert_eq!(error_line(502, "\n\nupstream\nmore"), "502 · upstream");
    assert_eq!(error_line(500, ""), "500");
}

#[test]
fn a_key_given_since_the_daemon_started_reaches_the_brain_built_from_its_variable() {
    let mut cfg = Config::parse(&home_file()).unwrap();
    assert!(cfg.think().is_err(), "no key yet");
    cfg.keys.insert(MAIN_KEY.into(), "sk-test".into());
    assert_eq!(cfg.think().unwrap().label(), "openai/m1");
}
