//! Sharing this u0 with desks on other machines: listening on
//! 127.0.0.1 (`link`), a cloudflared tunnel that brings the world to it,
//! the links a device pairs with and the devices paired (`access`), and
//! keeping the Mac awake while it serves. What Settings › Devices shows
//! and changes under "Let other devices control this Mac", from this
//! machine's desk or a paired one; and the link a desk pairing elsewhere
//! gives for that server to pair back with.
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
    access::{Device, Grant, Named},
    link,
    proto::Event,
    reach::{self, Bind, Route, Way},
    server::Server,
};

/// What is asked of sharing.
#[derive(Debug, Serialize, Deserialize)]
pub enum Share {
    /// How it stands: answered, as each change is, with [`Event::Shared`].
    Look,
    /// Listen, now and each time u0 starts; or stop, cutting off each
    /// device linked.
    Serve(bool),
    /// A new pairing link, for a device as the grant says, naming the
    /// route given (none: the tunnel once it is up, else the first there
    /// is of Tailscale, the local network and 127.0.0.1).
    Link(Grant, Option<Route>),
    /// Listen on a network too, or no longer, now and each time u0
    /// starts.
    Bind(Bind),
    /// Run the tunnel, or stop it, now and each time u0 starts.
    Tunnel(bool),
    /// The named tunnel to run; none, a quick one.
    Named(Option<Named>),
    /// Keep the Mac awake while serving.
    Awake(bool),
    /// Unpair the device known by this public key, or turn it away while
    /// it waits.
    Revoke(String),
    /// Let in the device known by this public key, which waits.
    Approve(String),
    /// Withdraw any unused pairing links without stopping listening.
    Withdraw,
    /// Set the icon this machine shows to other devices.
    Icon(crate::access::MachineIcon),
}

/// How sharing stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shared {
    /// Listening on 127.0.0.1 at this port, or why it could not; none,
    /// not asked to.
    pub serving: Option<Result<u16, String>>,
    pub tunnel: Tunnel,
    /// When the running tunnel came up (seconds since 1970).
    #[serde(default)]
    pub tunnel_since: Option<u64>,
    /// The named tunnel's address; none, a quick one.
    pub named: Option<String>,
    pub awake: bool,
    /// The icon this machine shows.
    #[serde(default)]
    pub icon: crate::access::MachineIcon,
    /// The devices paired, or why they do not read.
    pub devices: Result<Vec<Device>, String>,
    /// Public keys of paired devices with an active desk link right now.
    #[serde(default)]
    pub linked: Vec<String>,
    /// When the pairing link last made was made (seconds since 1970), and
    /// what it lets in, while it is still good (once, for ten minutes).
    /// Never the link itself: that is told only to whoever made it
    /// (`Server::offered`), so that no other device can pair with it.
    pub offer: Option<(u64, Grant)>,
    /// Where it listens beyond 127.0.0.1.
    pub bind: Bind,
    /// The routes a pairing link can name, each with the host it gives;
    /// or why it could not listen on a network.
    pub routes: Result<Vec<(Route, String)>, String>,
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

/// Listening on a network: the task, the port and the pin; or why not.
type Listening = Result<(tokio::task::JoinHandle<()>, u16, String), String>;

/// The process of cloudflared, once it has started.
type Pid = Arc<Mutex<Option<u32>>>;

/// What runs for sharing.
#[derive(Default)]
pub struct Sharing {
    /// Listening, and on what port; or why not.
    listening: Option<Result<(tokio::task::JoinHandle<()>, u16), String>>,
    /// Listening on a network too, with TLS: on what port, and the pin of
    /// its certificate; or why not.
    network: Option<Listening>,
    tunnel: Arc<Mutex<Tunnel>>,
    tunnel_since: Arc<Mutex<Option<u64>>>,
    /// The task that holds cloudflared, and its process.
    cloudflared: Option<(tokio::task::JoinHandle<()>, Pid)>,
    caffeinate: Option<std::process::Child>,
    link: Option<(String, u64, Grant)>,
    linked: Mutex<Vec<String>>,
}

impl Drop for Sharing {
    fn drop(&mut self) {
        self.stop_tunnel();
        self.stop_listening();
    }
}

