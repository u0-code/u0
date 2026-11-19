//! What a worker is told, and what its tools are said to be. Data, not
//! code: see [`ghost_core::prompts`] for where it is looked for and how a
//! change is taken in.
//!
//! `system/work/work.md` is the working instructions,
//! `system/work/texts/` what is said along the way, a file per feature read
//! as one table, and `system/tools.toml` what every tool is, in a
//! conversation as at work. They are the work layer of the system, shared
//! by every character, and are built in from the tree `prompts/` at the top
//! of the repository. A worker looks at the files before each step, so an
//! edit reaches work that is already running at its next step.
//!
//! Everything the work loop puts in front of a brain is one of these: a
//! tool's results and refusals as much as its instructions. The code asks
//! for a text by its name and fills in its values.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use chrono::{DateTime, Utc};
use ghost_brain::ToolSpec;
use ghost_core::prompt;
pub use ghost_core::prompts::{File, Kind, Layer, Listed, Loop, Origin, Store};
use serde_json::{Value, json};

/// The work loop's table of texts, `system/work/texts/`: what a worker is
/// told along the way, and what its tools say back.
pub const TEXTS: &str = "work";
/// The template a worker starts from.
const WORK: &str = "system/work/work.md";

/// Every file the work loop reads, with what is built in.
pub const FILES: &[File] = &[
    prompt!("system/work/work.md", Kind::Whole, Layer::Work),
    prompt!(
        "system/work/texts/sitting.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/plan.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/record.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/calls.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/outside.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/files.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/shell.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/web.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/servers.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/skills.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!(
        "system/work/texts/parts.toml",
        Kind::Texts(TEXTS),
        Layer::Work
    ),
    prompt!("system/tools.toml", Kind::Tools, Layer::Work),
];

/// The words of the work loop, as they are now.
#[derive(Debug, Clone)]
pub struct Prompts {
    store: Store,
}

/// One set of words for a worker, the helpers it hands work to and the
/// tools it carries, looked at again before each step.
pub type Shared = Arc<Mutex<Prompts>>;

impl Prompts {
    /// What is built in, and nothing else.
    pub fn shipped() -> Self {
        Self {
            store: Store::shipped(FILES),
        }
    }

    /// The words for work kept in the directory of prompts `dir`, if
    /// there is one, then what is built in.
    pub fn load(dir: Option<&Path>) -> Self {
        Self {
            store: Store::load(FILES, dir),
        }
    }

    /// The same, shared.
    pub fn shared(self) -> Shared {
        Arc::new(Mutex::new(self))
    }

    /// Take in any file that changed. Returns whether anything did.
    pub fn refresh(&mut self, now: DateTime<Utc>) -> bool {
        self.store.refresh(now)
    }

    /// Take in any file that changed, however recently they were looked
    /// at. A worker does this before each step: a step takes seconds, and
    /// a few files are cheap to look at.
    pub fn refresh_now(&mut self) -> bool {
        self.store.refresh_now()
    }

    /// `system/work/work.md`, as it is.
    pub fn work(&self) -> &str {
        self.store.whole(WORK)
    }

    /// A text of the work loop's, as it is.
    pub fn text(&self, key: &str) -> &str {
        self.store.text(TEXTS, key)
    }

    /// A text of the work loop's, with its placeholders filled.
    pub fn fill(&self, key: &str, pairs: &[(&str, String)]) -> String {
        self.store.fill(TEXTS, key, pairs)
    }

    /// Every template, text and tool, with where what is in use came from.
    pub fn listing(&self) -> Vec<Listed> {
        self.store.listing()
    }

    /// The store underneath, for a reader that keeps more files beside
    /// these.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The directories looked in, first to last.
    pub fn layers(&self) -> Vec<(Origin, PathBuf)> {
        self.store.layers().to_vec()
    }

    /// The words kept in `layers`, first to last.
    pub fn layered(layers: &[(Origin, PathBuf)]) -> Self {
        Self {
            store: Store::layered(FILES, layers),
        }
    }
}

impl std::ops::Deref for Prompts {
    type Target = Store;

    fn deref(&self) -> &Store {
        &self.store
    }
}

/// The spec of tool `name` as it is offered in `lp`, in the words of
/// `words`: `properties` are its arguments' JSON schemas, to which what
/// `tools.toml` says of each is added as its description. An argument
/// inside a list of objects is described under its path, `steps.text`.
#[allow(clippy::needless_pass_by_value)]
pub fn spec(
    words: &Store,
    lp: Loop,
    name: &str,
    mut properties: Value,
    required: &[&str],
) -> ToolSpec {
    if let Value::Object(map) = &mut properties {
        for (argument, schema) in map.iter_mut() {
            describe(words, lp, name, argument, schema);
        }
    }
    ToolSpec {
        name: name.to_owned(),
        description: words.tool(name, lp).to_owned(),
        parameters: json!({
            "type": "object",
            "properties": properties,
            "required": required,
        }),
    }
}

/// Put what is said of `argument` of tool `name`, and of any argument
/// inside it, into its schema.
fn describe(store: &Store, lp: Loop, name: &str, argument: &str, schema: &mut Value) {
    if let Some(said) = store.parameter(name, argument, lp) {
        schema["description"] = Value::String(said.to_owned());
    }
    if let Some(Value::Object(inner)) = schema
        .get_mut("items")
        .and_then(|items| items.get_mut("properties"))
    {
        for (field, schema) in inner.iter_mut() {
            describe(store, lp, name, &format!("{argument}.{field}"), schema);
        }
    }
}

