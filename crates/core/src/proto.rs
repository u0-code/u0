//! What the server says to the desk: each answer to what was asked of an
//! agent, and what an agent says on its own, as an [`Event`].
//!
//! What goes over the wire to an agent is the ACP schema's, carried here as
//! it is, never mirrored.

use std::{collections::HashMap, path::PathBuf};

use agent_client_protocol::v1::schema::{
    ForkSessionResponse, InitializeResponse, NewSessionResponse, SessionConfigOption, SessionInfo,
    SessionModeState, StopReason,
};

use crate::{
    acp::{self, Error},
    registry,
};

/// What the server sends back.
pub enum Event {
    Registry(Result<registry::Registry, String>),
    Installing {
        agent: String,
        what: String,
        share: Option<f32>,
    },
    Installed {
        agent: String,
        result: Result<(), String>,
    },
    /// An agent started, with its process's id and what it said of itself,
    /// or why it did not.
    Started {
        agent: String,
        result: Result<(Option<u32>, Box<InitializeResponse>), Error>,
    },
    /// Something an agent said on its own.
    Said {
        agent: String,
        event: Box<acp::Event>,
    },
    Opened {
        key: u64,
        result: Result<Box<NewSessionResponse>, Error>,
    },
    /// The sessions `agent` keeps in the projects, each with its project.
    Listed {
        agent: String,
        result: Result<Vec<(PathBuf, SessionInfo)>, String>,
    },
    /// The folders `agent` has sessions in that are not projects, with how
    /// many in each.
    Elsewhere {
        agent: String,
        folders: Vec<(PathBuf, usize)>,
    },
    /// A session the agent keeps was loaded: what was said in it came
    /// before this, as updates.
    Loaded {
        key: u64,
        /// Resumed from the desk's record, not loaded with what was said.
        resumed: bool,
        result: Result<Opened, Error>,
    },
    /// What a session of `agent` offers before it is opened, or why the
    /// agent would not open one.
    Probed {
        agent: String,
        result: Result<Box<NewSessionResponse>, Error>,
    },
    /// A session branched off the session `from` (before the item `at` of
    /// its thread, or whole), or why it was not.
    Forked {
        from: u64,
        at: Option<usize>,
        result: Result<Box<ForkSessionResponse>, Error>,
    },
    /// A session was deleted from its agent, or could not be.
    Deleted {
        key: u64,
        result: Result<(), String>,
    },
    Ended {
        key: u64,
        result: Result<StopReason, Error>,
    },
    /// What a session's options are after one was changed.
    Options {
        key: u64,
        options: Vec<SessionConfigOption>,
    },
    /// A change asked of a session that the agent refused.
    Refused {
        key: u64,
        why: String,
    },
    /// Signing in to `agent` is done, or failed: why.
    Signed {
        agent: String,
        result: Result<(), String>,
    },
    /// An agent signed out, or could not.
    LoggedOut {
        agent: String,
        result: Result<(), String>,
    },
    /// How much memory each running agent holds: its id, process id and
    /// kilobytes.
    Weighed(Vec<(String, u32, u64)>),
    /// What of each agent the registry lists is here, by its id.
    Here(HashMap<String, Here>),
    /// What a CLI of yours says its version is.
    CliVersion {
        cli: PathBuf,
        version: String,
    },
}

/// What a session loaded or resumed starts with.
pub struct Opened {
    pub modes: Option<SessionModeState>,
    pub options: Option<Vec<SessionConfigOption>>,
}

/// What of an agent is here: whether it can be started, the version
/// installed, and the CLI of yours it drives.
#[derive(Debug, Clone, Default)]
pub struct Here {
    pub launches: bool,
    pub installed: Option<String>,
    pub cli: Option<PathBuf>,
}
