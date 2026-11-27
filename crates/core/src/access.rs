//! Who may reach this u0 from elsewhere (`link`), and how (`share`):
//! the devices paired with it, each by the public half of its key
//! (`key`), kept in `devices.json` with how it is shared, as
//! `authorized_keys` keeps them; and the one-time codes that let a new key
//! in, kept only in memory, each good once and for ten minutes.
//!
//! A code is 128 bits from the system's randomness: what keeps it from
//! being guessed is its length, not a limit on tries. A failed attempt to
//! come in is answered after a second, one at a time while a few wait,
//! and past those at once, so that failures pile up nowhere.

use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::{key, proto::Hello, whole};

/// How many failed attempts wait their second at once: more are turned
/// away without it.
const WAITING: usize = 8;

/// How long a pairing code is good for.
const CODE_LIFE: Duration = Duration::from_mins(10);

/// What `devices.json` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    pub devices: Vec<Device>,
    /// The port u0 listens on, on 127.0.0.1: another for a second
    /// u0 on one machine, each with a home of its own.
    #[serde(default = "port")]
    pub port: u16,
    /// A named tunnel of yours (cloudflared), to be reached by the same
    /// address each time; none, a quick one with an address of its own.
    #[serde(default)]
    pub named: Option<Named>,
    /// Whether u0 listens, and runs the tunnel, each time it starts,
    /// and keeps the Mac awake while it does.
    #[serde(default)]
    pub serve: bool,
    #[serde(default)]
    pub tunnel: bool,
    #[serde(default)]
    pub awake: bool,
    /// Where it listens beyond 127.0.0.1, with TLS of its own, on the
    /// port after `port`.
    #[serde(default)]
    pub bind: crate::reach::Bind,
}

fn port() -> u16 {
    4317
}

impl Default for Kept {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            port: port(),
            named: None,
            serve: false,
            tunnel: false,
            awake: false,
            bind: crate::reach::Bind::Here,
        }
    }
}

/// A device paired with this u0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    /// What it called itself as it paired.
    pub label: String,
    /// The public half of its key, `ssh-ed25519 AAAA…`: what it is known
    /// by.
    pub key: String,
    /// What it may do here.
    pub scope: Scope,
    /// When its access ends (seconds since 1970); none, never.
    #[serde(default)]
    pub until: Option<u64>,
    /// It paired, and waits for you to let it in.
    #[serde(default)]
    pub waiting: bool,
    /// When it paired, and when it last came in (seconds since 1970).
    pub paired: u64,
    pub seen: u64,
}

/// What a device paired may do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Watch: what is said and done, and the projects' files and git;
    /// it changes nothing, runs nothing and reads nothing elsewhere.
    Read,
    /// All a desk here does: agents, sessions, terminals, settings.
    #[default]
    Full,
}

/// What a pairing code lets in: a device of `scope`, for `days` (none,
/// until revoked), let in at once or (`ask`) once you say so; and
/// whether (`both`) this Mac pairs back with it as it asks, so that it is
/// controlled from here too: for one of full scope alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub scope: Scope,
    pub days: Option<u32>,
    pub ask: bool,
    pub both: bool,
}

/// A device let in: its key, what it may do, and whether this Mac pairs
/// back with it (it paired now by a code whose grant said so).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    pub key: String,
    pub scope: Scope,
    pub back: bool,
}

/// Why a device is not let in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Turned {
    Away(String),
    /// Its access ended, as said.
    Ended(String),
    /// It is paired, and waits for you.
    Waiting,
}

/// A named tunnel: its token, from Cloudflare, and the address it serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Named {
    pub token: String,
    pub host: String,
}

pub struct Access {
    path: PathBuf,
    /// What `devices.json` holds, or why it does not read: then no device
    /// comes in, and nothing is written over it.
    kept: Mutex<Result<Kept, String>>,
    codes: Mutex<Vec<(String, Instant, Grant)>>,
    /// Held through each failed attempt's wait.
    failing: tokio::sync::Mutex<()>,
    /// How many failed attempts wait for it.
    waiting: AtomicUsize,
    /// Told each time what is kept changes: a link whose device was
    /// unpaired then closes.
    changed: tokio::sync::watch::Sender<()>,
}

impl Access {
    /// The access kept in u0's home `home`.
    pub fn new(home: &Path) -> Self {
        let path = home.join("devices.json");
        let kept = whole::read_json(&path).map_err(|e| {
            format!(
                "{e}. Move it away and pair each device again: until then no device can come in."
            )
        });
        Self {
            path,
            kept: Mutex::new(kept),
            codes: Mutex::default(),
            failing: tokio::sync::Mutex::const_new(()),
            waiting: AtomicUsize::new(0),
            changed: tokio::sync::watch::Sender::new(()),
        }
    }

