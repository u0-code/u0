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
use crate::tool::Outcome;

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
/// within `timeout`.
pub async fn local(
    command: &[String],
    env: &[(String, String)],
    dir: &Path,
    timeout: Duration,
) -> Result<Connection, String> {
    let Some((program, args)) = command.split_first() else {
        return Err("there is no command to run it with.".to_owned());
    };
    std::fs::create_dir_all(dir.join("tmp"))
        .map_err(|e| format!("its folder could not be made ({e})."))?;
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("stderr.log"))
        .map_err(|e| format!("its log could not be opened ({e})."))?;
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
        .map_err(|e| format!("{program} could not be started ({e})."))?;
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
            Err(format!(
                "it started but did not speak the protocol ({e}). What it printed is in {}.",
                dir.join("stderr.log").display()
            ))
        }
        Err(_) => {
            kill_group(group);
            Err(format!(
                "it did not answer within {} seconds of starting.",
                timeout.as_secs()
            ))
        }
    }
}

/// Reach a server elsewhere at `url`, whose host is `host`, at exactly
/// `addresses`, with no cookies and no redirects: a redirect could lead
/// anywhere, past every check made of the address.
pub async fn remote(
    url: &str,
    host: &str,
    addresses: &[std::net::SocketAddr],
    timeout: Duration,
) -> Result<Connection, String> {
    let http = reqwest::Client::builder()
        .resolve_to_addrs(host, addresses)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("ghost/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("no connection could be made ({e})."))?;
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
        Ok(Err(e)) => Err(format!("it did not speak the protocol ({e}).")),
        Err(_) => Err(format!(
            "it did not answer within {} seconds.",
            timeout.as_secs()
        )),
    }
}

impl Connection {
    /// Every tool it lists.
    pub async fn list(&self, timeout: Duration) -> Result<Vec<Listed>, String> {
        let tools = tokio::time::timeout(timeout, self.service.peer().list_all_tools())
            .await
            .map_err(|_| "it did not list its tools in time.".to_owned())?
            .map_err(|e| format!("it would not list its tools ({e})."))?;
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
    /// any images it sent; `Err` only when it did not answer at all.
    pub async fn call(
        &self,
        tool: &str,
        args: Map<String, Value>,
        timeout: Duration,
    ) -> Result<Outcome, String> {
        let request = CallToolRequestParams::new(tool.to_owned()).with_arguments(args);
        let answered = tokio::time::timeout(timeout, self.service.peer().call_tool(request))
            .await
            .map_err(|_| format!("it did not answer within {} seconds.", timeout.as_secs()))?
            .map_err(|e| format!("it fell over ({e})."))?;
        Ok(read(&serde_json::to_value(&answered).unwrap_or_default()))
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
/// pictures, and structured content when there is nothing else.
fn read(result: &Value) -> Outcome {
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
                    None => text.push("[an image that could not be shown]".to_owned()),
                }
            }
            "resource" => {
                let resource = &block["resource"];
                match resource["text"].as_str() {
                    Some(body) => text.push(format!(
                        "{}:\n{body}",
                        resource["uri"].as_str().unwrap_or("a resource")
                    )),
                    None => text.push(format!(
                        "[a file: {}]",
                        resource["uri"].as_str().unwrap_or("unnamed")
                    )),
                }
            }
            "resource_link" => text.push(format!(
                "[a link: {}]",
                block["uri"].as_str().unwrap_or("unnamed")
            )),
            other => text.push(format!("[{other} content, not shown]")),
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
