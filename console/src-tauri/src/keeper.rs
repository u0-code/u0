//! Keeping ghosts on this machine: where they live, giving birth to one,
//! what each one thinks with, and starting and stopping the daemon that is
//! her body.
//!
//! Everything here happens before or around a ghost, never to her. Birth
//! writes a certificate once and seals it. After that this module can start
//! her body, stop it, and change what it runs on. It has no way to change who
//! she is, because `ghost-core` has none to offer.
//!
//! Each ghost has her own daemon on her own port, so several can be awake at
//! once. A daemon is its own process and outlives this window: closing the
//! console does not stop anyone.
//!
//! ```text
//! <home>/
//! ├── ghosts/<slug>/     a ghost: certificate, seal, journal, and a picture
//! ├── models.json        the brains available on this machine (private)
//! ├── keeper.json        which model each ghost uses, and the pace
//! └── run/<slug>.*       her daemon's config, pid and port, and log
//! ```

use std::{
    collections::BTreeMap,
    fs::{self, File},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use ghost_core::{
    Ghost,
    identity::{self, Seed},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MODELS_FILE: &str = "models.json";
const KEEPER_FILE: &str = "keeper.json";
/// A daemon config written by hand, from before the console kept models.
const LEGACY_CONFIG: &str = "ghostd.toml";
const FIRST_PORT: u16 = 7437;
/// The largest picture the window may hand over, decoded.
const AVATAR_LIMIT: usize = 2 * 1024 * 1024;

type Result<T> = std::result::Result<T, String>;

fn io<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> String + '_ {
    move |e| format!("{what}: {e}")
}

/// A brain available on this machine. Never part of a ghost.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Model {
    pub id: String,
    /// `anthropic`, `openai` (any compatible endpoint), `claude-code`, `codex`.
    pub kind: String,
    pub model: String,
    pub base_url: Option<String>,
    /// A key kept on this machine. Never sent back to the window.
    pub api_key: Option<String>,
    /// Or the name of an environment variable that holds it.
    pub api_key_env: Option<String>,
    pub json_mode: bool,
}

/// What the keeper remembers about how ghosts are run.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Book {
    /// Ghost slug to the id of the model she thinks with.
    thinks_with: BTreeMap<String, String>,
    /// The pace ghosts live at. 1.0 is human.
    tempo: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Running {
    pid: u32,
    port: u16,
}

