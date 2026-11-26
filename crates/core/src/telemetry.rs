//! What Ghost tells the people who make it, as far as you let it
//! (`settings.telemetry`), as KDE's `KUserFeedback` does: four levels, each
//! sending what the one below it sends and more. README.md lists what
//! each sends. None ever sends what is said to or by an agent, a path, the
//! name of a project, repository, branch or session, a host, an address, a
//! token, or the words of an error: only counts, and names Ghost or the
//! ACP registry gives (an agent's id, a tool's, a page's, a model's).
//!
//! The server counts as it goes ([`Counts`]), keeps the day's counts in
//! its home (`telemetry.json`) so that a restart does not lose them, and
//! sends one batch a day to [`ENDPOINT`]; a batch that does not go is
//! dropped, and the next day's goes as usual. The first launch says once,
//! whatever the level and before you are asked, which build runs on what
//! (level 0's batch, with no id); never again, not after an update. From
//! level 1 on, nothing goes until you chose the level (the first-run
//! sheet), and each batch carries a random id of this install, which you
//! can have made anew.

use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use rmcp::schemars::{self, JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

use crate::{
    proto::{Call, Event},
    store::{self, Job},
};

/// Where the batches go: the Worker in `telemetry/`.
pub const ENDPOINT: &str = "https://ghost-telemetry.aac6fef-aed.workers.dev";

/// How often the server looks whether a day's batch is due.
const LOOK: Duration = Duration::from_hours(1);

/// How much Ghost tells, each level all the one below it does and more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Level {
    Off,
    Basic,
    Usage,
    Detailed,
}

impl Level {
    pub const ALL: [Self; 4] = [Self::Off, Self::Basic, Self::Usage, Self::Detailed];

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Basic => "Basic",
            Self::Usage => "Usage",
            Self::Detailed => "Detailed",
        }
    }

    /// What it sends, in a sentence.
    pub fn about(self) -> &'static str {
        match self {
            Self::Off => {
                "Nothing, but once, at the first launch: the version, the system and the processor."
            }
            Self::Basic => {
                "Each day: an id made for this install, the agents in use and their versions, the Macs paired, whether others may control this one."
            }
            Self::Usage => {
                "Also how much each part is used: sessions and turns, worktrees, tasks and workflows run, Ghost's tools called, pages opened, crashes."
            }
            Self::Detailed => {
                "Also how Ghost runs: time to start, memory, the kinds of errors, models chosen, the window's size and zoom, the look and the language."
            }
        }
    }
}

impl TryFrom<u8> for Level {
    type Error = String;

    fn try_from(n: u8) -> Result<Self, String> {
        Self::ALL
            .get(usize::from(n))
            .copied()
            .ok_or_else(|| format!("telemetry is a level from 0 to 3, not {n}"))
    }
}

impl From<Level> for u8 {
    fn from(level: Level) -> u8 {
        level as u8
    }
}

/// What was counted since the last batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    /// Sessions opened, by agent.
    pub sessions: BTreeMap<String, u64>,
    pub turns: u64,
    pub worktrees: u64,
    /// Scheduled tasks' turns, and workflows' runs.
    pub tasks: u64,
    pub runs: u64,
    /// Ghost's tools called, by tool.
    pub tools: BTreeMap<String, u64>,
    /// The window's panels and pages opened, by name.
    pub opened: BTreeMap<String, u64>,
    pub crashes: u64,
    /// What went wrong, by kind.
    pub errors: BTreeMap<String, u64>,
    /// Models chosen, by id.
    pub models: BTreeMap<String, u64>,
    /// The window, as it last said.
    pub window: Option<Window>,
}

/// The window, as the desk says it. A server with no window says its look and language empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// Its size in points.
    #[schemars(range(max = MOST))]
    pub width: u32,
    #[schemars(range(max = MOST))]
    pub height: u32,
    /// Its zoom, in percent.
    #[schemars(range(max = MOST))]
    pub zoom: u32,
    /// `light`, `dark` or `system`.
    #[schemars(extend("enum" = ["light", "dark", "system", ""]))]
    pub look: String,
    /// The system's language, as `en` or `pt-BR`.
    #[schemars(regex(pattern = r"^([a-z]{2,3}(-[A-Za-z0-9]{2,8})?)?$"))]
    pub language: String,
    /// From launch to the window's first frame.
    #[schemars(range(max = MOST))]
    pub startup_ms: u64,
}

