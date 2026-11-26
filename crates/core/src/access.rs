//! Who may reach this Ghost from elsewhere (`link`), and how (`share`):
//! the devices paired with it, each by the public half of its key
//! (`key`), kept in `devices.json` with how it is shared, as
//! `authorized_keys` keeps them; and the one-time codes that let a new key
//! in, kept only in memory, each good once and for ten minutes.
//!
//! A code is 128 bits from the system's randomness. A failed attempt to
//! come in is answered after a second, one at a time, so that guessing
//! goes no faster than that.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::{key, proto::Hello, whole};

/// How long a pairing code is good for.
const CODE_LIFE: Duration = Duration::from_mins(10);

/// What `devices.json` holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    pub devices: Vec<Device>,
    /// A named tunnel of yours (cloudflared), to be reached by the same
    /// address each time; none, a quick one with an address of its own.
    #[serde(default)]
    pub named: Option<Named>,
    /// Whether Ghost listens, and runs the tunnel, each time it starts,
    /// and keeps the Mac awake while it does.
    #[serde(default)]
    pub serve: bool,
    #[serde(default)]
    pub tunnel: bool,
    #[serde(default)]
    pub awake: bool,
}

/// A device paired with this Ghost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    /// What it called itself as it paired.
    pub label: String,
    /// The public half of its key, `ssh-ed25519 AAAA…`: what it is known
    /// by.
    pub key: String,
    /// When it paired, and when it last came in (seconds since 1970).
    pub paired: u64,
    pub seen: u64,
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
    codes: Mutex<Vec<(String, Instant)>>,
    /// Held through each failed attempt's wait.
    failing: tokio::sync::Mutex<()>,
    /// Told each time what is kept changes: a link whose device was
    /// unpaired then closes.
    changed: tokio::sync::watch::Sender<()>,
}

impl Access {
    /// The access kept in Ghost's home `home`.
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

    /// A new pairing code.
    pub fn code(&self) -> String {
        let code = random::<16>();
        let mut codes = self.codes.lock().expect("codes");
        codes.retain(|(_, made)| made.elapsed() < CODE_LIFE);
        codes.push((code.clone(), Instant::now()));
        code
    }

    /// Whether the pairing code `code` is still good.
    pub fn live(&self, code: &str) -> bool {
        (self.codes.lock().expect("codes").iter())
            .any(|(c, made)| c == code && made.elapsed() < CODE_LIFE)
    }

    /// Let in the desk that says `hello` to the challenge `nonce`: the
    /// key it is known by; or, a second later, why not.
    pub async fn admit(&self, hello: &Hello, nonce: &str) -> Result<String, String> {
        let admitted = self.check(hello, nonce);
        if admitted.is_err() {
            let _one_at_a_time = self.failing.lock().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        admitted
    }

    fn check(&self, hello: &Hello, nonce: &str) -> Result<String, String> {
        if let Err(why) = &*self.kept.lock().expect("access") {
            return Err(why.clone());
        }
        if !key::verify(&hello.key, nonce, &hello.signature) {
            return Err("This device's signature does not verify".into());
        }
        let now = crate::store::secs(std::time::SystemTime::now());
        let mut known = false;
        if let Some(pair) = &hello.pair {
            let mut codes = self.codes.lock().expect("codes");
            let at = codes
                .iter()
                .position(|(c, made)| c == &pair.code && made.elapsed() < CODE_LIFE)
                .ok_or("The pairing link is used up or out of date: make another")?;
            codes.remove(at);
            drop(codes);
            known = true;
            self.change(|k| {
                k.devices.retain(|d| d.key != hello.key);
                k.devices.push(Device {
                    label: pair.device.clone(),
                    key: hello.key.clone(),
                    paired: now,
                    seen: now,
                });
            });
        } else {
            self.change(|k| {
                if let Some(d) = k.devices.iter_mut().find(|d| d.key == hello.key) {
                    d.seen = now;
                    known = true;
                }
            });
        }
        known
            .then(|| hello.key.clone())
            .ok_or_else(|| "This device is not paired with this Ghost".into())
    }

    /// Unpair the device known by `key`: its links close.
    pub fn revoke(&self, key: &str) {
        self.change(|k| k.devices.retain(|d| d.key != key));
    }

    /// Whether the device known by `key` is still paired.
    pub fn paired(&self, key: &str) -> bool {
        (self.kept.lock().expect("access").as_ref())
            .is_ok_and(|k| k.devices.iter().any(|d| d.key == key))
    }
}

/// `N` random bytes, as URL-safe base64.
pub fn random<const N: usize>() -> String {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the system's randomness");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A number from the system's randomness.
pub fn number() -> u64 {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the system's randomness");
    u64::from_le_bytes(bytes)
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
            }),
            since: None,
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
        let code = access.code();
        assert_eq!(
            block(access.admit(&hello(&key, "n1", Some(code.clone())), "n1")),
            Ok(key.public())
        );
        assert!(!access.live(&code), "a code used is no longer shown");
        let used = block(access.admit(&hello(&other, "n2", Some(code)), "n2"));
        assert!(used.unwrap_err().contains("used up"));
        let written = std::fs::read_to_string(home.path().join("devices.json")).unwrap();
        assert!(written.contains(&key.public()) && !written.contains(key.kept()));
        // Read again, the device comes in by its signature alone.
        let again = Access::new(home.path());
        assert!(block(again.admit(&hello(&key, "n3", None), "n3")).is_ok());
        let stranger = block(again.admit(&hello(&other, "n4", None), "n4"));
        assert!(stranger.unwrap_err().contains("not paired"));
    }

    #[test]
    fn a_signature_of_another_nonce_is_turned_away_after_a_wait() {
        let home = tempfile::tempdir().unwrap();
        let access = Access::new(home.path());
        let key = Key::generate();
        let code = access.code();
        let began = Instant::now();
        let replayed = block(access.admit(&hello(&key, "old", Some(code.clone())), "new"));
        assert!(replayed.unwrap_err().contains("does not verify"));
        assert!(began.elapsed() >= Duration::from_secs(1));
        // The code was not spent on it.
        assert!(block(access.admit(&hello(&key, "new", Some(code)), "new")).is_ok());
    }

    #[test]
    fn an_expired_code_lets_no_key_in() {
        let home = tempfile::tempdir().unwrap();
        let access = Access::new(home.path());
        let code = access.code();
        access.codes.lock().unwrap()[0].1 -= CODE_LIFE;
        let key = Key::generate();
        let late = block(access.admit(&hello(&key, "n", Some(code)), "n"));
        assert!(late.unwrap_err().contains("out of date"));
    }

    #[test]
    fn a_devices_file_of_tokens_lets_no_one_in_and_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let old = r#"{"devices":[{"label":"laptop","hash":"h","paired":1,"seen":2}]}"#;
        std::fs::write(home.path().join("devices.json"), old).unwrap();
        let access = Access::new(home.path());
        let key = Key::generate();
        let refused = block(access.admit(&hello(&key, "n", Some(access.code())), "n"));
        assert!(refused.unwrap_err().contains("pair each device again"));
        let left = std::fs::read_to_string(home.path().join("devices.json")).unwrap();
        assert_eq!(left, old);
    }
}