    /// What is kept, or why it does not read.
    pub fn kept(&self) -> Result<Kept, String> {
        self.kept.lock().expect("access").clone()
    }

    /// Change what is kept by `change` and write it, unless it did not
    /// read.
    pub fn change(&self, change: impl FnOnce(&mut Kept)) {
        let mut kept = self.kept.lock().expect("access");
        if let Ok(kept) = &mut *kept {
            change(kept);
            let text = serde_json::to_vec_pretty(kept).expect("devices serialize");
            if let Err(e) = whole::write(&self.path, &text) {
                tracing::warn!("{} was not written: {e}", self.path.display());
            }
            self.changed.send_replace(());
        }
    }

    /// What tells each time what is kept changes.
    pub fn changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.changed.subscribe()
    }

    /// A new pairing code, letting in what `grant` says.
    pub fn code(&self, grant: Grant) -> String {
        let code = random::<16>();
        let mut codes = self.codes.lock().expect("codes");
        codes.retain(|(_, made, _)| made.elapsed() < CODE_LIFE);
        codes.push((code.clone(), Instant::now(), grant));
        code
    }

    /// Let in no key by a code made so far.
    pub fn withdraw(&self) {
        self.codes.lock().expect("codes").clear();
    }

    /// What the pairing code `code` lets in, while it is still good.
    pub fn live(&self, code: &str) -> Option<Grant> {
        (self.codes.lock().expect("codes").iter())
            .find(|(c, made, _)| same(c, code) && made.elapsed() < CODE_LIFE)
            .map(|(_, _, grant)| *grant)
    }

    /// Let in the desk that says `hello` to the challenge `nonce`: the
    /// key it is known by and what it may do; or, a second later, why
    /// not.
    pub async fn admit(&self, hello: &Hello, nonce: &str) -> Result<Admitted, Turned> {
        let admitted = self.check(hello, nonce);
        // Past a few waiting, one more is turned away at once: the wait
        // slows guessing, and is no queue for anyone to fill.
        if admitted.is_err() && self.waiting.fetch_add(1, Ordering::SeqCst) < WAITING {
            let _one_at_a_time = self.failing.lock().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        if admitted.is_err() {
            self.waiting.fetch_sub(1, Ordering::SeqCst);
        }
        admitted
    }

    fn check(&self, hello: &Hello, nonce: &str) -> Result<Admitted, Turned> {
        if let Err(why) = &*self.kept.lock().expect("access") {
            return Err(Turned::Away(why.clone()));
        }
        if !key::verify(&hello.key, nonce, &hello.signature) {
            return Err(Turned::Away(
                "This device's signature does not verify".into(),
            ));
        }
        let now = crate::store::secs(std::time::SystemTime::now());
        let mut back = false;
        if let Some(pair) = &hello.pair {
            let mut codes = self.codes.lock().expect("codes");
            let at = (codes.iter())
                .position(|(c, made, _)| same(c, &pair.code) && made.elapsed() < CODE_LIFE)
                .ok_or_else(|| {
                    Turned::Away("The pairing link is used up or out of date: make another".into())
                })?;
            let (_, _, grant) = codes.remove(at);
            drop(codes);
            back = grant.both && grant.scope == Scope::Full;
            self.change(|k| {
                k.devices.retain(|d| d.key != hello.key);
                k.devices.push(Device {
                    label: pair.device.clone(),
                    key: hello.key.clone(),
                    scope: grant.scope,
                    until: grant.days.map(|d| now + u64::from(d) * 86_400),
                    waiting: grant.ask,
                    paired: now,
                    seen: now,
                });
            });
        }
        let scope = self.lets_in(&hello.key)?;
        // Written only for a device let in, and only as its `seen`
        // changes: a key unknown here, one that waits and one whose
        // access ended change nothing, and so write and tell nothing.
        let seen = |k: &Kept| (k.devices.iter()).any(|d| d.key == hello.key && d.seen != now);
        if (self.kept.lock().expect("access").as_ref()).is_ok_and(seen) {
            self.change(|k| {
                if let Some(d) = k.devices.iter_mut().find(|d| d.key == hello.key) {
                    d.seen = now;
                }
            });
        }
        Ok(Admitted {
            key: hello.key.clone(),
            scope,
            back,
        })
    }

    /// Unpair the device known by `key`, or turn it away while it waits:
    /// its links close.
    pub fn revoke(&self, key: &str) {
        self.change(|k| k.devices.retain(|d| d.key != key));
    }

    /// Let in the device known by `key`, which waits for you.
    pub fn approve(&self, key: &str) {
        self.change(|k| {
            for d in k.devices.iter_mut().filter(|d| d.key == key) {
                d.waiting = false;
            }
        });
    }

    /// What the device known by `key` may do, now: paired, let in, and
    /// its access not ended; or why it may do nothing.
    pub fn lets_in(&self, key: &str) -> Result<Scope, Turned> {
        let kept = self.kept.lock().expect("access");
        let kept = kept.as_ref().map_err(|why| Turned::Away(why.clone()))?;
        let d = (kept.devices.iter())
            .find(|d| d.key == key)
            .ok_or_else(|| Turned::Away("This device is not paired with this u0".into()))?;
        let now = std::time::SystemTime::now();
        if let Some(until) = d.until.filter(|u| *u <= crate::store::secs(now)) {
            let ended = crate::when::said_at(crate::store::time(until), now, false);
            return Err(Turned::Ended(format!(
                "This device's access ended {ended}: pair it again"
            )));
        }
        if d.waiting {
            return Err(Turned::Waiting);
        }
        Ok(d.scope)
    }

    /// Whether a device of `scope` may make `call` here, in u0's home
    /// `home`: one of full scope anything; one that may only read, a call
    /// that only reads, of what lies in a project of yours or a worktree
    /// of u0's, by their real paths, so that no `..` or link leads out.
    pub fn allows(scope: Scope, call: &crate::proto::Call, home: &Path) -> bool {
        if scope == Scope::Full {
            return true;
        }
        let Some(paths) = call.reads() else {
            return false;
        };
        within(home, &paths)
    }
}