/// What the desk tells or asks of telemetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Ask {
    /// A panel or page of the window opened, by its name.
    Opened(String),
    /// The window as it is.
    Window(Window),
    /// The batch that goes next at this level, as JSON: answered as
    /// [`Event::Telemetry`].
    Show(Level),
    /// A new id for this install, from the next batch on.
    Forget,
}

/// What `telemetry.json` keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    /// The first launch's batch went.
    pinged: bool,
    /// This install's id, made with the first batch from level 1 on.
    id: Option<String>,
    /// The day (since 1970) the counts are of.
    day: u64,
    counts: Counts,
}

/// What is read of the machine as a batch is made.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Facts {
    build: Build,
    /// The agents switched on, by registry id, with the version here.
    agents: BTreeMap<String, String>,
    servers: usize,
    listening: bool,
    memory_mb: u64,
}

impl Facts {
    fn here(home: &Path) -> Self {
        let settings = crate::settings::Settings::load(home).0;
        let registry = crate::registry::kept(home).unwrap_or_default();
        let agents = (registry.agents.iter())
            .filter(|e| settings.enabled.contains(&e.id))
            .map(|e| (e.id.clone(), e.installed(home).unwrap_or_default()))
            .filter(|(id, version)| named(id) && named(version))
            .collect();
        let devices: crate::access::Kept =
            crate::whole::read_json(&home.join("devices.json")).unwrap_or_default();
        // This process, and what it runs.
        let me = std::process::id();
        let children: u64 = (crate::diagnostics::processes(me).procs.iter())
            .map(|p| p.kb)
            .sum();
        let own = sysinfo::System::new().ok().map_or(0, |mut system| {
            let pid = sysinfo::Pid::from_u32(me);
            system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
            system.process(pid).map_or(0, |p| p.memory() / 1024)
        });
        let major = sysinfo::System::os_version().unwrap_or_default();
        Self {
            build: Build {
                version: crate::update::VERSION.to_owned(),
                commit: crate::update::short(crate::update::COMMIT).to_owned(),
                channel: crate::update::BUILT_AS.to_owned(),
                os: if cfg!(target_os = "macos") {
                    format!("macos {}", major.split('.').next().unwrap_or_default())
                } else {
                    "linux".to_owned()
                },
                arch: std::env::consts::ARCH.to_owned(),
            },
            agents,
            servers: crate::servers::Servers::load(home).map_or(0, |s| s.servers.len()),
            listening: devices.serve,
            memory_mb: (own + children) / 1024,
        }
    }
}

/// A name a batch may carry: a short token, never a path or words.
const NAME: &str = r"^[A-Za-z0-9][A-Za-z0-9._:@/+-]{0,63}$";

/// Whether `s` is a [`NAME`].
fn named(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || "._:@/+-".contains(c))
}

/// Add one to `name` in `map`, if it is a name.
pub(crate) fn tally(map: &mut BTreeMap<String, u64>, name: &str) {
    if named(name) {
        *map.entry(name.to_owned()).or_default() += 1;
    }
}

/// The most a count may be, and the most names a map may hold.
const MOST: u64 = 10_000_000;
const ENTRIES: usize = 64;

/// Names to counts, as a batch's maps are.
fn counts_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "maxProperties": ENTRIES,
        "propertyNames": { "pattern": NAME },
        "additionalProperties": { "type": "integer", "minimum": 0, "maximum": MOST },
    })
}

/// Names to names: the agents, with their versions.
fn names_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "maxProperties": ENTRIES,
        "propertyNames": { "pattern": NAME },
        "additionalProperties": { "type": "string", "pattern": NAME },
    })
}

/// A batch's `level`, which is `N` and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Exactly<const N: u8>;

impl<const N: u8> Serialize for Exactly<N> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(N)
    }
}

impl<const N: u8> JsonSchema for Exactly<N> {
    fn schema_name() -> Cow<'static, str> {
        format!("Level{N}").into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "const": N })
    }
}

/// What every batch says: the build, and what it runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
struct Build {
    #[schemars(regex(pattern = r"^\d{1,4}\.\d{1,4}\.\d{1,4}[0-9A-Za-z.+-]{0,24}$"))]
    version: String,
    #[schemars(regex(pattern = r"^[0-9a-f]{7,40}$"))]
    commit: String,
    #[schemars(extend("enum" = ["stable", "nightly", "dev"]))]
    channel: String,
    /// `macos 26`, or `linux`.
    #[schemars(regex(pattern = r"^(macos \d{1,3}|linux)$"))]
    os: String,
    #[schemars(extend("enum" = ["aarch64", "x86_64"]))]
    arch: String,
}

