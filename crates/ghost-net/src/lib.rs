//! # ghost-net
//!
//! How every HTTP request a Ghost program makes leaves the machine, decided
//! in one place.
//!
//! The same ghost runs on a laptop behind a local proxy and on a server with
//! a direct line out, and the two need different things. On the owner's Mac
//! the proxy the shell exports stalls long model streams, and the cure was
//! to list the model's host in `NO_PROXY` by hand before every run. So no
//! crate builds an HTTP client of its own: each asks here for a [`builder`],
//! adds its own timeouts, and follows the same rule as every other.
//!
//! The rule is one setting in two parts:
//!
//! - `proxy`: `system`, `none`, or the address of an HTTP proxy such as
//!   `http://127.0.0.1:7890`. `system` is the default and what happened
//!   before this crate: the `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and
//!   `NO_PROXY` variables, in either case.
//! - `direct`: hosts that never go through the proxy. `api.deepseek.com` is
//!   that host alone; `*.deepseek.com` is `deepseek.com` and every name
//!   under it; `*` is everything. This machine's own addresses never go
//!   through a proxy, listed or not: a proxy elsewhere cannot reach them.
//!
//! Each part is taken from the environment where it is set there
//! (`GHOST_PROXY`, and `GHOST_DIRECT` as a comma-separated list; the ignored
//! `.env` can hold both), otherwise from `[network]` in `ghostd.toml`,
//! otherwise the default. The presences have no `ghostd.toml`, and read the
//! environment alone.
//!
//! The programs she starts (her shell, the command-line agents and brains,
//! and her servers of tools) are started by [`spawn`], which gives them the
//! same rule in the variables programs of every kind read, and stops each
//! with everything it started.
//!
//! Nothing here changes route by itself. A model API and a platform follow
//! the setting and nothing else, because a request that silently took
//! another way out would be a request nobody can account for. A caller that
//! wants to try the other way when one fails, as her web tools do, asks for
//! a client that always goes one [`Route`] and decides for itself.

mod program;

use std::{
    fmt,
    net::IpAddr,
    sync::{Arc, OnceLock},
};

pub use program::{Child, spawn};

pub use reqwest::Url;
use reqwest::{ClientBuilder, Proxy};
use serde::Deserialize;

/// The variable that sets `proxy`, over anything in `ghostd.toml`.
pub const PROXY_VAR: &str = "GHOST_PROXY";
/// The variable that sets `direct`, comma-separated, over `ghostd.toml`.
pub const DIRECT_VAR: &str = "GHOST_DIRECT";

/// `[network]` in `ghostd.toml`, as written. A part left out is the
/// default, unless the environment sets it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Setting {
    /// `system`, `none`, or a proxy's address.
    pub proxy: Option<String>,
    /// Hosts, and `*.suffix` patterns, that never use the proxy.
    pub direct: Option<Vec<String>>,
}

/// One of the two ways out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    /// Straight to the host.
    Direct,
    /// Through the configured proxy.
    Proxy,
}

impl Route {
    /// The way that is not this one.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::Direct => Self::Proxy,
            Self::Proxy => Self::Direct,
        }
    }
}

/// A setting that cannot be used. It never holds a proxy's password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// The rule, resolved: which proxy each kind of address goes through, and
/// which hosts go direct. Cheap to clone.
#[derive(Clone)]
pub struct Network(Arc<Rule>);

struct Rule {
    /// Whether the proxies were read from the standard variables.
    system: bool,
    /// For `http://` addresses.
    http: Option<Url>,
    /// For `https://` and `wss://` addresses.
    https: Option<Url>,
    /// Lower case: a host, `.suffix` for a name and everything under it,
    /// or `*`.
    direct: Vec<String>,
}

