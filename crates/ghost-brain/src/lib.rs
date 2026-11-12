//! # ghost-brain
//!
//! Model providers behind one trait. The brain is tissue, not identity: a
//! ghost can change brains and remain herself, so nothing here knows what a
//! ghost is.

pub mod anthropic;
pub mod cli;
pub mod config;
pub mod openai;

use std::{fmt::Write as _, time::Duration};

use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            text: text.into(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            text: text.into(),
        }
    }
}

/// A request for one reply.
#[derive(Debug, Clone)]
pub struct Completion {
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
}

impl Completion {
    /// Flatten the exchange into one prompt, for providers that only take a
    /// single string. `them` and `her` label the two sides.
    pub fn transcript(&self, them: &str, her: &str) -> String {
        let mut out = String::new();
        if !self.system.trim().is_empty() {
            out.push_str(self.system.trim());
            out.push_str("\n\n");
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
    #[error("{0} is not installed or not on PATH")]
    NotInstalled(String),
    #[error("{program} exited with {status}: {stderr}")]
    Cli {
        program: String,
        status: String,
        stderr: String,
    },
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

/// Something that can produce her next message.
#[async_trait]
pub trait Brain: Send + Sync {
    /// Human-readable, e.g. `anthropic/claude-sonnet-5`.
    fn label(&self) -> String;

    async fn complete(&self, req: &Completion) -> Result<Reply>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_labels_both_sides() {
        let c = Completion {
            system: "Be Lin.".into(),
            messages: vec![
                Message::user("hi"),
                Message::assistant("hey"),
                Message::user("?"),
            ],
            max_tokens: 10,
            temperature: None,
        };
        let t = c.transcript("someone", "Lin");
        assert!(t.starts_with("Be Lin."));
        assert!(t.contains("[someone]: hi\n[Lin]: hey\n[someone]: ?"));
        assert!(t.ends_with("Output only the message text."));
    }
}
