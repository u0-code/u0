//! # ghost-work
//!
//! The loop a ghost does things with.
//!
//! A [`Worker`] takes a model that can use tools ([`ghost_brain::Brain`]), a
//! [`Toolbox`], and an [`Undertaking`], and works until the undertaking is
//! finished or something stops it. It knows nothing about who is working:
//! that is a paragraph handed in by whoever starts it. On its own it is a
//! task-solving agent; inside a ghost it is how she does anything that takes
//! more than one thought.
//!

pub mod catalog;
pub mod claude;
pub mod codex;
pub mod machine;
pub mod mcp;
pub mod prompts;
pub mod tool;
pub mod tools;
pub mod undertaking;
pub mod worker;

pub use machine::{Docker, Local, Machine};
pub use prompts::Prompts;
pub use tool::{Outcome, Tool, Toolbox};
pub use undertaking::{Undertaking, WORK_DB};
pub use worker::{Ended, Limits, Report, Worker};
