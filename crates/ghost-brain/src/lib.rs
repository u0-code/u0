//! # ghost-brain
//!
//! Model providers behind one trait. The brain is tissue, not identity: a
//! ghost can change brains and remain herself, so nothing here knows what a
//! ghost is.

pub mod anthropic;
pub mod cli;
pub mod config;
pub mod discover;
mod http;
pub mod openai;
pub mod step;

use std::{fmt::Write as _, time::Duration};

use async_trait::async_trait;

pub use step::{Call, Effort, Item, PICTURE_WEIGHT, Step, Stop, ToolSpec, Turn, Usage, not_shown};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
    /// Images that go with the text. Only a brain that [`Brain::sees`] is
    /// shown them; the rest read the text alone.
    pub images: Vec<Picture>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            text: text.into(),
            images: Vec::new(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            text: text.into(),
            images: Vec::new(),
        }
    }

    /// The same message, with images to look at.
    #[must_use]
    pub fn with_images(mut self, images: Vec<Picture>) -> Self {
        self.images = images;
        self
    }
}

/// An image, as bytes and the type they are. Written out, it is the type
/// and the bytes as base64.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(into = "WrittenPicture", try_from = "WrittenPicture")]
pub struct Picture {
    /// e.g. `image/png`.
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WrittenPicture {
    mime: String,
    data: String,
}

impl From<Picture> for WrittenPicture {
    fn from(p: Picture) -> Self {
        Self {
            data: p.base64(),
            mime: p.mime,
        }
    }
}

impl TryFrom<WrittenPicture> for Picture {
    type Error = base64::DecodeError;

    fn try_from(w: WrittenPicture) -> std::result::Result<Self, Self::Error> {
        use base64::Engine as _;
        Ok(Self {
            mime: w.mime,
            bytes: base64::engine::general_purpose::STANDARD.decode(w.data)?,
        })
    }
}

impl Picture {
    /// The bytes as base64, which is how every provider takes them.
    pub fn base64(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(&self.bytes)
    }

    /// A `data:` URL holding the image.
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.base64())
    }
}

// The bytes are left out: an image in a log is noise at best.
impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({}, {} bytes)", self.mime, self.bytes.len())
    }
}

/// A request for one reply.
#[derive(Debug, Clone)]
pub struct Completion {
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    /// The caller expects a single JSON object back. Providers that can
    /// enforce this do; the rest rely on the prompt.
    pub json: bool,
    /// How hard to think first. `None` leaves it to the provider.
    pub effort: Option<Effort>,
}

impl Completion {
    /// One instruction, one answer.
    pub fn ask(system: impl Into<String>, prompt: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            system: system.into(),
            messages: vec![Message::user(prompt)],
            max_tokens,
            temperature: None,
            json: false,
            effort: None,
        }
    }

    #[must_use]
    pub fn expecting_json(mut self) -> Self {
        self.json = true;
        self
    }

    #[must_use]
    pub fn thinking(mut self, effort: Effort) -> Self {
        self.effort = Some(effort);
        self
    }

    /// Flatten the request into one prompt, for providers that only take a
    /// single string. A lone user message passes through as written; a longer
    /// exchange is laid out with `them` and `her` labelling the two sides.
    pub fn transcript(&self, them: &str, her: &str) -> String {
        let mut out = String::new();
        if !self.system.trim().is_empty() {
            out.push_str(self.system.trim());
            out.push_str("\n\n");
        }
        if let [only] = self.messages.as_slice() {
            out.push_str(only.text.trim());
            return out;
        }
        out.push_str("<conversation>\n");
        for m in &self.messages {
            let who = match m.role {
                Role::User => them,
                Role::Assistant => her,
            };
            let _ = writeln!(out, "[{who}]: {}", m.text.trim());
        }
        out.push_str("</conversation>\n\n");
        let _ = write!(
            out,
            "Write {her}'s next message, as {her}. Output only the message text."
        );
        out
    }
}

