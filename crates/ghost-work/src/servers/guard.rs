//! The structural safeguards around a server of tools, in one place.
//!
//! Nobody approves what she installs, so what keeps an installed server
//! from doing harm is its shape, checked here: names and versions that
//! cannot smuggle in anything else, an environment that holds nothing of
//! hers, remotes that are on the public internet, tool lists of a bounded
//! size with nothing that runs arbitrary code, and a hash that tells when
//! a server starts describing its tools differently from the day she took
//! it.

use std::net::{IpAddr, SocketAddr};

use serde_json::Value;

use super::Config;
use crate::prompts::{Store, TEXTS};

/// The longest description of a tool she is shown.
const DESCRIPTION: usize = 1024;
/// The largest schema of a tool's arguments that is offered, written out.
const SCHEMA_BYTES: usize = 16 * 1024;
/// The deepest a schema may nest.
const SCHEMA_DEPTH: usize = 12;
/// The longest name of a tool, as offered to a model. Providers take 64.
const NAME: usize = 64;

/// A package name as npm or `PyPI` spell one, and nothing that a command
/// line would read as an option, a path or an address.
pub fn package_name(name: &str) -> bool {
    let plain = |part: &str| {
        !part.is_empty()
            && !part.starts_with(['-', '.'])
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, rest)| plain(scope) && plain(rest)),
        None => plain(name),
    }
}

/// One version, written out: no range, no tag such as `latest`.
pub fn exact_version(version: &str) -> bool {
    version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// What can be said of a remote's address without looking it up: it is
/// `https`, and if it is written as an address, that address is public.
/// Why not, in `words`.
pub fn plain_remote(words: &Store, url: &str) -> Result<reqwest::Url, String> {
    let said = |key: &str| words.fill(TEXTS, key, &[("url", url.to_owned())]);
    let parsed = reqwest::Url::parse(url).map_err(|_| said("remote_not_an_address"))?;
    if parsed.scheme() != "https" {
        return Err(said("remote_not_https"));
    }
    let Some(host) = parsed.host_str() else {
        return Err(said("remote_no_host"));
    };
    let literal = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>();
    let local =
        host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost");
    if local || literal.is_ok_and(|ip| !public(ip)) {
        return Err(said("remote_private"));
    }
    Ok(parsed)
}

/// The addresses a remote's host has, every one of them public. They are
/// what it is then reached at, so that a name that answers differently
/// the second time it is asked still cannot lead inside.
pub async fn reachable(words: &Store, url: &str) -> Result<(String, Vec<SocketAddr>), String> {
    let parsed = plain_remote(words, url)?;
    let host = parsed.host_str().unwrap_or_default().to_owned();
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| {
            words.fill(
                TEXTS,
                "remote_unknown",
                &[("url", url.to_owned()), ("why", e.to_string())],
            )
        })?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|a| !public(a.ip())) {
        return Err(words.fill(TEXTS, "remote_private", &[("url", url.to_owned())]));
    }
    Ok((host, addresses))
}

/// Whether `ip` is on the public internet: not this machine, not a
/// private or link-local network, not a shared or reserved range.
pub fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                // Shared address space, used inside carriers and clouds.
                || (a == 100 && (64..128).contains(&b))
                || a == 0
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // Unique local and link-local.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// The whole environment a server on this machine is started with: a
/// `PATH` to find its runtime, a home, a working directory and a place
/// for temporary files of its own, the values it declared as defaults,
/// and the variables whoever runs her body gave it by name. Nothing else
/// of the body's environment, so none of her keys, reaches it.
pub fn environment(
    cfg: &Config,
    name: &str,
    dir: &std::path::Path,
    set: &[String],
    pass: &[String],
) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "PATH".to_owned(),
            cfg.path
                .clone()
                .or_else(|| std::env::var("PATH").ok())
                .unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".to_owned()),
        ),
        ("HOME".to_owned(), dir.display().to_string()),
        ("TMPDIR".to_owned(), dir.join("tmp").display().to_string()),
    ];
    // The same way out to the network as everything else she runs: the
    // proxy rule, and nothing more of this process's environment.
    for (key, value) in ghost_net::current().child_env() {
        if let Some(value) = value {
            env.push((key.to_owned(), value));
        }
    }
    for pair in set {
        if let Some((key, value)) = pair.split_once('=') {
            env.push((key.to_owned(), value.to_owned()));
        }
    }
    for key in pass {
        if given(cfg, name, key)
            && let Ok(value) = std::env::var(key)
        {
            env.push((key.clone(), value));
        }
    }
    env
}