/// What Basic adds: this install, and what it runs with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
struct Install {
    /// Random, made for this install.
    #[schemars(regex(pattern = r"^[A-Za-z0-9_-]{22}$"))]
    id: String,
    #[schemars(schema_with = "names_schema")]
    agents: BTreeMap<String, String>,
    #[schemars(range(max = MOST))]
    servers: usize,
    listening: bool,
}

/// What Usage adds: how much each part is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
struct Use {
    #[schemars(schema_with = "counts_schema")]
    sessions: BTreeMap<String, u64>,
    #[schemars(range(max = MOST))]
    turns: u64,
    #[schemars(range(max = MOST))]
    worktrees: u64,
    #[schemars(range(max = MOST))]
    tasks: u64,
    #[schemars(range(max = MOST))]
    runs: u64,
    #[schemars(schema_with = "counts_schema")]
    tools: BTreeMap<String, u64>,
    #[schemars(schema_with = "counts_schema")]
    opened: BTreeMap<String, u64>,
    #[schemars(range(max = MOST))]
    crashes: u64,
}

/// What Detailed adds: how Ghost runs, and the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
struct Detail {
    #[schemars(range(max = MOST))]
    memory_mb: u64,
    #[schemars(schema_with = "counts_schema")]
    errors: BTreeMap<String, u64>,
    #[schemars(schema_with = "counts_schema")]
    models: BTreeMap<String, u64>,
    #[serde(flatten)]
    window: Window,
}

/// What goes at Off: the first launch's, once ever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Off {
    level: Exactly<0>,
    #[serde(flatten)]
    build: Build,
}

/// What goes each day at Basic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Basic {
    level: Exactly<1>,
    #[serde(flatten)]
    build: Build,
    #[serde(flatten)]
    install: Install,
}

/// What goes each day at Usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Usage {
    level: Exactly<2>,
    #[serde(flatten)]
    build: Build,
    #[serde(flatten)]
    install: Install,
    #[serde(flatten)]
    used: Use,
}

/// What goes each day at Detailed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Detailed {
    level: Exactly<3>,
    #[serde(flatten)]
    build: Build,
    #[serde(flatten)]
    install: Install,
    #[serde(flatten)]
    used: Use,
    #[serde(flatten)]
    detail: Detail,
}

/// A batch of Ghost's telemetry: one of a level, every field of it and
/// nothing more. The Worker checks each against its schema,
/// `telemetry/batch.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
enum Batch {
    Off(Off),
    Basic(Basic),
    Usage(Usage),
    Detailed(Detailed),
}

/// The batch of `level`, of `counts`, from the install `id`.
fn batch(level: Level, id: &str, counts: &Counts, facts: &Facts) -> Batch {
    let build = facts.build.clone();
    let install = || Install {
        id: id.to_owned(),
        agents: facts.agents.clone(),
        servers: facts.servers,
        listening: facts.listening,
    };
    let usage = || Use {
        sessions: counts.sessions.clone(),
        turns: counts.turns,
        worktrees: counts.worktrees,
        tasks: counts.tasks,
        runs: counts.runs,
        tools: counts.tools.clone(),
        opened: counts.opened.clone(),
        crashes: counts.crashes,
    };
    match level {
        Level::Off => Batch::Off(Off {
            level: Exactly,
            build,
        }),
        Level::Basic => Batch::Basic(Basic {
            level: Exactly,
            build,
            install: install(),
        }),
        Level::Usage => Batch::Usage(Usage {
            level: Exactly,
            build,
            install: install(),
            used: usage(),
        }),
        Level::Detailed => Batch::Detailed(Detailed {
            level: Exactly,
            build,
            install: install(),
            used: usage(),
            detail: Detail {
                memory_mb: facts.memory_mb,
                errors: counts.errors.clone(),
                models: counts.models.clone(),
                window: counts.window.clone().unwrap_or_default(),
            },
        }),
    }
}

