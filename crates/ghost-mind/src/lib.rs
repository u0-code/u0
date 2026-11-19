//! # ghost-mind
//!
//! The part of Ghost that lives. A [`Mind`] is a single-writer actor that owns
//! a [`ghost_core::Ghost`] and is driven by time, not by requests: messages
//! land in her journal as unread, an agenda decides when she looks, a brain
//! returns a set of [`actions`] rather than a reply, and a small state machine
//! delivers what she says at the pace a person types.
//!
//! The mind knows nothing about platforms or wire formats. It acts through a
//! [`Body`] and perceives the world through [`Senses`]; the daemon supplies
//! both.

pub mod actions;
pub mod body;
pub mod config;
pub mod mind;
mod narrate;
pub mod prompts;
pub mod senses;

pub use body::{Act, Acted, Body, Can, ChannelFacts, Feed, Incoming, Measure};
pub use config::{CONSOLE, ChannelConfig, MindConfig, Operators, PlaceConfig, WorkConfig};
pub use mind::{
    Mind, MindHandle, Operator, Order, Parts, Question, Refused, SessionChange, Workshop,
};
pub use prompts::Prompts;
pub use senses::{Forecast, Numb, OpenMeteo, Senses};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] ghost_core::Error),
    #[error(transparent)]
    Brain(#[from] ghost_brain::Error),
    #[error("could not understand the brain: {0}")]
    Parse(String),
    #[error("the mind has stopped")]
    Stopped,
}

pub type Result<T> = std::result::Result<T, Error>;
