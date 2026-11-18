//! The brains in a home's `ghostd.toml`, and the keys in its `.env`, as an
//! operator changes them through the Ghost Protocol. The configuration is
//! the person's as much as the daemon's, so it is edited the way a person
//! would edit it: every comment, and the order of what is there, kept. A
//! key never goes into it, only the name of the variable it is read from;
//! the key itself goes into `.env`, which only its owner may read.

use std::{fs, io::Write as _, path::Path};

use anyhow::{Context as _, anyhow, bail};
use toml_edit::{DocumentMut, Item, Table, Value};

/// Write a brain as `[providers.<name>]`, setting the keys in `set` and
/// taking out those in `clear`. With `was`, the table saved under that name
/// is changed in place, every other key and comment in it kept, and moved
/// to `name` if that is another, with `[brain]` following it. A new brain
/// may not take a name already used. A key is not among what may be set,
/// but for the stand-in a server on the daemon's own machine is given,
/// which checks none.
pub(crate) fn write(
    doc: &mut DocumentMut,
    was: Option<&str>,
    name: &str,
    set: &[(String, Value)],
    clear: &[String],
) -> anyhow::Result<()> {
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        bail!("a brain's name is letters, digits, - and _");
    }
    if !doc.contains_key("providers") {
        let mut providers = Table::new();
        providers.set_implicit(true);
        doc["providers"] = Item::Table(providers);
    }
    let providers = doc["providers"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("[providers] is not a table"))?;
    if was != Some(name) && providers.contains_key(name) {
        bail!("{name} is taken");
    }
    let mut table = match was {
        Some(was) => match providers.get(was) {
            Some(Item::Table(t)) => t.clone(),
            _ => bail!("no brain is saved as {was}"),
        },
        None => Table::new(),
    };
    // What is added is written in the order a person would write it: what
    // it is, then where, then the rest as it came.
    let mut ordered: Vec<&(String, Value)> = set.iter().collect();
    ordered.sort_by_key(|(k, _)| ORDER.iter().position(|o| o == k).unwrap_or(ORDER.len()));
    for (k, v) in ordered {
        match table.get_mut(k).and_then(Item::as_value_mut) {
            Some(old) => replace(old, v.clone()),
            None => table[k.as_str()] = Item::Value(v.clone()),
        }
    }
    for k in clear {
        table.remove(k);
    }
    if set.iter().any(|(k, _)| k == "api_key") {
        let base = table.get("base_url").and_then(Item::as_str).unwrap_or("");
        if !on_this_machine(base) {
            bail!(
                "a key is not written into the configuration: give it as `key`, and it is kept \
                 in .env"
            );
        }
    }
    // A table keeps the position it was read at, so one changed or renamed
    // is written where it was.
    if let Some(was) = was {
        providers.remove(was);
    }
    providers.insert(name, Item::Table(table));
    if let Some(was) = was.filter(|w| *w != name)
        && let Some(brain) = doc.get_mut("brain").and_then(Item::as_table_like_mut)
    {
        for role in ["think", "tick"] {
            if let Some(old) = brain.get_mut(role).and_then(Item::as_value_mut)
                && old.as_str() == Some(was)
            {
                replace(old, name.into());
            }
        }
    }
    Ok(())
}

/// The keys of a brain's table, in the order they are written when added.
const ORDER: [&str; 6] = [
    "kind",
    "program",
    "model",
    "base_url",
    "api_key_env",
    "api_key",
];

/// Put `new` in place of `old`, keeping the comment beside it.
fn replace(old: &mut Value, new: Value) {
    let decor = old.decor().clone();
    *old = new;
    *old.decor_mut() = decor;
}

/// Take `[providers.<name>]` out. The brain `[brain]` thinks or ticks with
/// stays: without it the daemon would not start.
pub(crate) fn remove(doc: &mut DocumentMut, name: &str) -> anyhow::Result<()> {
    if in_use(doc, name) {
        bail!("{name} is in use: have her think with another brain first");
    }
    doc.get_mut("providers")
        .and_then(Item::as_table_like_mut)
        .and_then(|p| p.remove(name))
        .map(|_| ())
        .ok_or_else(|| anyhow!("no brain is saved as {name}"))
}

/// Make `name` the brain she thinks and ticks with, on `model` if one is
/// given; an empty model names none, for a command-line agent to use its
/// own default.
pub(crate) fn choose(doc: &mut DocumentMut, name: &str, model: Option<&str>) -> anyhow::Result<()> {
    if let Some(model) = model {
        let table = doc
            .get_mut("providers")
            .and_then(|p| p.get_mut(name))
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| anyhow!("no brain is saved as {name}"))?;
        if model.is_empty() {
            table.remove("model");
        } else {
            match table.get_mut("model").and_then(Item::as_value_mut) {
                Some(old) => replace(old, model.into()),
                None => {
                    table.insert("model", toml_edit::value(model));
                }
            }
        }
    }
    if !doc.contains_key("brain") {
        doc["brain"] = toml_edit::table();
    }
    for role in ["think", "tick"] {
        match doc["brain"].get_mut(role).and_then(Item::as_value_mut) {
            Some(old) => replace(old, name.into()),
            None => doc["brain"][role] = toml_edit::value(name),
        }
    }
    Ok(())
}

