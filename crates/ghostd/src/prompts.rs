//! The deployment's prompts: what `ghostd run` says about them at start,
//! where a character's mind reads hers from, and the `ghostd prompts`
//! commands that list, export and update them.

use std::path::{Path, PathBuf};

use anyhow::Context;
use ghost_core::prompts::{
    self as words, Store,
    exported::{self, Updated},
};
use ghost_mind::Prompts;
use tracing::{info, warn};

/// The prompts for a ghost, from the deployment's directory `deployment`
/// if there is one, then `over`, then built in: her own copy, which her
/// mind takes changes into by itself.
pub(crate) fn prompts_for(deployment: Option<&Path>, over: &[(&str, &'static str)]) -> Prompts {
    Prompts::load_over(deployment, over)
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
/// build, and say once what to do about it. `over` is what is built in
/// for them in place of the binary's own.
pub(crate) fn say_stale(prompts: &Prompts, over: &[(&str, &'static str)]) {
    let files = ghost_mind::prompts::files_over(over);
    let Some(dir) = prompts.store().dir() else {
        return;
    };
    let stale = exported::stale(&files, dir);
    if stale.is_empty() {
        return;
    }
    for one in &stale {
        let what = match &one.key {
            Some(key) => format!("{key} in {}", one.file),
            None => one.file.to_owned(),
        };
        let lacks = if one.missing.is_empty() {
            String::new()
        } else {
            let names: Vec<String> = one.missing.iter().map(|m| format!("{{{{{m}}}}}")).collect();
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

/// `ghostd prompts list`.
pub(crate) fn list_prompts(dir: Option<&Path>) {
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
        .dir()
        .into_iter()
        .flat_map(|dir| exported::stale(&files, dir))
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
            let from = match &listed.path {
                None => "built-in".to_owned(),
                Some(path) => format!("{:<10} {}", "deployment", path.display()),
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
        assert!(line("system/work/texts/sitting.toml", Some("budget")).starts_with("work "));
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
