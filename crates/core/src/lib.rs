//! Ghost without its window: what touches the machine, built without GPUI
//! so that it can run where nothing is drawn.
//!
//! Each agent runs as its own program, spoken to over the Agent Client
//! Protocol (`acp`), installed from the ACP registry (`registry`), and held
//! by the `server`, which the desk asks by the agent's id and which answers
//! with what `proto` defines. Here too are the sessions built from what
//! the agents say (`session`), what Ghost keeps of them (`store`), git and
//! its checkpoints (`git`), a project's pull requests and issues
//! (`github`), what is kept secret (`secrets`), the process of a terminal (`pty`), and text an agent
//! writes for you (`writer`). The desk (`ghost-desk`) draws all of it.

pub mod acp;
pub mod forge;
pub mod git;
pub mod github;
pub mod path;
pub mod proto;
pub mod pty;
pub mod records;
pub mod registry;
pub mod remote;
pub mod repo;
pub mod scripts;
pub mod secrets;
pub mod server;
pub mod session;
pub mod settings;
pub mod store;
pub mod terminals;
pub mod tidy;
pub mod update;
pub mod when;
pub mod whole;
pub mod writer;
