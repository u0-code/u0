//! Keeping ghosts on this machine: where they live, giving birth to one, and
//! starting and stopping the daemon that is her body.
//!
//! Everything here happens before or around a ghost, never to her. Birth
//! writes a certificate once and seals it; after that this module can start
//! her body, stop it, and say what it runs on. It has no way to change who
//! she is, because `ghost-core` has none to offer.
//!
//! The daemon is started as its own process and outlives this window: closing
//! the console does not stop her.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use ghost_core::{
    Ghost,
    identity::{self, Seed},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const BODY_FILE: &str = "body.json";
const DAEMON_CONFIG: &str = "ghostd.toml";
const PID_FILE: &str = "ghostd.pid";
const LOG_FILE: &str = "ghostd.log";

type Result<T> = std::result::Result<T, String>;

fn io<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> String + '_ {
    move |e| format!("{what}: {e}")
}

/// A ghost that lives on this machine.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub path: PathBuf,
    pub name: String,
    pub pronouns: String,
    pub city: String,
    pub country: String,
    pub born: String,
    pub seal: String,
}

/// What a ghost's body runs on. Never part of her; always replaceable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Body {
    /// `anthropic`, `openai` (any compatible endpoint), `claude-code`, `codex`.
    pub kind: String,
    pub model: String,
    pub base_url: Option<String>,
    /// A key kept in this machine's body file. Never sent back to the window.
    pub api_key: Option<String>,
    /// Or the name of an environment variable that holds it.
    pub api_key_env: Option<String>,
    pub json_mode: bool,
    /// The pace she lives at. 1.0 is human.
    pub tempo: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Running {
    pid: u32,
    path: PathBuf,
}

pub struct Keeper {
    home: PathBuf,
}

impl Keeper {
    /// The keeper for this machine. `GHOST_HOME` overrides where ghosts live.
    pub fn here() -> Result<Self> {
        let home = match std::env::var_os("GHOST_HOME") {
            Some(home) => PathBuf::from(home),
            None => dirs::data_dir()
                .ok_or("this system has no data directory")?
                .join("dev.ghost.console"),
        };
        Ok(Self::at(home))
    }

    pub fn at(home: PathBuf) -> Self {
        Self { home }
    }

    fn ghosts_dir(&self) -> PathBuf {
        self.home.join("ghosts")
    }

    /// Every born ghost under this home, by name.
    pub fn ghosts(&self) -> Vec<Summary> {
        let Ok(entries) = fs::read_dir(self.ghosts_dir()) else {
            return Vec::new();
        };
        let mut ghosts: Vec<Summary> = entries
            .filter_map(|entry| summary(&entry.ok()?.path()))
            .collect();
        ghosts.sort_by(|a, b| a.name.cmp(&b.name));
        ghosts
    }

