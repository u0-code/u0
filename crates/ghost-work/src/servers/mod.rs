//! Servers of tools: the ones she installs from the MCP registry herself,
//! and the ones whoever runs her body configures.
//!
//! What she can do should grow with what she meets, the way a person
//! picks up a new app when a job calls for one, and without anyone
//! approving it. So the tools she does not have yet are not written here
//! one by one: a browser, the tools of a social site, a maker of video
//! come from the registry, where others publish servers of tools, and she
//! finds and installs them herself (`find_tools`, `install`, `uninstall`).
//!
//! **Hers, on the record.** What she installs is in her journal, as an
//! `Installed` entry with exactly what was run, and a body started later
//! runs exactly that and nothing newer. This module does not write her
//! journal: it queues what happened, and the mind, which is the one
//! writer, takes it with [`Servers::take_changes`] and records it.
//!
//! **Loaded when wanted.** One server's tool descriptions cost thousands
//! of tokens at every step. So her toolbox shows each server as one line,
//! and `load_tools` makes a server's tools callable, as
//! `<server>__<tool>`, for the rest of a look at a conversation or of a
//! sitting of work. A server is started the first time it is loaded, and
//! keeps running for whoever loads it next.
//!
//! **Safe by shape, not by asking.** Nothing here asks anyone. What keeps
//! a server in its place is in [`guard`] and in [`registry`]: it is the
//! current listing of a name whose repository is its owner's; its
//! version is pinned and its install scripts are not run; it starts with
//! an environment that holds nothing of hers; a remote is on the public
//! internet; its tools are bounded in number and size and none of them
//! runs arbitrary code; and if it starts describing its tools differently
//! from the day she installed it, it is put aside until she installs it
//! again. A server that keeps falling over is uninstalled, with why.
//! Everything a server returns is outside content, and reaches her marked
//! as such (see [`crate::outside`]).

mod client;
pub mod guard;
pub mod registry;

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use ghost_core::{
    journal::{Event, Launch},
    memory,
};
use serde::Deserialize;
use serde_json::{Map, Value};

use self::{
    client::Connection,
    guard::{Listed, Vetted},
    registry::{Plan, Registry},
};
use crate::{
    prompts::{Store, TEXTS},
    tool::Outcome,
};

/// Starts that fail in a row before a server is uninstalled.
const STARTS: u32 = 3;
/// How many listings `find_tools` shows.
const SHOWN: usize = 10;

/// `[tools]` in `ghostd.toml`: how she finds and runs servers of tools.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Let her find and install servers from the registry. Servers given
    /// under `[[mcp]]` are hers either way.
    pub enabled: bool,
    /// The registry, without its `/v0.1`.
    pub registry: String,
    /// Most servers she may have installed at once.
    pub max_servers: usize,
    /// Most tools of one server she is offered.
    pub max_tools: usize,
    /// Seconds a server has to start and say what it offers.
    pub start_seconds: u64,
    /// Seconds it has the first time, when its package is fetched.
    pub install_seconds: u64,
    /// Seconds one call to one of its tools may take before the server is
    /// killed, with everything it started.
    pub call_seconds: u64,
    /// A tool whose name has any of these in it, in any case, is never
    /// offered: they name tools that run whatever code they are handed,
    /// on this machine.
    pub deny: Vec<String>,
    /// Variables of this body's environment a server may be given, by the
    /// server's registry name. Only those it declares are given, and only
    /// to it: `"io.github.acme/slack" = ["SLACK_TOKEN"]`.
    pub env: BTreeMap<String, Vec<String>>,
    /// The `PATH` servers are started with. Left out, this process's own.
    pub path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            registry: "https://registry.modelcontextprotocol.io".to_owned(),
            max_servers: 12,
            max_tools: 40,
            start_seconds: 30,
            install_seconds: 300,
            call_seconds: 120,
            deny: [
                "run_code",
                "exec",
                "eval",
                "shell",
                "terminal",
                "run_command",
                "spawn",
                "unsafe",
            ]
            .map(str::to_owned)
            .to_vec(),
            env: BTreeMap::new(),
            path: None,
        }
    }
}

/// One `[[mcp]]` in `ghostd.toml`: a server whoever runs her body gives
/// her. It is trusted as they are, and is not hers to uninstall.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configured {
    /// What she calls it.
    pub name: String,
    /// What it is for, in a line.
    #[serde(default)]
    pub description: String,
    /// The program, for a server on this machine.
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// The address, for a server elsewhere.
    pub url: Option<String>,
    /// Variables of this body's environment it is given, by name.
    #[serde(default)]
    pub env: Vec<String>,
}

