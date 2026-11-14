//! One step of work: the model sees a transcript and a set of tools, and
//! either says something or asks for tools to be run.
//!
//! This is the interface a loop is built on. A provider that can do it is a
//! *runtime* for work; one that cannot can still answer a [`Completion`].
//!
//! [`Completion`]: crate::Completion

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Picture;

/// A tool the model may ask for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments object.
    pub parameters: Value,
}

/// The model asking for a tool to be run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    /// The provider's id for this call. The result must carry the same one.
    pub id: String,
    pub name: String,
    /// The arguments as an object. If the model wrote something that is not
    /// JSON, this is that text as a string, and the tool should refuse it.
    pub args: Value,
}

/// One thing in a working transcript, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "snake_case")]
pub enum Item {
    /// Something said to the model: the task, a nudge, new information.
    User { text: String },
    /// What the model said and asked for in one step.
    Assistant {
        text: String,
        calls: Vec<Call>,
        /// Whatever the provider needs handed back unchanged on the next
        /// request (for instance a reasoning trace). Opaque to everyone else.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        carry: Option<Value>,
    },
    /// What running a tool produced.
    Result {
        id: String,
        name: String,
        output: String,
        #[serde(default)]
        failed: bool,
        /// Images the tool returned. A brain that [sees] is shown them with
        /// the output; one that does not is told how many there were.
        ///
        /// [sees]: crate::Brain::sees
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pictures: Vec<Picture>,
    },
}

/// What one picture is counted as in [`Item::weight`], in characters:
/// 4,800, about 1,600 tokens at three characters a token. That is what
/// Anthropic charges for an image at the largest size it takes without
/// scaling it down, and more than `OpenAI` charges for one in high detail,
/// so a transcript of pictures is never thought smaller than it is.
pub const PICTURE_WEIGHT: usize = 4800;

/// What a model that cannot see is told in place of `n` pictures.
pub fn not_shown(n: usize) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("[{n} image{s}, not shown: this model cannot see images]")
}

impl Item {
    pub fn user(text: impl Into<String>) -> Self {
        Self::User { text: text.into() }
    }

    /// The result of a call, with no pictures.
    pub fn result(
        id: impl Into<String>,
        name: impl Into<String>,
        output: impl Into<String>,
        failed: bool,
    ) -> Self {
        Self::Result {
            id: id.into(),
            name: name.into(),
            output: output.into(),
            failed,
            pictures: Vec::new(),
        }
    }

    /// A rough size in characters, for budgeting context.
    pub fn weight(&self) -> usize {
        match self {
            Self::User { text } => text.len(),
            Self::Assistant { text, calls, carry } => {
                text.len()
                    + calls
                        .iter()
                        .map(|c| c.name.len() + c.args.to_string().len())
                        .sum::<usize>()
                    + carry.as_ref().map_or(0, |c| c.to_string().len())
            }
            Self::Result {
                output, pictures, ..
            } => output.len() + pictures.len() * PICTURE_WEIGHT,
        }
    }
}

/// How hard a model that thinks before it answers should think. What a
/// model thinks is spent from the same allowance as what it writes, so this
/// is also how much of `max_tokens` is left for the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// Do not think: answer.
    Off,
    Low,
    High,
    Max,
}

/// A request for one step.
#[derive(Debug, Clone)]
pub struct Turn {
    pub system: String,
    pub items: Vec<Item>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    /// `None` leaves it to the provider. A provider that cannot be told
    /// ignores it.
    pub effort: Option<Effort>,
}

/// Why the model stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
    /// It finished what it had to say.
    End,
    /// It wants tools run.
    Calls,
    /// It ran out of output tokens mid-thought.
    Length,
}

/// Tokens spent on one step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Everything the model read, cached or not.
    pub input: u64,
    /// The part of `input` served from the provider's prompt cache.
    pub cached: u64,
    /// Everything the model produced, thinking included.
    pub output: u64,
    /// The part of `output` that was thinking, where the provider says.
    #[serde(default)]
    pub thought: u64,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input += other.input;
        self.cached += other.cached;
        self.output += other.output;
        self.thought += other.thought;
    }
}

/// What came back from one step.
#[derive(Debug, Clone)]
pub struct Step {
    pub text: String,
    pub calls: Vec<Call>,
    pub stop: Stop,
    pub usage: Usage,
    pub model: String,
    /// To be stored on the [`Item::Assistant`] this step becomes.
    pub carry: Option<Value>,
}

impl Step {
    /// This step as a transcript item.
    pub fn into_item(self) -> Item {
        Item::Assistant {
            text: self.text,
            calls: self.calls,
            carry: self.carry,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_result_with_pictures_is_written_with_them_as_base64_and_one_without_is_written_as_before()
    {
        let plain = Item::result("a", "shell", "ok", false);
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({"is": "result", "id": "a", "name": "shell", "output": "ok", "failed": false})
        );
        let seen = Item::Result {
            id: "b".into(),
            name: "fetch".into(),
            output: "An image.".into(),
            failed: false,
            pictures: vec![Picture {
                mime: "image/png".into(),
                bytes: vec![1, 2, 3],
            }],
        };
        let written = serde_json::to_value(&seen).unwrap();
        assert_eq!(
            written["pictures"],
            json!([{"mime": "image/png", "data": "AQID"}])
        );
        assert_eq!(serde_json::from_value::<Item>(written).unwrap(), seen);
        // A picture weighs the same however small its bytes.
        assert_eq!(seen.weight(), "An image.".len() + PICTURE_WEIGHT);
    }
}
