//! Tools, and the box a worker carries them in.
//!
//! A tool is a name, a description for the model, a schema for its arguments
//! and something to run. Tools never fail the loop: whatever goes wrong comes
//! back as text the model can read and act on, in the words it is given.

use std::sync::Arc;

use async_trait::async_trait;
use ghost_brain::{Call, Picture, ToolSpec};
use serde_json::{Value, json};

use crate::prompts::{Loop, Store, TEXTS};

/// What running a tool produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub output: String,
    pub failed: bool,
    /// Images to show the model with the output. Most tools have none.
    pub pictures: Vec<Picture>,
    /// Who wrote what came back, when the tool knows and its arguments do
    /// not say: a skill, by the repository and commit it was installed
    /// from. Named where it is marked as from outside (see
    /// [`crate::outside`]). Most tools leave it out.
    pub origin: Option<String>,
}

impl Outcome {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            failed: false,
            pictures: Vec::new(),
            origin: None,
        }
    }

    pub fn fail(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            failed: true,
            pictures: Vec::new(),
            origin: None,
        }
    }

    /// A picture, with a line or two about it.
    pub fn seen(output: impl Into<String>, picture: Picture) -> Self {
        Self {
            pictures: vec![picture],
            ..Self::ok(output)
        }
    }

    /// The same, saying who wrote it.
    #[must_use]
    pub fn from(self, origin: impl Into<String>) -> Self {
        Self {
            origin: Some(origin.into()),
            ..self
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    /// What the model calls it by.
    fn name(&self) -> &str;
    /// What the model is told about it in `lp`, in the words of `words`.
    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec;
    /// Run it with `args`. What it says back, it says in the words of
    /// `words`, as its spec does.
    async fn call(&self, args: &Value, words: &Store) -> Outcome;
    /// Tools this one has made usable, offered right after it. A tool that
    /// loads others, such as `load_tools`, brings them; the rest bring
    /// none. Asked each time the tools are listed, so what is brought can
    /// grow while the box is in use.
    fn brings(&self) -> Vec<Arc<dyn Tool>> {
        Vec::new()
    }
}

/// Build a spec without the ceremony, for a tool whose words are not
/// kept in `tools.toml`.
#[allow(clippy::needless_pass_by_value)]
pub fn spec(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolSpec {
    ToolSpec {
        name: name.to_owned(),
        description: description.to_owned(),
        parameters: json!({
            "type": "object",
            "properties": properties,
            "required": required,
        }),
    }
}

/// A required string argument, or a sentence in `words` saying it is
/// missing.
pub fn text<'a>(words: &Store, args: &'a Value, key: &str) -> Result<&'a str, Outcome> {
    args[key].as_str().ok_or_else(|| {
        Outcome::fail(words.fill(TEXTS, "missing_argument", &[("argument", key.to_owned())]))
    })
}

#[derive(Clone, Default)]
pub struct Toolbox {
    tools: Vec<Arc<dyn Tool>>,
}

impl Toolbox {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    /// The same tools, with the one called `name` replaced, in its place,
    /// by what `wrap` makes of it.
    #[must_use]
    pub fn wrapping(
        mut self,
        name: &str,
        wrap: impl FnOnce(Arc<dyn Tool>) -> Arc<dyn Tool>,
    ) -> Self {
        if let Some(at) = self.tools.iter().position(|t| t.name() == name) {
            let tool = self.tools[at].clone();
            self.tools[at] = wrap(tool);
        }
        self
    }

    /// Every tool, each followed by what it brings, in order.
    fn all(&self) -> Vec<Arc<dyn Tool>> {
        let mut all = Vec::with_capacity(self.tools.len());
        for tool in &self.tools {
            all.push(tool.clone());
            all.extend(tool.brings());
        }
        all
    }

    /// What the model is told about each tool, in `lp`.
    pub fn specs(&self, words: &Store, lp: Loop) -> Vec<ToolSpec> {
        self.all().iter().map(|t| t.spec(words, lp)).collect()
    }

    /// The name of each tool, in order.
    pub fn names(&self) -> Vec<String> {
        self.all().iter().map(|t| t.name().to_owned()).collect()
    }

    /// Whether it carries a tool called `name`.
    pub fn has(&self, name: &str) -> bool {
        self.all().iter().any(|t| t.name() == name)
    }

    /// Whether the tool called `name` was brought by another rather than
    /// carried: installed from elsewhere, and so outside (see
    /// [`crate::outside`]).
    pub fn brought(&self, name: &str) -> bool {
        self.tools
            .iter()
            .flat_map(|t| t.brings())
            .any(|t| t.name() == name)
    }

    /// Only the named tools.
    #[must_use]
    pub fn only(&self, names: &[&str]) -> Self {
        Self {
            tools: self
                .tools
                .iter()
                .filter(|t| names.contains(&t.name()))
                .cloned()
                .collect(),
        }
    }

    /// Run `call`, saying what went wrong with it in `words`.
    pub async fn run(&self, call: &Call, words: &Store) -> Outcome {
        let Some(tool) = self.all().into_iter().find(|t| t.name() == call.name) else {
            return Outcome::fail(words.fill(
                TEXTS,
                "no_tool",
                &[
                    ("tool", call.name.clone()),
                    ("tools", self.names().join(", ")),
                ],
            ));
        };
        if !call.args.is_object() {
            return Outcome::fail(words.text(TEXTS, "not_json"));
        }
        tool.call(&call.args, words).await
    }
}