/// The batches due `today` at `level` (none: not chosen yet), with
/// `kept` moved on past them: the first launch's, once ever; and the
/// day's, once a day, from level 1 on. The counts begin again each day,
/// sent or not.
fn due(kept: &mut Kept, level: Option<Level>, today: u64, facts: &Facts) -> Vec<Batch> {
    let mut out = Vec::new();
    if !kept.pinged {
        kept.pinged = true;
        out.push(batch(Level::Off, "", &Counts::default(), facts));
    }
    if level == Some(Level::Off) {
        kept.id = None;
    }
    if kept.day == today {
        return out;
    }
    if kept.day != 0
        && let Some(level) = level.filter(|l| *l > Level::Off)
    {
        let id = kept
            .id
            .get_or_insert_with(crate::access::random::<16>)
            .clone();
        out.push(batch(level, &id, &kept.counts, facts));
    }
    // The window is as it was until it says otherwise.
    let window = kept.counts.window.take();
    kept.counts = Counts {
        window,
        ..Counts::default()
    };
    kept.day = today;
    out
}

/// Telemetry, as the server keeps it.
pub struct Telemetry {
    home: PathBuf,
    /// What `telemetry.json` holds; none where it does not read, when
    /// nothing is counted, sent or saved over it.
    kept: Mutex<Option<Kept>>,
    /// Where batches go.
    endpoint: Option<String>,
}

fn path(home: &Path) -> PathBuf {
    home.join("telemetry.json")
}

fn today() -> u64 {
    store::secs(std::time::SystemTime::now()) / 86_400
}

impl Telemetry {
    /// Telemetry for `home`, sending to `endpoint`; none, nowhere (the
    /// tests').
    pub fn new(home: &Path, endpoint: Option<&str>) -> Arc<Self> {
        let kept = crate::whole::read_json(&path(home))
            .inspect_err(|why| tracing::warn!("{why}; nothing is counted or sent"))
            .ok();
        Arc::new(Self {
            home: home.to_path_buf(),
            kept: Mutex::new(kept),
            endpoint: endpoint.map(str::to_owned),
        })
    }

    /// Change what is kept by `change`, and write it.
    fn change<T>(&self, change: impl FnOnce(&mut Kept) -> T) -> Option<T> {
        let mut kept = self.kept.lock().expect("telemetry");
        let kept = kept.as_mut()?;
        let out = change(kept);
        let bytes = serde_json::to_vec(kept).expect("the counts serialize");
        store::queue(Job::Write(path(&self.home), bytes));
        Some(out)
    }

    /// Count by `count`.
    pub fn count(&self, count: impl FnOnce(&mut Counts)) {
        self.change(|kept| count(&mut kept.counts));
    }

    /// Count what `call` does.
    pub fn call(&self, call: &Call) {
        match call {
            Call::Open { agent, .. } => self.count(|c| tally(&mut c.sessions, agent)),
            Call::Prompt { .. } => self.count(|c| c.turns += 1),
            Call::MakeWorktree { .. } => self.count(|c| c.worktrees += 1),
            Call::StartFlow { .. } => self.count(|c| c.runs += 1),
            _ => {}
        }
    }

    /// Count what `event` says happened.
    pub fn event(&self, event: &Event) {
        let kind = match event {
            Event::Started { result: Err(_), .. } => "agent_start",
            Event::Installed { result: Err(_), .. } => "install",
            Event::Refused { .. } => "refused",
            Event::TaskTurn { ended: None, .. } => return self.count(|c| c.tasks += 1),
            Event::TaskTurn {
                ended: Some(Err(_)),
                ..
            } => "task",
            _ => return,
        };
        self.count(|c| tally(&mut c.errors, kind));
    }

    /// Do what the desk asks; what it is answered goes to `say`.
    pub fn ask(&self, ask: Ask, say: &(dyn Fn(Event) + Send + Sync)) {
        match ask {
            Ask::Opened(name) => self.count(|c| tally(&mut c.opened, &name)),
            Ask::Window(window) => self.count(|c| c.window = Some(window)),
            Ask::Forget => {
                self.change(|kept| kept.id = None);
            }
            Ask::Show(level) => {
                let facts = Facts::here(&self.home);
                // The id is made now, from Basic on, so that what is shown
                // is what goes.
                let shown = self.change(|kept| {
                    let id = match level {
                        Level::Off => String::new(),
                        _ => (kept.id.get_or_insert_with(crate::access::random::<16>)).clone(),
                    };
                    batch(level, &id, &kept.counts, &facts)
                });
                say(Event::Telemetry(shown.map_or_else(
                    || "telemetry.json does not read, so nothing is sent.".to_owned(),
                    |b| serde_json::to_string_pretty(&b).expect("a batch serializes"),
                )));
            }
        }
    }

