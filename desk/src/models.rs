//! The models a brain can think with, as the picker offers them, each
//! from the brain itself rather than from a list written here.
//!
//! Claude Code is not asked over the network: the ids its installed build
//! knows are read out of the binary on this machine, once per build, and
//! shown by the names Claude Code itself gives them. Codex lists the
//! models your account may use in its own cache, `~/.codex/models_cache.json`,
//! which is read here. Either can also be left to its own default. An
//! API's models are what its `/models` lists, asked for when the desk
//! starts and again after a day, and kept in `~/.config/ghost-desk/models.json`
//! so that the picker has them at once, network or not (`brain_menu.rs`).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// A model to choose. An empty `id` is the CLI's own default: no model is
/// named to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

impl Choice {
    fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
        }
    }
}

/// How long a listing is trusted before it is asked for again.
const FRESH: chrono::Duration = chrono::Duration::hours(24);

/// Where listings are kept between runs.
fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("ghost-desk")
            .join("models.json"),
    )
}

fn read_cache() -> BTreeMap<String, Value> {
    cache_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The ids kept under `key`, and when they were listed.
pub fn cached(key: &str) -> Option<(Vec<String>, DateTime<Utc>)> {
    let entry = read_cache().remove(key)?;
    let ids = entry
        .get("ids")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let at = entry
        .get("at")
        .and_then(Value::as_str)
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map_or_else(|| DateTime::<Utc>::MIN_UTC, |t| t.with_timezone(&Utc));
    Some((ids, at))
}

/// Whether a listing made at `at` is still trusted.
pub fn fresh(at: DateTime<Utc>) -> bool {
    Utc::now() - at < FRESH
}

/// Keep `ids` under `key`, as of now.
pub fn remember(key: &str, ids: &[String]) {
    let Some(path) = cache_path() else { return };
    let mut all = read_cache();
    all.insert(
        key.to_owned(),
        json!({"ids": ids, "at": Utc::now().to_rfc3339()}),
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(path, text);
    }
}

/// The key an API's listing is kept under: the brain, where it is.
pub fn api_key(name: &str, base_url: Option<&str>) -> String {
    format!("{name}@{}", base_url.unwrap_or_default())
}

/// Where `program` is, as the shell would find it.
pub fn find_program(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.components().count() > 1 {
        return p.is_file().then(|| p.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join(program))
            .find(|c| c.is_file())
    })
}

/// The models the Claude Code at `program` knows, read from its build on
/// this machine: its default first, then each dated first-party id its
/// binary carries, newest first, by the name Claude Code shows it under.
/// A build is read once; its listing is kept by the file it is.
pub fn claude_code(program: &str) -> Result<Vec<Choice>, String> {
    let path = find_program(program).ok_or_else(|| format!("{program} is not on PATH"))?;
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let key = format!("claude-code@{}@{stamp}@{}", path.display(), meta.len());
    let ids = if let Some((ids, _)) = cached(&key) {
        ids
    } else {
        let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let ids = claude_ids(std::io::BufReader::new(file))?;
        remember(&key, &ids);
        ids
    };
    let mut all = vec![Choice::new("", "Default")];
    all.extend(
        ids.iter()
            .map(|id| Choice::new(id, &claude_name(id).unwrap_or_else(|| id.clone()))),
    );
    Ok(all)
}

/// Every dated first-party Claude id in `bytes`, newest first: a run of
/// `claude-<family>-<major>[-<minor>]-<yyyymmdd>` with a family of opus,
/// sonnet or haiku, from the fourth generation on. Other spellings of the
/// same model (Bedrock's `-v1:0`, Vertex's `@date`) and older models are
/// left out.
fn claude_ids(mut reader: impl std::io::Read) -> Result<Vec<String>, String> {
    const NEEDLE: &[u8] = b"claude-";
    let is_id_byte = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
    let mut found = std::collections::BTreeSet::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut data: Vec<u8> = Vec::new();
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        let eof = n == 0;
        data.extend_from_slice(&buf[..n]);
        let mut i = 0;
        // Where the next read carries on from: a candidate that reaches
        // the end may go on, so it is kept whole for the next round.
        let mut keep_from = data.len();
        while let Some(at) = data[i..].windows(NEEDLE.len()).position(|w| w == NEEDLE) {
            let start = i + at;
            let mut end = start;
            while end < data.len() && is_id_byte(data[end]) {
                end += 1;
            }
            if end == data.len() && !eof {
                keep_from = start;
                break;
            }
            if let Ok(id) = std::str::from_utf8(&data[start..end])
                && is_dated_claude(id)
            {
                found.insert(id.to_owned());
            }
            i = end;
        }
        if eof {
            break;
        }
        // A needle itself may be cut by the read: its tail stays.
        if keep_from == data.len() {
            keep_from = data.len().saturating_sub(NEEDLE.len() - 1);
        }
        data.drain(..keep_from);
    }
    let mut ids: Vec<String> = found.into_iter().collect();
    ids.sort_by_key(|id| std::cmp::Reverse(claude_sort_key(id)));
    Ok(ids)
}

