//! Who may reach this Ghost from elsewhere (`link`), and how (`share`):
//! the devices paired
//! with it, each by the SHA-256 of the token it was given, kept in
//! `devices.json` with how it is shared; and the one-time codes a device pairs with, kept only
//! in memory, each good once and for ten minutes.
//!
//! A token or code is 128 or 256 bits from the system's randomness. A
//! failed attempt to come in is answered after a second, one at a time,
//! so that guessing goes no faster than that.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::{proto::Auth, whole};

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
    /// The SHA-256 of its token, base64: what it is known by.
    pub hash: String,
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
        let kept = whole::read_json(&path)
            .map_err(|e| format!("{e}. No device can come in until it is moved away or mended."));
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

    /// Let in the desk that shows `auth`: the hash it is known by, and
    /// its new token where it paired; or, a second later, why not.
    pub async fn admit(&self, auth: &Auth) -> Result<(String, Option<String>), String> {
        let admitted = self.check(auth);
        if admitted.is_err() {
            let _one_at_a_time = self.failing.lock().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        admitted
    }

    fn check(&self, auth: &Auth) -> Result<(String, Option<String>), String> {
        if let Err(why) = &*self.kept.lock().expect("access") {
            return Err(why.clone());
        }
        let now = crate::store::secs(std::time::SystemTime::now());
        match auth {
            Auth::Token(token) => {
                let hash = hash(token);
                let mut known = false;
                self.change(|k| {
                    if let Some(d) = k.devices.iter_mut().find(|d| d.hash == hash) {
                        d.seen = now;
                        known = true;
                    }
                });
                known
                    .then_some((hash, None))
                    .ok_or_else(|| "This device is not paired with this Ghost".into())
            }
            Auth::Pair { code, device } => {
                let mut codes = self.codes.lock().expect("codes");
                let at = codes
                    .iter()
                    .position(|(c, made)| c == code && made.elapsed() < CODE_LIFE)
                    .ok_or("The pairing link is used up or out of date: make another")?;
                codes.remove(at);
                drop(codes);
                let token = random::<32>();
                let hash = hash(&token);
                self.change(|k| {
                    k.devices.push(Device {
                        label: device.clone(),
                        hash: hash.clone(),
                        paired: now,
                        seen: now,
                    });
                });
                Ok((hash, Some(token)))
            }
        }
    }

    /// Unpair the device known by `hash`: its links close.
    pub fn revoke(&self, hash: &str) {
        self.change(|k| k.devices.retain(|d| d.hash != hash));
    }

    /// Whether the device known by `hash` is still paired.
    pub fn paired(&self, hash: &str) -> bool {
        (self.kept.lock().expect("access").as_ref())
            .is_ok_and(|k| k.devices.iter().any(|d| d.hash == hash))
    }
}

/// `N` random bytes, as URL-safe base64.
fn random<const N: usize>() -> String {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the system's randomness");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The SHA-256 of `token`, as base64.
fn hash(token: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(token))
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

    fn pair(access: &Access, code: &str) -> Result<(String, Option<String>), String> {
        acp_block(access.admit(&Auth::Pair {
            code: code.into(),
            device: "laptop".into(),
        }))
    }

    fn acp_block<F: std::future::Future>(f: F) -> F::Output {
        crate::acp::runtime().block_on(f)
    }

    #[test]
    fn a_pairing_code_gives_a_token_once_and_only_its_hash_is_kept() {
        let home = tempfile::tempdir().unwrap();
        let access = Access::new(home.path());
        let code = access.code();
        let (hash, token) = pair(&access, &code).unwrap();
        let token = token.unwrap();
        assert!(pair(&access, &code).is_err(), "a code is good once");
        let written = std::fs::read_to_string(home.path().join("devices.json")).unwrap();
        assert!(written.contains(&hash) && !written.contains(&token));
        // Read again, the device comes in with its token.
        let again = Access::new(home.path());
        let (known, none) = acp_block(again.admit(&Auth::Token(token))).unwrap();
        assert_eq!((known, none), (hash, None));
    }

    #[test]
    fn a_wrong_token_is_turned_away_after_a_wait() {
        let home = tempfile::tempdir().unwrap();
        let access = Access::new(home.path());
        let began = Instant::now();
        assert!(acp_block(access.admit(&Auth::Token("guess".into()))).is_err());
        assert!(began.elapsed() >= Duration::from_secs(1));
    }

    #[test]
    fn a_devices_file_that_does_not_read_lets_no_one_in_and_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("devices.json"), r#"{"who":"me"}"#).unwrap();
        let access = Access::new(home.path());
        let code = access.code();
        assert!(pair(&access, &code).unwrap_err().contains("moved away"));
        let left = std::fs::read_to_string(home.path().join("devices.json")).unwrap();
        assert_eq!(left, r#"{"who":"me"}"#);
    }
}
