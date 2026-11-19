use std::{fs, path::PathBuf};

use crate::config::Config;

use super::*;

/// What the home's configuration says, as ghostd reads it.
fn config(home: &Path) -> Config {
    Config::load(&home.join(CONFIG)).unwrap()
}

/// Every file under `dir`, with what is in it.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut all = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                todo.push(path);
            } else {
                all.push((path.clone(), fs::read(&path).unwrap()));
            }
        }
    }
    all.sort();
    all
}

#[test]
fn an_empty_home_is_made_ready_once_with_its_configuration_and_the_mascot_born() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("Ghost");
    let made = ready(&home).unwrap();
    assert_eq!(
        made,
        Made {
            config: true,
            born: true,
        }
    );
    let cfg = config(&home);
    assert_eq!(cfg.listen.addr, ghost_protocol::DEFAULT_ADDR);
    assert_eq!(cfg.brain.think, "deepseek");
    assert_eq!(
        cfg.providers["deepseek"].kind(),
        Preset::by_id("deepseek").unwrap().kind,
        "a new home's brains are written as their presets are"
    );
    assert!(cfg.providers.contains_key("claude-code") && cfg.providers.contains_key("codex"));
    let solver = home.join(GHOSTS).join(SOLVER);
    assert_eq!(Ghost::peek(&solver).unwrap().identity.name(), "Ghost");
    assert!(
        !home.join(ENV).exists(),
        "no keys are made up: they are the owner's to put there"
    );

    // Ready again, it is left as it is.
    let written = snapshot(&home);
    assert_eq!(ready(&home).unwrap(), Made::default());
    assert_eq!(snapshot(&home), written);
}

#[test]
fn a_home_already_set_up_is_left_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    ready(home).unwrap();
    fs::write(home.join(CONFIG), format!("# mine\n{SETTINGS}{}", brains())).unwrap();
    let before = snapshot(home);
    assert_eq!(ready(home).unwrap(), Made::default());
    assert_eq!(snapshot(home), before);
}

#[test]
fn a_homes_ghosts_are_given_the_desks_persona_and_being() {
    let prompts = ghost_mind::Prompts::load_over(None, PROMPTS);
    assert_eq!(
        prompts.being,
        include_str!("../../../../prompts/system/desk/system/being.md")
    );
    assert_ne!(prompts.persona, ghost_mind::Prompts::shipped().persona);
    for (path, _) in PROMPTS {
        assert!(
            ghost_mind::prompts::all_files()
                .iter()
                .any(|f| f.path == *path),
            "{path} is a prompt the binary reads"
        );
    }
}
