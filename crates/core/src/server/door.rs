//! How an agent reaches u0's tools (`tools`), and who it is there.
//! The server listens on 127.0.0.1 alone, on two ports `mcp.json` names
//! (the same at each start, where they are free) with a key only that
//! file and the Keychain hold, kept until you ask for a new one: one
//! where `u0 --mcp` pipes MCP once it proves it holds a key (an HMAC
//! of the server's nonce, `mcp::proof`), and one that
//! speaks MCP's streamable HTTP
//! (rmcp's, one request a turn, no session kept), the key its bearer
//! token. A session u0 opens is given a key of its own; a device
//! paired reaches them over its link, signed in as a desk is (`link`).

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use agent_client_protocol::v1::schema::{EnvVariable, McpServer, McpServerStdio, SessionId};
use http_body_util::{BodyExt as _, Full, combinators::BoxBody};
use rmcp::{
    ServiceExt as _,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    },
};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

use super::{Server, Shared, tools::Tools};
use crate::{access::Scope, acp, mcp, proto::Event, settings::Settings};

/// Who each key lets in, and the tools you allowed it always.
pub(super) type Callers = Arc<Mutex<HashMap<String, Called>>>;

#[derive(Debug, Clone)]
pub(super) struct Called {
    pub who: Caller,
    pub always: Arc<Mutex<HashSet<&'static str>>>,
}

/// Who calls u0's tools, and so what it may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Caller {
    /// An agent outside u0, by the key in `mcp.json`: it reads, and
    /// acts on a session only where your settings let it
    /// (`outside_acts`), asked of you in that session.
    Outside,
    /// An agent on a device paired, over its link: it reads, and one of
    /// full scope acts on a session, asked of you in that session.
    Device(Scope),
    /// An agent in a session of u0's (its agent, and its id once it
    /// has one): what acts is asked of you in its own session.
    Session(String, Option<SessionId>),
}

/// How many connections to the tools' pipe may be proving their key at
/// once: one more is closed. A caller with a key proves it at once.
pub(crate) const KEYING: usize = 16;

/// How long a connection to either of the tools' ports has to say who it
/// is (its proof, or its request's head).
const KEY_WAIT: std::time::Duration = mcp::WAIT;

/// How many requests to the tools over HTTP go on at once: one more is
/// closed.
const CALLS: usize = 64;

/// Where the key agents outside u0 come in by is kept (`secrets`).
const OUTSIDE_KEY: &str = "mcp/key";

/// The key agents outside u0 come in by: the one kept, unless `anew`;
/// else a new one, kept from now on. Where nothing can be kept (a system
/// without the Keychain), one for as long as this server runs.
fn outside_key(anew: bool) -> String {
    if !anew && let Ok(Some(key)) = crate::secrets::get(OUTSIDE_KEY) {
        return key;
    }
    let key = crate::access::random::<32>();
    if let Err(why) = crate::secrets::set(OUTSIDE_KEY, Some(&key)) {
        tracing::warn!("the key for agents outside u0 lasts until u0 stops: {why}");
    }
    key
}

/// A listener on 127.0.0.1 at `port`, where there is one and it is free;
/// else at a port of the system's choosing. Where it could not take
/// `port`, a word saying so.
async fn bind(port: Option<u16>) -> std::io::Result<(tokio::net::TcpListener, Option<String>)> {
    let at = |port| tokio::net::TcpListener::bind(("127.0.0.1", port));
    if let Some(port) = port.filter(|p| *p != 0)
        && let Ok(listener) = at(port).await
    {
        return Ok((listener, None));
    }
    let listener = at(0).await?;
    let now = listener.local_addr()?.port();
    let moved = port.map(|was| format!("{was} was taken: they are at {now} now"));
    Ok((listener, moved))
}

