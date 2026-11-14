//! What a worker is told, and what its tools are said to be. Data, not
//! code: see [`ghost_core::prompts`] for where it is looked for and how a
//! change is taken in.
//!
//! `work.md` is the working instructions, `work_texts.toml` what is said
//! along the way, and `tools.toml` what every tool is, in a conversation as
//! at work. A worker looks at the files before each step, so an edit
//! reaches work that is already running at its next step.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use chrono::{DateTime, Utc};
use ghost_brain::ToolSpec;
pub use ghost_core::prompts::{DIR, File, Kind, Listed, Loop, Origin, Store};
use serde_json::{Value, json};

const TEXTS: &str = "work_texts.toml";

/// Every file the work loop reads, with what is built in.
pub const FILES: &[File] = &[
    File {
        name: "work.md",
        built_in: include_str!("../prompts/work.md"),
        kind: Kind::Whole,
    },
    File {
        name: TEXTS,
        built_in: include_str!("../prompts/work_texts.toml"),
        kind: Kind::Texts,
    },
    File {
        name: "tools.toml",
        built_in: include_str!("../prompts/tools.toml"),
        kind: Kind::Tools,
    },
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

    /// The words for work kept in `dir`: its own `prompts/` first, then
    /// `config` if there is one, then what is built in.
    pub fn load(dir: &Path, config: Option<&Path>) -> Self {
        Self {
            store: Store::load(FILES, dir, config),
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

    /// `work.md`, as it is.
    pub fn work(&self) -> &str {
        self.store.whole("work.md")
    }

    /// A text from `work_texts.toml`, as it is.
    pub fn text(&self, key: &str) -> &str {
        self.store.text(TEXTS, key)
    }

    /// A text from `work_texts.toml`, with its placeholders filled.
    pub fn fill(&self, key: &str, pairs: &[(&str, String)]) -> String {
        self.store.fill(TEXTS, key, pairs)
    }

    /// Every template, text and tool, with where what is in use came from.
    pub fn listing(&self) -> Vec<Listed> {
        self.store.listing()
    }

    /// The spec of tool `name` as it is offered in `lp`: `properties` are
    /// its arguments' JSON schemas, to which what `tools.toml` says of each
    /// is added as its description. An argument inside a list of objects is
    /// described under its path, `steps.text`.
    #[allow(clippy::needless_pass_by_value)]
    pub fn spec(&self, lp: Loop, name: &str, mut properties: Value, required: &[&str]) -> ToolSpec {
        if let Value::Object(map) = &mut properties {
            for (argument, schema) in map.iter_mut() {
                describe(&self.store, lp, name, argument, schema);
            }
        }
        ToolSpec {
            name: name.to_owned(),
            description: self.store.tool(name, lp).to_owned(),
            parameters: json!({
                "type": "object",
                "properties": properties,
                "required": required,
            }),
        }
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
