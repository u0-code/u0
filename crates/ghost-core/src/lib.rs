//! # ghost-core
//!
//! The domain core of Ghost: who she is, what has happened to her, and what
//! time does to her. No network, no model calls, no channels. Everything here
//! can be embedded in any binary.
//!
//! Three rules shape this crate:
//!
//! 1. **Identity is sealed.** [`identity::SealedIdentity`] has no mutator.
//! 2. **The journal is append-only.** [`journal::Journal`] has no update or
//!    delete, and the storage refuses both.
//! 3. **Everything else is derived.** Any state not in the journal can be
//!    thrown away and rebuilt.

pub mod error;
pub mod ghost;
pub mod identity;
pub mod journal;
pub mod life;
pub mod narrate;

pub use error::{Error, Result};
pub use ghost::Ghost;