/// The token of an `Authorization` header of the Bearer scheme, the
/// scheme read case aside (RFC 9110 §11.1).
fn bearer(said: &str) -> Option<&str> {
    let (scheme, token) = said.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

impl Shared {
    /// Let `who` in by a new key: the key.
    fn let_in(&self, who: Caller) -> String {
        let key = crate::access::random::<32>();
        self.let_in_by(key.clone(), who);
        key
    }

    /// The key that `proof` is made with, for `nonce` at `port`, where
    /// one lets anyone in: each is tried, in constant time.
    fn proven(&self, nonce: &str, port: u16, proof: &str) -> Option<String> {
        use subtle::ConstantTimeEq as _;
        (self.callers.lock().expect("callers"))
            .keys()
            .find(|key| {
                bool::from(
                    mcp::proof(key, port, nonce)
                        .as_bytes()
                        .ct_eq(proof.as_bytes()),
                )
            })
            .cloned()
    }

    /// Let `who` in by `key`.
    fn let_in_by(&self, key: String, who: Caller) {
        let called = Called {
            who,
            always: Arc::default(),
        };
        (self.callers.lock().expect("callers")).insert(key, called);
    }

    /// Listen for agents calling u0's tools, from now on: at the ports
    /// the door names where it was written before and they are free (said
    /// where one is not), by the key kept; the door written in the home,
    /// each one let in by its key and served on a task of its own.
    pub(super) fn serve_tools(&self) {
        let this = self.clone();
        acp::runtime().spawn(async move {
            let was = mcp::door(&this.home).ok();
            let ports = (was.as_ref().map(|d| d.port), was.as_ref().map(|d| d.http));
            let ((pipe, moved), (http, moved_too)) = match (bind(ports.0).await, bind(ports.1).await) {
                (Ok(pipe), Ok(http)) => (pipe, http),
                (Err(e), _) | (_, Err(e)) => {
                    tracing::warn!("u0's tools are not served: {e}");
                    return;
                }
            };
            // An HTTP client sends its key to whatever listens at the
            // address it was given: one that held the port remembered
            // while u0 was down may have heard it, so it is made anew.
            let anew = moved_too.is_some();
            for moved in [moved, moved_too].into_iter().flatten() {
                tracing::warn!("u0's tools moved: port {moved}");
                let what = "u0's tools moved to another port".to_owned();
                let and_key = if anew { " and key" } else { "" };
                let why = format!("Port {moved}: an app set up with their old address needs the new one{and_key} in mcp.json");
                (this.send)(Event::Notice(Err((what, why))));
            }
            let port = |l: &tokio::net::TcpListener| l.local_addr().map_or(0, |a| a.port());
            let key = outside_key(anew);
            // It lets in before the door names it.
            this.let_in_by(key.clone(), Caller::Outside);
            let door = mcp::Door {
                port: port(&pipe),
                http: port(&http),
                key,
            };
            if let Err(e) = mcp::open(&this.home, &door) {
                tracing::warn!("u0's tools are not served: {e}");
                return;
            }
            acp::runtime().spawn(this.clone().serve_http(http));
            let keying = Arc::new(tokio::sync::Semaphore::new(KEYING));
            loop {
                let stream = crate::link::next(&pipe).await;
                // Past `KEYING` not yet keyed, one more is closed at once.
                let Ok(keyed) = keying.clone().try_acquire_owned() else {
                    continue;
                };
                let (this, port) = (this.clone(), door.port);
                tokio::spawn(async move {
                    let mut stream = BufReader::new(stream);
                    let nonce = crate::access::random::<32>();
                    let mut proof = String::new();
                    // A proof is a line, short, and said at once.
                    let read = async {
                        stream.write_all(format!("{nonce}\n").as_bytes()).await?;
                        (&mut stream).take(256).read_line(&mut proof).await
                    };
                    let read = tokio::time::timeout(KEY_WAIT, read).await;
                    drop(keyed);
                    if matches!(read, Ok(Ok(_)))
                        && let Some(key) = this.proven(&nonce, port, proof.trim_end())
                    {
                        this.serve(&key, stream).await;
                    }
                });
            }
        });
    }

    /// Serve the tools to the caller of `key` over `io`, until either
    /// ends; nothing to a key that lets no one in.
    async fn serve<IO>(&self, key: &str, io: IO)
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        let Some(tools) = Tools::of(self, key) else {
            return;
        };
        if let Ok(serving) = tools.serve(io).await {
            let _ = serving.waiting().await;
        }
    }

    /// Answer MCP's streamable HTTP at `listener`, from now on: each
    /// request by the caller its bearer token lets in, else 401.
    async fn serve_http(self, listener: tokio::net::TcpListener) {
        // One request is one turn: no session is kept, so each is served
        // afresh, as its key says, and "always" is the key's.
        // Called by a name of this Mac alone (rmcp's default list), and by
        // no page in a browser: a request that says it comes from one
        // (Origin) is refused, as one renamed by DNS is.
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .enforce_origin_validation();
        let calls = Arc::new(tokio::sync::Semaphore::new(CALLS));
        loop {
            let stream = crate::link::next(&listener).await;
            // Past `CALLS` going on, one more is closed at once.
            let Ok(held) = calls.clone().try_acquire_owned() else {
                continue;
            };
            let (this, config) = (self.clone(), config.clone());
            let answer =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let (this, config) = (this.clone(), config.clone());
                    async move {
                        let key = (req.headers().get(hyper::header::AUTHORIZATION))
                            .and_then(|v| v.to_str().ok())
                            .and_then(bearer)
                            .unwrap_or_default();
                        let Some(tools) = Tools::of(&this, key) else {
                            let mut refused =
                                hyper::Response::new(Full::new("Not a key of u0's".into()).boxed());
                            *refused.status_mut() = hyper::StatusCode::UNAUTHORIZED;
                            return Ok::<_, std::convert::Infallible>(refused);
                        };
                        let never = Arc::new(NeverSessionManager::default());
                        let service =
                            StreamableHttpService::new(move || Ok(tools.clone()), never, config);
                        let answer: hyper::Response<BoxBody<_, _>> = service.handle(req).await;
                        Ok(answer)
                    }
                });
            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(stream);
                // One request a connection, its head read in time.
                let _ = hyper::server::conn::http1::Builder::new()
                    .timer(hyper_util::rt::TokioTimer::new())
                    .header_read_timeout(KEY_WAIT)
                    .keep_alive(false)
                    .serve_connection(io, answer)
                    .await;
                drop(held);
            });
        }
    }

    /// u0's tools, for a session of `agent` (its id where it has one):
    /// the key it calls with, and the server to give it as it is opened;
    /// none where you switched them off for that agent.
    pub(super) fn tools(&self, agent: &str, id: Option<SessionId>) -> (String, Vec<McpServer>) {
        if Settings::load(&self.home)
            .0
            .no_tools
            .iter()
            .any(|a| a == agent)
        {
            return (String::new(), Vec::new());
        }
        let key = self.let_in(Caller::Session(agent.to_owned(), id));
        let program = std::env::current_exe().unwrap_or_else(|_| "u0".into());
        let server = McpServerStdio::new("u0", program)
            .args(vec!["--mcp".into()])
            .env(vec![
                EnvVariable::new("U0_HOME", self.home.display().to_string()),
                EnvVariable::new(mcp::KEY, key.clone()),
            ]);
        (key, vec![McpServer::Stdio(server)])
    }

    /// Give agents outside u0 a new key, kept from now on and written
    /// in the door: the old one lets no one in (a connection made with it
    /// calls nothing more, `Tools::call_tool`), and its "allowed" go with
    /// it.
    pub(super) fn new_outside_key(&self) {
        let mut door = match mcp::door(&self.home) {
            Ok(door) => door,
            Err(why) => {
                let what = "u0's tools are not served here".to_owned();
                (self.send)(Event::Notice(Err((what, why))));
                return;
            }
        };
        door.key = outside_key(true);
        (self.callers.lock().expect("callers")).retain(|_, c| c.who != Caller::Outside);
        self.let_in_by(door.key.clone(), Caller::Outside);
        (self.send)(Event::Notice(match mcp::open(&self.home, &door) {
            Ok(()) => Ok("Agents outside u0 have a new key: the old one lets no one in".into()),
            Err(e) => Err(("The new key was not written".into(), e.to_string())),
        }));
    }

    /// The session opened with the key `key` is `id`.
    pub(super) fn bind(&self, key: &str, id: SessionId) {
        let mut callers = self.callers.lock().expect("callers");
        if let Some(Called {
            who: Caller::Session(_, at),
            ..
        }) = callers.get_mut(key)
        {
            *at = Some(id);
        }
    }
}

impl Server {
    /// Serve u0's tools over `io` to an agent on a device of `scope`,
    /// come in over its link, until either ends.
    pub(crate) async fn tools_for<IO>(&self, scope: Scope, io: IO)
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        let key = self.shared.let_in(Caller::Device(scope));
        self.shared.serve(&key, io).await;
        (self.shared.callers.lock().expect("callers")).remove(&key);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_bearer_token_is_read_whatever_the_case_of_its_scheme() {
        for said in ["Bearer k1", "bearer k1", "BEARER  k1"] {
            assert_eq!(super::bearer(said), Some("k1"), "{said}");
        }
        assert_eq!(super::bearer("Basic k1"), None);
        assert_eq!(super::bearer("k1"), None);
    }
}