/// Whether `[brain]` thinks or ticks with `name`.
pub(crate) fn in_use(doc: &DocumentMut, name: &str) -> bool {
    ["think", "tick"].iter().any(|role| {
        doc.get("brain")
            .and_then(|b| b.get(role))
            .and_then(Item::as_str)
            == Some(name)
    })
}

/// A value from the protocol as `ghostd.toml` writes it: a string, a
/// number, a truth, or a list of them.
pub(crate) fn value(json: &serde_json::Value) -> anyhow::Result<Value> {
    Ok(match json {
        serde_json::Value::String(s) => s.as_str().into(),
        serde_json::Value::Bool(b) => (*b).into(),
        serde_json::Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.into(),
            (None, Some(f)) => f.into(),
            (None, None) => bail!("{n} is not a number ghostd.toml can hold"),
        },
        serde_json::Value::Array(items) => {
            let mut array = toml_edit::Array::new();
            for item in items {
                array.push(value(item)?);
            }
            Value::Array(array)
        }
        other => bail!("{other} is not a value a brain's table takes"),
    })
}

/// Whether a base URL is on the daemon's own machine.
pub(crate) fn on_this_machine(base_url: &str) -> bool {
    let host = authority(base_url).1;
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]")
}

/// The authority (`host:port`) and the host of a URL, lower case.
pub(crate) fn authority(url: &str) -> (String, String) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let host = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or(authority.as_str(), |(h, _)| h)
        .to_owned();
    (authority, host)
}

