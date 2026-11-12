//! Tools, and the box a worker carries them in.
//!
//! A tool is a name, a description for the model, a schema for its arguments
//! and something to run. Tools never fail the loop: whatever goes wrong comes
//! back as text the model can read and act on.

use std::sync::Arc;

use async_trait::async_trait;
use ghost_brain::{Call, ToolSpec};
use serde_json::{Value, json};

/// What running a tool produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub output: String,
    pub failed: bool,
}

impl Outcome {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            failed: false,
        }
    }

    pub fn fail(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            failed: true,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn call(&self, args: &Value) -> Outcome;
}

/// Build a spec without the ceremony.
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

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }

    /// Only the named tools.
    #[must_use]
    pub fn only(&self, names: &[&str]) -> Self {
        Self {
            tools: self
                .tools
                .iter()
                .filter(|t| names.contains(&t.spec().name.as_str()))
                .cloned()
                .collect(),
        }
    }

    pub async fn run(&self, call: &Call) -> Outcome {
        let Some(tool) = self.tools.iter().find(|t| t.spec().name == call.name) else {
            let known: Vec<String> = self.tools.iter().map(|t| t.spec().name).collect();
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