impl Sharing {
    fn stop_listening(&mut self) {
        if let Some(Ok((listening, _))) = self.listening.take() {
            listening.abort();
        }
        self.stop_network();
        self.link = None;
        if let Some(mut c) = self.caffeinate.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn stop_network(&mut self) {
        if let Some(Ok((listening, ..))) = self.network.take() {
            listening.abort();
        }
    }

    /// The port listened on, if it is.
    fn port(&self) -> Option<u16> {
        match self.listening {
            Some(Ok((_, port))) => Some(port),
            _ => None,
        }
    }

    fn stop_tunnel(&mut self) {
        if let Some((task, pid)) = self.cloudflared.take() {
            task.abort();
            if let Some(pid) = *pid.lock().expect("pid") {
                crate::diagnostics::terminate(pid);
            }
        }
        *self.tunnel.lock().expect("tunnel") = Tunnel::Off;
        *self.tunnel_since.lock().expect("tunnel_since") = None;
    }
}

impl Server {
    /// Record that a desk with `key` opened or closed a link.
    pub(crate) fn set_desk_linked(&self, key: &str, up: bool) {
        let sharing = self.sharing();
        let mut linked = sharing.linked.lock().expect("linked");
        if up {
            linked.push(key.to_owned());
        } else if let Some(at) = linked.iter().position(|k| k == key) {
            linked.remove(at);
        }
    }