/// The parts of a dated Claude id: family, major, minor, date.
fn claude_parts(id: &str) -> Option<(&str, u32, u32, u32)> {
    let mut parts = id.split('-');
    (parts.next()? == "claude").then_some(())?;
    let family = parts.next()?;
    if !["opus", "sonnet", "haiku"].contains(&family) {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let (major, minor, date): (&str, &str, &str) = match rest.as_slice() {
        [major, date] => (major, "0", date),
        [major, minor, date] => (major, minor, date),
        _ => return None,
    };
    let major: u32 = major.parse().ok()?;
    let minor: u32 = minor.parse().ok()?;
    if date.len() != 8 || major < 4 {
        return None;
    }
    let date: u32 = date.parse().ok()?;
    Some((family, major, minor, date))
}

fn is_dated_claude(id: &str) -> bool {
    claude_parts(id).is_some()
}

/// Newest and largest first: by generation, then the family's weight.
fn claude_sort_key(id: &str) -> (u32, u32, u32, u8) {
    claude_parts(id).map_or((0, 0, 0, 0), |(family, major, minor, date)| {
        let weight = match family {
            "opus" => 3,
            "sonnet" => 2,
            _ => 1,
        };
        (major, minor, date, weight)
    })
}

/// The name Claude Code shows a dated id under: `Opus 4.5`, `Sonnet 4`.
fn claude_name(id: &str) -> Option<String> {
    let (family, major, minor, _) = claude_parts(id)?;
    let mut name = family.to_owned();
    if let Some(first) = name.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    Some(if minor == 0 {
        format!("{name} {major}")
    } else {
        format!("{name} {major}.{minor}")
    })
}

/// Claude Code's model families, by the name it takes and the name shown,
/// its own default first: what `--model opus` means, whichever build.
const CLAUDE: [(&str, &str); 4] = [
    ("", "Default"),
    ("opus", "Opus"),
    ("sonnet", "Sonnet"),
    ("haiku", "Haiku"),
];

/// The name a CLI's model `id` is shown by, where it is known without
/// reading anything: Claude Code's families, and the name it gives a
/// dated id.
pub fn cli_label(kind: &str, id: &str) -> Option<String> {
    if kind != "claude-code" {
        return None;
    }
    CLAUDE
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, label)| (*label).to_owned())
        .or_else(|| claude_name(id))
}

/// The models a CLI of `kind` offers without anything being run or read
/// at length, its default first: Codex's, from its own cache. `None` for
/// an API, and for Claude Code, whose build is read by [`claude_code`].
pub fn of_cli(kind: &str, home: Option<&Path>) -> Option<Vec<Choice>> {
    match kind {
        "codex" => {
            let dir = home.map(|h| h.join(".codex"));
            let read = |name: &str| {
                dir.as_ref()
                    .and_then(|d| std::fs::read_to_string(d.join(name)).ok())
            };
            let default = read("config.toml").and_then(|t| codex_default(&t));
            let label = default
                .as_deref()
                .map_or_else(|| "Default".to_owned(), |m| format!("Default · {m}"));
            let mut all = vec![Choice {
                id: String::new(),
                label,
            }];
            all.extend(read("models_cache.json").map_or_else(Vec::new, |j| codex_listed(&j)));
            Some(all)
        }
        _ => None,
    }
}

/// The context window, in tokens, of the model `id` a CLI of `kind` runs,
/// where the CLI says: Codex keeps it with each model in its cache, and
/// its default model in its config. `None` for anything else.
pub fn cli_window(kind: &str, id: &str, home: Option<&Path>) -> Option<u64> {
    if kind != "codex" {
        return None;
    }
    let dir = home?.join(".codex");
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let id = if id.is_empty() {
        codex_default(&read("config.toml")?)?
    } else {
        id.to_owned()
    };
    codex_window(&read("models_cache.json")?, &id)
}

/// The context window Codex's cache gives the model `id`.
pub fn codex_window(json: &str, id: &str) -> Option<u64> {
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("models")?
        .as_array()?
        .iter()
        .find(|m| m.get("slug").and_then(Value::as_str) == Some(id))?
        .get("context_window")?
        .as_u64()
}

