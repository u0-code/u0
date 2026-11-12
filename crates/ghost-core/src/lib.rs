//! # ghost-core
//!
//! The domain core of Ghost: who she is, what has happened to her, what she
//! can recall, where she stands with people, and what time does to her. No
//! network, no model calls, no async. Everything here can be embedded in any
//! binary.
//!
//! Three rules shape this crate:
//!
//! 1. **Identity is sealed.** [`identity::SealedIdentity`] has no mutator.
//! 2. **The journal is append-only.** [`journal::Journal`] has no update or
//!    delete, and the storage refuses both. [`Ghost::record`] is the only way
//!    in.
//! 3. **Everything else is derived.** [`memory::Memory`] and every other view
//!    can be thrown away and rebuilt from the journal.

pub mod error;
pub mod ghost;
pub mod identity;
pub mod journal;
pub mod life;
pub mod memory;
pub mod narrate;
pub mod social;
pub mod tempo;
pub mod will;

pub use error::{Error, Result};
pub use ghost::Ghost;