/// A ghost that lives on this machine.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub path: PathBuf,
    pub slug: String,
    pub name: String,
    pub pronouns: String,
    pub city: String,
    pub country: String,
    pub born: String,
    pub seal: String,
    /// The id of the model she thinks with, if one is chosen or available.
    pub model: Option<String>,
    /// Where her daemon is listening, if it is running.
    pub url: Option<String>,
    /// Her picture as a data URL, if she has been given one.
    pub avatar: Option<String>,
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

    fn run_file(&self, slug: &str, ext: &str) -> PathBuf {
        self.home.join("run").join(format!("{slug}.{ext}"))
    }

    // --- models -----------------------------------------------------------

    /// The brains available here. The first time, any daemon config written
    /// by hand is read in so nothing set up earlier is lost.
    fn models(&self) -> Vec<Model> {
        match fs::read_to_string(self.home.join(MODELS_FILE)) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => self.legacy_models(),
        }
    }

    fn legacy_models(&self) -> Vec<Model> {
        let Ok(text) = fs::read_to_string(self.home.join(LEGACY_CONFIG)) else {
            return Vec::new();
        };
        let Ok(config) = text.parse::<toml::Table>() else {
            return Vec::new();
        };
        let think = config
            .get("brain")
            .and_then(|b| b.get("think"))
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        let field = |table: &toml::Value, key: &str| {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        };
        let mut models: Vec<Model> = config
            .get("providers")
            .and_then(toml::Value::as_table)
            .into_iter()
            .flatten()
            .map(|(id, p)| Model {
                id: id.clone(),
                kind: field(p, "kind").unwrap_or_default(),
                model: field(p, "model").unwrap_or_default(),
                base_url: field(p, "base_url"),
                api_key: field(p, "api_key"),
                api_key_env: field(p, "api_key_env"),
                json_mode: p
                    .get("json_mode")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
            })
            .collect();
        // The one she was thinking with comes first, so it stays the default.
        models.sort_by_key(|m| m.id != think);
        models
    }

    fn write_models(&self, models: &[Model]) -> Result<()> {
        fs::create_dir_all(&self.home).map_err(io("make the home directory"))?;
        let text = serde_json::to_string_pretty(models).map_err(io("write the models"))?;
        write_private(&self.home.join(MODELS_FILE), &text)
    }

    /// Add a model, or replace the one with the same id. A blank key keeps
    /// the key already stored for that id.
    pub fn save_model(&self, mut model: Model) -> Result<Model> {
        model.kind = model.kind.trim().to_owned();
        model.model = model.model.trim().to_owned();
        if !matches!(
            model.kind.as_str(),
            "anthropic" | "openai" | "claude-code" | "codex"
        ) {
            return Err(format!("unknown kind of brain: {:?}", model.kind));
        }
        if model.model.is_empty() {
            if matches!(model.kind.as_str(), "anthropic" | "openai") {
                return Err("a brain needs a model name".to_owned());
            }
            model.model.clone_from(&model.kind);
        }
        let blank = |s: &Option<String>| s.as_deref().is_none_or(|s| s.trim().is_empty());
        if blank(&model.base_url) {
            model.base_url = None;
        }
        if blank(&model.api_key_env) {
            model.api_key_env = None;
        }
        let mut models = self.models();
        if model.id.trim().is_empty() {
            let base = slug(&model.model);
            model.id.clone_from(&base);
            let mut n = 2;
            while models.iter().any(|m| m.id == model.id) {
                model.id = format!("{base}-{n}");
                n += 1;
            }
        }
        let existing = models.iter().position(|m| m.id == model.id);
        if blank(&model.api_key) {
            model.api_key = existing.and_then(|i| models[i].api_key.clone());
        }
        match existing {
            Some(i) => models[i] = model.clone(),
            None => models.push(model.clone()),
        }
        self.write_models(&models)?;
        Ok(model)
    }

    pub fn delete_model(&self, id: &str) -> Result<()> {
        let mut models = self.models();
        models.retain(|m| m.id != id);
        self.write_models(&models)
    }

    // --- the book ---------------------------------------------------------

    fn book(&self) -> Book {
        fs::read_to_string(self.home.join(KEEPER_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write_book(&self, book: &Book) -> Result<()> {
        fs::create_dir_all(&self.home).map_err(io("make the home directory"))?;
        let text = serde_json::to_string_pretty(book).map_err(io("write the keeper's book"))?;
        fs::write(self.home.join(KEEPER_FILE), text).map_err(io("write the keeper's book"))
    }

    fn tempo(&self) -> f32 {
        self.book().tempo.unwrap_or_else(|| {
            // What a hand-written config asked for, if there is one.
            fs::read_to_string(self.home.join(LEGACY_CONFIG))
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok())
                .and_then(|c| c.get("mind")?.get("tempo")?.as_float())
                .map_or(1.0, |t| t as f32)
        })
    }

    pub fn set_tempo(&self, tempo: f32) -> Result<()> {
        let mut book = self.book();
        book.tempo = Some(if tempo.is_finite() && tempo >= 0.0 {
            tempo
        } else {
            1.0
        });
        self.write_book(&book)
    }

    /// The model a ghost thinks with: the one chosen for her, or the first.
    fn model_for(&self, slug: &str) -> Option<Model> {
        let models = self.models();
        let chosen = self.book().thinks_with.get(slug).cloned();
        chosen
            .and_then(|id| models.iter().find(|m| m.id == id).cloned())
            .or_else(|| models.into_iter().next())
    }

    // --- ghosts -----------------------------------------------------------

    /// Every born ghost under this home, by name.
    pub fn ghosts(&self) -> Vec<Summary> {
        let Ok(entries) = fs::read_dir(self.ghosts_dir()) else {
            return Vec::new();
        };
        let mut ghosts: Vec<Summary> = entries
            .filter_map(|entry| self.summary(&entry.ok()?.path()))
            .collect();
        ghosts.sort_by(|a, b| a.name.cmp(&b.name));
        ghosts
    }

    fn summary(&self, dir: &Path) -> Option<Summary> {
        let seal = identity::read_seal_file(dir).ok()??;
        let certificate = identity::read_certificate(dir).ok()?;
        let seed = certificate.seed();
        let slug = dir.file_name()?.to_str()?.to_owned();
        Some(Summary {
            path: dir.to_path_buf(),
            name: seed.name.clone(),
            pronouns: seed.pronouns.clone(),
            city: seed.home.city.clone(),
            country: seed.home.country.clone(),
            born: seed.born.clone(),
            seal: seal.to_string(),
            model: self.model_for(&slug).map(|m| m.id),
            url: self
                .running(&slug)
                .map(|r| format!("ws://127.0.0.1:{}", r.port)),
            avatar: avatar(dir),
            slug,
        })
    }

    /// Give birth. `certificate` is the exact text of her `ghost.toml`; its
    /// bytes are written as given, so the seal the window computed while it
    /// was being written is the seal she is born with.
    pub fn birth(&self, certificate: &str, picture: Option<&str>) -> Result<Summary> {
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
        if let Some(picture) = picture {
            // A picture is not part of who she is; failing to save it is not
            // a reason to undo a birth.
            let _ = set_avatar(&dir, picture);
        }
        self.summary(&dir)
            .ok_or_else(|| "she was born but cannot be read back".to_owned())
    }

    /// A directory for a ghost of this name that nothing else is using.
    fn free_dir(&self, name: &str) -> PathBuf {
        let slug = slug(name);
        let base = self.ghosts_dir();
        let mut dir = base.join(&slug);
        let mut n = 2;
        while dir.exists() {
            dir = base.join(format!("{slug}-{n}"));
            n += 1;
        }
        dir
    }

    // --- bodies -----------------------------------------------------------

    fn running(&self, slug: &str) -> Option<Running> {
        let text = fs::read_to_string(self.run_file(slug, "json")).ok()?;
        let running: Running = serde_json::from_str(&text).ok()?;
        alive(running.pid).then_some(running)
    }

    /// Start a ghost's body. It is its own process and outlives this one.
    pub fn wake(&self, path: &Path) -> Result<()> {
        let slug = slug_of(path)?;
        if self.running(&slug).is_some() {
            return Ok(());
        }
        let model = self
            .model_for(&slug)
            .ok_or("she has nothing to think with yet: add a model first")?;
        let ghostd = find_ghostd()
            .ok_or("cannot find the ghostd binary; build it, put it on PATH, or set GHOSTD_BIN")?;
        fs::create_dir_all(self.home.join("run")).map_err(io("make the run directory"))?;
        let port = free_port().ok_or("no free port for her to listen on")?;
        let config = self.run_file(&slug, "toml");
        write_private(&config, &daemon_config(&model, port, self.tempo())?)?;

        let log_path = self.run_file(&slug, "log");
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
            port,
        };
        let text = serde_json::to_string(&running).map_err(io("remember her process"))?;
        fs::write(self.run_file(&slug, "json"), text).map_err(io("remember her process"))
    }

    /// Stop a ghost's body. Her journal will record the time she was not running.
    pub fn rest(&self, path: &Path) -> Result<()> {
        let slug = slug_of(path)?;
        if let Some(running) = self.running(&slug) {
            let pid = running.pid.to_string();
            #[cfg(unix)]
            let stopped = Command::new("kill").args(["-INT", &pid]).status();
            #[cfg(not(unix))]
            let stopped = Command::new("taskkill").args(["/PID", &pid]).status();
            stopped.map_err(io("stop her body"))?;
            // Give her a moment to close her journal and let go of the lock.
            for _ in 0..40 {
                if !alive(running.pid) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let _ = fs::remove_file(self.run_file(&slug, "json"));
        Ok(())
    }

    /// Change what a ghost thinks with. If she is awake her body is restarted
    /// on the new brain; she is the same ghost when she comes back.
    pub fn set_model(&self, path: &Path, model: &str) -> Result<()> {
        let slug = slug_of(path)?;
        if !self.models().iter().any(|m| m.id == model) {
            return Err(format!("no model called {model:?} on this machine"));
        }
        let mut book = self.book();
        book.thinks_with.insert(slug.clone(), model.to_owned());
        self.write_book(&book)?;
        if self.running(&slug).is_some() {
            self.rest(path)?;
            self.wake(path)?;
        }
        Ok(())
    }

    /// Everything the window needs to know about this machine.
    pub fn state(&self) -> Value {
        let models: Vec<Value> = self
            .models()
            .into_iter()
            .map(|m| {
                let has_key = m.api_key.as_deref().is_some_and(|k| !k.is_empty());
                json!({"has_key": has_key, "model": Model { api_key: None, ..m }})
            })
            .collect();
        json!({
            "home": self.home,
            "ghosts": self.ghosts(),
            "models": models,
            "tempo": self.tempo(),
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
        "state" => {}
        "birth" => {
            let certificate = args["certificate"].as_str().ok_or("no certificate")?;
            return Ok(json!(keeper.birth(certificate, args["avatar"].as_str())?));
        }
        "wake" => keeper.wake(&path()?)?,
        "rest" => keeper.rest(&path()?)?,
        "set_model" => keeper.set_model(&path()?, args["model"].as_str().ok_or("which model?")?)?,
        "save_model" => {
            let model: Model =
                serde_json::from_value(args["model"].clone()).map_err(io("read the model"))?;
            keeper.save_model(model)?;
        }
        "delete_model" => keeper.delete_model(args["id"].as_str().ok_or("which model?")?)?,
        "set_tempo" => {
            #[allow(clippy::cast_possible_truncation)]
            keeper.set_tempo(args["tempo"].as_f64().ok_or("what pace?")? as f32)?;
        }
        "set_avatar" => set_avatar(&path()?, args["avatar"].as_str().ok_or("no picture")?)?,
        other => return Err(format!("the keeper does not know how to {other}")),
    }
    Ok(keeper.state())
}

fn slug(name: &str) -> String {
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "ghost".to_owned()
    } else {
        slug.to_owned()
    }
}

fn slug_of(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "that is not a ghost's directory".to_owned())
}

/// Her picture, as a data URL the window can show.
fn avatar(dir: &Path) -> Option<String> {
    for (ext, mime) in [
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("webp", "image/webp"),
    ] {
        if let Ok(bytes) = fs::read(dir.join(format!("avatar.{ext}"))) {
            return Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)));
        }
    }
    None
}

