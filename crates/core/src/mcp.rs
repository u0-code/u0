//! Ghost's tools, as an agent reaches them: the one MCP server
//! (`server::tools`) listens on 127.0.0.1, at a port the home's `mcp.json`
//! names beside a key that file alone holds, readable by you alone. What an
//! agent starts is `ghost --mcp` (the app's program, or `ghost-server`'s),
//! an MCP server on its standard input and output that passes all it is
//! sent to that port, after a key, and all it hears back.
//!
//! A session of Ghost's starts it with a key of its own in [`KEY`], which
//! says which session calls; an agent outside Ghost (`claude mcp add ghost
//! -- …/ghost --mcp`) starts it with none, and is let in by the file's.
//! The same tools speak MCP's streamable HTTP at the file's `http` port,
//! the key as a bearer token. `ghost --mcp --on <server>` reaches the
//! tools of a server this Mac is paired with instead, over a link of its
//! own, signed in as the window is: there it acts as that Mac's scope
//! lets it.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt as _;

/// The variable a session's key is given in.
pub const KEY: &str = "GHOST_MCP_KEY";

/// Where the server listens, and the key that lets an agent outside Ghost
/// in: made anew each time it starts.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Door {
    /// Where the pipe of `--mcp` reaches, after the key on a line.
    pub port: u16,
    /// Where MCP's streamable HTTP is answered: `http://127.0.0.1:<it>/mcp`,
    /// with the key as a bearer token.
    pub http: u16,
    pub key: String,
}

pub(crate) fn path(home: &Path) -> PathBuf {
    home.join("mcp.json")
}

/// Write `door` in `home`, readable by you alone: whole, so that whoever
/// reads it finds the door before or the door after, never part of one.
pub(crate) fn open(home: &Path, door: &Door) -> std::io::Result<()> {
    std::fs::create_dir_all(home)?;
    // A temporary file is made readable by you alone.
    let mut file = tempfile::NamedTempFile::new_in(home)?;
    file.write_all(&serde_json::to_vec(door).expect("the door serializes"))?;
    file.persist(path(home)).map(drop).map_err(|e| e.error)
}

/// Be the pipe of `ghost --mcp`: pass what is said on the standard input
/// to the tools of the server of the home this runs with, after the
/// session's key (or the door's), or to those of the server paired with as
/// `on`; and what they say back to the standard output, until either ends.
pub(crate) fn pipe(on: Option<&str>) -> Result<(), String> {
    if let Some(name) = on {
        return pipe_on(name);
    }
    let home = crate::settings::home();
    let door: Door = std::fs::read_to_string(path(&home))
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
        .map_err(|e| format!("Ghost is not running with {} ({e})", home.display()))?;
    let key = std::env::var(KEY).unwrap_or(door.key);
    crate::acp::runtime()
        .block_on(async {
            let mut server = tokio::net::TcpStream::connect(("127.0.0.1", door.port)).await?;
            server.write_all(format!("{key}\n").as_bytes()).await?;
            let mut here = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
            tokio::io::copy_bidirectional(&mut here, &mut server).await
        })
        .map(|_| ())
        .map_err(|e| format!("Ghost's tools could not be reached: {e}"))
}

/// Pass what is said on the standard input to the tools of the server
/// this Mac is paired with as `name`, over a link signed with this Mac's
/// key for it, and what they say back to the standard output.
fn pipe_on(name: &str) -> Result<(), String> {
    let servers = crate::servers::Servers::load(&crate::settings::home())?;
    let server = (servers.servers.iter())
        .find(|s| s.name == name)
        .ok_or_else(|| format!("This Mac is not paired with {name}"))?;
    let (way, key) = server.reach()?;
    let here = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    crate::acp::runtime().block_on(crate::link::tools(&way, &key, here))
}
