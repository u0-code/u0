//! Sharing this Ghost with desks on other machines: listening on
//! 127.0.0.1 (`link`), a cloudflared tunnel that brings the world to it,
//! the links a device pairs with and the devices paired (`access`), and
//! keeping the Mac awake while it serves. What Settings › Sharing & access
//! shows and changes, from this machine's desk or a paired one.
//!
//! The tunnel is cloudflared, found on the PATH and run as a child: a
//! quick tunnel (no account; an address of its own each time it starts,
//! read from what it logs) or a named one of yours (its token, from
//! Cloudflare, and the address you gave it there).

use std::{
    process::Stdio,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt as _;

use crate::{
    access::{Device, Named},
    link::{self, PORT},
    proto::Event,
    server::Server,
};

/// What is asked of sharing.
#[derive(Debug, Serialize, Deserialize)]
pub enum Share {
    /// How it stands: answered, as each change is, with [`Event::Shared`].
    Look,
    /// Listen, or stop, now and each time Ghost starts.
    Serve(bool),
    /// Run the tunnel, or stop it, now and each time Ghost starts.
    Tunnel(bool),
    /// The named tunnel to run; none, a quick one.
    Named(Option<Named>),
    /// Keep the Mac awake while serving.
    Awake(bool),
    /// Make a pairing link.
    Pair,
    /// Unpair the device known by this public key.
    Revoke(String),
}

/// How sharing stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shared {
    /// Listening on 127.0.0.1 at this port, or why it could not; none,
    /// not asked to.
    pub serving: Option<Result<u16, String>>,
    pub tunnel: Tunnel,
    /// The named tunnel's address; none, a quick one.
    pub named: Option<String>,
    pub awake: bool,
    /// The devices paired, or why they do not read.
    pub devices: Result<Vec<Device>, String>,
    /// The pairing link last made, and when (seconds since 1970): good
    /// once, for ten minutes.
    pub link: Option<(String, u64)>,
}

/// How the tunnel stands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tunnel {
    #[default]
    Off,
    Starting,
    /// Reached the world at this address.
    Up(String),
    /// Stopped by itself, why.
    Down(String),
}

/// The process of cloudflared, once it has started.
type Pid = Arc<Mutex<Option<u32>>>;

/// What runs for sharing.
#[derive(Default)]
pub struct Sharing {
    listening: Option<Result<tokio::task::JoinHandle<()>, String>>,
    tunnel: Arc<Mutex<Tunnel>>,
    /// The task that holds cloudflared, and its process.
    cloudflared: Option<(tokio::task::JoinHandle<()>, Pid)>,
    caffeinate: Option<std::process::Child>,
    link: Option<(String, u64)>,
}

impl Drop for Sharing {
    fn drop(&mut self) {
        self.stop_tunnel();
        self.stop_listening();
    }
}

impl Sharing {
    fn stop_listening(&mut self) {
        if let Some(Ok(listening)) = self.listening.take() {
            listening.abort();
        }
        if let Some(mut c) = self.caffeinate.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn stop_tunnel(&mut self) {
        if let Some((task, pid)) = self.cloudflared.take() {
            task.abort();
            if let Some(pid) = *pid.lock().expect("pid") {
                // SAFETY: a signal to the process this one started.
                unsafe { libc::kill(pid.cast_signed(), libc::SIGTERM) };
            }
        }
        *self.tunnel.lock().expect("tunnel") = Tunnel::Off;
    }
}

impl Server {
    /// Do what `share` asks, and say how sharing stands.
    pub fn share(self: &Arc<Self>, share: Share) {
        let access = self.access();
        match share {
            Share::Look => {}
            Share::Serve(on) => {
                access.change(|k| k.serve = on);
                if on {
                    self.share_as_kept(true);
                } else {
                    let mut sharing = self.sharing();
                    sharing.stop_tunnel();
                    sharing.stop_listening();
                }
            }
            Share::Tunnel(on) => {
                access.change(|k| k.tunnel = on);
                if on {
                    self.tunnel();
                } else {
                    self.sharing().stop_tunnel();
                }
            }
            Share::Named(named) => access.change(|k| k.named = named),
            Share::Awake(on) => {
                access.change(|k| k.awake = on);
                self.stay_awake();
            }
            Share::Pair => {
                let host = match &*self.sharing().tunnel.lock().expect("tunnel") {
                    Tunnel::Up(host) => host.clone(),
                    _ => format!("127.0.0.1:{PORT}"),
                };
                let made = crate::store::secs(std::time::SystemTime::now());
                self.sharing().link = Some((link::pairing_link(&host, &access.code()), made));
            }
            Share::Revoke(key) => access.revoke(&key),
        }
        self.say_shared();
    }

    /// Share as Ghost was last left sharing; `serve`, listen whatever it
    /// was left at.
    pub fn share_as_kept(self: &Arc<Self>, serve: bool) {
        let kept = self.access().kept().unwrap_or_default();
        if (serve || kept.serve) && self.serve().is_ok() && kept.tunnel {
            self.tunnel();
        }
    }

    /// Stop sharing, as Ghost quits: cloudflared is not left behind.
    pub fn unshare(&self) {
        let mut sharing = self.sharing();
        sharing.stop_tunnel();
        sharing.stop_listening();
    }