impl Network {
    /// The rule from `setting`, with each part the environment sets taking
    /// its place. `env` reads a variable; it is the process's environment
    /// except in tests.
    pub fn new(setting: &Setting, env: impl Fn(&str) -> Option<String>) -> Result<Self, Error> {
        let var = |name: &str| {
            env(name)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let proxy = var(PROXY_VAR)
            .or_else(|| setting.proxy.clone())
            .unwrap_or_default();
        let mut direct: Vec<String> = match var(DIRECT_VAR) {
            Some(list) => list.split(',').filter_map(pattern).collect(),
            None => setting
                .direct
                .iter()
                .flatten()
                .filter_map(pattern)
                .collect(),
        };
        let (system, http, https) = match proxy.trim().to_ascii_lowercase().as_str() {
            "" | "system" => {
                let first = |names: [&str; 2]| names.into_iter().find_map(var);
                let all = first(["ALL_PROXY", "all_proxy"]);
                let https = first(["HTTPS_PROXY", "https_proxy"]).or_else(|| all.clone());
                let http = first(["HTTP_PROXY", "http_proxy"]).or(all);
                if let Some(list) = first(["NO_PROXY", "no_proxy"]) {
                    direct.extend(list.split(',').filter_map(pattern));
                }
                (true, from_system(http), from_system(https))
            }
            "none" => (false, None, None),
            _ => {
                let url = proxy_url(&proxy)?;
                (false, Some(url.clone()), Some(url))
            }
        };
        Ok(Self(Arc::new(Rule {
            system,
            http,
            https,
            direct,
        })))
    }

    /// The rule from `setting` and this process's environment.
    pub fn from_env(setting: &Setting) -> Result<Self, Error> {
        Self::new(setting, |name| std::env::var(name).ok())
    }

    /// Whether anything goes through a proxy at all.
    pub fn has_proxy(&self) -> bool {
        self.0.http.is_some() || self.0.https.is_some()
    }

    /// The way the rule sends a request to `url`.
    pub fn route(&self, url: &Url) -> Route {
        match (self.proxy_for(url), url.host_str()) {
            (Some(_), Some(host)) if !self.goes_direct(host) => Route::Proxy,
            _ => Route::Direct,
        }
    }

    /// Whether a request to `url` could go either way: there is a proxy
    /// for it, and it is not on this machine.
    pub fn either_way(&self, url: &Url) -> bool {
        self.proxy_for(url).is_some() && url.host_str().is_some_and(|h| !on_this_machine(h))
    }

    /// A client builder that follows the rule. Add timeouts and the rest to
    /// it; leave the proxy alone.
    pub fn builder(&self) -> ClientBuilder {
        if !self.has_proxy() {
            return reqwest::Client::builder().no_proxy();
        }
        let rule = self.clone();
        reqwest::Client::builder().proxy(Proxy::custom(move |url| match rule.route(url) {
            Route::Proxy => rule.proxy_for(url).cloned(),
            Route::Direct => None,
        }))
    }

    /// A client builder that always goes `route`, whatever the rule says of
    /// a host, for a caller that chooses the route itself. Going by the
    /// proxy where there is none for an address goes direct.
    pub fn builder_by(&self, route: Route) -> ClientBuilder {
        match route {
            Route::Direct => reqwest::Client::builder().no_proxy(),
            Route::Proxy => {
                let rule = self.clone();
                reqwest::Client::builder()
                    .proxy(Proxy::custom(move |url| rule.proxy_for(url).cloned()))
            }
        }
    }

    /// The variables that give a program started from here the same rule:
    /// each with its value, or `None` to remove it. Both cases of each
    /// standard name, because programs disagree on which they read.
    ///
    /// `GHOST_PROXY` and `GHOST_DIRECT` are removed: a program of ours
    /// started with neither reads the standard variables, which say the
    /// same thing, and a `system` it resolved again could not differ.
    pub(crate) fn child_env(&self) -> Vec<(&'static str, Option<String>)> {
        let written = |url: &Option<Url>| url.as_ref().map(|u| u.as_str().to_owned());
        let mut direct = vec!["localhost".to_owned(), "127.0.0.1".into(), "::1".into()];
        direct.extend(self.0.direct.iter().cloned());
        let mut env = Vec::new();
        for (names, value) in [
            (["HTTPS_PROXY", "https_proxy"], written(&self.0.https)),
            (["HTTP_PROXY", "http_proxy"], written(&self.0.http)),
            (["ALL_PROXY", "all_proxy"], None),
            (["NO_PROXY", "no_proxy"], Some(direct.join(","))),
            ([PROXY_VAR, DIRECT_VAR], None),
        ] {
            for name in names {
                env.push((name, value.clone()));
            }
        }
        env
    }

    fn proxy_for(&self, url: &Url) -> Option<&Url> {
        match url.scheme() {
            "https" | "wss" => self.0.https.as_ref(),
            _ => self.0.http.as_ref(),
        }
    }

    fn goes_direct(&self, host: &str) -> bool {
        let host = bare(host);
        on_this_machine(&host)
            || self.0.direct.iter().any(|p| {
                p == "*"
                    || *p == host
                    || p.strip_prefix('.')
                        .is_some_and(|apex| host == apex || host.ends_with(p.as_str()))
            })
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = &self.0;
        match (&rule.http, &rule.https) {
            (None, None) if rule.system => f.write_str("no proxy (none set in the environment)")?,
            (None, None) => f.write_str("no proxy")?,
            (http, https) if http == https => {
                write!(f, "proxy {}", shown(http.as_ref()))?;
            }
            (http, https) => write!(
                f,
                "proxy {} for http, {} for https",
                shown(http.as_ref()),
                shown(https.as_ref())
            )?,
        }
        if rule.system && self.has_proxy() {
            f.write_str(" (from the environment)")?;
        }
        if self.has_proxy() && !rule.direct.is_empty() {
            let direct: Vec<String> = rule
                .direct
                .iter()
                .map(|p| match p.strip_prefix('.') {
                    Some(apex) => format!("*.{apex}"),
                    None => p.clone(),
                })
                .collect();
            write!(f, "; direct to {}", direct.join(", "))?;
        }
        Ok(())
    }
}

impl fmt::Debug for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Network({self})")
    }
}

