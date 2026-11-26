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
//! The key never crosses the wire: the server's first word is a nonce,
//! and the pipe answers with an HMAC of it and the port by the key
//! (`proof`), as a desk signs a link's challenge. One listening at that
//! port in Ghost's stead (while Ghost is down) hears nothing it can use:
//! the proof is for that nonce alone, and that port, not Ghost's.
//! The same tools speak MCP's streamable HTTP at the file's `http` port,
//! the key as a bearer token: an app set up with that address keeps it,
//! as the ports are taken again at each start and the key is kept (in
//! the Keychain) until you ask for a new one. `ghost --mcp --on <server>` reaches the
//! tools of a server this Mac is paired with instead, over a link of its
//! own, signed in as the window is: there it acts as that Mac's scope
//! lets it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};

/// The variable a session's key is given in.
pub const KEY: &str = "GHOST_MCP_KEY";

/// Where the server listens, and the key that lets an agent outside Ghost
/// in: the ports taken again each time it starts (others where they are
/// not free), the key kept in the Keychain until you ask for a new one.
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

/// How long either end of the pipe waits for the other's first line.
pub(crate) const WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// What a caller of the pipe at `port` answers the server's `nonce` with,
/// holding `key`: an HMAC of both by it.
pub(crate) fn proof(key: &str, port: u16, nonce: &str) -> String {
    use base64::Engine as _;
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes());
    let tag = ring::hmac::sign(&key, format!("ghost mcp {port}\n{nonce}").as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(tag)
}

pub(crate) fn path(home: &Path) -> PathBuf {
    home.join("mcp.json")
}

/// The door written in `home`, or why there is none.
pub(crate) fn door(home: &Path) -> Result<Door, String> {
    std::fs::read_to_string(path(home))
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
}

/// Write `door` in `home`, readable by you alone: whole, so that whoever
/// reads it finds the door before or the door after, never part of one.
pub(crate) fn open(home: &Path, door: &Door) -> std::io::Result<()> {
    crate::whole::write(
        &path(home),
        &serde_json::to_vec(door).expect("the door serializes"),
    )
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
    let door =
        door(&home).map_err(|e| format!("Ghost is not running with {} ({e})", home.display()))?;
    let key = std::env::var(KEY).unwrap_or(door.key);
    crate::acp::runtime()
        .block_on(async {
            let server = tokio::net::TcpStream::connect(("127.0.0.1", door.port)).await?;
            let mut server = tokio::io::BufReader::new(server);
            knock(&mut server, &key, door.port).await?;
            let mut here = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
            tokio::io::copy_bidirectional(&mut here, &mut server).await
        })
        .map(|_| ())
        .map_err(|e| format!("Ghost's tools could not be reached: {e}"))
}

/// Prove to the tools' pipe at `port`, `server`, that this holds `key`:
/// its nonce (a line, short, said at once) answered with the proof.
async fn knock<S>(server: &mut tokio::io::BufReader<S>, key: &str, port: u16) -> std::io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut nonce = String::new();
    let mut line = (&mut *server).take(256);
    let said = line.read_line(&mut nonce);
    tokio::time::timeout(WAIT, said).await??;
    let proof = proof(key, port, nonce.trim_end());
    server.write_all(format!("{proof}\n").as_bytes()).await
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

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, Write as _};

    use super::*;

    #[test]
    fn one_listening_in_ghosts_stead_hears_no_key_and_nothing_it_can_use() {
        let squatter = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = squatter.local_addr().unwrap().port();
        let heard = std::thread::spawn(move || {
            let (mut s, _) = squatter.accept().unwrap();
            writeln!(s, "a-nonce").unwrap();
            let mut line = String::new();
            std::io::BufReader::new(s).read_line(&mut line).unwrap();
            line.trim_end().to_owned()
        });
        let key = "the-key-in-mcp-json";
        crate::acp::runtime()
            .block_on(async {
                let s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
                knock(&mut tokio::io::BufReader::new(s), key, port).await
            })
            .unwrap();
        let heard = heard.join().unwrap();
        assert!(!heard.contains(key), "the key never crosses the wire");
        assert_eq!(heard, proof(key, port, "a-nonce"));
        // What it heard proves nothing at Ghost's own port, nor to another
        // nonce.
        assert_ne!(heard, proof(key, port + 1, "a-nonce"));
        assert_ne!(heard, proof(key, port, "another"));
    }
}
