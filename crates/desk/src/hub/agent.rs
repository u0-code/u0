//! The agents as the desk sees them: each one's slot and where it stands.
//! The agent itself, once started, is the server's (`ghost_core::server`);
//! the desk names it by its id.

use agent_client_protocol::v1::schema::{
    AgentCapabilities, AuthMethod, InitializeResponse, SessionConfigOption, SessionModeState,
};

use crate::{
    acp,
    proto::Here,
    registry::{self, Entry, Missing},
};

/// Where an agent stands.
#[derive(Clone)]
pub enum Stage {
    /// It cannot be started yet.
    Missing(Missing),
    Installing(Install),
    /// It can be started, and is not running.
    Ready,
    Starting,
    /// It runs, as the process `pid`.
    Running {
        pid: Option<u32>,
    },
    /// It stopped, or could not be started: why.
    Failed(String),
}

/// An install or a clone going on: what it is doing, and how far it is (0
/// to 1) when known.
#[derive(Clone)]
pub struct Install {
    pub what: String,
    pub share: Option<f32>,
}

pub struct Slot {
    /// The server it runs on.
    pub host: super::Host,
    pub entry: Entry,
    pub stage: Stage,
    /// What a session of it offers before one is opened: read from a
    /// session opened and deleted again as it starts.
    pub probe: Option<Probe>,
    pub auth: Auth,
    /// When it last did anything: one idle long enough is let go, and
    /// started again when it is wanted.
    pub used: std::time::Instant,
    /// The commands its sessions take, as the probe's session listed them.
    pub commands: Vec<agent_client_protocol::v1::schema::AvailableCommand>,
    /// A command line of yours that starts it instead of the registry's.
    pub command: Option<String>,
    /// What it said of itself when it last started (what it can do, how
    /// it signs in): kept while it is let go; none before it first starts.
    pub known: Option<Box<InitializeResponse>>,
}

/// The modes and options a session of an agent starts with.
#[derive(Clone)]
pub struct Probe {
    pub modes: Option<SessionModeState>,
    pub options: Vec<SessionConfigOption>,
}

/// Whether an agent is signed in, as far as the desk can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Auth {
    /// Not known yet.
    Unknown,
    /// It opened a session without asking.
    Ready,
    /// It asked to be signed in, or said its sign-in failed: what it said.
    Needed(String),
}

/// Where an agent stands as the server says what of it is here: until it
/// says, ready, and starting it tells why it cannot be.
fn stage(here: Option<&Here>) -> Stage {
    match here.and_then(|h| h.missing.clone()) {
        Some(missing) => Stage::Missing(missing),
        None => Stage::Ready,
    }
}

impl Slot {
    /// A slot for `entry` on `host`, ready or missing what its server
    /// says it needs (`here`).
    pub fn new(
        host: super::Host,
        entry: Entry,
        here: Option<&Here>,
        command: Option<String>,
    ) -> Self {
        let stage = stage(here);
        Self {
            host,
            command,
            entry,
            stage,
            probe: None,
            auth: Auth::Unknown,
            used: std::time::Instant::now(),
            commands: Vec::new(),
            known: None,
        }
    }

    /// Whether a session can be begun with it now: running, starting, or
    /// ready to start.
    pub fn usable(&self) -> bool {
        matches!(
            self.stage,
            Stage::Ready | Stage::Starting | Stage::Running { .. }
        )
    }

    pub fn launch(&self, home: &std::path::Path) -> Result<acp::Launch, Missing> {
        registry::launch_with(&self.entry, home, self.command.as_deref())
    }

    /// Where it stands now that the server says it can (or cannot) be
    /// started; one running or on its way is left as it is.
    pub fn restage(&mut self, here: Option<&Here>) {
        if matches!(
            self.stage,
            Stage::Missing(_) | Stage::Ready | Stage::Failed(_)
        ) {
            self.stage = stage(here);
        }
    }

    /// Give the draft `s` the modes and options a session of this agent
    /// starts with, so they can be chosen before it is opened.
    pub fn seed(&self, s: &mut crate::session::Session) {
        if let Some(p) = &self.probe
            && s.meta.id.is_none()
        {
            s.modes.clone_from(&p.modes);
            s.options.clone_from(&p.options);
        }
    }

    /// What it can do, as it said when it last started.
    pub fn caps(&self) -> Option<&AgentCapabilities> {
        self.known.as_ref().map(|k| &k.agent_capabilities)
    }

    /// The ways it offers, while it runs, to sign in that Ghost can take:
    /// done by the agent itself, or typed in a terminal.
    pub fn sign_ins(&self) -> impl Iterator<Item = &AuthMethod> {
        (self.known.as_ref())
            .filter(|_| self.running())
            .into_iter()
            .flat_map(|k| &k.auth_methods)
            .filter(|m| matches!(m, AuthMethod::Agent(_) | AuthMethod::Terminal(_)))
    }

    pub fn running(&self) -> bool {
        matches!(self.stage, Stage::Running { .. })
    }

    /// Whether the process `pid` is its agent running now, not one let go
    /// or started before it.
    pub fn runs(&self, pid: Option<u32>) -> bool {
        matches!(self.stage, Stage::Running { pid: p } if p == pid)
    }
}