static CURRENT: OnceLock<Network> = OnceLock::new();

/// Make `network` the rule for the life of this process. Only the first
/// call counts, and only if nothing has asked for the rule yet, since a
/// client built before it would follow another. Returns whether it took.
pub fn install(network: Network) -> bool {
    CURRENT.set(network).is_ok()
}

/// The rule for this process: what was installed, or else what the
/// environment says. A `GHOST_PROXY` that cannot be used is warned about
/// and passed over for the standard variables, which is how things were
/// before there was a setting.
pub fn current() -> &'static Network {
    CURRENT.get_or_init(|| {
        Network::from_env(&Setting::default()).unwrap_or_else(|e| {
            tracing::warn!("{PROXY_VAR}: {e}; using the standard proxy variables instead");
            Network::new(&Setting::default(), |name| {
                (name != PROXY_VAR)
                    .then(|| std::env::var(name).ok())
                    .flatten()
            })
            .unwrap_or_else(|_| unreachable!("the standard variables are never refused"))
        })
    })
}

/// A client builder that follows this process's rule. Every HTTP client a
/// Ghost program makes starts here.
pub fn builder() -> ClientBuilder {
    current().builder()
}

/// An entry of a `direct` list as it is matched: lower case, no port, and
/// `*.name` written `.name`. `NO_PROXY` writes that `.name` already.
fn pattern(entry: impl AsRef<str>) -> Option<String> {
    let entry = entry.as_ref().trim().to_ascii_lowercase();
    if entry.is_empty() {
        return None;
    }
    if entry == "*" {
        return Some(entry);
    }
    match entry.strip_prefix("*.").or_else(|| entry.strip_prefix('.')) {
        Some(apex) => Some(format!(".{}", bare(without_port(apex)))),
        None => Some(bare(without_port(&entry))),
    }
}