    /// Send what is due, now and every hour from now on, off the window's
    /// thread; and count a panic as a crash, written at once. Nothing,
    /// with nowhere to send to.
    pub fn start(self: &Arc<Self>) {
        let Some(endpoint) = self.endpoint.clone() else {
            return;
        };
        let this = self.clone();
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            // Written here and now, not by the store's thread, which may be
            // the one panicking; and not at all if the counts are held.
            if let Ok(mut kept) = this.kept.try_lock()
                && let Some(kept) = kept.as_mut()
            {
                kept.counts.crashes += 1;
                let bytes = serde_json::to_vec(kept).expect("the counts serialize");
                let _ = crate::whole::write(&path(&this.home), &bytes);
            }
            before(panic);
        }));
        let this = self.clone();
        std::thread::spawn(move || {
            loop {
                this.send_due(&endpoint);
                std::thread::sleep(LOOK);
            }
        });
    }

    /// Send the batches due to `endpoint`, as the settings' level says.
    fn send_due(&self, endpoint: &str) {
        let level = crate::settings::Settings::load(&self.home).0.telemetry;
        let facts = Facts::here(&self.home);
        let batches = self
            .change(|kept| due(kept, level, today(), &facts))
            .unwrap_or_default();
        for batch in batches {
            let body = serde_json::to_vec(&batch).expect("a batch serializes");
            let sent = crate::http::call(
                "POST",
                endpoint,
                &[("Content-Type", "application/json")],
                Some(body),
            );
            if !matches!(sent, Ok((true, _))) {
                tracing::debug!("a telemetry batch did not go; it is dropped");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    /// `batch` as the JSON it goes as.
    fn sent(level: Level, id: &str, counts: &Counts) -> Value {
        serde_json::to_value(batch(level, id, counts, &facts())).expect("a batch serializes")
    }

    fn facts() -> Facts {
        Facts {
            build: Build {
                version: "0.2.0".into(),
                commit: "abc1234".into(),
                channel: "nightly".into(),
                os: "macos 26".into(),
                arch: "aarch64".into(),
            },
            agents: [("claude-code".to_owned(), "1.2.3".to_owned())].into(),
            servers: 1,
            listening: true,
            memory_mb: 310,
        }
    }

    /// Counts as a day of use with names that must not leak would leave
    /// them: a folder, a prompt, a session's words, an error's text.
    fn used() -> Counts {
        let home = tempfile::tempdir().expect("a home");
        let t = Telemetry {
            home: home.path().to_path_buf(),
            kept: Mutex::new(Some(Kept::default())),
            endpoint: None,
        };
        let calls: Vec<Call> = serde_json::from_value(json!([
            {"Open": {"agent": "claude-code", "key": 1, "folder": "/Users/zorblax/secret-project", "also": []}},
            {"Prompt": {"agent": "claude-code", "key": 1, "session": "s-private",
                        "content": [{"type": "text", "text": "the secret plan"}]}},
            {"MakeWorktree": {"dir": "/Users/zorblax/secret-project", "about": "secret branch",
                              "under": "/Users/zorblax/.ghost/worktrees", "prefix": "zorblax/",
                              "from": null, "submodules": "skip", "key": 1}},
        ]))
        .expect("the calls read");
        for call in &calls {
            t.call(call);
        }
        t.event(&Event::Refused {
            key: 1,
            why: "secret error text".into(),
        });
        let say = |_| {};
        for ask in [
            Ask::Opened("settings/telemetry".into()),
            Ask::Opened("/Users/zorblax/secret-project".into()),
            Ask::Window(Window {
                width: 1440,
                height: 900,
                zoom: 100,
                look: "dark".into(),
                language: "en".into(),
                startup_ms: 812,
            }),
        ] {
            t.ask(ask, &say);
        }
        t.count(|c| tally(&mut c.tools, "list_sessions"));
        t.count(|c| tally(&mut c.models, "claude-opus-4"));
        // Its counts written before the home goes.
        store::settle();
        t.kept.lock().unwrap().take().unwrap().counts
    }

    #[test]
    fn each_level_sends_what_the_one_below_does_and_its_own() {
        let counts = used();
        let id = "AAAAAAAAAAAAAAAAAAAAAA";
        let off = json!({"level": 0, "version": "0.2.0", "commit": "abc1234",
                         "channel": "nightly", "os": "macos 26", "arch": "aarch64"});
        assert_eq!(sent(Level::Off, id, &counts), off);
        let mut basic = off.clone();
        basic.as_object_mut().unwrap().extend(
            json!({"level": 1, "id": id, "agents": {"claude-code": "1.2.3"},
                   "servers": 1, "listening": true})
            .as_object()
            .unwrap()
            .clone(),
        );
        assert_eq!(sent(Level::Basic, id, &counts), basic);
        let mut usage = basic.clone();
        usage.as_object_mut().unwrap().extend(
            json!({"level": 2, "sessions": {"claude-code": 1}, "turns": 1, "worktrees": 1,
                   "tasks": 0, "runs": 0, "tools": {"list_sessions": 1},
                   "opened": {"settings/telemetry": 1}, "crashes": 0})
            .as_object()
            .unwrap()
            .clone(),
        );
        assert_eq!(sent(Level::Usage, id, &counts), usage);
        let mut detailed = usage.clone();
        detailed.as_object_mut().unwrap().extend(
            json!({"level": 3, "startup_ms": 812, "memory_mb": 310, "errors": {"refused": 1},
                   "models": {"claude-opus-4": 1}, "width": 1440, "height": 900, "zoom": 100,
                   "look": "dark", "language": "en"})
            .as_object()
            .unwrap()
            .clone(),
        );
        assert_eq!(sent(Level::Detailed, id, &counts), detailed);
    }

    #[test]
    fn no_path_prompt_name_or_error_text_reaches_a_batch() {
        let sent = sent(Level::Detailed, "id", &used()).to_string();
        for never in ["/Users", "zorblax", "secret", "s-private", ".ghost"] {
            assert!(!sent.contains(never), "{never} leaked into {sent}");
        }
    }

    #[test]
    fn the_first_launch_says_its_build_once_ever_and_nothing_more_unasked() {
        let mut kept = Kept::default();
        let first = due(&mut kept, None, 100, &facts());
        assert_eq!(first.len(), 1);
        assert_eq!(serde_json::to_value(&first[0]).unwrap()["level"], 0);
        assert!(matches!(first[0], Batch::Off(_)));
        // Not again: not the next day unasked, not at Off, not after an
        // update (the same file).
        assert!(due(&mut kept, None, 101, &facts()).is_empty());
        assert!(due(&mut kept, Some(Level::Off), 102, &facts()).is_empty());
        assert!(kept.pinged);
    }

    #[test]
    fn a_level_chosen_sends_one_batch_a_day_with_the_days_counts() {
        let mut kept = Kept {
            pinged: true,
            day: 100,
            ..Kept::default()
        };
        kept.counts.turns = 5;
        assert!(due(&mut kept, Some(Level::Usage), 100, &facts()).is_empty());
        let sent = due(&mut kept, Some(Level::Usage), 101, &facts());
        assert_eq!(sent.len(), 1);
        assert_eq!(serde_json::to_value(&sent[0]).unwrap()["turns"], 5);
        assert_eq!(kept.counts.turns, 0);
        assert!(due(&mut kept, Some(Level::Usage), 101, &facts()).is_empty());
        // The id is kept from day to day, and dropped at Off.
        let id = kept.id.clone().expect("an id");
        let next = due(&mut kept, Some(Level::Basic), 102, &facts());
        assert_eq!(serde_json::to_value(&next[0]).unwrap()["id"], id.as_str());
        due(&mut kept, Some(Level::Off), 103, &facts());
        assert_eq!(kept.id, None);
    }

    #[test]
    fn a_level_above_three_is_refused() {
        assert!(serde_json::from_str::<Level>("4").is_err());
        assert_eq!(serde_json::from_str::<Level>("2").unwrap(), Level::Usage);
    }

    #[test]
    fn the_workers_schema_is_the_batchs() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../telemetry/batch.schema.json");
        let made = serde_json::to_string_pretty(&schemars::schema_for!(Batch)).unwrap() + "\n";
        if std::fs::read_to_string(&path).ok().as_deref() != Some(made.as_str()) {
            std::fs::write(&path, made).expect("the schema is written");
            panic!(
                "{} was not the batch's schema; it is now: deploy the Worker with it",
                path.display()
            );
        }
    }
}
