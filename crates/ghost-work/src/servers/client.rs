//! Talking to one server of tools, over the Model Context Protocol.
//!
//! The protocol is spoken by `rmcp`, the official Rust SDK, and nothing
//! outside this file sees it: the rest of the crate deals in [`Listed`]
//! tools and [`Outcome`]s. A connection asks with `server/discover` first
//! and falls back to the `initialize` handshake, since the servers in the
//! registry today speak the older revision and later ones will speak the
//! newer.
//!
//! A server on this machine runs as the leader of a process group of its
//! own, so that killing it takes down whatever it started: a browser, a
//! language runtime's children. What it writes to stderr goes to a file in
//! its folder, for whoever looks into why it fell over.

use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use base64::Engine;
use rmcp::{
    ClientHandler, RoleClient,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, ProtocolVersion,
    },
    service::{ClientLifecycleMode, ClientServiceExt, NotificationContext, RunningService},
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value};

use super::guard::Listed;
use crate::{
    prompts::{Store, TEXTS},
    tool::Outcome,
};

/// Hears the server say its tools changed.
#[derive(Clone)]
struct Listener {
    changed: Arc<AtomicBool>,
}

impl ClientHandler for Listener {
    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.changed.store(true, Ordering::Relaxed);
        std::future::ready(())
    }

    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("ghost", env!("CARGO_PKG_VERSION")),
        )
    }
}

/// A running server, ready to be asked.
pub struct Connection {
    service: RunningService<RoleClient, Listener>,
    /// The process group of a server on this machine.
    group: Option<u32>,
    changed: Arc<AtomicBool>,
}

/// How a connection is made in either revision of the protocol.
fn lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    }
}

/// Start `command` in `dir` with exactly `env`, and shake hands with it
/// within `timeout`. Why it could not be, in `words`.
pub async fn local(
    words: &Store,
    command: &[String],
    env: &[(String, String)],
    dir: &Path,
    timeout: Duration,
) -> Result<Connection, String> {
    let why =
        |key: &str, e: &dyn std::fmt::Display| words.fill(TEXTS, key, &[("why", e.to_string())]);
    let Some((program, args)) = command.split_first() else {
        return Err(words.text(TEXTS, "no_command").to_owned());
    };
    std::fs::create_dir_all(dir.join("tmp")).map_err(|e| why("no_folder", &e))?;
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("stderr.log"))
        .map_err(|e| why("no_log", &e))?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(dir)
        .kill_on_drop(true);
    let mut wrapped = process_wrap::tokio::CommandWrap::from(cmd);
    #[cfg(unix)]
    wrapped.wrap(process_wrap::tokio::ProcessGroup::leader());
    let (transport, _) = TokioChildProcess::builder(wrapped)
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| {
            words.fill(
                TEXTS,
                "program_not_started",
                &[("program", program.clone()), ("why", e.to_string())],
            )
        })?;
    let group = transport.id();
    let changed = Arc::new(AtomicBool::new(false));
    let listener = Listener {
        changed: changed.clone(),
    };
    match tokio::time::timeout(
        timeout,
        listener.serve_with_lifecycle(transport, lifecycle()),
    )
    .await
    {
        Ok(Ok(service)) => Ok(Connection {
            service,
            group,
            changed,
        }),
        Ok(Err(e)) => {
            kill_group(group);
            Err(words.fill(
                TEXTS,
                "started_unspoken",
                &[
                    ("why", e.to_string()),
                    ("log", dir.join("stderr.log").display().to_string()),
                ],
            ))
        }
        Err(_) => {
            kill_group(group);
            Err(words.fill(
                TEXTS,
                "unanswered_at_start",
                &[("seconds", timeout.as_secs().to_string())],
            ))
        }
    }
}