/// Whether whoever runs her body gave server `name` the variable `key`,
/// and it is set.
pub fn given(cfg: &Config, name: &str, key: &str) -> bool {
    cfg.env
        .get(name)
        .is_some_and(|keys| keys.iter().any(|k| k == key))
        && std::env::var_os(key).is_some()
}

/// A tool as a server listed it.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// A tool as she is offered it.
#[derive(Debug, Clone, PartialEq)]
pub struct Vetted {
    /// Its name on the server.
    pub remote: String,
    /// What she calls it: `<server>__<tool>`.
    pub called: String,
    pub description: String,
    pub schema: Value,
}

/// A hash of everything a server says of its tools, whatever order it
/// says it in and however it spaces it.
pub fn hash(tools: &[Listed]) -> String {
    let mut sorted: Vec<&Listed> = tools.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut hasher = blake3::Hasher::new();
    for tool in sorted {
        hasher.update(tool.name.as_bytes());
        hasher.update(b"\0");
        hasher.update(tool.description.as_bytes());
        hasher.update(b"\0");
        hasher.update(canonical(&tool.schema).as_bytes());
        hasher.update(b"\x1e");
    }
    hasher.finalize().to_hex().to_string()
}

/// JSON with every object's keys in order.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), canonical(&map[k])))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// The tools of server `server` that are offered, and a sentence in
/// `words` for each kind of tool that was not. A tool whose name suggests it runs
/// whatever code it is handed is never offered: a server runs on her
/// body's machine, not on hers, and such a tool would hand that machine
/// to whoever wrote the code. Past `max_tools`, descriptions past their
/// length, and schemas too large or too deep are cut for the same reason
/// as any long output: they cost her room to think, at every step.
pub fn vet(
    words: &Store,
    server: &str,
    tools: &[Listed],
    cfg: &Config,
) -> (Vec<Vetted>, Vec<String>) {
    let mut offered: Vec<Vetted> = Vec::new();
    let (mut denied, mut oversized, mut over) = (Vec::new(), Vec::new(), 0);
    for tool in tools {
        let lower = tool.name.to_lowercase();
        if cfg.deny.iter().any(|d| lower.contains(&d.to_lowercase())) {
            denied.push(tool.name.clone());
            continue;
        }
        if tool.schema.to_string().len() > SCHEMA_BYTES || depth(&tool.schema) > SCHEMA_DEPTH {
            oversized.push(tool.name.clone());
            continue;
        }
        if offered.len() >= cfg.max_tools {
            over += 1;
            continue;
        }
        let called = called(server, &tool.name);
        if offered.iter().any(|t| t.called == called) {
            continue;
        }
        let mut schema = tool.schema.clone();
        if !schema.is_object() {
            schema = serde_json::json!({"type": "object", "properties": {}});
        }
        offered.push(Vetted {
            remote: tool.name.clone(),
            called,
            description: crate::tools::clip(words, &tool.description, DESCRIPTION - 200, 150),
            schema,
        });
    }
    let mut notes = Vec::new();
    if !denied.is_empty() {
        notes.push(words.fill(TEXTS, "tools_denied", &[("tools", denied.join(", "))]));
    }
    if !oversized.is_empty() {
        notes.push(words.fill(TEXTS, "tools_oversized", &[("tools", oversized.join(", "))]));
    }
    if over > 0 {
        notes.push(words.fill(
            TEXTS,
            "tools_over",
            &[
                ("count", over.to_string()),
                ("most", cfg.max_tools.to_string()),
            ],
        ));
    }
    (offered, notes)
}