/// A host without the brackets of an IPv6 address or a final dot, in lower
/// case.
fn bare(host: &str) -> String {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

/// `host:port` as `host`. More than one colon is an IPv6 address, which
/// has a port only inside brackets.
fn without_port(entry: &str) -> &str {
    if let Some(inside) = entry.strip_prefix('[') {
        return inside.split(']').next().unwrap_or(inside);
    }
    match entry.split_once(':') {
        Some((host, port)) if !port.contains(':') => host,
        _ => entry,
    }
}

fn on_this_machine(host: &str) -> bool {
    let host = bare(host);
    host == "localhost"
        || host.ends_with(".localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
}

/// A proxy's address from the setting. Only HTTP proxies are taken: that
/// is what every client here can speak to.
fn proxy_url(text: &str) -> Result<Url, Error> {
    let text = text.trim();
    let written = if text.contains("://") {
        text.to_owned()
    } else {
        format!("http://{text}")
    };
    let url = Url::parse(&written)
        .map_err(|e| Error(format!("the proxy setting is not an address ({e})")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error(format!(
            "the proxy {} is {}://, and only an http:// or https:// proxy can be used",
            shown(Some(&url)),
            url.scheme()
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(Error("the proxy setting names no host".to_owned()));
    }
    Ok(url)
}

/// A proxy from a standard variable, if it is one that can be used. A
/// SOCKS proxy in `ALL_PROXY` is common beside an HTTP one in
/// `HTTPS_PROXY`; on its own it is passed over, with a word.
fn from_system(value: Option<String>) -> Option<Url> {
    let value = value?;
    proxy_url(&value)
        .map_err(|e| tracing::warn!("{e}; going without it"))
        .ok()
}

/// A proxy's address without its user or password, for a log line.
fn shown(url: Option<&Url>) -> String {
    let Some(url) = url else {
        return "none".to_owned();
    };
    let mut url = url.clone();
    let _ = url.set_password(None);
    let _ = url.set_username("");
    url.as_str().trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, net::SocketAddr};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    /// A server on this machine that answers anything with `body`. As a
    /// proxy it is sent the whole address and answers just the same.
    async fn answering(body: &'static str) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        addr
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn setting(proxy: &str, direct: &[&str]) -> Setting {
        Setting {
            proxy: Some(proxy.to_owned()),
            direct: Some(direct.iter().map(|&d| d.to_owned()).collect()),
        }
    }

    /// Which server answered a request for `host`, every name being
    /// looked up as the site.
    async fn who_answers(network: &Network, host: &str, site: SocketAddr) -> String {
        let client = network.builder().resolve(host, site).build().unwrap();
        client
            .get(format!("http://{host}:{}/", site.port()))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_proxy_carries_everything_but_the_hosts_listed_direct() {
        let (site, proxy) = (answering("site").await, answering("proxy").await);
        let network = Network::new(
            &setting(&format!("http://{proxy}"), &["*.near.test", "exact.test"]),
            env(&[]),
        )
        .unwrap();
        for (host, expected) in [
            ("far.test", "proxy"),
            ("near.test", "site"),
            ("a.b.near.test", "site"),
            ("exact.test", "site"),
            ("under.exact.test", "proxy"),
        ] {
            assert_eq!(who_answers(&network, host, site).await, expected, "{host}");
        }
    }

    #[tokio::test]
    async fn none_goes_direct_whatever_the_environment_says() {
        let (site, proxy) = (answering("site").await, answering("proxy").await);
        let proxy = format!("http://{proxy}");
        let network = Network::new(
            &setting("none", &[]),
            env(&[("HTTP_PROXY", &proxy), ("HTTPS_PROXY", &proxy)]),
        )
        .unwrap();
        assert!(!network.has_proxy());
        assert_eq!(who_answers(&network, "far.test", site).await, "site");
    }

    #[tokio::test]
    async fn system_reads_the_standard_variables_with_no_proxy() {
        let (site, proxy) = (answering("site").await, answering("proxy").await);
        // Written without a scheme, as people do.
        let proxy = proxy.to_string();
        let network = Network::new(&Setting::default(), env(&[("http_proxy", &proxy)])).unwrap();
        assert_eq!(who_answers(&network, "far.test", site).await, "proxy");
        let network = Network::new(
            &Setting::default(),
            env(&[("HTTP_PROXY", &proxy), ("NO_PROXY", ".far.test,other:8080")]),
        )
        .unwrap();
        assert_eq!(who_answers(&network, "far.test", site).await, "site");
        assert_eq!(
            network.route(&Url::parse("http://other/").unwrap()),
            Route::Direct
        );
    }

    #[tokio::test]
    async fn this_machine_is_never_reached_through_a_proxy() {
        let (site, proxy) = (answering("site").await, answering("proxy").await);
        let network = Network::new(&setting(&format!("http://{proxy}"), &[]), env(&[])).unwrap();
        let client = network.builder().build().unwrap();
        let body = client
            .get(format!("http://{site}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "site");
        for address in [
            "http://localhost:9/",
            "http://[::1]:9/",
            "https://a.localhost/",
        ] {
            let url = Url::parse(address).unwrap();
            assert_eq!(network.route(&url), Route::Direct, "{address}");
            assert!(!network.either_way(&url));
        }
    }

    #[tokio::test]
    async fn a_client_can_be_sent_one_way_whatever_the_rule_says() {
        let (site, proxy) = (answering("site").await, answering("proxy").await);
        let network = Network::new(&setting(&format!("http://{proxy}"), &["*"]), env(&[])).unwrap();
        assert_eq!(who_answers(&network, "far.test", site).await, "site");
        let by_proxy = network
            .builder_by(Route::Proxy)
            .resolve("far.test", site)
            .build()
            .unwrap();
        let body = by_proxy
            .get(format!("http://far.test:{}/", site.port()))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "proxy");
    }

    #[test]
    fn the_environment_wins_over_the_setting_one_part_at_a_time() {
        let written = setting("http://written.example:3128", &["written.example"]);
        let far = Url::parse("https://far.example/").unwrap();

        let network =
            Network::new(&written, env(&[(DIRECT_VAR, "far.example, *.b.example")])).unwrap();
        assert_eq!(
            network.to_string(),
            "proxy http://written.example:3128; direct to far.example, *.b.example"
        );
        assert_eq!(network.route(&far), Route::Direct);

        let network = Network::new(&written, env(&[(PROXY_VAR, "none")])).unwrap();
        assert!(!network.has_proxy());

        // A blank line in `.env` is not a setting.
        let network = Network::new(&written, env(&[(PROXY_VAR, " "), (DIRECT_VAR, "")])).unwrap();
        assert_eq!(network.route(&far), Route::Proxy);
    }

    #[test]
    fn programs_she_starts_are_given_the_same_rule() {
        let network = Network::new(
            &setting("http://user:secret@127.0.0.1:7890", &["*.deepseek.com"]),
            env(&[]),
        )
        .unwrap();
        let given: HashMap<_, _> = network.child_env().into_iter().collect();
        assert_eq!(
            given["https_proxy"].as_deref(),
            Some("http://user:secret@127.0.0.1:7890/")
        );
        assert_eq!(given["HTTP_PROXY"], given["https_proxy"]);
        assert_eq!(
            given["NO_PROXY"].as_deref(),
            Some("localhost,127.0.0.1,::1,.deepseek.com")
        );
        for gone in ["ALL_PROXY", "all_proxy", PROXY_VAR, DIRECT_VAR] {
            assert_eq!(given[gone], None, "{gone}");
        }
        let none = Network::new(&setting("none", &[]), env(&[])).unwrap();
        let given: HashMap<_, _> = none.child_env().into_iter().collect();
        assert_eq!(given["HTTPS_PROXY"], None);
        assert_eq!(given["http_proxy"], None);
    }

    #[test]
    fn only_an_http_proxy_is_taken_and_its_password_is_never_said() {
        let refused = Network::new(&setting("socks5://u:secret@127.0.0.1:7891", &[]), env(&[]))
            .unwrap_err()
            .to_string();
        assert!(refused.contains("socks5://"), "{refused}");
        assert!(!refused.contains("secret"), "{refused}");

        let network = Network::new(
            &setting("http://u:secret@proxy.example:8080", &[]),
            env(&[]),
        )
        .unwrap();
        assert_eq!(network.to_string(), "proxy http://proxy.example:8080");
        assert!(!format!("{network:?}").contains("secret"));

        // A SOCKS proxy among the standard variables is passed over.
        let network = Network::new(
            &Setting::default(),
            env(&[("ALL_PROXY", "socks5://127.0.0.1:7891")]),
        )
        .unwrap();
        assert!(!network.has_proxy());
    }
}