    /// Give birth. `certificate` is the exact text of her `ghost.toml`; its
    /// bytes are written as given, so the seal the window computed while it
    /// was being written is the seal she is born with.
    pub fn birth(&self, certificate: &str) -> Result<Summary> {
        let seed = Seed::parse(certificate).map_err(|e| e.to_string())?;
        let dir = self.free_dir(&seed.name);
        fs::create_dir_all(&dir).map_err(io("make her a place"))?;
        let sealed = fs::write(dir.join(identity::SEED_FILE), certificate)
            .map_err(io("write her certificate"))
            .and_then(|()| Ghost::birth(&dir).map(drop).map_err(|e| e.to_string()));
        if let Err(e) = sealed {
            // Nobody was born. Leave nothing half-made behind.
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
        summary(&dir).ok_or_else(|| "she was born but cannot be read back".to_owned())
    }

    /// A directory for a ghost of this name that nothing else is using.
    fn free_dir(&self, name: &str) -> PathBuf {
        let slug: String = name
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let slug = slug.trim_matches('-');
        let slug = if slug.is_empty() { "ghost" } else { slug };
        let base = self.ghosts_dir();
        let mut dir = base.join(slug);
        let mut n = 2;
        while dir.exists() {
            dir = base.join(format!("{slug}-{n}"));
            n += 1;
        }
        dir
    }

    /// The body as the window may see it: without the key itself.
    pub fn body(&self) -> Value {
        let by_hand = self.home.join(DAEMON_CONFIG).exists();
        match self.read_body() {
            Some(body) => json!({
                "configured": true,
                "has_key": body.api_key.as_deref().is_some_and(|k| !k.is_empty()),
                "body": Body { api_key: None, ..body },
            }),
            // A daemon config written by hand works; it is just not ours to edit.
            None => {
                json!({"configured": by_hand, "by_hand": by_hand, "has_key": false, "body": null})
            }
        }
    }

    fn read_body(&self) -> Option<Body> {
        let text = fs::read_to_string(self.home.join(BODY_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Set what her body runs on. An absent key keeps the one already stored.
    pub fn set_body(&self, mut body: Body) -> Result<()> {
        body.kind = body.kind.trim().to_owned();
        body.model = body.model.trim().to_owned();
        if !matches!(
            body.kind.as_str(),
            "anthropic" | "openai" | "claude-code" | "codex"
        ) {
            return Err(format!("unknown kind of brain: {:?}", body.kind));
        }
        let needs_model = matches!(body.kind.as_str(), "anthropic" | "openai");
        if needs_model && body.model.is_empty() {
            return Err("a brain needs a model".to_owned());
        }
        let blank = |s: &Option<String>| s.as_deref().is_none_or(|s| s.trim().is_empty());
        if blank(&body.api_key) {
            body.api_key = self.read_body().and_then(|old| old.api_key);
        }
        if blank(&body.base_url) {
            body.base_url = None;
        }
        if blank(&body.api_key_env) {
            body.api_key_env = None;
        }
        if !body.tempo.is_finite() || body.tempo < 0.0 {
            body.tempo = 1.0;
        }
        fs::create_dir_all(&self.home).map_err(io("make the home directory"))?;
        let text = serde_json::to_string_pretty(&body).map_err(io("write the body"))?;
        write_private(&self.home.join(BODY_FILE), &text)?;
        write_private(&self.home.join(DAEMON_CONFIG), &daemon_config(&body)?)
    }

    /// The daemon running from this home, if its process is still alive.
    pub fn running(&self) -> Option<Value> {
        let text = fs::read_to_string(self.home.join(PID_FILE)).ok()?;
        let running: Running = serde_json::from_str(&text).ok()?;
        alive(running.pid).then(|| json!(running))
    }

    /// Start her body. It is its own process and outlives this one.
    pub fn wake(&self, path: &Path) -> Result<Value> {
        if let Some(running) = self.running() {
            if running["path"] == json!(path) {
                return Ok(running);
            }
            return Err("another ghost is awake on this machine; let her rest first".to_owned());
        }
        let config = self.home.join(DAEMON_CONFIG);
        if !config.exists() {
            return Err("she has no body yet: choose what she runs on first".to_owned());
        }
        let ghostd = find_ghostd()
            .ok_or("cannot find the ghostd binary; build it, put it on PATH, or set GHOSTD_BIN")?;
        let log_path = self.home.join(LOG_FILE);
        let log = File::create(&log_path).map_err(io("open her log"))?;
        let mut command = Command::new(&ghostd);
        command
            .arg("run")
            .arg(path)
            .arg("--config")
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(io("open her log"))?)
            .stderr(log);
        // Her own process group: quitting the console does not take her along.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|e| format!("start {}: {e}", ghostd.display()))?;

        // A daemon that cannot start says why and exits at once.
        std::thread::sleep(Duration::from_millis(900));
        if let Ok(Some(status)) = child.try_wait() {
            let said = fs::read_to_string(&log_path).unwrap_or_default();
            let last = said
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            return Err(format!(
                "her body would not start ({status}): {}",
                strip_ansi(last)
            ));
        }
        let running = Running {
            pid: child.id(),
            path: path.to_path_buf(),
        };
        let text = serde_json::to_string(&running).map_err(io("remember her process"))?;
        fs::write(self.home.join(PID_FILE), text).map_err(io("remember her process"))?;
        Ok(json!(running))
    }

    /// Stop her body. Her journal will record the time she was not running.
    pub fn rest(&self) -> Result<()> {
        let Some(running) = self.running() else {
            let _ = fs::remove_file(self.home.join(PID_FILE));
            return Ok(());
        };
        let pid = running["pid"].as_u64().unwrap_or(0).to_string();
        #[cfg(unix)]
        let stopped = Command::new("kill").args(["-INT", &pid]).status();
        #[cfg(not(unix))]
        let stopped = Command::new("taskkill").args(["/PID", &pid]).status();
        stopped.map_err(io("stop her body"))?;
        let _ = fs::remove_file(self.home.join(PID_FILE));
        Ok(())
    }

    /// Everything the window needs to know about this machine.
    pub fn state(&self) -> Value {
        json!({
            "home": self.home,
            "ghosts": self.ghosts(),
            "body": self.body(),
            "running": self.running(),
            "ghostd": find_ghostd(),
        })
    }
}

/// One entry point for the window and for the development bridge.
pub fn dispatch(cmd: &str, args: &Value) -> Result<Value> {
    let keeper = Keeper::here()?;
    let path = || {
        args["path"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| "which ghost?".to_owned())
    };
    match cmd {
        "state" => Ok(keeper.state()),
        "birth" => {
            let certificate = args["certificate"].as_str().ok_or("no certificate")?;
            Ok(json!(keeper.birth(certificate)?))
        }
        "set_body" => {
            let body: Body =
                serde_json::from_value(args["body"].clone()).map_err(io("read the body"))?;
            keeper.set_body(body)?;
            Ok(keeper.body())
        }
        "wake" => keeper.wake(&path()?),
        "rest" => keeper.rest().map(|()| Value::Null),
        other => Err(format!("the keeper does not know how to {other}")),
    }
}

fn summary(dir: &Path) -> Option<Summary> {
    let seal = identity::read_seal_file(dir).ok()??;
    let certificate = identity::read_certificate(dir).ok()?;
    let seed = certificate.seed();
    Some(Summary {
        path: dir.to_path_buf(),
        name: seed.name.clone(),
        pronouns: seed.pronouns.clone(),
        city: seed.home.city.clone(),
        country: seed.home.country.clone(),
        born: seed.born.clone(),
        seal: seal.to_string(),
    })
}

/// The daemon's configuration for a body: one brain for everything.
fn daemon_config(body: &Body) -> Result<String> {
    let mut provider = toml::Table::new();
    provider.insert("kind".into(), body.kind.clone().into());
    if !body.model.is_empty() {
        provider.insert("model".into(), body.model.clone().into());
    }
    if matches!(body.kind.as_str(), "anthropic" | "openai") {
        if let Some(url) = &body.base_url {
            provider.insert("base_url".into(), url.clone().into());
        }
        // A stored key wins; otherwise the daemon reads the named variable.
        match (&body.api_key, &body.api_key_env) {
            (Some(key), _) if !key.is_empty() => {
                provider.insert("api_key".into(), key.clone().into());
            }
            (_, Some(env)) => {
                provider.insert("api_key_env".into(), env.clone().into());
            }
            _ => {}
        }
    }
    if body.kind == "openai" && body.json_mode {
        provider.insert("json_mode".into(), true.into());
    }
    let brain: toml::Table = [("think", "main"), ("tick", "main")]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), toml::Value::from(v)))
        .collect();
    let mind: toml::Table = [("tempo".to_owned(), toml::Value::from(f64::from(body.tempo)))]
        .into_iter()
        .collect();
    let providers: BTreeMap<&str, toml::Table> = [("main", provider)].into_iter().collect();
    let mut root = toml::Table::new();
    root.insert("brain".into(), brain.into());
    root.insert("mind".into(), mind.into());
    root.insert(
        "providers".into(),
        toml::Value::try_from(providers).map_err(io("write the daemon config"))?,
    );
    toml::to_string(&root).map_err(io("write the daemon config"))
}