/// One server she has, running or not.
struct Slot {
    /// Its registry name, or its configured one.
    name: String,
    /// What she calls it among her tools.
    handle: String,
    description: String,
    launch: Launch,
    /// The hash of its tools when she installed it; none for a server
    /// that was configured.
    hash: Option<String>,
    /// The tools it has been seen to offer, after vetting.
    offered: Mutex<Vec<Vetted>>,
    /// Their names when it was installed, for the line she sees before
    /// it has been started.
    names: Vec<String>,
    live: tokio::sync::Mutex<Live>,
}

#[derive(Default)]
struct Live {
    connection: Option<Arc<Connection>>,
    /// Starts that failed in a row.
    failures: u32,
    retry_at: Option<Instant>,
}

impl Slot {
    fn installed(&self) -> bool {
        self.hash.is_some()
    }
}

/// A server as her toolbox lists it.
#[derive(Debug, Clone)]
pub struct Summary {
    pub handle: String,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
}

/// Every server she has, and the registry she finds more in.
pub struct Servers {
    cfg: Config,
    /// Where each server's files are, a folder each.
    dir: PathBuf,
    registry: Registry,
    slots: Mutex<BTreeMap<String, Arc<Slot>>>,
    /// What happened that her journal should say, oldest first.
    changes: Mutex<Vec<Event>>,
}