    /// Listen on 127.0.0.1, unless it does already.
    pub fn serve(self: &Arc<Self>) -> Result<(), String> {
        let mut sharing = self.sharing();
        if let Some(Ok(_)) = sharing.listening {
            return Ok(());
        }
        let bound = std::net::TcpListener::bind(("127.0.0.1", PORT))
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
            .map_err(|e| format!("Ghost could not listen on 127.0.0.1:{PORT}: {e}"));
        let listening = bound.and_then(|l| {
            let _in_runtime = crate::acp::runtime().enter();
            tokio::net::TcpListener::from_std(l).map_err(|e| e.to_string())
        });
        sharing.listening =
            Some(listening.map(|l| crate::acp::runtime().spawn(link::listen(self.clone(), l))));
        let result = match &sharing.listening {
            Some(Err(why)) => Err(why.clone()),
            _ => Ok(()),
        };
        drop(sharing);
        self.stay_awake();
        result
    }

    /// Run cloudflared to this server's port, as kept: a named tunnel, or
    /// a quick one. How it stands is read from what it logs.
    fn tunnel(self: &Arc<Self>) {
        let mut sharing = self.sharing();
        sharing.stop_tunnel();
        let Some(cloudflared) = which::which("cloudflared").ok() else {
            *sharing.tunnel.lock().expect("tunnel") =
                Tunnel::Down("cloudflared is not installed: brew install cloudflared".into());
            return;
        };
        let named = self.access().kept().ok().and_then(|k| k.named);
        let mut command = tokio::process::Command::new(cloudflared);
        command.args(["tunnel", "--no-autoupdate"]);
        match &named {
            Some(n) => command.arg("run").env("TUNNEL_TOKEN", &n.token),
            None => command.args(["--url", &format!("http://127.0.0.1:{PORT}")]),
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let state = sharing.tunnel.clone();
        *state.lock().expect("tunnel") = Tunnel::Starting;
        let me = Arc::downgrade(self);
        let mut host = named.map(|n| n.host);
        let pid = Arc::new(Mutex::new(None));
        let its = pid.clone();
        let task = crate::acp::runtime().spawn(async move {
            let say = |now: Tunnel| {
                *state.lock().expect("tunnel") = now;
                if let Some(server) = me.upgrade() {
                    server.say_shared();
                }
            };
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(e) => return say(Tunnel::Down(format!("cloudflared did not start: {e}"))),
            };
            *its.lock().expect("pid") = child.id();
            let mut lines = tokio::io::BufReader::new(child.stderr.take().expect("piped")).lines();
            let mut last = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("cloudflared: {line}");
                if host.is_none() {
                    host = quick_host(&line);
                }
                if line.contains("Registered tunnel connection")
                    && let Some(host) = &host
                    && *state.lock().expect("tunnel") != Tunnel::Up(host.clone())
                {
                    say(Tunnel::Up(host.clone()));
                }
                if line.contains(" ERR ") {
                    last = line;
                }
            }
            let ended = child
                .wait()
                .await
                .map_or_else(|e| e.to_string(), |s| s.to_string());
            *its.lock().expect("pid") = None;
            say(Tunnel::Down(if last.is_empty() { ended } else { last }));
        });
        sharing.cloudflared = Some((task, pid));
    }

    /// Keep the Mac awake while serving, if asked to (macOS's
    /// `caffeinate`, while this process lives); else let it sleep.
    fn stay_awake(&self) {
        let awake = self.access().kept().is_ok_and(|k| k.awake);
        let mut sharing = self.sharing();
        let serving = matches!(sharing.listening, Some(Ok(_)));
        match (&sharing.caffeinate, awake && serving) {
            (None, true) => {
                sharing.caffeinate = which::which("caffeinate").ok().and_then(|c| {
                    std::process::Command::new(c)
                        .args(["-i", "-w", &std::process::id().to_string()])
                        .spawn()
                        .ok()
                });
            }
            (Some(_), false) => {
                if let Some(mut c) = sharing.caffeinate.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
            _ => {}
        }
    }

    /// Say how sharing stands.
    fn say_shared(&self) {
        let shared = {
            let sharing = self.sharing();
            let kept = self.access().kept();
            Shared {
                serving: (sharing.listening.as_ref())
                    .map(|l| l.as_ref().map(|_| PORT).map_err(Clone::clone)),
                tunnel: sharing.tunnel.lock().expect("tunnel").clone(),
                named: (kept.as_ref().ok())
                    .and_then(|k| k.named.as_ref())
                    .map(|n| n.host.clone()),
                awake: kept.as_ref().is_ok_and(|k| k.awake),
                devices: kept.map(|k| k.devices),
                // Only while its code is good: not once used, or past its
                // ten minutes.
                link: (sharing.link.clone()).filter(|(l, _)| {
                    link::pairing(l).is_some_and(|(_, code)| self.access().live(&code))
                }),
            }
        };
        self.say(Event::Shared(Box::new(shared)));
    }
}

/// The address a quick tunnel says it has, in a line of its log.
fn quick_host(line: &str) -> Option<String> {
    let at = line.find("https://")? + "https://".len();
    let host = line[at..]
        .split(|c: char| c.is_whitespace() || c == '|')
        .next()?;
    host.ends_with(".trycloudflare.com")
        .then(|| host.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quick_tunnel_is_known_by_the_address_it_logs() {
        let line =
            "2026-11-26T10:00:00Z INF |  https://calm-river-1.trycloudflare.com                |";
        assert_eq!(
            quick_host(line).as_deref(),
            Some("calm-river-1.trycloudflare.com")
        );
        let other = "INF Visit it at https://www.cloudflare.com/website-terms/";
        assert_eq!(quick_host(other), None);
    }
}