/// The models in Codex's cache it shows in its own picker, in its order.
pub fn codex_listed(json: &str) -> Vec<Choice> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut listed: Vec<(i64, Choice)> = v
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|m| {
            let id = m.get("slug").and_then(Value::as_str)?;
            let label = m.get("display_name").and_then(Value::as_str).unwrap_or(id);
            let order = m
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            Some((order, Choice::new(id, label)))
        })
        .collect();
    listed.sort_by_key(|(order, _)| *order);
    listed.into_iter().map(|(_, c)| c).collect()
}

/// The model Codex uses when none is named: `model` in its config.
fn codex_default(toml: &str) -> Option<String> {
    let doc = toml.parse::<toml_edit::DocumentMut>().ok()?;
    doc.get("model")?.as_str().map(str::to_owned)
}

/// An API's models as choices, the one it is set to first, so that it is
/// there even when the list leaves it out.
pub fn of_api(current: &str, ids: Vec<String>) -> Vec<Choice> {
    let mut all = Vec::with_capacity(ids.len() + 1);
    if !current.is_empty() {
        all.push(Choice::new(current, current));
    }
    all.extend(
        ids.into_iter()
            .filter(|id| id != current)
            .map(|id| Choice::new(&id, &id)),
    );
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_offers_what_it_lists_in_its_own_order_and_hides_the_rest() {
        let json = r#"{"models":[
            {"slug":"b","display_name":"B","visibility":"list","priority":2},
            {"slug":"hidden","visibility":"hide","priority":0},
            {"slug":"a","display_name":"A","visibility":"list","priority":1}
        ]}"#;
        let ids: Vec<_> = codex_listed(json).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(codex_listed("not json").is_empty());
    }

    #[test]
    fn codex_says_how_large_each_models_window_is() {
        let json = r#"{"models":[
            {"slug":"a","context_window":272000},
            {"slug":"b"}
        ]}"#;
        assert_eq!(codex_window(json, "a"), Some(272_000));
        assert_eq!(codex_window(json, "b"), None, "a model it says nothing of");
        assert_eq!(codex_window(json, "c"), None);
        assert_eq!(cli_window("claude-code", "opus", None), None);
    }

    #[test]
    fn codex_default_is_named_from_its_config() {
        assert_eq!(
            codex_default("model = \"gpt-x\"\n[tui]\n").as_deref(),
            Some("gpt-x")
        );
        assert_eq!(codex_default("[tui]\n"), None);
    }

    #[test]
    fn claude_codes_build_gives_its_dated_ids_newest_first_by_their_names() {
        let bytes = b"xx\0claude-opus-4-5-20251101\0us.anthropic.claude-opus-4-5-20251101-v1:0 \
            claude-haiku-4-5@20251001 claude-3-5-sonnet-20241022 claude-sonnet-4-20250514 \
            claude-haiku-4-5-20251001 claude-opus-4-1-20250805 claude-opus-4-5-20251101";
        let ids = claude_ids(&bytes[..]).unwrap();
        assert_eq!(
            ids,
            [
                "claude-opus-4-5-20251101",
                "claude-haiku-4-5-20251001",
                "claude-opus-4-1-20250805",
                "claude-sonnet-4-20250514",
            ]
        );
        assert_eq!(
            claude_name("claude-opus-4-5-20251101").as_deref(),
            Some("Opus 4.5")
        );
        assert_eq!(
            claude_name("claude-sonnet-4-20250514").as_deref(),
            Some("Sonnet 4")
        );
        assert_eq!(cli_label("claude-code", "opus").as_deref(), Some("Opus"));
        assert_eq!(cli_label("claude-code", "").as_deref(), Some("Default"));
        assert_eq!(
            cli_label("claude-code", "claude-haiku-4-5-20251001").as_deref(),
            Some("Haiku 4.5")
        );
        assert_eq!(cli_label("codex", "opus"), None);
        assert!(of_cli("claude-code", None).is_none());
        assert!(of_cli("openai", None).is_none());
    }

    #[test]
    fn an_id_split_across_two_reads_is_still_seen_whole() {
        struct Dribble(Vec<u8>, usize);
        impl std::io::Read for Dribble {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = (self.0.len() - self.1).min(buf.len()).min(7);
                buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
                self.1 += n;
                Ok(n)
            }
        }
        let bytes = b"...claude-sonnet-4-5-20250929...".to_vec();
        let ids = claude_ids(Dribble(bytes, 0)).unwrap();
        assert_eq!(ids, ["claude-sonnet-4-5-20250929"]);
    }

    #[test]
    fn an_api_keeps_the_model_it_is_set_to_even_when_unlisted() {
        let all = of_api("mine", vec!["x".into(), "mine".into()]);
        let ids: Vec<_> = all.into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["mine", "x"]);
    }
}