/// The words, held for a moment.
pub fn hold(shared: &Shared) -> MutexGuard<'_, Prompts> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::Path};

    use super::*;

    /// Keys the code picks before it asks for them, with an `if`, a
    /// `match` or a helper of its own, so that the name is not written
    /// right after `text(` or `fill(`. Each must still be written in the
    /// code: see the test.
    const CHOSEN: &[&str] = &[
        // instructions.rs: the opening of a sitting, by how it began.
        "cleared_before",
        "cleared_after",
        "picked_up_before",
        "picked_up_after",
        // worker/mod.rs and worker/context.rs: by what went wrong, and by
        // how many images there were.
        "no_call",
        "cut_off",
        "thought_out",
        "interrupted",
        "not_started",
        "no_longer_shown_one",
        "no_longer_shown_many",
        // outside.rs: where a result came from, by tool.
        "outside_search",
        "outside_fetch",
        "outside_read",
        "outside_shell",
        "outside_job",
        "outside_tool",
        "outside_server",
        "outside_registry",
        "outside_install",
        "outside_skills",
        "outside_install_skill",
        "outside_skill",
        "outside_skill_file",
        // undertaking/state.rs: the parts of a step that it has.
        "plan_after",
        "plan_part",
        "plan_check",
        "plan_result",
        // tools/files.rs and tools/fence.rs.
        "unreadable",
        "a_directory",
        "a_binary_file",
        "too_large_whole",
        "old_whitespace_differs",
        "old_read_again",
        "in_their_folder",
        "outside_the_folder",
        // servers/: why a remote, a package or a start was refused.
        "remote_not_an_address",
        "remote_not_https",
        "remote_no_host",
        "package_not_run",
        "package_not_stdio",
        "package_name_unplain",
        "package_version_unpinned",
        "needs_ungiven",
        "needs_choosing",
        "no_folder",
        "no_log",
        "a_resource",
        "unnamed",
        // skills/: why a folder was refused, and what is not found.
        "repository_too_large",
        "no_skill_folder",
        "has_a_link",
        "has_a_repository",
        "path_leads_out",
        // parts.rs: how a part ended, where it stands, and its changes.
        "part_clashed",
        "part_kept",
        "part_step_done",
        "part_step_failed",
        "status_running",
        "status_queued",
        "status_asking",
        "status_finished",
        "status_blocked",
        "status_gave_up",
        "status_dropped",
    ];

    /// The code of the crate that is not a test, file by file.
    fn code() -> Vec<String> {
        fn walk(dir: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n != "tests") {
                        walk(&path, out);
                    }
                } else if path.extension().is_some_and(|e| e == "rs")
                    && path.file_name().is_some_and(|n| n != "tests.rs")
                {
                    // Up to the tests written in the file. A test module
                    // kept in a file of its own is only declared here.
                    let text = std::fs::read_to_string(&path).unwrap();
                    let mut chunks = text.split("#[cfg(test)]");
                    let mut code = chunks.next().unwrap_or_default().to_owned();
                    for chunk in chunks {
                        match chunk.trim_start().split_once('\n') {
                            Some((first, rest)) if first.ends_with(';') => code.push_str(rest),
                            _ => break,
                        }
                    }
                    out.push(code);
                }
            }
        }
        let mut out = Vec::new();
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out
    }

    /// The keys `code` asks for by name: a string written right after
    /// `text(` or `fill(`, with the table first or not.
    fn asked(code: &str) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for call in ["text(", "fill("] {
            for (at, _) in code.match_indices(call) {
                let named = code[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                if named {
                    continue;
                }
                let rest = code[at + call.len()..].trim_start();
                let rest = rest
                    .strip_prefix("TEXTS")
                    .and_then(|r| r.trim_start().strip_prefix(','))
                    .map_or(rest, str::trim_start);
                if let Some(key) = rest
                    .strip_prefix('"')
                    .and_then(|r| r.split_once('"'))
                    .map(|(key, _)| key)
                    .filter(|key| {
                        key.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    })
                {
                    keys.insert(key.to_owned());
                }
            }
        }
        keys
    }

    #[test]
    fn every_text_of_the_work_loop_is_asked_for_and_every_one_asked_for_is_there() {
        let tables: Vec<&str> = FILES
            .iter()
            .filter(|f| f.kind == Kind::Texts(TEXTS))
            .map(|f| f.path)
            .collect();
        let table: BTreeSet<String> = Prompts::shipped()
            .listing()
            .into_iter()
            .filter(|l| tables.contains(&l.file))
            .filter_map(|l| l.key)
            .collect();
        let code = code();
        let mut asked: BTreeSet<String> = code.iter().flat_map(|c| asked(c)).collect();
        for key in CHOSEN {
            let quoted = format!("\"{key}\"");
            assert!(
                code.iter().any(|c| c.contains(&quoted)),
                "{key} is listed as chosen, but nothing writes it"
            );
            asked.insert((*key).to_owned());
        }
        let missing: Vec<&String> = asked.difference(&table).collect();
        assert!(missing.is_empty(), "asked for, and in no file: {missing:?}");
        let unasked: Vec<&String> = table.difference(&asked).collect();
        assert!(
            unasked.is_empty(),
            "in a file, and never asked for: {unasked:?}"
        );
    }
}
