//! How an agent reaches Ghost's tools (`tools`), and who it is there.
//! The server listens on 127.0.0.1 alone, on two ports `mcp.json` names
//! with a key only that file holds: one where `ghost --mcp` pipes MCP
//! after a key on a line, and one that speaks MCP's streamable HTTP
//! (rmcp's, one request a turn, no session kept), the key its bearer
//! token. A session Ghost opens is given a key of its own; a device
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
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, BufReader};

use super::{Server, Shared, tools::Tools};
use crate::{access::Scope, acp, mcp, settings::Settings};

/// Who each key lets in, and the tools you allowed it always.
pub(super) type Callers = Arc<Mutex<HashMap<String, Called>>>;

#[derive(Debug, Clone)]
pub(super) struct Called {
    pub who: Caller,
    pub always: Arc<Mutex<HashSet<&'static str>>>,
}

/// Who calls Ghost's tools, and so what it may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Caller {
    /// An agent outside Ghost, by the key in `mcp.json`: it reads, and
    /// acts on a session only where your settings let it
    /// (`outside_acts`), asked of you in that session.
    Outside,
    /// An agent on a device paired, over its link: it reads, and one of
    /// full scope acts on a session, asked of you in that session.
    Device(Scope),
    /// An agent in a session of Ghost's (its agent, and its id once it
    /// has one): what acts is asked of you in its own session.
    Session(String, Option<SessionId>),
}

impl Shared {
    /// Let `who` in by a new key: the key.
    fn let_in(&self, who: Caller) -> String {
        let key = crate::access::random::<32>();
        let called = Called {
            who,
            always: Arc::default(),
        };
        (self.callers.lock().expect("callers")).insert(key.clone(), called);
        key
    }

    /// Listen for agents calling Ghost's tools, from now on: the door
    /// written in the home, each one let in by its key and served on a
    /// task of its own.
    pub(super) fn serve_tools(&self) {
        let this = self.clone();
        acp::runtime().spawn(async move {
            let bind = || tokio::net::TcpListener::bind(("127.0.0.1", 0));
            let (pipe, http) = match (bind().await, bind().await) {
                (Ok(pipe), Ok(http)) => (pipe, http),
                (Err(e), _) | (_, Err(e)) => {
                    tracing::warn!("Ghost's tools are not served: {e}");
                    return;
                }
            };
            let port = |l: &tokio::net::TcpListener| l.local_addr().map_or(0, |a| a.port());
            let door = mcp::Door {
                port: port(&pipe),
                http: port(&http),
                // It lets in before the door names it.
                key: this.let_in(Caller::Outside),
            };
            if let Err(e) = mcp::open(&this.home, &door) {
                tracing::warn!("Ghost's tools are not served: {e}");
                return;
            }
            acp::runtime().spawn(this.clone().serve_http(http));
            while let Ok((stream, _)) = pipe.accept().await {
                let this = this.clone();
                tokio::spawn(async move {
                    let mut stream = BufReader::new(stream);
                    let mut key = String::new();
                    // A key is a line, and short.
                    let read = (&mut stream).take(256).read_line(&mut key).await;
                    if read.is_ok() {
                        this.serve(key.trim_end(), stream).await;
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
        while let Ok((stream, _)) = listener.accept().await {
            let (this, config) = (self.clone(), config.clone());
            let answer =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let (this, config) = (this.clone(), config.clone());
                    async move {
                        let key = (req.headers().get(hyper::header::AUTHORIZATION))
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.strip_prefix("Bearer "))
                            .unwrap_or_default();
                        let Some(tools) = Tools::of(&this, key) else {
                            let mut refused = hyper::Response::new(
                                Full::new("Not a key of Ghost's".into()).boxed(),
                            );
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
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, answer)
                    .await;
            });
        }
    }

    /// Ghost's tools, for a session of `agent` (its id where it has one):
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
        let program = std::env::current_exe().unwrap_or_else(|_| "ghost".into());
        let server = McpServerStdio::new("ghost", program)
            .args(vec!["--mcp".into()])
            .env(vec![
                EnvVariable::new("GHOST_HOME", self.home.display().to_string()),
                EnvVariable::new(mcp::KEY, key.clone()),
            ]);
        (key, vec![McpServer::Stdio(server)])
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
    /// Serve Ghost's tools over `io` to an agent on a device of `scope`,
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