fn hold<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Servers {
    /// The servers kept under `dir`, with those whoever runs her body
    /// configured.
    pub fn new(cfg: Config, dir: PathBuf, configured: Vec<Configured>) -> Arc<Self> {
        let registry = Registry::new(&cfg.registry);
        let servers = Self {
            cfg,
            dir,
            registry,
            slots: Mutex::new(BTreeMap::new()),
            changes: Mutex::new(Vec::new()),
        };
        for c in configured {
            let launch = Launch {
                command: c.command.into_iter().chain(c.args).collect(),
                url: c.url,
                set: Vec::new(),
                pass: c.env,
            };
            let handle = guard::sanitise(&c.name);
            servers.put(Slot {
                handle,
                description: c.description,
                launch,
                hash: None,
                offered: Mutex::new(Vec::new()),
                names: Vec::new(),
                live: tokio::sync::Mutex::default(),
                name: c.name,
            });
        }
        Arc::new(servers)
    }

    /// Whether she may find and install servers herself.
    pub fn installs(&self) -> bool {
        self.cfg.enabled
    }

    /// Take in what her memory says she has installed. A body does this
    /// once, as it starts: nothing is started until it is loaded.
    pub fn know(&self, installed: Vec<memory::Installed>) {
        for i in installed {
            self.put(Slot {
                handle: guard::handle(&i.name),
                description: i.description,
                launch: i.how,
                hash: Some(i.hash),
                offered: Mutex::new(Vec::new()),
                names: i.tools,
                live: tokio::sync::Mutex::default(),
                name: i.name,
            });
        }
    }

    fn put(&self, slot: Slot) {
        hold(&self.slots).insert(slot.handle.clone(), Arc::new(slot));
    }

    /// What her journal should say that it does not yet: servers she
    /// installed, and servers that are gone and why. Taken once.
    pub fn take_changes(&self) -> Vec<Event> {
        std::mem::take(&mut *hold(&self.changes))
    }

    fn record(&self, event: Event) {
        hold(&self.changes).push(event);
    }

    /// Every server she has, as her toolbox lists it.
    pub fn listing(&self) -> Vec<Summary> {
        hold(&self.slots)
            .values()
            .map(|s| {
                let offered = hold(&s.offered);
                Summary {
                    handle: s.handle.clone(),
                    name: s.name.clone(),
                    description: s.description.clone(),
                    tools: if offered.is_empty() {
                        s.names.clone()
                    } else {
                        offered.iter().map(|t| t.remote.clone()).collect()
                    },
                }
            })
            .collect()
    }

    /// Whether she has any server at all.
    pub fn any(&self) -> bool {
        !hold(&self.slots).is_empty()
    }

    /// The server she calls `which`, by what she calls it or by its name.
    fn slot(&self, which: &str) -> Option<Arc<Slot>> {
        let slots = hold(&self.slots);
        slots
            .get(which)
            .or_else(|| slots.values().find(|s| s.name == which))
            .cloned()
    }

    /// The tools of server `handle` she is offered once it is loaded.
    pub fn offered(&self, handle: &str) -> Vec<Vetted> {
        self.slot(handle)
            .map(|s| hold(&s.offered).clone())
            .unwrap_or_default()
    }

    /// What `find_tools` shows for `query`, in `words`: the current
    /// listings, each with how it would run, what it needs, and whether it
    /// can be installed here as it stands.
    pub async fn find(&self, words: &Store, query: &str) -> Result<String, String> {
        let query = query.trim();
        if query.chars().count() < 3 {
            return Err(words.text(TEXTS, "find_tools_short").to_owned());
        }
        let mut found = self.registry.search(words, query).await?;
        // A name taken again by someone later is shown once, by whoever
        // took it first.
        found.sort_by_key(|l| l.official().published_at);
        let mut seen = Vec::new();
        let mut copies = 0;
        found.retain(|l| {
            let short = guard::handle(&l.server.name);
            if seen.contains(&short) {
                copies += 1;
                false
            } else {
                seen.push(short);
                true
            }
        });
        if found.is_empty() {
            return Ok(words.fill(TEXTS, "no_servers_found", &[("query", query.to_owned())]));
        }
        let mut out = Vec::new();
        for (n, listing) in found.iter().take(SHOWN).enumerate() {
            let server = &listing.server;
            let planned = registry::plan(words, listing, &|key| {
                guard::given(&self.cfg, &server.name, key)
            });
            let runs = match &planned {
                Ok(plan) => plan.source.clone(),
                Err(_) => runs(words, server),
            };
            let needs = registry::needs(words, server);
            out.push(words.fill(
                TEXTS,
                "server_found",
                &[
                    ("n", (n + 1).to_string()),
                    ("name", server.name.clone()),
                    ("version", server.version.clone()),
                    ("description", short(server.description.trim(), 200)),
                    ("runs", runs),
                    (
                        "needs",
                        if needs.is_empty() {
                            words.text(TEXTS, "needs_nothing").to_owned()
                        } else {
                            needs.join(", ")
                        },
                    ),
                    (
                        "installs",
                        match planned {
                            Ok(_) => words.text(TEXTS, "installs_yes").to_owned(),
                            Err(why) => words.fill(TEXTS, "installs_no", &[("why", why)]),
                        },
                    ),
                ],
            ));
        }
        if found.len() > SHOWN {
            out.push(words.fill(
                TEXTS,
                "more_found",
                &[("count", (found.len() - SHOWN).to_string())],
            ));
        }
        if copies > 0 {
            out.push(words.fill(TEXTS, "copies_left_out", &[("count", copies.to_string())]));
        }
        Ok(out.join("\n"))
    }

    /// Install the server called `name` in the registry: check it, start
    /// it, see what it offers, and keep it. Returns what it offers, in a
    /// paragraph in `words`.
    pub async fn install(&self, words: &Store, name: &str) -> Result<String, String> {
        let name = name.trim();
        let installed = hold(&self.slots).values().filter(|s| s.installed()).count();
        let again = self.slot(name).is_some_and(|s| s.name == name);
        if !again && installed >= self.cfg.max_servers {
            return Err(words.fill(TEXTS, "servers_full", &[("count", installed.to_string())]));
        }
        let Some(listing) = self.registry.named(words, name).await? else {
            return Err(words.fill(TEXTS, "no_such_listing", &[("name", name.to_owned())]));
        };
        let plan = registry::plan(words, &listing, &|key| guard::given(&self.cfg, name, key))?;
        let handle = guard::handle(name);
        if let Some(other) = self.slot(&handle).filter(|s| s.name != name) {
            return Err(words.fill(
                TEXTS,
                "handle_taken",
                &[("server", handle), ("name", other.name.clone())],
            ));
        }
        let dir = self.dir.join(guard::folder(name));
        let (connection, listed) = self
            .start(
                words,
                name,
                &plan.launch,
                &dir,
                Duration::from_secs(self.cfg.install_seconds),
            )
            .await?;
        let (offered, notes) = guard::vet(words, &handle, &listed, &self.cfg);
        if offered.is_empty() {
            connection.kill();
            let mut why = words.text(TEXTS, "no_usable_tool").to_owned();
            for note in notes {
                why.push(' ');
                why.push_str(&note);
            }
            return Err(why);
        }
        let hash = guard::hash(&listed);
        let summary = offered
            .iter()
            .map(|t| t.remote.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let Plan {
            name,
            version,
            description,
            source,
            owner,
            launch,
            ..
        } = plan;
        // Installing again replaces what was there, running or not.
        if let Some(old) = hold(&self.slots).remove(&handle)
            && let Ok(live) = old.live.try_lock()
            && let Some(c) = &live.connection
        {
            c.kill();
        }
        let tools: Vec<String> = listed.iter().map(|t| t.name.clone()).collect();
        let count = offered.len();
        self.put(Slot {
            handle: handle.clone(),
            description: description.clone(),
            launch: launch.clone(),
            hash: Some(hash.clone()),
            offered: Mutex::new(offered),
            names: tools.clone(),
            live: tokio::sync::Mutex::new(Live {
                connection: Some(Arc::new(connection)),
                ..Live::default()
            }),
            name: name.clone(),
        });
        self.record(Event::Installed {
            name: name.clone(),
            version: version.clone(),
            description,
            source,
            owner,
            how: launch,
            tools,
            hash,
        });
        let mut said = words.fill(
            TEXTS,
            "server_installed",
            &[
                ("name", name),
                ("version", version),
                ("server", handle),
                ("count", count.to_string()),
                ("tools", summary),
            ],
        );
        for note in notes {
            said.push(' ');
            said.push_str(&note);
        }
        Ok(said)
    }

    /// Take out the server she installed called `name`, for `why`. What
    /// is said of it is said in `words`.
    pub fn uninstall(&self, words: &Store, name: &str, why: &str) -> Result<String, String> {
        let Some(slot) = self.slot(name.trim()) else {
            return Err(words.fill(TEXTS, "no_server_called", &[("name", name.to_owned())]));
        };
        let named = [("name", slot.name.clone())];
        if !slot.installed() {
            return Err(words.fill(TEXTS, "server_not_hers", &named));
        }
        self.drop_slot(&slot, why);
        let _ = std::fs::remove_dir_all(self.dir.join(guard::folder(&slot.name)));
        Ok(words.fill(TEXTS, "server_uninstalled", &named))
    }

    /// Stop offering `slot` and stop it, and say so in her journal.
    fn drop_slot(&self, slot: &Slot, why: &str) {
        hold(&self.slots).remove(&slot.handle);
        if let Ok(live) = slot.live.try_lock()
            && let Some(c) = &live.connection
        {
            c.kill();
        }
        if slot.installed() {
            self.record(Event::Uninstalled {
                name: slot.name.clone(),
                why: why.to_owned(),
            });
        }
    }

    /// Start a server, and list what it offers. Why it could not be, in
    /// `words`.
    async fn start(
        &self,
        words: &Store,
        name: &str,
        launch: &Launch,
        dir: &std::path::Path,
        timeout: Duration,
    ) -> Result<(Connection, Vec<Listed>), String> {
        let connection = if let Some(url) = &launch.url {
            let (host, addresses) = guard::reachable(words, url).await?;
            client::remote(words, url, &host, &addresses, timeout).await?
        } else {
            let env = guard::environment(&self.cfg, name, dir, &launch.set, &launch.pass);
            client::local(words, &launch.command, &env, dir, timeout).await?
        };
        let listed = connection.list(words, timeout).await?;
        Ok((connection, listed))
    }

    /// The running connection to `slot`, started if it is not. A server
    /// that will not start is tried again later, waiting longer each
    /// time, and after several tries in a row it is uninstalled. One that
    /// describes its tools differently from when she installed it is put
    /// aside the same way, until she installs it again. Why it is not
    /// running is said in `words`, as is why it was put aside, in her
    /// journal.
    async fn connect(&self, words: &Store, slot: &Arc<Slot>) -> Result<Arc<Connection>, String> {
        let mut live = slot.live.lock().await;
        if let Some(c) = &live.connection {
            if !c.closed() {
                if c.changed() {
                    let listed = c
                        .list(words, Duration::from_secs(self.cfg.start_seconds))
                        .await?;
                    if let Err(why) = self.check(words, slot, &listed) {
                        c.kill();
                        live.connection = None;
                        return Err(why);
                    }
                }
                return Ok(c.clone());
            }
            live.connection = None;
        }
        if let Some(at) = live.retry_at
            && Instant::now() < at
        {
            return Err(words.fill(
                TEXTS,
                "server_resting",
                &[
                    ("server", slot.handle.clone()),
                    (
                        "seconds",
                        (at.saturating_duration_since(Instant::now()).as_secs() + 1).to_string(),
                    ),
                ],
            ));
        }
        let dir = self.dir.join(guard::folder(&slot.name));
        match self
            .start(
                words,
                &slot.name,
                &slot.launch,
                &dir,
                Duration::from_secs(self.cfg.start_seconds),
            )
            .await
        {
            Ok((connection, listed)) => {
                if let Err(why) = self.check(words, slot, &listed) {
                    connection.kill();
                    return Err(why);
                }
                live.failures = 0;
                live.retry_at = None;
                let connection = Arc::new(connection);
                live.connection = Some(connection.clone());
                Ok(connection)
            }
            Err(why) => {
                live.failures += 1;
                let pairs = [
                    ("server", slot.handle.clone()),
                    ("times", STARTS.to_string()),
                    ("why", why),
                ];
                if live.failures >= STARTS && slot.installed() {
                    drop(live);
                    self.drop_slot(slot, &words.fill(TEXTS, "failed_to_start", &pairs));
                    return Err(words.fill(TEXTS, "server_given_up", &pairs));
                }
                live.retry_at =
                    Some(Instant::now() + Duration::from_secs(5 << live.failures.min(6)));
                Err(words.fill(TEXTS, "server_not_started", &pairs))
            }
        }
    }

    /// Take in what `slot` lists now. For a server she installed, if that
    /// is not what it listed when she installed it, it is put aside, and
    /// why is said in `words`.
    fn check(&self, words: &Store, slot: &Slot, listed: &[Listed]) -> Result<(), String> {
        if let Some(hash) = &slot.hash
            && guard::hash(listed) != *hash
        {
            let why = words.text(TEXTS, "server_changed");
            self.drop_slot(slot, why);
            return Err(words.fill(
                TEXTS,
                "server_put_aside",
                &[("server", slot.handle.clone()), ("why", why.to_owned())],
            ));
        }
        let (offered, _) = guard::vet(words, &slot.handle, listed, &self.cfg);
        *hold(&slot.offered) = offered;
        Ok(())
    }

    /// Start server `which` if it is not running, and return the tools of
    /// it she is offered. Why not, in `words`.
    pub async fn load(&self, words: &Store, which: &str) -> Result<(String, Vec<Vetted>), String> {
        let Some(slot) = self.slot(which.trim()) else {
            let have: Vec<String> = hold(&self.slots).keys().cloned().collect();
            let name = ("name", which.to_owned());
            return Err(if have.is_empty() {
                words.fill(TEXTS, "no_servers_yet", &[name])
            } else {
                words.fill(
                    TEXTS,
                    "no_server_but",
                    &[name, ("servers", have.join(", "))],
                )
            });
        };
        self.connect(words, &slot).await?;
        let offered = hold(&slot.offered).clone();
        Ok((slot.handle.clone(), offered))
    }

    /// Call `tool` of server `handle`. A call that takes too long has the
    /// server killed, with everything it started; the next call starts it
    /// again. What is said around what it returns is said in `words`.
    pub async fn call(&self, words: &Store, handle: &str, tool: &str, args: &Value) -> Outcome {
        let server = || ("server", handle.to_owned());
        let Some(slot) = self.slot(handle) else {
            return Outcome::fail(words.fill(TEXTS, "server_gone", &[server()]));
        };
        let connection = match self.connect(words, &slot).await {
            Ok(c) => c,
            Err(why) => return Outcome::fail(why),
        };
        let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
        match connection
            .call(
                words,
                tool,
                args,
                Duration::from_secs(self.cfg.call_seconds),
            )
            .await
        {
            Ok(mut outcome) => {
                outcome.output = crate::tools::clip(words, &outcome.output, 12_000, 8_000);
                outcome
            }
            Err(why) => {
                connection.kill();
                slot.live.lock().await.connection = None;
                Outcome::fail(words.fill(TEXTS, "server_stopped", &[server(), ("why", why)]))
            }
        }
    }
}

/// The first `most` characters of `text`, and a mark if there were more.
pub(crate) fn short(text: &str, most: usize) -> String {
    let mut out: String = text.chars().take(most).collect();
    if text.chars().count() > most {
        out.push('…');
    }
    out
}

/// How a listing would run, said plainly in `words`, for one that cannot.
fn runs(words: &Store, server: &registry::Server) -> String {
    if let Some(p) = server.packages.first() {
        format!("{} {}@{}", p.registry_type, p.identifier, p.version)
    } else if let Some(r) = server.remotes.first() {
        words.fill(
            TEXTS,
            "runs_remote",
            &[("kind", r.kind.clone()), ("url", r.url.clone())],
        )
    } else {
        words.text(TEXTS, "runs_nothing").to_owned()
    }
}
