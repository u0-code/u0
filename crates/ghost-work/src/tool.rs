//! Tools, and the box a worker carries them in.
//!
//! A tool is a name, a description for the model, a schema for its arguments
//! and something to run. Tools never fail the loop: whatever goes wrong comes
//! back as text the model can read and act on.

use std::sync::Arc;

use async_trait::async_trait;
use ghost_brain::{Call, Picture, ToolSpec};
use serde_json::{Value, json};

use crate::prompts::{Loop, Store};

/// What running a tool produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub output: String,
    pub failed: bool,
    /// Images to show the model with the output. Most tools have none.
    pub pictures: Vec<Picture>,
}

impl Outcome {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            failed: false,
            pictures: Vec::new(),
        }
    }

    pub fn fail(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            failed: true,
            pictures: Vec::new(),
        }
    }

    /// A picture, with a line or two about it.
    pub fn seen(output: impl Into<String>, picture: Picture) -> Self {
        Self {
            pictures: vec![picture],
            ..Self::ok(output)
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    /// What the model calls it by.
    fn name(&self) -> &'static str;
    /// What the model is told about it in `lp`, in the words of `words`.
    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec;
    async fn call(&self, args: &Value) -> Outcome;
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

/// A required string argument, or a sentence saying it is missing.
pub fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, Outcome> {
    args[key]
        .as_str()
        .ok_or_else(|| Outcome::fail(format!("missing argument `{key}` (a string)")))
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

    /// What the model is told about each tool, in `lp`.
    pub fn specs(&self, words: &Store, lp: Loop) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec(words, lp)).collect()
    }

    /// The name of each tool, in order.
    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name().to_owned()).collect()
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

    pub async fn run(&self, call: &Call) -> Outcome {
        let Some(tool) = self.tools.iter().find(|t| t.name() == call.name) else {
            let known = self.names();
            return Outcome::fail(format!(
                "there is no tool called `{}`. The tools are: {}.",
                call.name,
                known.join(", ")
            ));
        };
        if !call.args.is_object() {
            return Outcome::fail(
                "the arguments were not a valid JSON object. Call the tool again with well-formed JSON; \
                 for long or awkward content, write it in smaller pieces.",
            );
        }
        tool.call(&call.args).await
    }
}
