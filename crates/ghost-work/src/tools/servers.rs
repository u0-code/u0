//! `find_tools`, `install`, `uninstall` and `load_tools`: the tools she
//! gets more tools with. See [`crate::servers`] for what they act on and
//! why it is safe to act on it without asking anyone.

use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use ghost_brain::ToolSpec;
use serde_json::{Value, json};

use crate::{
    prompts::{self, Loop, Store, TEXTS},
    servers::{Servers, guard::Vetted},
    tool::{Outcome, Tool, text},
};

/// Search the registry.
pub struct FindTools(Arc<Servers>);

impl FindTools {
    pub fn new(servers: Arc<Servers>) -> Self {
        Self(servers)
    }
}

#[async_trait]
impl Tool for FindTools {
    fn name(&self) -> &'static str {
        "find_tools"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "find_tools",
            json!({"query": {"type": "string"}}),
            &["query"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let query = match text(args, "query") {
            Ok(q) => q,
            Err(refusal) => return refusal,
        };
        match self.0.find(query).await {
            Ok(found) => Outcome::ok(found),
            Err(why) => Outcome::fail(why),
        }
    }
}

/// Install a server from the registry.
pub struct Install(Arc<Servers>);

impl Install {
    pub fn new(servers: Arc<Servers>) -> Self {
        Self(servers)
    }
}

#[async_trait]
impl Tool for Install {
    fn name(&self) -> &'static str {
        "install"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "install",
            json!({"name": {"type": "string"}}),
            &["name"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let name = match text(args, "name") {
            Ok(n) => n,
            Err(refusal) => return refusal,
        };
        match self.0.install(name).await {
            Ok(said) => Outcome::ok(said),
            Err(why) => Outcome::fail(format!("{name} was not installed: {why}")),
        }
    }
}

/// Take out a server she installed.
pub struct Uninstall(Arc<Servers>);

impl Uninstall {
    pub fn new(servers: Arc<Servers>) -> Self {
        Self(servers)
    }
}

#[async_trait]
impl Tool for Uninstall {
    fn name(&self) -> &'static str {
        "uninstall"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "uninstall",
            json!({"name": {"type": "string"}, "why": {"type": "string"}}),
            &["name", "why"],
        )
    }

    async fn call(&self, args: &Value) -> Outcome {
        let (name, why) = match (text(args, "name"), text(args, "why")) {
            (Ok(n), Ok(w)) => (n, w),
            (Err(refusal), _) | (_, Err(refusal)) => return refusal,
        };
        match self.0.uninstall(name, why) {
            Ok(said) => Outcome::ok(said),
            Err(why) => Outcome::fail(why),
        }
    }
}

/// The servers loaded in one look at a conversation or one sitting of
/// work. Whoever holds the look keeps it, so that a toolbox made again
/// for the next turn still has what was loaded.
#[derive(Clone, Default, Debug)]
pub struct Loaded(Arc<Mutex<Vec<String>>>);

impl Loaded {
    fn add(&self, handle: &str) {
        let mut loaded = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !loaded.iter().any(|h| h == handle) {
            loaded.push(handle.to_owned());
        }
    }

    /// The servers loaded so far, in the order they were.
    pub fn handles(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Make a server's tools callable, and bring them.
pub struct LoadTools {
    servers: Arc<Servers>,
    loaded: Loaded,
}

impl LoadTools {
    pub fn new(servers: Arc<Servers>, loaded: Loaded) -> Self {
        Self { servers, loaded }
    }
}

#[async_trait]
impl Tool for LoadTools {
    fn name(&self) -> &'static str {
        "load_tools"
    }

    /// Its description ends with every server she has, a line each, so
    /// that she knows what there is to load without any of it loaded.
    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        let mut spec = prompts::spec(
            words,
            lp,
            "load_tools",
            json!({"server": {"type": "string"}}),
            &["server"],
        );
        let lines: Vec<String> = self
            .servers
            .listing()
            .into_iter()
            .map(|s| {
                words.fill(
                    TEXTS,
                    "server_line",
                    &[
                        ("server", s.handle),
                        (
                            "description",
                            crate::servers::short(s.description.trim(), 160),
                        ),
                        ("tools", s.tools.join(", ")),
                    ],
                )
            })
            .collect();
        spec.description =
            ghost_core::narrate::fill(&spec.description, &[("servers", lines.join("\n"))]);
        spec
    }

    async fn call(&self, args: &Value) -> Outcome {
        let which = match text(args, "server") {
            Ok(s) => s,
            Err(refusal) => return refusal,
        };
        match self.servers.load(which).await {
            Ok((handle, offered)) => {
                self.loaded.add(&handle);
                Outcome::ok(format!(
                    "Loaded {handle}. You can now call its tools directly: {}.",
                    offered
                        .iter()
                        .map(|t| t.called.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
            Err(why) => Outcome::fail(why),
        }
    }

    fn brings(&self) -> Vec<Arc<dyn Tool>> {
        self.loaded
            .handles()
            .into_iter()
            .flat_map(|handle| {
                self.servers
                    .offered(&handle)
                    .into_iter()
                    .map(move |tool| (handle.clone(), tool))
            })
            .map(|(handle, tool)| {
                Arc::new(Served {
                    servers: self.servers.clone(),
                    handle,
                    tool,
                }) as Arc<dyn Tool>
            })
            .collect()
    }
}

/// One tool of a server, as she calls it.
struct Served {
    servers: Arc<Servers>,
    handle: String,
    tool: Vetted,
}

#[async_trait]
impl Tool for Served {
    fn name(&self) -> &str {
        &self.tool.called
    }

    fn spec(&self, _words: &Store, _lp: Loop) -> ToolSpec {
        ToolSpec {
            name: self.tool.called.clone(),
            description: self.tool.description.clone(),
            parameters: self.tool.schema.clone(),
        }
    }

    async fn call(&self, args: &Value) -> Outcome {
        self.servers
            .call(&self.handle, &self.tool.remote, args)
            .await
    }
}
