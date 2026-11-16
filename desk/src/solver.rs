//! The solver's deployment, as the desk looks after it: the directory
//! `solver/` with its `ghostd.toml`, the ghost born from `ghosts/_solver`,
//! and the daemon that runs it. The desk can read which brains the
//! deployment knows, pick one, and start or restart the daemon as a child
//! of its own. It never edits a ghost's own files; the deployment's
//! configuration is what a person would edit by hand, and it is edited the
//! way a person would, keeping every comment.

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use anyhow::{Context as _, anyhow};
use tracing::{info, warn};

/// One brain the deployment could run on: a `[providers.<name>]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub name: String,
    /// `openai`, `anthropic`, `claude-code` or `codex`.
    pub kind: String,
    pub model: String,
    /// The environment variable its key comes from, for an API.
    pub key_env: Option<String>,
    /// Whether this machine can run it: the key is set, or the CLI is on PATH.
    pub ready: bool,
}

impl Provider {
    /// The icon that stands for it, among the desk's assets.
    pub fn icon(&self) -> &'static str {
        match self.kind.as_str() {
            "claude-code" | "anthropic" => "claude",
            "codex" => "openai",
            _ if self.model.contains("deepseek") || self.name.contains("deepseek") => "deepseek",
            _ => "openai",
        }
    }

    /// How it is spoken of: the CLI, or the model.
    pub fn label(&self) -> String {
        match self.kind.as_str() {
            "claude-code" => format!("Claude Code · {}", self.model),
            "codex" => format!("Codex · {}", self.model),
            _ => self.model.clone(),
        }
        .trim_end_matches(" · ")
        .to_owned()
    }
}

/// What the solver can do besides talk, as `[mind.work]` sets it: the
/// empty page says so, and offers only what it can do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Abilities {
    /// Work is on at all.
    pub work: bool,
    /// The machine its work runs on, as written: `local:` is this one.
    pub machine: Option<String>,
    /// It may search and read the web.
    pub web: bool,
}

impl Abilities {
    /// Whether it has a shell on this machine, as you.
    pub fn local(&self) -> bool {
        self.work && self.machine.as_deref() == Some("local:")
    }

    /// One line saying what it can do, for the empty page.
    pub fn sentence(&self) -> String {
        let web = self.work && self.web;
        match (self.local(), web) {
            (true, true) => {
                "It has a shell on this machine, as you, and can search and read the web."
            }
            (true, false) => "It has a shell on this machine, as you. It has no web.",
            (false, true) => "It can search and read the web. It has no machine here.",
            (false, false) => "It can only talk: its deployment gives it no machine and no web.",
        }
        .to_owned()
    }
}

/// Where the deployment is, and the daemon if the desk started it.
pub struct Deployment {
    /// The checkout: `solver/`, `ghosts/` and `target/` are under it.
    pub root: PathBuf,
    child: Option<Child>,
}