/// Give a ghost a picture. `data` is a data URL. A picture is how she is
/// shown, not who she is: it is not sealed and can be replaced.
fn set_avatar(dir: &Path, data: &str) -> Result<()> {
    identity::read_seal_file(dir)
        .ok()
        .flatten()
        .ok_or("that is not a ghost's directory")?;
    let (head, body) = data
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
        .ok_or("the picture is not a data URL")?;
    let ext = match head {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        other => return Err(format!("{other} is not a picture she can wear")),
    };
    let bytes = STANDARD.decode(body).map_err(io("read the picture"))?;
    if bytes.len() > AVATAR_LIMIT {
        return Err("the picture is too large".to_owned());
    }
    for old in ["png", "jpg", "webp"] {
        let _ = fs::remove_file(dir.join(format!("avatar.{old}")));
    }
    fs::write(dir.join(format!("avatar.{ext}")), bytes).map_err(io("save the picture"))
}

/// The daemon's configuration for one ghost: one brain, her own port.
fn daemon_config(model: &Model, port: u16, tempo: f32) -> Result<String> {
    let mut provider = toml::Table::new();
    provider.insert("kind".into(), model.kind.clone().into());
    if matches!(model.kind.as_str(), "anthropic" | "openai") || model.model != model.kind {
        provider.insert("model".into(), model.model.clone().into());
    }
    if matches!(model.kind.as_str(), "anthropic" | "openai") {
        if let Some(url) = &model.base_url {
            provider.insert("base_url".into(), url.clone().into());
        }
        // A stored key wins; otherwise the daemon reads the named variable.
        match (&model.api_key, &model.api_key_env) {
            (Some(key), _) if !key.is_empty() => {
                provider.insert("api_key".into(), key.clone().into());
            }
            (_, Some(env)) => {
                provider.insert("api_key_env".into(), env.clone().into());
            }
            _ => {}
        }
    }
    if model.kind == "openai" && model.json_mode {
        provider.insert("json_mode".into(), true.into());
    }
    let table = |pairs: &[(&str, toml::Value)]| -> toml::Table {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    };
    let mut root = toml::Table::new();
    root.insert(
        "brain".into(),
        table(&[("think", "main".into()), ("tick", "main".into())]).into(),
    );
    root.insert(
        "listen".into(),
        table(&[("addr", format!("127.0.0.1:{port}").into())]).into(),
    );
    root.insert(
        "mind".into(),
        table(&[("tempo", f64::from(tempo).into())]).into(),
    );
    root.insert(
        "providers".into(),
        table(&[("main", provider.into())]).into(),
    );
    toml::to_string(&root).map_err(io("write the daemon config"))
}