/// Whether each of `paths` lies, by its real path (so that no `..` or link
/// leads out), in a project of yours or a worktree of u0's home `home`:
/// all that one who may only read is let read.
pub fn within(home: &Path, paths: &[&Path]) -> bool {
    if paths.is_empty() {
        return true;
    }
    let settings = crate::settings::Settings::load(home).0;
    let roots: Vec<PathBuf> = (settings.folders.iter())
        .chain(&settings.worktree_folders(home))
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .collect();
    paths
        .iter()
        .all(|p| std::fs::canonicalize(p).is_ok_and(|p| roots.iter().any(|r| p.starts_with(r))))
}

/// Whether a pairing code made is `said`, compared in constant time, as a
/// webhook's token is: how long a guess matched tells nothing.
fn same(made: &str, said: &str) -> bool {
    use subtle::ConstantTimeEq as _;
    made.as_bytes().ct_eq(said.as_bytes()).into()
}

/// `N` bytes from the system's randomness (ring's, as the keys are made).
fn bytes<const N: usize>() -> [u8; N] {
    ring::rand::generate(&ring::rand::SystemRandom::new())
        .expect("the system's randomness")
        .expose()
}

/// `N` random bytes, as URL-safe base64.
pub fn random<const N: usize>() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes::<N>())
}