#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub text: String,
    pub model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing API key: set {0}")]
    MissingKey(String),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{provider} returned {status}: {body}")]
    Api {
        provider: String,
        status: u16,
        body: String,
    },
    /// The request held more than the model's context window takes. Sending
    /// it again will not help; sending less will.
    #[error("{provider} says the context is too long: {detail}")]
    TooLong { provider: String, detail: String },
    #[error("{0} is not installed or not on PATH")]
    NotInstalled(String),
    #[error("{program} exited with {status}: {stderr}")]
    Cli {
        program: String,
        status: String,
        stderr: String,
    },
    #[error("{0} cannot run tools; it can only answer")]
    NoTools(String),
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("malformed reply from {provider}: {detail}")]
    Malformed { provider: String, detail: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A command-line agent that brings its own loop and its own tools: it is
/// handed a part of a piece of work, not stepped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliAgent {
    pub kind: CliKind,
    /// The program to run.
    pub program: String,
    pub model: Option<String>,
    /// Extra arguments for every run, such as `-c` settings that point the
    /// Codex CLI at another provider.
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliKind {
    Claude,
    Codex,
}

/// Something that can think for her.
///
/// Every brain can answer a [`Completion`]: text in, text out. A brain that
/// is also a runtime for work can take a [`Turn`]: a transcript and a set of
/// tools, one step at a time. The loop that drives those steps lives
/// elsewhere; a brain never runs a tool itself.
#[async_trait]
pub trait Brain: Send + Sync {
    /// Human-readable, e.g. `anthropic/claude-sonnet-5`.
    fn label(&self) -> String;

    async fn complete(&self, req: &Completion) -> Result<Reply>;

    /// Whether [`Brain::step`] is implemented.
    fn works(&self) -> bool {
        false
    }

    /// Whether this brain is shown the images in a [`Message`]. One that is
    /// not reads the text and nothing else.
    fn sees(&self) -> bool {
        false
    }

    /// One step of work. The default refuses.
    async fn step(&self, _turn: &Turn) -> Result<Step> {
        Err(Error::NoTools(self.label()))
    }

    /// How many tokens one request to this model may hold, input and
    /// output together, if that is known. A loop that keeps a transcript
    /// uses it to decide when the transcript has to shrink; `None` means
    /// assume little.
    fn window(&self) -> Option<u64> {
        None
    }

    /// The most this model can write in one answer, thinking included, if
    /// that is known. A caller that does not know asks for little.
    fn max_output(&self) -> Option<u32> {
        None
    }

    /// If this brain is a command-line agent, how to start it. Such an
    /// agent cannot take a [`Turn`], but it can be given our tools and left
    /// to step through a piece of work itself.
    fn cli(&self) -> Option<CliAgent> {
        None
    }
}

/// The most a model this crate has been run against writes in one answer.
pub(crate) fn known_max_output(model: &str) -> Option<u32> {
    let model = model.to_lowercase();
    if model.starts_with("deepseek-flash") || model.starts_with("deepseek-v4") {
        Some(393_216)
    } else if model.starts_with("claude-") {
        Some(64_000)
    } else {
        None
    }
}

/// Whether a model can be told how hard to think, in the way its provider's
/// `OpenAI`-compatible endpoint takes it.
pub(crate) fn known_to_think(model: &str) -> bool {
    let model = model.to_lowercase();
    model.starts_with("deepseek-flash") || model.starts_with("deepseek-v4")
}

/// The context window of a model this crate has been run against, by name.
/// Anything else has to say so in its configuration (`context_window`).
pub(crate) fn known_window(model: &str) -> Option<u64> {
    let model = model.to_lowercase();
    if model.starts_with("deepseek-flash") || model.starts_with("deepseek-v4") {
        // What DeepSeek's `/models` listing declares for these two.
        Some(1_048_576)
    } else if model.starts_with("claude-") {
        Some(200_000)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_known_for_what_was_run() {
        assert_eq!(known_window("deepseek-flash"), Some(1_048_576));
        assert_eq!(known_window("claude-sonnet-5"), Some(200_000));
        assert_eq!(known_window("deepseek-v4-pro"), Some(1_048_576));
        // An older name the listing no longer declares anything for.
        assert_eq!(known_window("deepseek-chat"), None);
        assert_eq!(known_window("qwen3"), None);
    }

    #[test]
    fn transcript_labels_both_sides() {
        let c = Completion {
            system: "Be Weiwei.".into(),
            messages: vec![
                Message::user("hi"),
                Message::assistant("hey"),
                Message::user("?"),
            ],
            max_tokens: 10,
            temperature: None,
            json: false,
            effort: None,
        };
        let t = c.transcript("someone", "Weiwei");
        assert!(t.starts_with("Be Weiwei."));
        assert!(t.contains("[someone]: hi\n[Weiwei]: hey\n[someone]: ?"));
        assert!(t.ends_with("Output only the message text."));
    }

    #[test]
    fn a_picture_is_a_data_url_and_its_bytes_stay_out_of_logs() {
        let p = Picture {
            mime: "image/png".into(),
            bytes: vec![1, 2, 3],
        };
        assert_eq!(p.data_url(), "data:image/png;base64,AQID");
        assert_eq!(format!("{p:?}"), "Picture(image/png, 3 bytes)");
    }

    #[test]
    fn a_lone_message_passes_through() {
        let c = Completion::ask("Be Weiwei.", "What now?", 10);
        assert_eq!(c.transcript("them", "you"), "Be Weiwei.\n\nWhat now?");
    }
}
