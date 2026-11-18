//! The deployment's prompts: what `ghostd run` says about them at start,
//! where a character's mind reads hers from, and the `ghostd prompts`
//! commands that list, export and update them.

use std::path::{Path, PathBuf};

use anyhow::Context;
use ghost_core::prompts::{
    self as words, Store,
    exported::{self, Updated},
};
use ghost_mind::{Prompts, prompts::Origin};
use tracing::{info, warn};

/// The prompts for the ghost in `ghost_dir`, from the deployment's
/// directory `deployment` if there is one, then `over`, then built in: her
/// own copy, which her mind takes changes into by itself. Prompts in her
/// own directory are warned about and not used.
pub(crate) fn prompts_for(
    ghost_dir: &Path,
    deployment: Option<&Path>,
    over: &[(&str, &'static str)],
) -> Prompts {
    if let Some(said) = misplaced(ghost_dir, deployment) {
        warn!("{said}");
    }
    Prompts::load_over(deployment, over)
}

/// What there is to say about prompts in the ghost directory `ghost_dir`,
/// if it has any. They are not read: how she works is shared by every
/// character, and only her persona is hers.
fn misplaced(ghost_dir: &Path, deployment: Option<&Path>) -> Option<String> {
    let mut found = words::misplaced(&ghost_mind::prompts::all_files(), ghost_dir);
    found.extend(
        retired(&ghost_dir.join(words::DIR)).map(|_| ghost_dir.join(words::DIR).join(RETIRED)),
    );
    if found.is_empty() {
        return None;
    }
    let hers = ghost_dir.join(words::DIR);
    let names: Vec<String> = found
        .iter()
        .filter_map(|p| p.strip_prefix(&hers).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let to = deployment.map_or_else(
        || "a directory of prompts named by `prompts` under [mind] in ghostd.toml".to_owned(),
        |dir| dir.display().to_string(),
    );
    Some(format!(
        "{} holds {}, which are not used: how she works is shared by every character this deployment runs, \
         and what is hers alone is in her ghost.toml. Move them to their place in the tree in {to} to use them for everyone",
        ghost_dir.join(words::DIR).display(),
        names.join(", ")
    ))
}

/// The prompt that was split into the persona template and her being.
const RETIRED: &str = "self.md";

/// What there is to say about a copy of `self.md` in `dir`, if there is
/// one: nothing reads it any more.
pub(crate) fn retired(dir: &Path) -> Option<String> {
    let path = dir.join(RETIRED);
    path.exists().then(|| {
        format!(
            "{} is no longer read: it is now character/persona.md, her own fields arranged, and system/being.md, \
             what she is and the ground rules. `ghostd prompts export --only character/persona.md system/being.md` \
             writes this build's, to carry your changes into",
            path.display()
        )
    })
}

/// Say which prompts are not the built-in ones, and where they come from.
pub(crate) fn say_overrides(prompts: &Prompts) {
    let mut from: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for listed in prompts.listing() {
        let Some(path) = listed.path else { continue };
        let what = listed.key.unwrap_or_else(|| listed.file.to_owned());
        match from.iter_mut().find(|(p, _)| *p == path) {
            Some((_, names)) => names.push(what),
            None => from.push((path, vec![what])),
        }
    }
    if from.is_empty() {
        info!("prompts: all built in");
    }
    for (path, names) in from {
        info!("prompts from {}: {}", path.display(), names.join(", "));
    }
}

/// Warn about each copy of a prompt that may hide something newer in this
/// build, and say once per directory what to do about it. `over` is what
/// is built in for them in place of the binary's own.
pub(crate) fn say_stale(prompts: &Prompts, over: &[(&str, &'static str)]) {
    let files = ghost_mind::prompts::files_over(over);
    for (_, dir) in prompts.store().layers() {
        let stale = exported::stale(&files, dir);
        if stale.is_empty() {
            continue;
        }
        for one in &stale {
            let what = match &one.key {
                Some(key) => format!("{key} in {}", one.file),
                None => one.file.to_owned(),
            };
            let lacks = if one.missing.is_empty() {
                String::new()
            } else {
                let names: Vec<String> =
                    one.missing.iter().map(|m| format!("{{{{{m}}}}}")).collect();
                format!(
                    "; it lacks {}, which the built-in fills in, so that is left out of what her brain reads",
                    names.join(", ")
                )
            };
            if one.changed {
                warn!(
                    "{}: the built-in {what} has changed since this copy was exported, and the copy hides the newer one{lacks}",
                    one.path.display()
                );
            } else {
                warn!(
                    "{}: there is no record of when this copy of {what} was exported{lacks}",
                    one.path.display()
                );
            }
        }
        let mut names: Vec<&str> = stale.iter().map(|s| s.file).collect();
        names.dedup();
        let names = names.join(" ");
        warn!(
            "if you never edited them, `ghostd prompts update {0}` brings them up to date. \
             If you did, write this build's next to them with `ghostd prompts export <somewhere> --only {names}`, \
             carry over what you want, then `ghostd prompts update {0} --looked-at {names}`",
            dir.display()
        );
    }
}

/// `ghostd prompts list`.
pub(crate) fn list_prompts(dir: Option<&Path>) {
    if let Some(said) = dir.and_then(retired) {
        warn!("{said}");
    }
    for line in listing(&Prompts::load(dir)) {
        println!("{line}");
    }
}

/// A line for each template, text and tool: its layer, its file in the
/// tree and its key there, whether it is not the built-in or hides a newer
/// one, and where it comes from.
fn listing(prompts: &Prompts) -> Vec<String> {
    // What hides something newer in this build, by where it is.
    let files = ghost_mind::prompts::all_files();
    let behind: Vec<(PathBuf, Option<String>)> = prompts
        .store()
        .layers()
        .iter()
        .flat_map(|(_, dir)| exported::stale(&files, dir))
        .map(|s| (s.path, s.key))
        .collect();
    prompts
        .listing()
        .into_iter()
        .map(|listed| {
            let name = match &listed.key {
                Some(key) => format!("{} {key}", listed.file),
                None => listed.file.to_owned(),
            };
            let from = match (&listed.origin, &listed.path) {
                (Origin::BuiltIn, _) | (_, None) => listed.origin.to_string(),
                (origin, Some(path)) => format!("{origin:<10} {}", path.display()),
            };
            let is_behind = listed.path.as_ref().is_some_and(|path| {
                behind
                    .iter()
                    .any(|(p, key)| p == path && (key.is_none() || *key == listed.key))
            });
            let state = if is_behind {
                "behind"
            } else if listed.differs {
                "changed"
            } else {
                ""
            };
            format!("{:<8} {name:<56} {state:<8} {from}", listed.layer)
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The prompt files `names` pick, by their path in the tree or a directory
/// of it, or every one if there are none.
fn prompt_files(names: &[String]) -> anyhow::Result<Vec<ghost_mind::prompts::File>> {
    let files = ghost_mind::prompts::all_files();
    words::pick(&files, names).map_err(|unknown| {
        let known: Vec<&str> = files.iter().map(|f| f.path).collect();
        anyhow::anyhow!(
            "there is no prompt file or directory {unknown:?} in the tree; there are {}",
            known.join(", ")
        )
    })
}

/// `ghostd prompts export`.
pub(crate) fn export_prompts(target: &Path, force: bool, only: &[String]) -> anyhow::Result<()> {
    words::say_flat(&ghost_mind::prompts::all_files(), target);
    let done = Store::export(&prompt_files(only)?, target, force)
        .with_context(|| format!("write into {}", target.display()))?;
    let skipped = done.iter().filter(|(_, written)| !written).count();
    for (path, written) in &done {
        if *written {
            println!("wrote    {}", path.display());
        } else {
            println!("skipped  {} (already there)", path.display());
        }
    }
    if skipped > 0 {
        println!("{skipped} left as they were; --force overwrites them");
    }
    Ok(())
}

/// `ghostd prompts update`.
pub(crate) fn update_prompts(target: &Path, looked_at: &[String]) -> anyhow::Result<()> {
    words::say_flat(&ghost_mind::prompts::all_files(), target);
    let looked_at: Vec<&str> = if looked_at.is_empty() {
        Vec::new()
    } else {
        prompt_files(looked_at)?.iter().map(|f| f.path).collect()
    };
    let done = exported::update(&ghost_mind::prompts::all_files(), target, &looked_at)
        .with_context(|| format!("update {}", target.display()))?;
    if done.is_empty() {
        println!("no copies in {}", target.display());
    }
    for (path, what) in &done {
        let said = match what {
            Updated::Replaced => "updated ",
            Updated::Current => "current ",
            Updated::Marked => "kept    ",
            Updated::LeftAlone => "edited  ",
        };
        println!("{said} {}", path.display());
    }
    if done.iter().any(|(_, what)| *what == Updated::LeftAlone) {
        println!(
            "edited copies were left as they are; `ghostd run` warns about any that hide something newer"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_shared_prompt_in_a_ghosts_own_directory_is_warned_about_and_the_deployments_is_used() {
        let (ghost, deployment) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let hers = ghost.path().join(words::DIR);
        fs::create_dir_all(hers.join("character")).unwrap();
        // One laid out as the tree, one the old flat way: neither is read.
        fs::write(hers.join("plan.md"), "hers").unwrap();
        fs::write(hers.join("character/persona.md"), "hers too").unwrap();

        let said = misplaced(ghost.path(), Some(deployment.path())).unwrap();
        assert!(said.contains("character/persona.md, plan.md"), "{said}");
        assert!(
            said.contains(&deployment.path().display().to_string()),
            "{said}"
        );
        // Not read, with or without a deployment's directory.
        assert_eq!(
            prompts_for(ghost.path(), None, &[]).plan,
            Prompts::shipped().plan
        );
        assert_eq!(
            prompts_for(ghost.path(), None, &[]).persona,
            Prompts::shipped().persona
        );
        fs::create_dir_all(deployment.path().join("system/logic")).unwrap();
        fs::write(
            deployment.path().join("system/logic/plan.md"),
            "the deployment's",
        )
        .unwrap();
        assert_eq!(
            prompts_for(ghost.path(), Some(deployment.path()), &[]).plan,
            "the deployment's"
        );
        // A ghost with no prompts of her own is not warned about.
        assert_eq!(misplaced(deployment.path(), None), None);
    }

    #[test]
    fn a_copy_of_the_retired_self_md_is_named_wherever_it_is() {
        let (ghost, deployment) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::create_dir_all(ghost.path().join(words::DIR)).unwrap();
        fs::write(ghost.path().join(words::DIR).join(RETIRED), "old").unwrap();
        fs::write(deployment.path().join(RETIRED), "old").unwrap();
        assert!(misplaced(ghost.path(), None).unwrap().contains("self.md"));
        let said = retired(deployment.path()).unwrap();
        assert!(
            said.contains("character/persona.md") && said.contains("system/being.md"),
            "{said}"
        );
    }

    #[test]
    fn prompts_list_shows_each_prompt_by_its_file_in_the_tree_with_its_layer() {
        let deployment = tempfile::tempdir().unwrap();
        fs::create_dir_all(deployment.path().join("system")).unwrap();
        fs::write(
            deployment.path().join("system/being.md"),
            "Being.\n\n{{abilities}}{{canon}}\n",
        )
        .unwrap();
        let lines = listing(&Prompts::load(Some(deployment.path())));
        let line = |file: &str, key: Option<&str>| {
            lines
                .iter()
                .find(|l| {
                    let mut words = l.split_whitespace().skip(1);
                    words.next() == Some(file) && (key.is_none() || words.next() == key)
                })
                .unwrap_or_else(|| panic!("no {file} {key:?} in {lines:#?}"))
                .clone()
        };
        assert!(line("character/persona.md", None).starts_with("persona "));
        assert!(line("character/texts.toml", Some("facet")).starts_with("persona "));
        let being = line("system/being.md", None);
        assert!(being.starts_with("being "));
        assert!(being.contains("changed"));
        assert!(being.contains(&format!(
            "deployment {}",
            deployment.path().join("system/being.md").display()
        )));
        assert!(line("system/channels/chat.md", None).starts_with("logic "));
        assert!(line("system/logic/texts/addressed.toml", Some("fact_note")).starts_with("logic "));
        assert!(line("system/work/working.md", None).starts_with("work "));
        assert!(line("system/work/work.md", None).starts_with("work "));
        assert!(line("system/work/texts.toml", Some("budget")).starts_with("work "));
        assert!(line("system/tools.toml", Some("say")).starts_with("work "));
        // Every line is of the tree, in one half of it or the other.
        for l in &lines {
            let file = l.split_whitespace().nth(1).unwrap();
            assert!(
                file.starts_with("system/") || file.starts_with("character/"),
                "{l}"
            );
        }
    }
}