/// Write a file only its owner can read: it may hold a key.
fn write_private(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text).map_err(io("write"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io("protect"))?;
    }
    Ok(())
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// Where the daemon binary is: named explicitly, shipped beside this
/// program, built in a source tree this program sits in, or on PATH.
fn find_ghostd() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ghostd.exe"
    } else {
        "ghostd"
    };
    if let Some(named) = std::env::var_os("GHOSTD_BIN").map(PathBuf::from)
        && named.is_file()
    {
        return Some(named);
    }
    let exe = std::env::current_exe().ok()?;
    if let Some(beside) = exe.parent().map(|dir| dir.join(name))
        && beside.is_file()
    {
        return Some(beside);
    }
    for ancestor in exe.ancestors().skip(1) {
        for profile in ["release", "debug"] {
            let built = ancestor.join("target").join(profile).join(name);
            if built.is_file() && built != exe {
                return Some(built);
            }
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Skip to the letter that ends the escape sequence.
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERTIFICATE: &str = r#"name = "Mei Lin"
pronouns = "she/her"
born = "2026-11-12"
temperament = "quiet"
voice = "short"
origin = "made"

[home]
city = "Chengdu"
country = "CN"
timezone = "Asia/Shanghai"

[rhythm]
wake = "08:30"
sleep = "00:30"
"#;

    fn keeper() -> (tempfile::TempDir, Keeper) {
        let dir = tempfile::tempdir().unwrap();
        let keeper = Keeper::at(dir.path().to_path_buf());
        (dir, keeper)
    }

    #[test]
    fn birth_seals_exactly_what_was_written() {
        let (_dir, keeper) = keeper();
        assert!(keeper.ghosts().is_empty());
        let born = keeper.birth(CERTIFICATE).unwrap();
        assert_eq!(born.name, "Mei Lin");
        assert!(born.path.ends_with("ghosts/mei-lin"));
        // The seal is the hash of the bytes the window sent, nothing else.
        assert_eq!(
            born.seal,
            identity::Seal::of(CERTIFICATE.as_bytes()).to_string()
        );
        assert_eq!(
            fs::read_to_string(born.path.join("ghost.toml")).unwrap(),
            CERTIFICATE
        );
        assert_eq!(keeper.ghosts().len(), 1);
        // A second ghost of the same name gets her own place.
        let twin = keeper.birth(&CERTIFICATE.replace("quiet", "loud")).unwrap();
        assert!(twin.path.ends_with("ghosts/mei-lin-2"));
        assert_ne!(twin.seal, born.seal);
    }

    #[test]
    fn a_bad_certificate_leaves_nothing_behind() {
        let (_dir, keeper) = keeper();
        let bad = CERTIFICATE.replace("Asia/Shanghai", "Mars/Olympus");
        assert!(keeper.birth(&bad).is_err());
        assert!(keeper.ghosts().is_empty());
        assert!(!keeper.ghosts_dir().join("mei-lin").exists());
    }

    #[test]
    fn the_body_is_stored_privately_and_the_key_never_comes_back() {
        let (_dir, keeper) = keeper();
        assert_eq!(keeper.body()["configured"], false);
        keeper
            .set_body(Body {
                kind: "openai".into(),
                model: "deepseek-chat".into(),
                base_url: Some("https://api.deepseek.com/v1".into()),
                api_key: Some("not-a-real-key".into()),
                api_key_env: None,
                json_mode: true,
                tempo: 1.0,
            })
            .unwrap();
        let seen = keeper.body();
        assert_eq!(seen["has_key"], true);
        assert!(seen["body"]["api_key"].is_null());
        assert!(!seen.to_string().contains("not-a-real-key"));

        let config = fs::read_to_string(keeper.home.join(DAEMON_CONFIG)).unwrap();
        let parsed: toml::Table = toml::from_str(&config).unwrap();
        assert_eq!(parsed["brain"]["think"].as_str(), Some("main"));
        assert_eq!(
            parsed["providers"]["main"]["api_key"].as_str(),
            Some("not-a-real-key")
        );
        assert_eq!(
            parsed["providers"]["main"]["json_mode"].as_bool(),
            Some(true)
        );

        // Saving again without a key keeps the stored one.
        let mut next = keeper.read_body().unwrap();
        next.api_key = None;
        next.tempo = 0.5;
        keeper.set_body(next).unwrap();
        assert_eq!(
            keeper.read_body().unwrap().api_key.as_deref(),
            Some("not-a-real-key")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(keeper.home.join(BODY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_body_can_name_a_variable_instead_of_holding_a_key() {
        let (_dir, keeper) = keeper();
        keeper
            .set_body(Body {
                kind: "anthropic".into(),
                model: "claude-sonnet-5".into(),
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
                tempo: 1.0,
                ..Body::default()
            })
            .unwrap();
        let config = fs::read_to_string(keeper.home.join(DAEMON_CONFIG)).unwrap();
        assert!(config.contains("api_key_env = \"ANTHROPIC_API_KEY\""));
        assert!(!config.contains("api_key ="));
        assert!(
            keeper
                .set_body(Body {
                    kind: "abacus".into(),
                    ..Body::default()
                })
                .is_err()
        );
    }

    #[test]
    fn nothing_wakes_without_a_body() {
        let (_dir, keeper) = keeper();
        let born = keeper.birth(CERTIFICATE).unwrap();
        let refused = keeper.wake(&born.path).unwrap_err();
        assert!(refused.contains("no body yet"), "{refused}");
        assert!(keeper.running().is_none());
        assert!(keeper.rest().is_ok());
    }
}
