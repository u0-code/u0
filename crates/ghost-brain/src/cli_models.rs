//! The models a command-line agent offers, and how large their windows
//! are, each read from the agent as it is installed on this machine rather
//! than from a list written here.
//!
//! Claude Code is not asked over the network: the ids its installed build
//! knows are read out of its binary, and named the way Claude Code names
//! them, after the families it takes by alias. Codex keeps the models your
//! account may use in its own cache, `models_cache.json`, with each one's
//! window, and the one it uses when none is named in its `config.toml`,
//! both in its home: `$CODEX_HOME`, or `~/.codex`.

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{CliAgent, CliKind};

/// A model an agent offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    /// What the agent is asked for it by; empty for its own default.
    pub id: String,
    /// The name the agent shows it by; for the default, the model that is,
    /// where the agent says. Empty where it gives none.
    pub name: String,
}

impl Offered {
    fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
        }
    }
}

/// The models `agent` offers, its own default first. Reads Claude Code's
/// build whole, so it is slow: call it off any thread that must not wait.
pub fn offered(agent: &CliAgent) -> Result<Vec<Offered>, String> {
    let path = find(&agent.program)
        .ok_or_else(|| format!("`{}` was not found on this machine", agent.program))?;
    match agent.kind {
        CliKind::Claude => {
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            let ids = claude_ids(std::io::BufReader::new(file))?;
            let mut all = vec![Offered::new("", "")];
            all.extend(CLAUDE.iter().map(|(id, name)| Offered::new(id, name)));
            all.extend(
                ids.iter()
                    .map(|id| Offered::new(id, &claude_name(id).unwrap_or_default())),
            );
            Ok(all)
        }
        CliKind::Codex => Ok(codex(codex_home().as_deref())),
    }
}

/// The context window, in tokens, of the model `agent` runs, where it is
/// known: Codex says in its cache; every model Claude Code runs is Claude.
pub(crate) fn window(agent: &CliAgent) -> Option<u64> {
    match agent.kind {
        CliKind::Claude => {
            let model = agent.model.as_deref().unwrap_or_default();
            if model.is_empty() || CLAUDE.iter().any(|(alias, _)| *alias == model) {
                crate::known_window(&format!("claude-{model}"))
            } else {
                crate::known_window(model)
            }
        }
        CliKind::Codex => codex_window(codex_home().as_deref()?, agent.model.as_deref()),
    }
}