/// A number from the system's randomness.
pub fn number() -> u64 {
    u64::from_le_bytes(bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{key::Key, proto::Pair};

    /// What `key` says to the challenge `nonce`, pairing with `code`.
    fn hello(key: &Key, nonce: &str, code: Option<String>) -> Hello {
        Hello {
            protocol: crate::proto::PROTOCOL,
            key: key.public(),
            signature: key.sign(nonce),
            pair: code.map(|code| Pair {
                code,
                device: "laptop".into(),
                back: None,
            }),
            since: None,
            then: crate::proto::Then::Desk,
        }
    }

    fn away(t: Turned) -> String {
        match t {
            Turned::Away(why) | Turned::Ended(why) => why,
            Turned::Waiting => "waiting".into(),
        }
    }

    fn block<F: std::future::Future>(f: F) -> F::Output {
        crate::acp::runtime().block_on(f)
    }

    #[test]
    fn a_pairing_code_lets_a_key_in_once_and_only_its_public_half_is_kept() {
        let home = tempfile::tempdir().unwrap();
        let access = Access::new(home.path());
        let (key, other) = (Key::generate(), Key::generate());
        let code = access.code(Grant::default());
        assert_eq!(
            block(access.admit(&hello(&key, "n1", Some(code.clone())), "n1")),
            Ok(Admitted {
                key: key.public(),
                scope: Scope::Full,
                back: false
            })
        );
        assert!(
            access.live(&code).is_none(),
            "a code used is no longer shown"
        );
        let used = block(access.admit(&hello(&other, "n2", Some(code)), "n2"));
        assert!(used.map_err(away).unwrap_err().contains("used up"));
        let written = std::fs::read_to_string(home.path().join("devices.json")).unwrap();
        assert!(written.contains(&key.public()) && !written.contains(key.kept()));
        // Read again, the device comes in by its signature alone.
        let again = Access::new(home.path());
        assert!(block(again.admit(&hello(&key, "n3", None), "n3")).is_ok());
        let stranger = block(again.admit(&hello(&other, "n4", None), "n4"));
        assert!(stranger.map_err(away).unwrap_err().contains("not paired"));
    }

    #[test]
    fn a_signature_of_another_nonce_is_turned_away_after_a_wait() {
        let home = tempfile::tempdir().unwrap();
        let access = Access::new(home.path());
        let key = Key::generate();
        let code = access.code(Grant::default());
        let began = Instant::now();
        let replayed = block(access.admit(&hello(&key, "old", Some(code.clone())), "new"));
        assert!(
            replayed
                .map_err(away)
                .unwrap_err()
                .contains("does not verify")
        );
        assert!(began.elapsed() >= Duration::from_secs(1));
        // The code was not spent on it.
        assert!(block(access.admit(&hello(&key, "new", Some(code)), "new")).is_ok());
    }

    #[test]
    fn failed_attempts_past_a_few_waiting_are_turned_away_without_waiting() {
        let home = tempfile::tempdir().unwrap();
        let access = std::sync::Arc::new(Access::new(home.path()));
        let began = Instant::now();
        let quick = block(async {
            let tries = (0..WAITING + 4).map(|n| {
                let access = access.clone();
                tokio::spawn(async move {
                    let stranger = hello(&Key::generate(), &format!("n{n}"), None);
                    let _ = access.admit(&stranger, &format!("n{n}")).await;
                    began.elapsed()
                })
            });
            let took = futures_util::future::join_all(tries).await;
            took.into_iter()
                .filter(|t| t.as_ref().is_ok_and(|t| *t < Duration::from_millis(500)))
                .count()
        });
        assert!(quick >= 4, "{quick} turned away at once");
    }

    #[test]
    fn an_expired_code_lets_no_key_in() {
        let home = tempfile::tempdir().unwrap();
        let access = Access::new(home.path());
        let code = access.code(Grant::default());
        access.codes.lock().unwrap()[0].1 -= CODE_LIFE;
        let key = Key::generate();
        let late = block(access.admit(&hello(&key, "n", Some(code)), "n"));
        assert!(late.map_err(away).unwrap_err().contains("out of date"));
    }

    #[test]
    fn a_device_that_may_only_read_reads_in_your_projects_alone() {
        use crate::proto::Call;
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("p");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("a.rs"), "x").unwrap();
        std::fs::write(home.path().join("mcp.json"), "{}").unwrap();
        std::os::unix::fs::symlink(home.path().join("mcp.json"), project.join("out")).unwrap();
        let settings = serde_json::json!({"enabled": [], "folders": [project]});
        std::fs::write(home.path().join("settings.json"), settings.to_string()).unwrap();
        let read = |path: PathBuf| Call::ReadFile { path };
        let may = |call: &Call| Access::allows(Scope::Read, call, home.path());
        assert!(may(&read(project.join("a.rs"))));
        assert!(may(&Call::Watch));
        assert!(!may(&read(home.path().join("mcp.json"))), "outside");
        assert!(!may(&read(project.join("../mcp.json"))), "led out by ..");
        assert!(!may(&read(project.join("out"))), "led out by a link");
        assert!(!may(&Call::Stash(Vec::new())), "it changes something");
        let act = Call::TermOpen {
            id: 1,
            dir: project.clone(),
            shell: None,
            first: None,
            size: (1, 1),
        };
        assert!(!may(&act) && Access::allows(Scope::Full, &act, home.path()));
    }

    #[test]
    fn a_devices_file_of_tokens_lets_no_one_in_and_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let old = r#"{"devices":[{"label":"laptop","hash":"h","paired":1,"seen":2}]}"#;
        std::fs::write(home.path().join("devices.json"), old).unwrap();
        let access = Access::new(home.path());
        let key = Key::generate();
        let refused =
            block(access.admit(&hello(&key, "n", Some(access.code(Grant::default()))), "n"));
        assert!(
            refused
                .map_err(away)
                .unwrap_err()
                .contains("pair each device again")
        );
        let left = std::fs::read_to_string(home.path().join("devices.json")).unwrap();
        assert_eq!(left, old);
    }
}