/// Reach a server elsewhere at `url`, whose host is `host`, at exactly
/// `addresses`, with no cookies and no redirects: a redirect could lead
/// anywhere, past every check made of the address.
pub async fn remote(
    words: &Store,
    url: &str,
    host: &str,
    addresses: &[std::net::SocketAddr],
    timeout: Duration,
) -> Result<Connection, String> {
    // The one way out to the network everything she does takes. Through a
    // proxy, the addresses checked are where it would have gone direct;
    // the proxy resolves the name itself.
    let http = ghost_net::builder()
        .resolve_to_addrs(host, addresses)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("ghost/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| words.fill(TEXTS, "no_connection", &[("why", e.to_string())]))?;
    let transport = StreamableHttpClientTransport::with_client(
        http,
        StreamableHttpClientTransportConfig::with_uri(url.to_owned()),
    );
    let changed = Arc::new(AtomicBool::new(false));
    let listener = Listener {
        changed: changed.clone(),
    };
    match tokio::time::timeout(
        timeout,
        listener.serve_with_lifecycle(transport, lifecycle()),
    )
    .await
    {
        Ok(Ok(service)) => Ok(Connection {
            service,
            group: None,
            changed,
        }),
        Ok(Err(e)) => Err(words.fill(TEXTS, "unspoken", &[("why", e.to_string())])),
        Err(_) => Err(words.fill(
            TEXTS,
            "unanswered",
            &[("seconds", timeout.as_secs().to_string())],
        )),
    }
}

impl Connection {
    /// Every tool it lists, or why not, in `words`.
    pub async fn list(&self, words: &Store, timeout: Duration) -> Result<Vec<Listed>, String> {
        let tools = tokio::time::timeout(timeout, self.service.peer().list_all_tools())
            .await
            .map_err(|_| words.text(TEXTS, "unlisted_in_time").to_owned())?
            .map_err(|e| words.fill(TEXTS, "unlisted", &[("why", e.to_string())]))?;
        Ok(tools
            .into_iter()
            .map(|tool| {
                let value = serde_json::to_value(&tool).unwrap_or_default();
                Listed {
                    name: tool.name.to_string(),
                    description: tool.description.map(|d| d.to_string()).unwrap_or_default(),
                    schema: value.get("inputSchema").cloned().unwrap_or(Value::Null),
                }
            })
            .collect())
    }

    /// Call `tool` with `args`. Whatever it returns is read as text, with
    /// any images it sent; `Err` only when it did not answer at all. What
    /// is said around it is said in `words`.
    pub async fn call(
        &self,
        words: &Store,
        tool: &str,
        args: Map<String, Value>,
        timeout: Duration,
    ) -> Result<Outcome, String> {
        let request = CallToolRequestParams::new(tool.to_owned()).with_arguments(args);
        let answered = tokio::time::timeout(timeout, self.service.peer().call_tool(request))
            .await
            .map_err(|_| {
                words.fill(
                    TEXTS,
                    "unanswered",
                    &[("seconds", timeout.as_secs().to_string())],
                )
            })?
            .map_err(|e| words.fill(TEXTS, "fell_over", &[("why", e.to_string())]))?;
        Ok(read(
            words,
            &serde_json::to_value(&answered).unwrap_or_default(),
        ))
    }

    /// Whether it has said its tools changed since this was last asked.
    pub fn changed(&self) -> bool {
        self.changed.swap(false, Ordering::Relaxed)
    }

    /// Whether the connection is gone: the process ended, or the remote
    /// hung up.
    pub fn closed(&self) -> bool {
        self.service.is_closed()
    }

    /// Stop it at once, and everything it started.
    pub fn kill(&self) {
        kill_group(self.group);
        self.service.cancellation_token().cancel();
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        kill_group(self.group);
    }
}

/// Kill a process group, if there is one. The leader's id is the group's.
fn kill_group(group: Option<u32>) {
    #[cfg(unix)]
    if let Some(group) = group {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &format!("-{group}")])
            .stderr(Stdio::null())
            .status();
    }
}

/// A tool's result as an outcome: text and resources as text, images as
/// pictures, and structured content when there is nothing else. What
/// cannot be shown is named in `words`.
fn read(words: &Store, result: &Value) -> Outcome {
    let uri = |value: &Value, otherwise: &str| {
        value
            .as_str()
            .map_or_else(|| words.text(TEXTS, otherwise).to_owned(), str::to_owned)
    };
    let mut text = Vec::new();
    let mut pictures = Vec::new();
    for block in result["content"].as_array().into_iter().flatten() {
        match block["type"].as_str().unwrap_or_default() {
            "text" => text.push(block["text"].as_str().unwrap_or_default().to_owned()),
            "image" => {
                let bytes = block["data"]
                    .as_str()
                    .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
                    .filter(|b| b.len() <= crate::tools::MAX_PICTURE);
                match bytes.and_then(crate::tools::picture) {
                    Some(picture) => pictures.push(picture),
                    None => text.push(words.text(TEXTS, "image_unshown").to_owned()),
                }
            }
            "resource" => {
                let resource = &block["resource"];
                match resource["text"].as_str() {
                    Some(body) => {
                        text.push(format!("{}:\n{body}", uri(&resource["uri"], "a_resource")));
                    }
                    None => text.push(words.fill(
                        TEXTS,
                        "a_file_resource",
                        &[("uri", uri(&resource["uri"], "unnamed"))],
                    )),
                }
            }
            "resource_link" => {
                text.push(words.fill(TEXTS, "a_link", &[("uri", uri(&block["uri"], "unnamed"))]));
            }
            other => text.push(words.fill(TEXTS, "content_unshown", &[("kind", other.to_owned())])),
        }
    }
    if text.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        text.push(structured.to_string());
    }
    Outcome {
        output: text.join("\n"),
        failed: result["isError"].as_bool().unwrap_or(false),
        pictures,
        origin: None,
    }
}