/// Where `program` is: the path given, or the first of its name on PATH.
pub fn find(program: &str) -> Option<PathBuf> {
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

/// The families Claude Code takes by alias, by the name it shows each.
const CLAUDE: [(&str, &str); 3] = [("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")];

/// Every dated first-party Claude id in what `reader` reads, newest first:
/// a run of `claude-<family>-<major>[-<minor>]-<yyyymmdd>` with a family
/// of opus, sonnet or haiku, from the fourth generation on. Other spellings
/// of the same model (Bedrock's `-v1:0`, Vertex's `@date`) and older models
/// are left out.
fn claude_ids(mut reader: impl Read) -> Result<Vec<String>, String> {
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
                && claude_parts(id).is_some()
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
    if !CLAUDE.iter().any(|(alias, _)| *alias == family) {
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

/// Newest and largest first: by generation, then the family's weight.
fn claude_sort_key(id: &str) -> (u32, u32, u32, usize) {
    claude_parts(id).map_or((0, 0, 0, 0), |(family, major, minor, date)| {
        let lighter = CLAUDE.iter().position(|(alias, _)| *alias == family);
        (
            major,
            minor,
            date,
            CLAUDE.len() - lighter.unwrap_or(CLAUDE.len()),
        )
    })
}

/// The name Claude Code shows a dated id under: `Opus 4.5`, `Sonnet 4`.
fn claude_name(id: &str) -> Option<String> {
    let (family, major, minor, _) = claude_parts(id)?;
    let (_, name) = CLAUDE.iter().find(|(alias, _)| *alias == family)?;
    Some(if minor == 0 {
        format!("{name} {major}")
    } else {
        format!("{name} {major}.{minor}")
    })
}

/// Codex's home on this machine.
fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
}

/// What Codex keeps in `dir` under `name`, if anything.
fn codex_read(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok()
}

/// The models Codex offers from its home `dir`: its default, by the model
/// its configuration names, then those its cache shows in its own picker,
/// in its order.
fn codex(dir: Option<&Path>) -> Vec<Offered> {
    let default = dir
        .and_then(|d| codex_read(d, "config.toml"))
        .and_then(|t| codex_default(&t));
    let mut all = vec![Offered::new("", default.as_deref().unwrap_or_default())];
    let cache = dir.and_then(|d| codex_read(d, "models_cache.json"));
    let Some(models) = cache.as_deref().and_then(codex_models) else {
        return all;
    };
    let mut listed: Vec<(i64, Offered)> = models
        .iter()
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|m| {
            let id = m.get("slug").and_then(Value::as_str)?;
            let name = m.get("display_name").and_then(Value::as_str).unwrap_or(id);
            let order = m
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            Some((order, Offered::new(id, name)))
        })
        .collect();
    listed.sort_by_key(|(order, _)| *order);
    all.extend(listed.into_iter().map(|(_, o)| o));
    all
}

/// The window Codex's cache in `dir` gives `model`, or its default.
fn codex_window(dir: &Path, model: Option<&str>) -> Option<u64> {
    let model = match model {
        Some(m) => m.to_owned(),
        None => codex_default(&codex_read(dir, "config.toml")?)?,
    };
    codex_models(&codex_read(dir, "models_cache.json")?)?
        .iter()
        .find(|m| m.get("slug").and_then(Value::as_str) == Some(model.as_str()))?
        .get("context_window")?
        .as_u64()
}

/// The models in Codex's cache.
fn codex_models(json: &str) -> Option<Vec<Value>> {
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("models")?.as_array().cloned()
}

/// The model Codex uses when none is named: `model` in its configuration.
fn codex_default(toml: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(toml).ok()?;
    table.get("model")?.as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(offered: &[Offered]) -> Vec<&str> {
        offered.iter().map(|o| o.id.as_str()).collect()
    }

    /// A Codex home holding `files`.
    fn codex_home_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        dir
    }

    const CACHE: &str = r#"{"models":[
        {"slug":"b","display_name":"B","visibility":"list","priority":2},
        {"slug":"hidden","visibility":"hide","priority":0,"context_window":1},
        {"slug":"a","display_name":"A","visibility":"list","priority":1,"context_window":272000}
    ]}"#;

    #[test]
    fn codex_offers_its_default_then_what_it_lists_in_its_own_order() {
        let home = codex_home_with(&[
            ("config.toml", "model = \"a\"\n[tui]\n"),
            ("models_cache.json", CACHE),
        ]);
        let offered = codex(Some(home.path()));
        assert_eq!(ids(&offered), ["", "a", "b"]);
        assert_eq!(offered[0].name, "a", "the default says which model it is");
        assert_eq!(offered[1].name, "A");

        // Nothing kept, only its default, which it does not name.
        let bare = codex(Some(codex_home_with(&[]).path()));
        assert_eq!(bare, [Offered::new("", "")]);
        let broken = codex_home_with(&[("models_cache.json", "not json")]);
        assert_eq!(ids(&codex(Some(broken.path()))), [""]);
    }

    #[test]
    fn codex_says_how_large_its_models_window_is_its_default_by_its_configuration() {
        let home = codex_home_with(&[
            ("config.toml", "model = \"a\"\n"),
            ("models_cache.json", CACHE),
        ]);
        assert_eq!(codex_window(home.path(), Some("a")), Some(272_000));
        assert_eq!(codex_window(home.path(), None), Some(272_000));
        assert_eq!(
            codex_window(home.path(), Some("b")),
            None,
            "it says nothing"
        );
        assert_eq!(codex_window(home.path(), Some("c")), None);
        let unset = codex_home_with(&[("models_cache.json", CACHE)]);
        assert_eq!(codex_window(unset.path(), None), None);
    }

    #[test]
    fn claude_codes_window_is_claudes_by_alias_by_id_and_by_default() {
        let agent = |model: Option<&str>| CliAgent {
            kind: CliKind::Claude,
            program: "claude".into(),
            model: model.map(str::to_owned),
            args: Vec::new(),
        };
        assert_eq!(window(&agent(None)), Some(200_000));
        assert_eq!(window(&agent(Some("opus"))), Some(200_000));
        assert_eq!(
            window(&agent(Some("claude-haiku-4-5-20251001"))),
            Some(200_000)
        );
        assert_eq!(window(&agent(Some("qwen3"))), None, "not a Claude model");
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
    }

    #[test]
    fn claude_code_offers_its_default_its_families_and_what_its_build_knows() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("claude");
        std::fs::write(&program, b"..claude-sonnet-4-5-20250929..").unwrap();
        let agent = CliAgent {
            kind: CliKind::Claude,
            program: program.display().to_string(),
            model: None,
            args: Vec::new(),
        };
        let offered = offered(&agent).unwrap();
        assert_eq!(
            ids(&offered),
            ["", "opus", "sonnet", "haiku", "claude-sonnet-4-5-20250929"]
        );
        assert_eq!(offered[4].name, "Sonnet 4.5");
        let gone = CliAgent {
            program: dir.path().join("nothing").display().to_string(),
            ..agent
        };
        assert!(super::offered(&gone).unwrap_err().contains("not found"));
    }

    #[test]
    fn an_id_split_across_two_reads_is_still_seen_whole() {
        struct Dribble(Vec<u8>, usize);
        impl Read for Dribble {
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
}
