//! A toolbox, served over the Model Context Protocol on stdio.
//!
//! This is how an agent that brings its own loop (the Claude Code CLI, for
//! one) is given a ghost's tools instead of its own: it is started with its
//! built-in tools turned off and pointed at this server. The tools, the
//! machine they act on and the undertaking they write to are all still ours;
//! only the stepping is theirs.
//!
//! The protocol is small when all you serve is tools: newline-delimited
//! JSON-RPC, an `initialize` handshake, `tools/list` and `tools/call`.

use std::{io, sync::Arc};

use ghost_brain::Call;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::{tool::Toolbox, undertaking::Undertaking};

/// The protocol revision offered when a client does not name one.
const PROTOCOL: &str = "2025-06-18";
/// The most a result may hold. Clients cut off or spill longer ones to a
/// place a tool-less agent cannot read.
const MAX_RESULT: usize = 40_000;

#[allow(clippy::needless_pass_by_value)]
fn answer(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn refuse(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Serve `tools` until the input ends. Once `undertaking` has an outcome
/// every further call is refused: an agent that keeps going after `done`
/// must not keep changing things.
pub async fn serve(
    tools: Toolbox,
    undertaking: Arc<Undertaking>,
    input: impl AsyncBufRead + Unpin,
    mut output: impl AsyncWrite + Unpin,
) -> io::Result<()> {
    let mut lines = input.lines();
    while let Some(line) = lines.next_line().await? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = message["method"].as_str().unwrap_or_default();
        // Notifications carry no id and get no reply.
        let Some(id) = message.get("id").filter(|id| !id.is_null()) else {
            continue;
        };
        let reply = match method {
            "initialize" => answer(
                id,
                json!({
                    "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "ghost", "version": env!("CARGO_PKG_VERSION")},
                }),
            ),
            "ping" => answer(id, json!({})),
            "tools/list" => answer(
                id,
                json!({
                    "tools": tools.specs().iter().map(|t| json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.parameters,
                    })).collect::<Vec<_>>(),
                }),
            ),
            "tools/call" => {
                let name = message["params"]["name"].as_str().unwrap_or_default();
                let outcome = if undertaking.state().finish.is_some() {
                    crate::tool::Outcome::fail(
                        "The work has been ended with `done`. Nothing more can be done; stop here.",
                    )
                } else {
                    tools
                        .run(&Call {
                            id: id.to_string(),
                            name: name.to_owned(),
                            args: message["params"]
                                .get("arguments")
                                .cloned()
                                .unwrap_or_else(|| json!({})),
                        })
                        .await
                };
                undertaking.log(&json!({
                    "at": chrono::Utc::now().to_rfc3339(),
                    "via": "mcp",
                    "name": name,
                    "failed": outcome.failed,
                    "chars": outcome.output.len(),
                }));
                answer(
                    id,
                    json!({
                        "content": [{"type": "text", "text": crate::tools::clip(&outcome.output, MAX_RESULT / 3, MAX_RESULT * 2 / 3)}],
                        "isError": outcome.failed,
                    }),
                )
            }
            _ => refuse(id, -32601, "Method not found"),
        };
        output.write_all(reply.to_string().as_bytes()).await?;
        output.write_all(b"\n").await?;
        output.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{machine::Local, tools};

    async fn talk(dir: &std::path::Path, requests: &[Value]) -> Vec<Value> {
        let undertaking =
            Arc::new(Undertaking::open(dir.join("state"), Some("make hi.txt")).unwrap());
        let toolbox = tools::standard(Arc::new(Local::new(dir)), &undertaking);
        let input = requests
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let mut out = Vec::new();
        serve(
            toolbox,
            undertaking,
            tokio::io::BufReader::new(input.as_bytes()),
            &mut out,
        )
        .await
        .unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn a_client_can_shake_hands_list_and_call() {
        let dir = tempfile::tempdir().unwrap();
        let replies = talk(
            dir.path(),
            &[
                json!({"jsonrpc": "2.0", "id": "probe", "method": "server/discover"}),
                json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}}),
                json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "write", "arguments": {"path": "hi.txt", "content": "hi\n"}}}),
                json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "nope", "arguments": {}}}),
            ],
        )
        .await;
        assert_eq!(replies.len(), 5, "the notification got no reply");
        assert_eq!(replies[0]["error"]["code"], -32601);
        assert_eq!(replies[1]["result"]["protocolVersion"], "2025-11-25");
        let names: Vec<&str> = replies[2]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["shell", "read", "write", "edit", "plan", "note", "done"]
        );
        assert_eq!(replies[3]["result"]["isError"], false);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hi.txt")).unwrap(),
            "hi\n"
        );
        assert_eq!(replies[4]["result"]["isError"], true);
    }

    #[tokio::test]
    async fn nothing_is_done_after_done() {
        let dir = tempfile::tempdir().unwrap();
        let replies = talk(
            dir.path(),
            &[
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "done", "arguments": {"outcome": "gave_up", "summary": "no"}}}),
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "write", "arguments": {"path": "late.txt", "content": "x"}}}),
            ],
        )
        .await;
        assert_eq!(replies[1]["result"]["isError"], true);
        assert!(!dir.path().join("late.txt").exists());
    }
}