    /// Do what `share` asks, and say how sharing stands.
    pub fn share(self: &Arc<Self>, share: Share) {
        let access = self.access();
        match share {
            Share::Look => {}
            Share::Serve(on) => {
                access.change(|k| k.serve = on);
                if on {
                    if self.sharing().port().is_none() {
                        self.share_as_kept(true);
                    }
                } else {
                    let mut sharing = self.sharing();
                    sharing.stop_tunnel();
                    sharing.stop_listening();
                    access.withdraw();
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
            Share::Revoke(key) => access.revoke(&key),
            Share::Approve(key) => access.approve(&key),
            Share::Withdraw => {
                access.withdraw();
                self.sharing().link = None;
            }
            Share::Icon(icon) => access.change(|k| k.icon = icon),
            Share::Link(grant, route) => {
                self.offer(grant, route);
            }
            Share::Bind(bind) => {
                access.change(|k| k.bind = bind);
                self.sharing().stop_network();
                if self.sharing().port().is_some() {
                    self.network();
                }
            }
        }
        self.say_shared();
    }

    /// Offer a new pairing link letting in what `grant` says, naming
    /// `route` (none: the tunnel once it is up, else Tailscale, the local
    /// network, 127.0.0.1, the first there is); none where u0 does not
    /// listen, or not that way.
    pub fn offer(&self, grant: Grant, route: Option<Route>) -> Option<String> {
        let ways = self.ways();
        let way = match route {
            Some(route) => ways.into_iter().find(|(r, _)| *r == route),
            None => ways.into_iter().max_by_key(|(r, _)| match r {
                Route::Here => 0,
                Route::Lan => 1,
                Route::Tailscale => 2,
                Route::Tunnel => 3,
            }),
        };
        let (_, way) = way?;
        let link = reach::pairing_link(&way, &self.access().code(grant));
        let made = crate::store::secs(std::time::SystemTime::now());
        self.sharing().link = Some((link.clone(), made, grant));
        Some(link)
    }

    /// The pairing link last made, when, and what it lets in: for the
    /// desk that asked for it alone, never said to every link.
    pub fn offered(&self) -> Option<(String, u64, Grant)> {
        self.sharing().link.clone()
    }

    /// Each way this u0 is reached, as it listens now: 127.0.0.1, each
    /// address on a network it listens on (its certificate pinned), the
    /// tunnel once it is up.
    pub(crate) fn ways(&self) -> Vec<(Route, Way)> {
        let sharing = self.sharing();
        let Some(port) = sharing.port() else {
            return Vec::new();
        };
        let here = Way {
            host: format!("127.0.0.1:{port}"),
            pin: None,
        };
        let mut ways = vec![(Route::Here, here)];
        if let Some(Ok((_, port, pin))) = &sharing.network {
            let bind = self.access().kept().map(|k| k.bind).unwrap_or_default();
            ways.extend(
                (reach::addresses().into_iter())
                    .filter(|(r, _)| bind == Bind::Network || *r == Route::Tailscale)
                    .map(|(r, ip)| {
                        let host = std::net::SocketAddr::new(ip, *port).to_string();
                        let pin = Some(pin.clone());
                        (r, Way { host, pin })
                    }),
            );
        }
        if let Tunnel::Up(host) = &*sharing.tunnel.lock().expect("tunnel") {
            let host = host.clone();
            ways.push((Route::Tunnel, Way { host, pin: None }));
        }
        ways
    }

    /// A pairing link for the server at `toward`, which this Mac's desk is
    /// pairing with, to pair back with this one by: reached as `toward`
    /// is: at 127.0.0.1, on the network it is reached on, where this Mac
    /// listens there too, else through this Mac's own tunnel. It listens
    /// from now on; or why there is no way back.
    pub fn back_link(self: &Arc<Self>, toward: &Way) -> Result<String, String> {
        if self.sharing().port().is_none() {
            self.access().change(|k| k.serve = true);
            self.share_as_kept(true);
            self.say_shared();
        }
        if let Some(Err(why)) = &self.sharing().listening {
            return Err(why.clone());
        }
        let ways = self.ways();
        let on = |route| ways.iter().find(|(r, _)| *r == route).map(|(_, w)| w);
        let way = if toward.url().starts_with("ws:") {
            on(Route::Here)
        } else if toward.pin.is_some() {
            let theirs = (toward.host.parse::<std::net::SocketAddr>().ok())
                .and_then(|at| reach::route_of(at.ip()));
            theirs.and_then(on).or_else(|| on(Route::Tunnel))
        } else {
            on(Route::Tunnel)
        };
        let way = way.ok_or_else(|| format!(
            "{} can reach this Mac only through a tunnel of this Mac's own, or on a network this Mac listens on, and neither is up. Start one in Settings › Devices, then pair again.",
            toward.host
        ))?;
        Ok(reach::pairing_link(
            way,
            &self.access().code(Grant::default()),
        ))
    }

    /// Pair back with the desk that gave `back` as it paired: a key made
    /// for it, and it kept as a Mac this one can open. The key's public
    /// half, or why not.
    pub async fn pair_back(&self, back: &str) -> Result<String, String> {
        let (way, code) = reach::pairing(back).ok_or("Its link to pair back with does not read")?;
        let key = crate::key::Key::generate();
        let public = key.public();
        crate::servers::pair(self.home(), way, code, key, None).await?;
        self.say_shared();
        Ok(public)
    }

    /// Share as u0 was last left sharing; `serve`, listen whatever it
    /// was left at.
    pub fn share_as_kept(self: &Arc<Self>, serve: bool) {
        let kept = self.access().kept().unwrap_or_default();
        if (serve || kept.serve) && self.serve().is_ok() && kept.tunnel {
            self.tunnel();
        }
    }

    /// Stop sharing, as u0 quits: cloudflared is not left behind.
    pub fn unshare(&self) {
        let mut sharing = self.sharing();
        sharing.stop_tunnel();
        sharing.stop_listening();
    }

    /// Listen on 127.0.0.1, unless it does already, or the policy forbids.
    pub fn serve(self: &Arc<Self>) -> Result<(), String> {
        let mut sharing = self.sharing();
        if sharing.port().is_some() {
            return Ok(());
        }
        let port = self.access().kept().map_or(4317, |k| k.port);
        let bound = crate::policy::allows(crate::policy::Limit::Remote).and_then(|()| {
            std::net::TcpListener::bind(("127.0.0.1", port))
                .and_then(|l| l.set_nonblocking(true).map(|()| l))
                .map_err(|e| format!("u0 could not listen on 127.0.0.1:{port}: {e}"))
        });
        let listening = bound.and_then(|l| {
            let port = l.local_addr().map_err(|e| e.to_string())?.port();
            let _in_runtime = crate::acp::runtime().enter();
            let l = tokio::net::TcpListener::from_std(l).map_err(|e| e.to_string())?;
            Ok((
                crate::acp::runtime().spawn(link::listen(self.clone(), l, None)),
                port,
            ))
        });
        sharing.listening = Some(listening);
        let result = match &sharing.listening {
            Some(Err(why)) => Err(why.clone()),
            _ => Ok(()),
        };
        drop(sharing);
        if result.is_ok() {
            self.network();
        }
        self.stay_awake();
        result
    }

    /// Listen on the networks kept (`Bind`), with TLS, on the port after
    /// this one, unless it does already or is to listen on none.
    fn network(self: &Arc<Self>) {
        let kept = self.access().kept().unwrap_or_default();
        let ip = match kept.bind {
            Bind::Here => return,
            Bind::Network => Ok(std::net::IpAddr::from([0, 0, 0, 0])),
            Bind::Tailscale => (reach::addresses().into_iter())
                .find(|(r, _)| *r == Route::Tailscale)
                .map(|(_, ip)| ip)
                .ok_or_else(|| "This Mac is not on Tailscale".to_owned()),
        };
        let mut sharing = self.sharing();
        if matches!(sharing.network, Some(Ok(_))) {
            return;
        }
        let Some(port) = sharing.port() else { return };
        let port = if port == 0 { 0 } else { port + 1 };
        let listening = ip.and_then(|ip| {
            let (tls, pin) = reach::acceptor()?;
            let l = std::net::TcpListener::bind((ip, port))
                .and_then(|l| l.set_nonblocking(true).map(|()| l))
                .map_err(|e| format!("u0 could not listen on {ip}:{port}: {e}"))?;
            let port = l.local_addr().map_err(|e| e.to_string())?.port();
            let _in_runtime = crate::acp::runtime().enter();
            let l = tokio::net::TcpListener::from_std(l).map_err(|e| e.to_string())?;
            let task = crate::acp::runtime().spawn(link::listen(self.clone(), l, Some(tls)));
            Ok((task, port, pin))
        });
        sharing.network = Some(listening);
    }

    /// Run cloudflared to this server's port, as kept: a named tunnel, or
    /// a quick one. How it stands is read from what it logs.
    fn tunnel(self: &Arc<Self>) {
        let mut sharing = self.sharing();
        sharing.stop_tunnel();
        let Some(port) = sharing.port() else { return };
        if let Err(why) = crate::policy::allows(crate::policy::Limit::Tunnel) {
            *sharing.tunnel.lock().expect("tunnel") = Tunnel::Down(why);
            return;
        }
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
            None => command.args(["--url", &format!("http://127.0.0.1:{port}")]),
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let state = sharing.tunnel.clone();
        let since = sharing.tunnel_since.clone();
        *state.lock().expect("tunnel") = Tunnel::Starting;
        *since.lock().expect("tunnel_since") = None;
        let me = Arc::downgrade(self);
        let mut host = named.map(|n| n.host);
        let pid = Arc::new(Mutex::new(None));
        let its = pid.clone();
        let task = crate::acp::runtime().spawn(async move {
            let say = |now: Tunnel| {
                let up = matches!(now, Tunnel::Up(_));
                *state.lock().expect("tunnel") = now;
                *since.lock().expect("tunnel_since") =
                    up.then(|| crate::store::secs(std::time::SystemTime::now()));
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
        let serving = sharing.port().is_some();
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
        let ways = self.ways();
        let shared = {
            let sharing = self.sharing();
            let kept = self.access().kept();
            let routes = match &sharing.network {
                Some(Err(why)) => Err(why.clone()),
                _ => Ok((ways.into_iter()).map(|(r, w)| (r, w.host)).collect()),
            };
            let mut linked = sharing.linked.lock().expect("linked").clone();
            linked.sort();
            linked.dedup();
            Shared {
                bind: kept.as_ref().map(|k| k.bind).unwrap_or_default(),
                routes,
                serving: (sharing.listening.as_ref())
                    .map(|l| l.as_ref().map(|(_, port)| *port).map_err(Clone::clone)),
                tunnel: sharing.tunnel.lock().expect("tunnel").clone(),
                tunnel_since: *sharing.tunnel_since.lock().expect("tunnel_since"),
                named: (kept.as_ref().ok())
                    .and_then(|k| k.named.as_ref())
                    .map(|n| n.host.clone()),
                awake: kept.as_ref().is_ok_and(|k| k.awake),
                icon: kept.as_ref().map(|k| k.icon).unwrap_or_default(),
                devices: kept.map(|k| k.devices),
                linked,
                // Only while its code is good: not once used, or past its
                // ten minutes.
                offer: (sharing.link.as_ref())
                    .filter(|(l, _, _)| {
                        reach::pairing(l).is_some_and(|(_, c)| self.access().live(&c).is_some())
                    })
                    .map(|(_, made, grant)| (*made, *grant)),
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