impl Deployment {
    /// Find the checkout: `GHOST_ROOT`, else the first directory upward
    /// from the current one that has `solver/ghostd.example.toml`, else the
    /// one this crate was built in.
    pub fn find() -> Self {
        let root = std::env::var_os("GHOST_ROOT")
            .map(PathBuf::from)
            .filter(|p| p.join("solver/ghostd.example.toml").is_file())
            .or_else(|| {
                let mut dir = std::env::current_dir().ok()?;
                loop {
                    if dir.join("solver/ghostd.example.toml").is_file() {
                        return Some(dir);
                    }
                    if !dir.pop() {
                        return None;
                    }
                }
            })
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from("."))
            });
        Self { root, child: None }
    }

    /// The configuration the daemon runs with: `solver/ghostd.toml`,
    /// copied from the published example the first time it is wanted. It
    /// is not published itself, since it names you and your choices.
    pub fn config(&self) -> PathBuf {
        let path = self.root.join("solver/ghostd.toml");
        if !path.is_file() {
            let example = self.root.join("solver/ghostd.example.toml");
            if let Err(e) = std::fs::copy(&example, &path) {
                warn!("could not copy {}: {e}", example.display());
            }
        }
        path
    }

    pub fn ghost_dir(&self) -> PathBuf {
        self.root.join("ghosts/solver")
    }

    /// The solver's prompts, wherever `[mind] prompts` in its configuration
    /// says, relative to the checkout as the daemon runs there. They belong
    /// to the checkout, not to the copy the desk made of its configuration,
    /// so when the directory that copy names has gone (they were in
    /// `solver/prompts` before every prompt moved to `prompts/`), the
    /// example's is written in its place.
    pub fn prompts(&self) -> Option<PathBuf> {
        let named = |path: &Path| -> Option<String> {
            let text = std::fs::read_to_string(path).ok()?;
            let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
            doc.get("mind")?.get("prompts")?.as_str().map(str::to_owned)
        };
        let path = self.config();
        let mine = named(&path)?;
        if self.root.join(&mine).is_dir() {
            return Some(self.root.join(mine));
        }
        let example = named(&self.root.join("solver/ghostd.example.toml"))?;
        if example == mine || !self.root.join(&example).is_dir() {
            return None;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let mut doc = text.parse::<toml_edit::DocumentMut>().ok()?;
        doc["mind"]["prompts"] = toml_edit::value(example.as_str());
        match std::fs::write(&path, doc.to_string()) {
            Ok(()) => info!("the solver's prompts are now read from {example}, not {mine}"),
            Err(e) => warn!("could not write {}: {e}", path.display()),
        }
        Some(self.root.join(example))
    }

    /// The daemon binary: `GHOSTD_BIN`, else the newer of the checkout's
    /// release and debug builds, else `ghostd` on PATH.
    pub fn ghostd(&self) -> PathBuf {
        if let Some(bin) = std::env::var_os("GHOSTD_BIN") {
            return PathBuf::from(bin);
        }
        ["target/release/ghostd", "target/debug/ghostd"]
            .iter()
            .map(|p| self.root.join(p))
            .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
            .max_by_key(|(when, _)| *when)
            .map_or_else(|| PathBuf::from("ghostd"), |(_, p)| p)
    }

    /// Every brain the deployment names, with whether it can run here.
    pub fn providers(&self) -> Vec<Provider> {
        let Ok(text) = std::fs::read_to_string(self.config()) else {
            return Vec::new();
        };
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            return Vec::new();
        };
        let Some(table) = doc.get("providers").and_then(|p| p.as_table()) else {
            return Vec::new();
        };
        table
            .iter()
            .map(|(name, p)| {
                let s = |k: &str| {
                    p.get(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned()
                };
                let kind = s("kind");
                let key_env = p
                    .get("api_key_env")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let ready = match kind.as_str() {
                    "claude-code" => on_path("claude"),
                    "codex" => on_path("codex"),
                    "anthropic" => {
                        key_env
                            .as_deref()
                            .or(Some("ANTHROPIC_API_KEY"))
                            .is_some_and(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()))
                            || p.get("api_key").is_some()
                    }
                    _ => {
                        key_env
                            .as_deref()
                            .is_some_and(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()))
                            || p.get("api_key").is_some()
                    }
                };
                Provider {
                    name: name.to_owned(),
                    kind,
                    model: s("model"),
                    key_env,
                    ready,
                }
            })
            .collect()
    }

    /// What `[mind.work]` lets it do. Work is on unless it says otherwise.
    pub fn abilities(&self) -> Abilities {
        let Some(doc) = std::fs::read_to_string(self.config())
            .ok()
            .and_then(|t| t.parse::<toml_edit::DocumentMut>().ok())
        else {
            return Abilities::default();
        };
        let work = doc.get("mind").and_then(|m| m.get("work"));
        let field = |k: &str| work.and_then(|w| w.get(k));
        Abilities {
            work: work.is_some()
                && field("enabled")
                    .and_then(toml_edit::Item::as_bool)
                    .unwrap_or(true),
            machine: field("machine")
                .and_then(toml_edit::Item::as_str)
                .map(str::to_owned),
            web: field("web")
                .and_then(toml_edit::Item::as_bool)
                .unwrap_or(false),
        }
    }

    /// The directory its machine works in: `machine = "local:"` runs as
    /// you, under the ghost's own `home`.
    pub fn home(&self) -> PathBuf {
        self.ghost_dir().join("home")
    }

    /// Which brain `[brain] think` names.
    pub fn brain(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.config()).ok()?;
        let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
        doc.get("brain")?.get("think")?.as_str().map(str::to_owned)
    }

    /// Make `name` the brain it thinks and ticks with, in the file, keeping
    /// everything else as it is.
    pub fn choose(&self, name: &str) -> anyhow::Result<()> {
        let path = self.config();
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut doc = text
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("parse {}", path.display()))?;
        if !doc.contains_key("brain") {
            doc["brain"] = toml_edit::table();
        }
        doc["brain"]["think"] = toml_edit::value(name);
        doc["brain"]["tick"] = toml_edit::value(name);
        std::fs::write(&path, doc.to_string())
            .with_context(|| format!("write {}", path.display()))?;
        info!("the solver thinks with {name} from its next start");
        Ok(())
    }

    /// Make whoever sits at the desk an operator of the solver on the
    /// `desk` channel. In private, an operator has the machine in the
    /// middle of a conversation, as at work; without it the solver could
    /// only talk. Returns whether the file changed, so a running daemon
    /// knows to be restarted.
    pub fn ensure_operator(&self, channel: &str, who: &str) -> anyhow::Result<bool> {
        let path = self.config();
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut doc = text
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("parse {}", path.display()))?;
        let id = who.to_lowercase();
        let present = doc
            .get("operators")
            .and_then(|o| o.get(channel))
            .and_then(|a| a.as_array())
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(id.as_str())));
        if present {
            return Ok(false);
        }
        if !doc.contains_key("operators") {
            doc["operators"] = toml_edit::table();
        }
        let mut ids = doc["operators"]
            .get(channel)
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default();
        ids.push(id.as_str());
        doc["operators"][channel] = toml_edit::value(ids);
        std::fs::write(&path, doc.to_string())
            .with_context(|| format!("write {}", path.display()))?;
        info!("{id} on {channel} is an operator of the solver");
        Ok(true)
    }

    /// Add a brain to the deployment, as `[providers.<name>]`, and leave
    /// everything else as it is. A key is never written: an API's comes
    /// from the variable `key_env` names, in the environment or `.env`.
    pub fn add_provider(&self, name: &str, fields: &[(&str, &str)]) -> anyhow::Result<()> {
        let path = self.config();
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut doc = text
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("parse {}", path.display()))?;
        if !doc.contains_key("providers") {
            let mut providers = toml_edit::Table::new();
            providers.set_implicit(true);
            doc["providers"] = toml_edit::Item::Table(providers);
        }
        let mut table = toml_edit::Table::new();
        for (k, v) in fields {
            if !v.is_empty() {
                table[k] = toml_edit::value(*v);
            }
        }
        doc["providers"][name] = toml_edit::Item::Table(table);
        std::fs::write(&path, doc.to_string())
            .with_context(|| format!("write {}", path.display()))?;
        info!("the solver can now think with {name}");
        Ok(())
    }

    /// Whether the daemon the desk started is still running.
    pub fn owns_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => {
                if let Ok(None) = child.try_wait() {
                    true
                } else {
                    self.child = None;
                    false
                }
            }
            None => false,
        }
    }

    /// Give birth to the solver if it has not been born: a copy of the
    /// template, then `ghostd birth`. Happens once.
    pub fn ensure_born(&self) -> anyhow::Result<()> {
        let dir = self.ghost_dir();
        if dir.join("journal.db").is_file() {
            return Ok(());
        }
        if !dir.join("ghost.toml").is_file() {
            copy_dir(&self.root.join("ghosts/_solver"), &dir)?;
        }
        let out = Command::new(self.ghostd())
            .arg("birth")
            .arg(&dir)
            .current_dir(&self.root)
            .output()
            .context("run ghostd birth")?;
        if !out.status.success() {
            return Err(anyhow!(
                "ghostd birth: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        info!("the solver is born in {}", dir.display());
        Ok(())
    }

    /// Start the daemon as a child of the desk, with the deployment's
    /// configuration. Its output goes to the desk's stderr.
    pub fn start(&mut self) -> anyhow::Result<()> {
        if self.owns_running() {
            return Ok(());
        }
        self.ensure_born()?;
        if self.prompts().is_none() {
            warn!(
                "the solver's configuration names no prompts that are there: it runs on the built-in ones, as a character would"
            );
        }
        let child = Command::new(self.ghostd())
            .arg("run")
            .arg(self.ghost_dir())
            .arg("--config")
            .arg(self.config())
            // Her lifeline: the desk holds this pipe open, and when the
            // desk goes, however it goes, the system closes it and she
            // stops in order. The desk's own ghost never outlives it.
            .arg("--with-parent")
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("start {}", self.ghostd().display()))?;
        info!("started the solver's daemon, pid {}", child.id());
        self.child = Some(child);
        Ok(())
    }

    /// Stop the daemon the desk started, if any, and start it again.
    pub fn restart(&mut self) -> anyhow::Result<()> {
        self.stop();
        self.start()
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Letting go of her lifeline stops her in order; SIGTERM as
            // well, for a ghostd built before it read one.
            drop(child.stdin.take());
            let pid = child.id();
            let _ = Command::new("kill").arg(pid.to_string()).status();
            for _ in 0..40 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            warn!("the solver's daemon did not stop in time; killing it");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Deployment {
    fn drop(&mut self) {
        self.stop();
    }
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

fn copy_dir(from: &Path, to: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from).with_context(|| format!("read {}", from.display()))? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checkout with only the published example in it.
    fn checkout(name: &str) -> Deployment {
        let root = std::env::temp_dir().join(format!("ghost-desk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("solver")).unwrap();
        std::fs::write(
            root.join("solver/ghostd.example.toml"),
            "# how it thinks\n[brain]\nthink = \"main\"\ntick = \"main\"\n\n[mind.work]\nenabled = true\nmachine = \"local:\"\nweb = true\n\n[operators]\n\n# the API\n[providers.main]\nkind = \"openai\"\nmodel = \"m\"\napi_key_env = \"NO_SUCH_KEY_FOR_A_TEST\"\n",
        )
        .unwrap();
        Deployment { root, child: None }
    }

    #[test]
    fn the_empty_page_says_only_what_the_deployment_allows() {
        let d = checkout("abilities");
        let a = d.abilities();
        assert!(a.local() && a.web);
        assert!(a.sentence().contains("shell on this machine"));
        let none = Abilities::default();
        assert!(!none.local());
        assert!(none.sentence().starts_with("It can only talk"));
        let off = Abilities { work: false, ..a };
        assert!(!off.local(), "a machine is nothing with work turned off");
    }

    #[test]
    fn the_first_use_copies_the_example_and_leaves_it_alone() {
        let d = checkout("copy");
        let config = d.config();
        assert!(config.is_file());
        assert!(d.root.join("solver/ghostd.example.toml").is_file());
        assert_eq!(d.brain().as_deref(), Some("main"));
    }

    #[test]
    fn the_solvers_prompts_follow_the_example_when_the_directory_its_copy_names_has_gone() {
        let d = checkout("prompts");
        std::fs::create_dir_all(d.root.join("prompts/system/desk")).unwrap();
        std::fs::write(
            d.root.join("solver/ghostd.example.toml"),
            "[mind]\nprompts = \"prompts/system/desk\"\n",
        )
        .unwrap();
        // A copy made when the prompts were in solver/prompts.
        std::fs::write(
            d.root.join("solver/ghostd.toml"),
            "# mine\n[mind]\nprompts = \"solver/prompts\"\ntempo = 0.05\n",
        )
        .unwrap();
        assert_eq!(d.prompts(), Some(d.root.join("prompts/system/desk")));
        let text = std::fs::read_to_string(d.config()).unwrap();
        assert!(text.contains("prompts = \"prompts/system/desk\""), "{text}");
        assert!(
            text.contains("# mine") && text.contains("tempo = 0.05"),
            "{text}"
        );
        // Where it is, it is left as it is.
        assert_eq!(d.prompts(), Some(d.root.join("prompts/system/desk")));
    }

    #[test]
    fn whoever_sits_at_the_desk_is_made_an_operator_once() {
        let d = checkout("operator");
        assert!(d.ensure_operator("desk", "Fox").unwrap());
        assert!(!d.ensure_operator("desk", "fox").unwrap());
        let text = std::fs::read_to_string(d.config()).unwrap();
        assert!(text.contains("desk = [\"fox\"]"), "{text}");
    }

    #[test]
    fn a_brain_added_and_chosen_keeps_the_rest_of_the_file() {
        let d = checkout("brain");
        d.add_provider(
            "claude-code",
            &[("kind", "claude-code"), ("model", "sonnet"), ("x", "")],
        )
        .unwrap();
        d.choose("claude-code").unwrap();
        let text = std::fs::read_to_string(d.config()).unwrap();
        assert!(
            text.contains("# how it thinks") && text.contains("# the API"),
            "{text}"
        );
        assert!(!text.contains("x ="), "an empty field is left out: {text}");
        assert_eq!(d.brain().as_deref(), Some("claude-code"));
        let providers = d.providers();
        let main = providers.iter().find(|p| p.name == "main").unwrap();
        assert!(!main.ready, "an API whose key is not set is not ready");
        assert_eq!(main.icon(), "openai");
        let claude = providers.iter().find(|p| p.name == "claude-code").unwrap();
        assert_eq!(
            (claude.icon(), claude.label().as_str()),
            ("claude", "Claude Code · sonnet")
        );
    }
}
