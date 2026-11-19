use std::fs;

use super::*;

/// What the home's configuration says, as ghostd reads it.
fn config(home: &Path) -> Config {
    Config::load(Some(&home.join(CONFIG))).unwrap().0
}

/// A checkout as the desk left it before there were homes: its core in
/// `ghosts/solver`, born and run once, with a brain and an operator of
/// its own in `desk/core/ghostd.toml`, and keys in `.env`.
fn old_checkout() -> tempfile::TempDir {
    let checkout = tempfile::tempdir().unwrap();
    let ghost = checkout.path().join("ghosts/solver");
    fs::create_dir_all(ghost.join("media/kept")).unwrap();
    fs::create_dir_all(ghost.join("home")).unwrap();
    fs::create_dir_all(ghost.join("derived")).unwrap();
    fs::write(ghost.join("ghost.toml"), PERSONA).unwrap();
    Ghost::birth(&ghost).unwrap();
    fs::write(ghost.join("media/kept/a.png"), "a picture").unwrap();
    fs::write(ghost.join("home/REPORT.md"), "what she wrote").unwrap();
    fs::write(ghost.join("derived/memory.db"), "rebuilt from the journal").unwrap();
    fs::create_dir_all(checkout.path().join("desk/core")).unwrap();
    fs::write(
        checkout.path().join("desk/core/ghostd.toml"),
        "[brain]\nthink = \"codex\"\ntick = \"codex\"\n\n[mind]\nprompts = \"prompts/system/desk\"\n\n\
         [operators]\ndesk = [\"fox\"]\n\n[providers.codex]\nkind = \"codex\"\ntimeout_secs = 90\n",
    )
    .unwrap();
    fs::write(checkout.path().join(".env"), "SOME_KEY_FOR_A_TEST=x\n").unwrap();
    checkout
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

/// Every file under the checkout `dir` but her journal and the lock taken
/// while she was copied, with what is in it.
fn snapshot_but_her_journal(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let ghost = dir.join("ghosts/solver");
    snapshot(dir)
        .into_iter()
        .filter(|(path, _)| {
            !path.starts_with(ghost.join("journal.db")) && *path != ghost.join("derived/lock")
        })
        .collect()
}

/// Where the journal in `dir` says she moved to, read the way any ghostd
/// reads it.
fn moved_to(dir: &Path) -> Option<String> {
    ghost_core::ghost::moved(dir).unwrap()
}

#[test]
fn an_empty_home_is_made_ready_once_with_its_configuration_and_the_mascot_born() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("Ghost");
    let made = ready(&home, None).unwrap();
    assert_eq!(
        made,
        Made {
            config: true,
            born: true,
            ..Made::default()
        }
    );
    let cfg = config(&home);
    assert_eq!(cfg.listen.addr, "127.0.0.1:7438");
    assert_eq!(cfg.brain.think, "main");
    let solver = home.join(GHOSTS).join(SOLVER);
    assert_eq!(Ghost::peek(&solver).unwrap().identity.name(), "Ghost");
    assert!(
        !home.join(ENV).exists(),
        "no keys are made up: they are the owner's to put there"
    );

    // Ready again, it is left as it is.
    let written = snapshot(&home);
    assert_eq!(ready(&home, None).unwrap(), Made::default());
    assert_eq!(snapshot(&home), written);
}

#[test]
fn a_home_already_set_up_is_left_alone_and_takes_nothing_in() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    ready(home, None).unwrap();
    fs::write(home.join(CONFIG), format!("# mine\n{SETTINGS}{BRAINS}")).unwrap();
    let before = snapshot(home);
    let checkout = old_checkout();
    let left = snapshot(checkout.path());
    assert_eq!(ready(home, Some(checkout.path())).unwrap(), Made::default());
    assert_eq!(
        snapshot(home),
        before,
        "a home with a ghost takes in no other"
    );
    assert_eq!(
        snapshot(checkout.path()),
        left,
        "nor marks one that is not hers"
    );
}