/// The text of a `.env` file with `var` set to `key`: the line that set it
/// before, if there was one, changed where it is; otherwise one added at
/// the end. Every other line is kept as it was. The key is written in
/// single quotes, which `.env` readers take literally.
pub(crate) fn with_key(text: &str, var: &str, key: &str) -> anyhow::Result<String> {
    let named = var
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !named {
        bail!("{var:?} is not the name of an environment variable");
    }
    let key = key.trim();
    if key.is_empty() {
        bail!("the key is empty");
    }
    if key.contains(['\n', '\r', '\'']) {
        bail!("a key is one line, with no ' in it");
    }
    let line = format!("{var}='{key}'");
    let sets = |l: &str| {
        let l = l.trim_start();
        let l = l.strip_prefix("export ").unwrap_or(l).trim_start();
        l.strip_prefix(var)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    for l in text.lines() {
        if sets(l) {
            if !placed {
                out.push(line.clone());
                placed = true;
            }
        } else {
            out.push(l.to_owned());
        }
    }
    if !placed {
        out.push(line);
    }
    Ok(out.join("\n") + "\n")
}

/// Write `text` as the `.env` at `path`, which only its owner may read:
/// beside it first, then renamed, so that a write cut short never leaves
/// half of one.
pub(crate) fn write_env(path: &Path, text: &str) -> anyhow::Result<()> {
    let part = path.with_extension("part");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&part)
        .with_context(|| format!("write {}", part.display()))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&part, path).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# how it thinks\n[brain]\nthink = \"main\" # the one\ntick = \"main\"\n\n\
        # the API\n[providers.main]\nkind = \"openai\"\nmodel = \"deepseek-chat\" # fast\n\
        base_url = \"https://api.deepseek.com/v1\"\napi_key_env = \"DEEPSEEK_API_KEY\"\n\
        json_mode = true\n\n# signed in\n[providers.codex]\nkind = \"codex\"\n";

    fn doc() -> DocumentMut {
        FILE.parse().unwrap()
    }

    fn set(pairs: &[(&str, serde_json::Value)]) -> Vec<(String, Value)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), value(v).unwrap()))
            .collect()
    }

    #[test]
    fn a_brain_added_keeps_every_comment_and_brain_already_there() {
        let mut doc = doc();
        write(
            &mut doc,
            None,
            "kimi",
            &set(&[
                ("kind", "openai".into()),
                ("model", "kimi-k3".into()),
                ("api_key_env", "MOONSHOT_API_KEY".into()),
            ]),
            &[],
        )
        .unwrap();
        let text = doc.to_string();
        assert!(text.starts_with(FILE), "{text}");
        assert!(
            text.ends_with(
                "[providers.kimi]\nkind = \"openai\"\nmodel = \"kimi-k3\"\napi_key_env = \"MOONSHOT_API_KEY\"\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_brain_changed_in_place_keeps_its_comments_its_other_keys_and_its_place() {
        let mut doc = doc();
        write(
            &mut doc,
            Some("main"),
            "main",
            &set(&[("model", "deepseek-flash".into())]),
            &["base_url".to_owned()],
        )
        .unwrap();
        let text = doc.to_string();
        assert!(
            text.contains(
                "# the API\n[providers.main]\nkind = \"openai\"\nmodel = \"deepseek-flash\" # fast\n\
                 api_key_env = \"DEEPSEEK_API_KEY\"\njson_mode = true\n"
            ),
            "{text}"
        );
        assert!(text.find("[providers.main]") < text.find("[providers.codex]"));
        // Renamed, it stays where it was and [brain] follows it.
        write(&mut doc, Some("main"), "deepseek", &[], &[]).unwrap();
        let text = doc.to_string();
        assert!(
            text.contains("think = \"deepseek\" # the one\ntick = \"deepseek\""),
            "{text}"
        );
        assert!(
            text.find("[providers.deepseek]") < text.find("[providers.codex]"),
            "{text}"
        );
        assert!(!text.contains("[providers.main]"), "{text}");
        // A new one may not take a name in use, nor a name TOML would split.
        assert!(write(&mut doc, None, "codex", &[], &[]).is_err());
        assert!(write(&mut doc, None, "a.b", &[], &[]).is_err());
        assert!(write(&mut doc, Some("gone"), "x", &[], &[]).is_err());
    }

    #[test]
    fn a_key_is_never_written_into_the_configuration_but_a_local_servers_stand_in_is() {
        let mut doc = doc();
        let refused = write(
            &mut doc,
            Some("main"),
            "main",
            &set(&[("api_key", "sk-live-secret".into())]),
            &[],
        )
        .unwrap_err();
        assert!(refused.to_string().contains("`key`"), "{refused}");
        assert!(!doc.to_string().contains("sk-live"), "{doc}");
        write(
            &mut doc,
            None,
            "ollama",
            &set(&[
                ("kind", "openai".into()),
                ("model", "gpt-oss:20b".into()),
                ("base_url", "http://localhost:11434/v1".into()),
                ("api_key", "ollama".into()),
            ]),
            &[],
        )
        .unwrap();
        assert!(doc.to_string().contains("api_key = \"ollama\""));
    }

    #[test]
    fn the_brain_in_use_is_not_removed_and_another_is_with_nothing_else_touched() {
        let mut doc = doc();
        assert!(remove(&mut doc, "main").is_err(), "the brain in use stays");
        remove(&mut doc, "codex").unwrap();
        let text = doc.to_string();
        assert!(!text.contains("codex"), "{text}");
        assert!(text.starts_with("# how it thinks\n"), "{text}");
        assert!(remove(&mut doc, "codex").is_err());
    }

    #[test]
    fn choosing_a_brain_sets_both_tiers_and_its_model_and_keeps_their_comments() {
        let mut doc = doc();
        choose(&mut doc, "codex", Some("gpt-x")).unwrap();
        let text = doc.to_string();
        assert!(
            text.contains("think = \"codex\" # the one\ntick = \"codex\"\n"),
            "{text}"
        );
        assert!(
            text.contains("[providers.codex]\nkind = \"codex\"\nmodel = \"gpt-x\"\n"),
            "{text}"
        );
        // An empty model is the agent's own default; none given, it is left.
        choose(&mut doc, "codex", Some("")).unwrap();
        assert!(!doc.to_string().contains("gpt-x"));
        choose(&mut doc, "main", None).unwrap();
        assert!(doc.to_string().contains("model = \"deepseek-chat\" # fast"));
        assert!(choose(&mut doc, "gone", Some("m")).is_err());
    }

    #[test]
    fn a_key_goes_into_env_in_place_of_the_old_one_and_every_other_line_stays() {
        let text = "# keys\nOTHER=1\nexport DEEPSEEK_API_KEY=old\n\n# end\n";
        let out = with_key(text, "DEEPSEEK_API_KEY", " sk-new ").unwrap();
        assert_eq!(out, "# keys\nOTHER=1\nDEEPSEEK_API_KEY='sk-new'\n\n# end\n");
        let read: Vec<(String, String)> = dotenvy::from_read_iter(out.as_bytes())
            .map(Result::unwrap)
            .collect();
        assert!(read.contains(&("DEEPSEEK_API_KEY".to_owned(), "sk-new".to_owned())));
        assert_eq!(
            with_key("", "NEW_KEY", "k").unwrap(),
            "NEW_KEY='k'\n",
            "added to an empty file"
        );
        assert!(with_key(text, "NOT A VAR", "k").is_err());
        assert!(with_key(text, "K", "two\nlines").is_err());
        assert!(with_key(text, "K", "  ").is_err());
        // A variable whose name begins another's is not taken for it.
        assert_eq!(
            with_key("KEY_TWO=2\n", "KEY", "1").unwrap(),
            "KEY_TWO=2\nKEY='1'\n"
        );
    }

    #[test]
    fn a_new_env_is_its_owners_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        write_env(&path, "K='v'\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "K='v'\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