/// The first port from the default upward that nothing is listening on.
fn free_port() -> Option<u16> {
    (FIRST_PORT..FIRST_PORT + 200).find(|port| TcpListener::bind(("127.0.0.1", *port)).is_ok())
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

    /// A one-pixel PNG.
    const PICTURE: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    fn keeper() -> (tempfile::TempDir, Keeper) {
        let dir = tempfile::tempdir().unwrap();
        let keeper = Keeper::at(dir.path().to_path_buf());
        (dir, keeper)
    }

    fn deepseek(key: Option<&str>) -> Model {
        Model {
            kind: "openai".into(),
            model: "deepseek-chat".into(),
            base_url: Some("https://api.deepseek.com/v1".into()),
            api_key: key.map(Into::into),
            json_mode: true,
            ..Model::default()
        }
    }

    #[test]
    fn birth_seals_exactly_what_was_written() {
        let (_dir, keeper) = keeper();
        assert!(keeper.ghosts().is_empty());
        let born = keeper.birth(CERTIFICATE, Some(PICTURE)).unwrap();
        assert_eq!(
            (born.name.as_str(), born.slug.as_str()),
            ("Mei Lin", "mei-lin")
        );
        // The seal is the hash of the bytes the window sent, nothing else.
        assert_eq!(
            born.seal,
            identity::Seal::of(CERTIFICATE.as_bytes()).to_string()
        );
        assert_eq!(
            fs::read_to_string(born.path.join("ghost.toml")).unwrap(),
            CERTIFICATE
        );
        assert_eq!(born.avatar.as_deref(), Some(PICTURE));
        // A second ghost of the same name gets her own place.
        let twin = keeper
            .birth(&CERTIFICATE.replace("quiet", "loud"), None)
            .unwrap();
        assert_eq!(twin.slug, "mei-lin-2");
        assert_ne!(twin.seal, born.seal);
        assert!(twin.avatar.is_none());
        assert_eq!(keeper.ghosts().len(), 2);
    }

    #[test]
    fn a_bad_certificate_leaves_nothing_behind() {
        let (_dir, keeper) = keeper();
        let bad = CERTIFICATE.replace("Asia/Shanghai", "Mars/Olympus");
        assert!(keeper.birth(&bad, None).is_err());
        assert!(keeper.ghosts().is_empty());
        assert!(!keeper.ghosts_dir().join("mei-lin").exists());
    }

    #[test]
    fn a_picture_can_be_replaced_but_only_with_a_picture() {
        let (_dir, keeper) = keeper();
        let born = keeper.birth(CERTIFICATE, None).unwrap();
        set_avatar(&born.path, PICTURE).unwrap();
        assert!(born.path.join("avatar.png").exists());
        set_avatar(&born.path, &PICTURE.replace("image/png", "image/webp")).unwrap();
        assert!(!born.path.join("avatar.png").exists());
        assert!(set_avatar(&born.path, "data:text/html;base64,PGI+").is_err());
        assert!(set_avatar(&born.path, "not a data url").is_err());
        // Nothing about her changed: she still opens.
        assert!(Ghost::open(&born.path).is_ok());
    }

    #[test]
    fn models_are_private_and_keys_never_come_back() {
        let (_dir, keeper) = keeper();
        let saved = keeper.save_model(deepseek(Some("not-a-real-key"))).unwrap();
        assert_eq!(saved.id, "deepseek-chat");
        let state = keeper.state();
        assert_eq!(state["models"][0]["has_key"], true);
        assert!(state["models"][0]["model"]["api_key"].is_null());
        assert!(!state.to_string().contains("not-a-real-key"));
        // Saving the same id again without a key keeps the stored one.
        keeper
            .save_model(Model {
                id: saved.id.clone(),
                ..deepseek(None)
            })
            .unwrap();
        assert_eq!(
            keeper.models()[0].api_key.as_deref(),
            Some("not-a-real-key")
        );
        // Another model of the same name gets its own id.
        assert_eq!(
            keeper.save_model(deepseek(None)).unwrap().id,
            "deepseek-chat-2"
        );
        assert!(
            keeper
                .save_model(Model {
                    kind: "abacus".into(),
                    ..Model::default()
                })
                .is_err()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(keeper.home.join(MODELS_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        keeper.delete_model("deepseek-chat-2").unwrap();
        assert_eq!(keeper.models().len(), 1);
    }

    #[test]
    fn each_ghost_thinks_with_her_own_model() {
        let (_dir, keeper) = keeper();
        let a = keeper.birth(CERTIFICATE, None).unwrap();
        assert!(a.model.is_none());
        keeper.save_model(deepseek(None)).unwrap();
        let claude = keeper
            .save_model(Model {
                kind: "anthropic".into(),
                model: "claude-sonnet-5".into(),
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
                ..Model::default()
            })
            .unwrap();
        // With no choice made she thinks with the first one.
        assert_eq!(keeper.ghosts()[0].model.as_deref(), Some("deepseek-chat"));
        keeper.set_model(&a.path, &claude.id).unwrap();
        assert_eq!(keeper.ghosts()[0].model.as_deref(), Some("claude-sonnet-5"));
        assert!(keeper.set_model(&a.path, "nothing").is_err());

        let config = daemon_config(&keeper.model_for("mei-lin").unwrap(), 7450, 0.5).unwrap();
        let parsed: toml::Table = toml::from_str(&config).unwrap();
        assert_eq!(parsed["listen"]["addr"].as_str(), Some("127.0.0.1:7450"));
        assert_eq!(
            parsed["providers"]["main"]["api_key_env"].as_str(),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(parsed["mind"]["tempo"].as_float(), Some(0.5));
    }

    #[test]
    fn a_config_written_by_hand_is_read_in() {
        let (_dir, keeper) = keeper();
        fs::write(
            keeper.home.join(LEGACY_CONFIG),
            r#"
[brain]
think = "deepseek"
tick = "deepseek"

[mind]
tempo = 0.15

[providers.claude-code]
kind = "claude-code"
model = "sonnet"

[providers.deepseek]
kind = "openai"
model = "deepseek-chat"
base_url = "https://api.deepseek.com/v1"
api_key_env = "DEEPSEEK_API_KEY"
json_mode = true
"#,
        )
        .unwrap();
        let models = keeper.models();
        assert_eq!(models.len(), 2);
        // The one she was thinking with stays the default.
        assert_eq!(models[0].id, "deepseek");
        assert_eq!(models[0].api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
        assert!((keeper.tempo() - 0.15).abs() < 1e-6);
    }

    #[test]
    fn nobody_wakes_without_a_model() {
        let (_dir, keeper) = keeper();
        let born = keeper.birth(CERTIFICATE, None).unwrap();
        let refused = keeper.wake(&born.path).unwrap_err();
        assert!(refused.contains("add a model first"), "{refused}");
        assert!(keeper.rest(&born.path).is_ok());
    }
}