#[test]
fn a_home_takes_in_the_old_core_its_brains_and_its_keys_and_marks_the_one_she_left() {
    let checkout = old_checkout();
    let old = Ghost::peek(&checkout.path().join("ghosts/solver")).unwrap();
    let (seal, entries) = (old.identity.seal().to_string(), old.entries);
    drop(old);
    let before = snapshot_but_her_journal(checkout.path());
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("Ghost");
    let made = ready(&home, Some(checkout.path())).unwrap();
    assert_eq!(made.adopted, Some(checkout.path().join("ghosts/solver")));
    assert!(made.config && !made.born, "{made:?}");

    let solver = home.join(GHOSTS).join(SOLVER);
    let new = Ghost::peek(&solver).unwrap();
    assert_eq!(new.identity.seal().to_string(), seal, "the same ghost");
    assert_eq!(new.entries, entries);
    assert_eq!(
        fs::read_to_string(solver.join("media/kept/a.png")).unwrap(),
        "a picture"
    );
    assert_eq!(
        fs::read_to_string(solver.join("home/REPORT.md")).unwrap(),
        "what she wrote"
    );
    assert!(
        !solver.join("derived/memory.db").exists(),
        "what is rebuilt from the journal is left to be rebuilt"
    );
    assert!(!home.join(ADOPTING).exists());

    let cfg = config(&home);
    assert_eq!(cfg.brain.think, "codex", "the brain chosen is kept");
    assert!(cfg.providers.contains_key("codex"));
    assert!(
        cfg.hers(SOLVER)
            .unwrap()
            .mind()
            .operators
            .includes("desk", "fox"),
        "and its operators"
    );
    assert_eq!(
        cfg.prompts().unwrap(),
        None,
        "but how it lives is the home's"
    );
    assert_eq!(cfg.listen.addr, "127.0.0.1:7438");

    let env = home.join(ENV);
    assert_eq!(fs::read_to_string(&env).unwrap(), "SOME_KEY_FOR_A_TEST=x\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&env).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // Where she was, her journal ends saying where she is, and she is not
    // run there again; the rest of the checkout is as it was.
    let left = checkout.path().join("ghosts/solver");
    let lives = std::path::absolute(&solver).unwrap();
    assert_eq!(moved_to(&left), Some(lives.display().to_string()));
    let Err(e) = Ghost::open(&left) else {
        panic!("she was run again in the checkout she moved out of");
    };
    assert!(matches!(e, ghost_core::Error::Moved(_)), "{e}");
    assert_eq!(Ghost::peek(&left).unwrap().entries, entries + 1);
    assert_eq!(
        snapshot_but_her_journal(checkout.path()),
        before,
        "the rest of the checkout is as it was"
    );
    // Once taken in, never again.
    assert_eq!(
        ready(&home, Some(checkout.path())).unwrap(),
        Made::default()
    );
}

#[test]
fn a_core_its_daemon_is_letting_go_of_is_waited_for_and_copied_whole() {
    let checkout = old_checkout();
    let dir = checkout.path().join("ghosts/solver");
    fs::remove_file(dir.join("derived/memory.db")).unwrap();
    // Her daemon has her open, with what it wrote still in its log, and is
    // stopping.
    let mut running = Ghost::open(&dir).unwrap();
    running.record(ghost_core::journal::Event::Woke).unwrap();
    assert!(dir.join("journal.db-wal").exists());
    let written = running.journal().len().unwrap();
    let stopping = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        drop(running);
    });
    let home = tempfile::tempdir().unwrap();
    ready(home.path(), Some(checkout.path())).unwrap();
    stopping.join().unwrap();
    let copy = Ghost::peek(&home.path().join(GHOSTS).join(SOLVER)).unwrap();
    assert_eq!(copy.entries, written, "everything it wrote");
    assert_eq!(Ghost::peek(&dir).unwrap().entries, written + 1, "and moved");
}

#[test]
fn a_core_a_home_took_in_before_is_marked_as_left_the_next_time_it_is_given_the_checkout() {
    let checkout = old_checkout();
    let left = checkout.path().join("ghosts/solver");
    // As a home took her in before it marked what it left: a copy of her,
    // and nothing written in the checkout.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("Ghost");
    let solver = home.join(GHOSTS).join(SOLVER);
    fs::create_dir_all(&solver).unwrap();
    for file in ["ghost.toml", "journal.db"] {
        fs::copy(left.join(file), solver.join(file)).unwrap();
    }
    let before = snapshot_but_her_journal(checkout.path());

    let made = ready(&home, Some(checkout.path())).unwrap();
    assert_eq!(made.marked.as_deref(), Some(left.as_path()));
    assert!(made.adopted.is_none());
    let lives = std::path::absolute(&solver).unwrap();
    assert_eq!(moved_to(&left), Some(lives.display().to_string()));
    assert!(
        moved_to(&solver).is_none(),
        "where she lives, nothing is written"
    );
    assert_eq!(snapshot_but_her_journal(checkout.path()), before);

    // Marked once: given the checkout again, nothing is done.
    let marked = snapshot(checkout.path());
    assert_eq!(
        ready(&home, Some(checkout.path())).unwrap(),
        Made::default()
    );
    assert_eq!(snapshot(checkout.path()), marked);
}

#[test]
fn a_checkout_whose_core_moved_out_gives_a_new_home_the_mascot_new() {
    let checkout = old_checkout();
    let first = tempfile::tempdir().unwrap();
    ready(first.path(), Some(checkout.path())).unwrap();
    // That home is gone, and another is made from the same checkout: what
    // she left there is not taken in again as her.
    let second = tempfile::tempdir().unwrap();
    let made = ready(second.path(), Some(checkout.path())).unwrap();
    assert!(made.born && made.adopted.is_none(), "{made:?}");
}

#[test]
fn a_checkout_with_no_core_born_in_it_gives_a_home_the_mascot_new() {
    let checkout = tempfile::tempdir().unwrap();
    fs::create_dir_all(checkout.path().join("desk/core")).unwrap();
    fs::write(checkout.path().join("desk/core/ghostd.toml"), "[brain]\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let made = ready(dir.path(), Some(checkout.path())).unwrap();
    assert!(made.born && made.adopted.is_none(), "{made:?}");
    assert_eq!(config(dir.path()).brain.think, "main");
}

#[test]
fn brains_that_would_not_run_are_not_carried_over() {
    let checkout = old_checkout();
    fs::write(
        checkout.path().join("desk/core/ghostd.toml"),
        "[brain]\nthink = \"gone\"\ntick = \"gone\"\n",
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let made = ready(dir.path(), Some(checkout.path())).unwrap();
    assert!(made.adopted.is_some());
    assert_eq!(config(dir.path()).brain.think, "main", "the built-in ones");
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