fn depth(value: &Value) -> usize {
    match value {
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// A name as a model may be offered it: letters, digits, `_` and `-`.
pub fn sanitise(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// What a server is called among her tools: the last part of its name.
pub fn handle(name: &str) -> String {
    let short = name.rsplit('/').next().unwrap_or(name);
    sanitise(short).chars().take(NAME / 2).collect()
}

/// What she calls `tool` of `server`.
pub fn called(server: &str, tool: &str) -> String {
    sanitise(&format!("{server}__{tool}"))
        .chars()
        .take(NAME)
        .collect()
}

/// Where a server's files are: under the tools directory, by its whole
/// name with anything that is not a plain character made `_`.
pub fn folder(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_pointing_at_a_private_address_is_refused() {
        for url in [
            "https://10.0.0.5/mcp",
            "https://192.168.1.2/mcp",
            "https://127.0.0.1:8443/mcp",
            "https://169.254.169.254/latest",
            "https://[::1]/mcp",
            "https://[fd00::1]/mcp",
            "https://localhost/mcp",
            "https://100.64.0.1/mcp",
        ] {
            let why = plain_remote(&crate::Prompts::shipped(), url).unwrap_err();
            assert!(why.contains("private network"), "{url}: {why}");
        }
        assert!(plain_remote(&crate::Prompts::shipped(), "http://example.com/mcp").is_err());
        assert!(plain_remote(&crate::Prompts::shipped(), "https://93.184.216.34/mcp").is_ok());
        assert!(plain_remote(&crate::Prompts::shipped(), "https://mcp.example.com/mcp").is_ok());
    }

    #[test]
    fn names_that_could_be_read_as_options_or_paths_are_not_package_names() {
        assert!(package_name("@playwright/mcp"));
        assert!(package_name("mcp-server-fetch"));
        for bad in [
            "-e",
            "--registry=x",
            "../x",
            "git+https://x",
            "@a/../b",
            "a b",
            "",
        ] {
            assert!(!package_name(bad), "{bad}");
        }
        assert!(exact_version("0.0.85"));
        assert!(exact_version("1.2.3-beta.1"));
        for bad in ["^1.0.0", "~1", "latest", "1.x || 2", ">=1"] {
            assert!(!exact_version(bad), "{bad}");
        }
    }

    #[test]
    fn the_hash_ignores_order_and_spacing_and_sees_a_changed_description() {
        let tool = |name: &str, description: &str| Listed {
            name: name.into(),
            description: description.into(),
            schema: serde_json::json!({"type": "object", "properties": {"a": {"type": "string"}, "b": {}}}),
        };
        let one = [tool("a", "x"), tool("b", "y")];
        let other = [tool("b", "y"), tool("a", "x")];
        assert_eq!(hash(&one), hash(&other));
        assert_ne!(hash(&one), hash(&[tool("a", "x"), tool("b", "y!")]));
    }

    #[test]
    fn the_deny_listed_tool_is_not_exposed() {
        let cfg = Config::default();
        let tool = |name: &str| Listed {
            name: name.into(),
            description: "d".into(),
            schema: serde_json::json!({"type": "object"}),
        };
        let (offered, notes) = vet(
            &crate::Prompts::shipped(),
            "playwright-mcp",
            &[tool("browser_navigate"), tool("browser_run_code_unsafe")],
            &cfg,
        );
        let names: Vec<&str> = offered.iter().map(|t| t.called.as_str()).collect();
        assert_eq!(names, ["playwright-mcp__browser_navigate"]);
        assert!(notes[0].contains("browser_run_code_unsafe"), "{notes:?}");
    }
}
