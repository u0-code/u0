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
    /// Listen at this port on 127.0.0.1 (and the next on a network), now
    /// and each time u0 starts, where it is free.
    Port(u16),
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
    /// Install or uninstall the launchd background service on this Mac.
    Service(bool),
}

/// How sharing stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shared {
    /// Listening on 127.0.0.1 at this port, or why it could not; none,
    /// not asked to.
    pub serving: Option<Result<u16, String>>,
    /// The port kept, listened on or not.
    #[serde(default)]
    pub port: u16,
    pub tunnel: Tunnel,
    /// When the running tunnel came up (seconds since 1970).
    #[serde(default)]
    pub tunnel_since: Option<u64>,
    /// The named tunnel's address; none, a quick one.
    pub named: Option<String>,
    pub awake: bool,
    /// Whether the launchd background service (`u0 service`) is installed.
    #[serde(default)]
    pub service: bool,
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
    /// Whether this is the server of the window in this process, which
    /// may move from a port another program holds (`Server::serve`).
    pub(crate) window: bool,
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
            Share::Service(on) => {
                if let Err(why) = crate::cli::set_service(self.home(), on) {
                    self.say(Event::Notice(Err((
                        "Couldn't change the background service".into(),
                        why,
                    ))));
                }
            }
            Share::Port(port) => self.set_port(port),
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
    /// At the port kept; where another program holds it, the window's
    /// server with no named tunnel moves to a free one (`moved`). Where
    /// something may point at that port that u0 cannot follow (a named
    /// tunnel's origin, an `ssh -L`, a desk of a server with no window),
    /// it does not listen, and says why: whoever holds the port would be
    /// reached in its stead. Where another u0 holds it, it is left to it.
    pub fn serve(self: &Arc<Self>) -> Result<(), String> {
        let window = {
            let sharing = self.sharing();
            if sharing.port().is_some() {
                return Ok(());
            }
            sharing.window
        };
        let kept = self.access().kept().unwrap_or_default();
        let (port, named) = (kept.port, kept.named.is_some());
        let bound = crate::policy::allows(crate::policy::Limit::Remote)
            .and_then(|()| link::take(port, |port| held(port, window && !named, named), spare()));
        let mut sharing = self.sharing();
        if sharing.port().is_some() {
            return Ok(());
        }
        let mut moved = None;
        let listening = bound.and_then(|(l, was)| {
            let port = l.local_addr().map_err(|e| e.to_string())?.port();
            moved = was.map(|was| (was, port));
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
        match (&result, moved) {
            (Err(why), _) => {
                tracing::warn!("{why}");
                let what = "u0 does not listen for other devices".to_owned();
                self.say(Event::Notice(Err((what, why.clone()))));
            }
            (Ok(()), Some((was, now))) => self.moved(was, now),
            (Ok(()), None) => {}
        }
        if result.is_ok() {
            self.network();
        }
        self.stay_awake();
        result
    }

    /// Listen at `now` from now on, as another program holds `was`: kept
    /// (`keep_port`), and you told, as the devices paired at `was` no
    /// longer reach it.
    fn moved(&self, was: u16, now: u16) {
        self.keep_port(was, now);
        tracing::warn!("u0 moved from port {was}, taken, to {now}");
        self.say(Event::Notice(Err((
            format!("u0 listens on port {now} now"),
            format!("Another program holds port {was}. Devices paired with this Mac before reach it there: pair them again. Settings › Devices sets the port."),
        ))));
    }

    /// Keep `now` as the port, in place of `was`: in `devices.json`, and
    /// this Mac's service where the window finds it (`Servers::service`),
    /// so that the window does not run a second server on its home. Said
    /// where that moved, as a window holding it reaches it there once
    /// reopened.
    fn keep_port(&self, was: u16, now: u16) {
        self.access().change(|k| k.port = now);
        let home = self.home();
        if was == now
            || !crate::servers::Servers::load(home).is_ok_and(|s| s.service(was).is_some())
        {
            return;
        }
        let service = format!("127.0.0.1:{was}");
        let changed = crate::servers::Servers::change(home, |s| {
            for s in s.servers.iter_mut().filter(|s| s.host == service) {
                s.host = format!("127.0.0.1:{now}");
            }
            Ok(())
        });
        self.say(Event::Notice(match changed {
            Ok(()) => Ok(format!(
                "This Mac's service listens on port {now} now: reopen u0 to reach it there"
            )),
            Err(why) => Err(("servers.json was not changed".into(), why)),
        }));
    }

    /// Listen at `port` from now on, where it is free (and the port after
    /// it, on a network): now, where u0 listens, its tunnel and network
    /// with it.
    fn set_port(self: &Arc<Self>, port: u16) {
        let (now, network) = {
            let sharing = self.sharing();
            let network = match &sharing.network {
                Some(Ok((_, at, _))) => Some(*at),
                _ => None,
            };
            (sharing.port(), network)
        };
        let Ok(kept) = self.access().kept() else {
            return;
        };
        let free = |p: u16| {
            std::net::TcpListener::bind(("127.0.0.1", p))
                .map(drop)
                .map_err(|e| format!("127.0.0.1:{p}: {e}"))
        };
        let checked = (1024..=u16::MAX)
            .contains(&port)
            .then_some(())
            .ok_or_else(|| "A port is a number from 1024 to 65535".to_owned())
            .and_then(|()| {
                if now == Some(port) {
                    Ok(())
                } else {
                    free(port)
                }
            })
            .and_then(|()| match (kept.bind, port.checked_add(1)) {
                (Bind::Here, _) => Ok(()),
                (_, None) => Err("No port follows 65535 for the network".to_owned()),
                (_, Some(next)) if network == Some(next) => Ok(()),
                (_, Some(next)) => free(next),
            });
        if let Err(why) = checked {
            let what = format!("u0 cannot listen on port {port}");
            return self.say(Event::Notice(Err((what, why))));
        }
        self.keep_port(kept.port, port);
        let mut sharing = self.sharing();
        if sharing.listening.is_some() && sharing.port() != Some(port) {
            sharing.stop_tunnel();
            sharing.stop_listening();
            drop(sharing);
            self.share_as_kept(true);
        }
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
        let next = (port == 0).then_some(0).or(port.checked_add(1));
        let listening = ip.and_then(|ip| {
            let port = next.ok_or("No port follows 65535: give u0 another port")?;
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
                port: kept.as_ref().map_or(0, |k| k.port),
                routes,
                serving: (sharing.listening.as_ref())
                    .map(|l| l.as_ref().map(|(_, port)| *port).map_err(Clone::clone)),
                tunnel: sharing.tunnel.lock().expect("tunnel").clone(),
                tunnel_since: *sharing.tunnel_since.lock().expect("tunnel_since"),
                named: (kept.as_ref().ok())
                    .and_then(|k| k.named.as_ref())
                    .map(|n| n.host.clone()),
                awake: kept.as_ref().is_ok_and(|k| k.awake),
                service: crate::cli::service_installed(self.home()),
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

/// Why the port another program holds is left to it, unless u0 `moves`
/// from it: another u0 listens there; or something may point at it that
/// u0 cannot follow, a `named` tunnel's origin among them.
fn held(port: u16, moves: bool, named: bool) -> Option<String> {
    if link::speaks_u0(port) {
        return Some(format!(
            "Another u0 on this Mac listens on 127.0.0.1:{port}: one with a home of its own, or its service. Give this one another port, or stop that one."
        ));
    }
    let then = if named {
        " Free it, or give u0 another port under Settings › Devices and point your named tunnel there."
    } else if moves {
        return None;
    } else {
        " Free it, or give u0 another port: Settings › Devices, or `port` in devices.json."
    };
    Some(format!(
        "Another program holds 127.0.0.1:{port}, where u0 listens for other devices: what reaches that port would reach it, not u0.{then}"
    ))
}

/// The ports the window's server moves to, where the one kept is taken:
/// up from 4317, then from 20000, below the ports the system lends out
/// for a while; each with the port after it free, for a network.
fn spare() -> impl Iterator<Item = u16> {
    let free = |p: u16| std::net::TcpListener::bind(("127.0.0.1", p)).is_ok();
    (4317..5000)
        .chain(20000..30000)
        .filter(move |p| free(p + 1))
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

    /// A server of `home` kept at `port`, the window's or one with none,
    /// and what it says: how sharing stands, and its notices.
    fn at(
        home: &std::path::Path,
        port: u16,
        window: bool,
    ) -> (Arc<Server>, std::sync::mpsc::Receiver<Event>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let send = move |e| {
            if matches!(e, Event::Shared(_) | Event::Notice(_)) {
                let _ = tx.lock().expect("tx").send(e);
            }
        };
        let home = home.to_path_buf();
        let server = if window {
            Server::in_window(home, send)
        } else {
            Server::new(home, send)
        };
        server.access().change(|k| k.port = port);
        (server, rx)
    }

    /// What it says next, within a few seconds.
    fn next(heard: &std::sync::mpsc::Receiver<Event>) -> Event {
        heard
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("said in time")
    }

    /// The port it listens on, as it says next; or why not; and what its
    /// notices said before.
    fn serving(heard: &std::sync::mpsc::Receiver<Event>) -> (Result<u16, String>, String) {
        let mut said = Vec::new();
        loop {
            match next(heard) {
                Event::Notice(Err((what, why))) => said.push(format!("{what}: {why}")),
                Event::Notice(Ok(what)) => said.push(what),
                Event::Shared(s) => return (s.serving.expect("asked to listen"), said.join("\n")),
                _ => {}
            }
        }
    }

    /// A port another program holds now, and that program.
    fn held() -> (u16, std::net::TcpListener) {
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        (held.local_addr().unwrap().port(), held)
    }

    /// Keep in `home` this Mac's service at `port`, and another server.
    fn service_at(home: &std::path::Path, port: u16) {
        let servers = format!(
            r#"{{"servers": [{{"name": "This Mac's service", "host": "127.0.0.1:{port}", "key": "ssh-ed25519 A"}}, {{"name": "box", "host": "box.example:{port}", "key": "ssh-ed25519 B"}}], "control": true}}"#
        );
        std::fs::write(home.join("servers.json"), servers).unwrap();
    }

    /// Where `home` keeps each server, by name.
    fn hosts(home: &std::path::Path) -> Vec<String> {
        let servers = crate::servers::Servers::load(home).unwrap().servers;
        servers.into_iter().map(|s| s.host).collect()
    }

    #[test]
    fn the_windows_server_moves_from_a_port_another_program_holds_keeps_it_and_says_so() {
        let home = tempfile::tempdir().unwrap();
        let (was, _held) = held();
        service_at(home.path(), was);
        let (server, heard) = at(home.path(), was, true);
        server.share(Share::Serve(true));
        let (now, said) = serving(&heard);
        let now = now.unwrap();
        assert!((4317..5000).chain(20000..30000).any(|p| p == now), "{now}");
        assert_eq!(server.access().kept().unwrap().port, now, "kept");
        assert!(said.contains(&format!("port {now} now")), "{said}");
        assert!(
            said.contains(&was.to_string()) && said.contains("pair them again"),
            "{said}"
        );
        assert_eq!(
            hosts(home.path()),
            [format!("127.0.0.1:{now}"), format!("box.example:{was}")],
            "the service moved with it, and nothing else"
        );
        server.share(Share::Serve(false));
        drop(server);
        // Free once its listener is let go of.
        while std::net::TcpListener::bind(("127.0.0.1", now)).is_err() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let (again, heard) = at(home.path(), now, true);
        again.share(Share::Serve(true));
        assert_eq!(serving(&heard).0, Ok(now), "the port kept, taken again");
        again.share(Share::Serve(false));
    }

    #[test]
    fn a_server_with_no_window_leaves_a_port_another_program_holds_and_says_why() {
        let home = tempfile::tempdir().unwrap();
        let (port, _held) = held();
        let (server, heard) = at(home.path(), port, false);
        server.share(Share::Serve(true));
        let (why, said) = serving(&heard);
        let why = why.unwrap_err();
        assert!(
            why.contains("Another program holds") && why.contains(&port.to_string()),
            "{why}"
        );
        assert!(said.contains("does not listen"), "{said}");
        assert_eq!(server.access().kept().unwrap().port, port, "not moved");
    }

    #[test]
    fn a_named_tunnel_keeps_the_window_from_moving_off_its_port() {
        let home = tempfile::tempdir().unwrap();
        let (port, _held) = held();
        let (server, heard) = at(home.path(), port, true);
        server.access().change(|k| {
            k.named = Some(Named {
                token: "t".into(),
                host: "u0.example.com".into(),
            });
        });
        server.share(Share::Serve(true));
        let why = serving(&heard).0.unwrap_err();
        assert!(why.contains("named tunnel"), "{why}");
        assert_eq!(server.access().kept().unwrap().port, port, "not moved");
        assert_eq!(*server.sharing().tunnel.lock().unwrap(), Tunnel::Off);
    }

    #[test]
    fn a_port_another_u0_holds_is_left_to_it() {
        let (one, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (first, heard) = at(one.path(), 0, true);
        first.share(Share::Serve(true));
        let port = serving(&heard).0.unwrap();
        let (second, heard) = at(other.path(), port, true);
        second.share(Share::Serve(true));
        let why = serving(&heard).0.unwrap_err();
        assert!(why.contains("Another u0"), "{why}");
        assert_eq!(second.access().kept().unwrap().port, port, "not moved");
        first.share(Share::Serve(false));
    }

    #[test]
    fn a_port_chosen_is_listened_on_at_once_with_the_service_and_one_taken_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let (server, heard) = at(home.path(), 0, true);
        server.share(Share::Serve(true));
        serving(&heard).0.unwrap();
        server.access().change(|k| k.port = 4317);
        service_at(home.path(), 4317);
        let chosen = held().0;
        server.share(Share::Port(chosen));
        let (now, said) = serving(&heard);
        assert_eq!(now, Ok(chosen));
        assert!(said.contains("reopen u0"), "{said}");
        assert_eq!(hosts(home.path())[0], format!("127.0.0.1:{chosen}"));
        let (taken, _held) = held();
        server.share(Share::Port(taken));
        assert!(matches!(next(&heard), Event::Notice(Err(_))));
        assert_eq!(serving(&heard).0, Ok(chosen), "still where it was");
        assert_eq!(server.access().kept().unwrap().port, chosen);
        server.share(Share::Port(80));
        assert!(matches!(next(&heard), Event::Notice(Err(_))));
        assert_eq!(serving(&heard).0, Ok(chosen));
        server.share(Share::Bind(Bind::Tailscale));
        assert_eq!(serving(&heard).0, Ok(chosen));
        server.share(Share::Port(u16::MAX));
        assert!(
            matches!(next(&heard), Event::Notice(Err(_))),
            "no port after it"
        );
        server.share(Share::Serve(false));
    }
}
