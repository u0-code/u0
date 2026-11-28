//! u0's one HTTP client (`ureq`), for all it reads from the web: the
//! registry and the archives it names, the forges' APIs, and the releases
//! the updater reads; and, with an agent of its own, an address someone
//! else wrote (`outward`). Blocking: each caller is already off the
//! window's thread. A proxy set in the environment (`https_proxy`, ...) is
//! used, and certificates are checked as the system checks them. A request
//! made over HTTPS, a download among them, is followed only to HTTPS, and
//! its `Authorization` is never carried to where it is redirected.

use std::{
    io::{Read as _, Write as _},
    net::{IpAddr, Ipv4Addr},
    path::Path,
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ureq::{
    Agent, Body,
    config::{Config, ConfigBuilder, RedirectAuthHeaders},
    http::{Request, Response, Uri},
    tls::{RootCerts, TlsConfig},
    typestate::AgentScope,
    unversioned::{
        resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
        transport::{DefaultConnector, NextTimeout},
    },
};

use crate::{
    registry::{CANCELLED, Say},
    tidy::shown,
};

/// What a request may take, whole, but a download.
const QUICK: Duration = Duration::from_secs(30);

/// What a download may take.
const LONG: Duration = Duration::from_mins(30);

/// The most an answer read whole may hold.
const MOST: u64 = 64 << 20;

/// How each agent is set up.
fn config() -> ConfigBuilder<AgentScope> {
    // rustls with ring, as the rest of the build has it; a provider already
    // chosen for the process is kept.
    let _ = rustls::crypto::ring::default_provider().install_default();
    Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .user_agent(concat!("u0/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(QUICK))
        // A token sent one host is never carried to another it redirects to.
        .redirect_auth_headers(RedirectAuthHeaders::Never)
        // What a host said went wrong is read from its answer.
        .http_status_as_error(false)
}

static AGENT: LazyLock<Agent> = LazyLock::new(|| config().build().into());

/// The agent for an address someone else wrote: over HTTPS alone, never
/// through a proxy (which would reach what it names for it), and only to
/// a host on the public internet, each hop of its redirects too.
static OUTWARD: LazyLock<Agent> = LazyLock::new(|| {
    let config = config().https_only(true).proxy(None).build();
    Agent::with_parts(config, DefaultConnector::default(), Public)
});

/// Resolves a name as the system does, and refuses it where any address
/// it has is not on the public internet. What is connected to is what was
/// checked, so a name that answers otherwise the next time changes nothing.
#[derive(Debug)]
struct Public;

impl Resolver for Public {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let addresses = DefaultResolver::default().resolve(uri, config, timeout)?;
        if addresses.iter().all(|a| public(a.ip())) {
            Ok(addresses)
        } else {
            Err(ureq::Error::Io(std::io::Error::other(
                "an address not on the public internet",
            )))
        }
    }
}

/// Whether `ip` is on the public internet: not this machine, a private
/// network (RFC 1918, a ULA), a link's own (the cloud's metadata address
/// among them), a carrier's NAT, multicast, a broadcast, unspecified,
/// documentation's or reserved. An IPv4 address carried in IPv6 is judged
/// as the IPv4 address it is.
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && (18..20).contains(&b))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public(IpAddr::V4(v4));
            }
            if !carried(v6).into_iter().all(|v4| public(IpAddr::V4(v4))) {
                return false;
            }
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] & 0xffc0) == 0xfec0
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || s[..6] == [0; 6])
        }
    }
}

/// The IPv4 addresses `v6` leads to, where it is a way to one: NAT64's
/// well-known prefix (its last 32 bits), 6to4 (`2002::/16`, the next 32
/// bits) and Teredo (`2001::/32`: its server, and its client with each bit
/// flipped). NAT64's local-use prefix (`64:ff9b:1::/48`) leads to one where
/// its network's own prefix length says, which no address tells: it is
/// judged as leading to `0.0.0.0`, refused.
fn carried(v6: std::net::Ipv6Addr) -> Vec<Ipv4Addr> {
    let s = v6.segments();
    let o = v6.octets();
    let at = |a: usize, b: usize, c: usize, d: usize| Ipv4Addr::new(o[a], o[b], o[c], o[d]);
    match s {
        [0x64, 0xff9b, 0, 0, 0, 0, ..] => vec![at(12, 13, 14, 15)],
        [0x64, 0xff9b, 1, ..] => vec![Ipv4Addr::UNSPECIFIED],
        [0x2002, ..] => vec![at(2, 3, 4, 5)],
        [0x2001, 0, ..] => vec![
            at(4, 5, 6, 7),
            Ipv4Addr::from(!u32::from(at(12, 13, 14, 15))),
        ],
        _ => Vec::new(),
    }
}

/// What `url`, an address someone else wrote, answers over the `OUTWARD`
/// agent, sent `headers`: at most `most` bytes, or an error. What went
/// wrong on the way is not said: it would tell what lies where.
pub fn outward(url: &str, headers: &[(&str, &str)], most: u64) -> Result<Vec<u8>, String> {
    let failed = |e: ureq::Error| {
        tracing::debug!("{url}: {e}");
        "it could not be read".to_owned()
    };
    let mut request = OUTWARD.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut answer = request.call().map_err(failed)?;
    if answer.status().as_u16() >= 400 {
        return Err("it could not be read".into());
    }
    answer
        .body_mut()
        .with_config()
        .limit(most)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => "it is too large".to_owned(),
            e => failed(e),
        })
}

/// What `method` at `url` answers, sent `headers` and `body`: whether it
/// succeeded (a status under 400), and what it said, or why nothing was
/// heard.
pub fn call(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<Vec<u8>>,
) -> Result<(bool, Vec<u8>), String> {
    let mut request = Request::builder().method(method).uri(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request
        .body(body.unwrap_or_default())
        .map_err(|e| e.to_string())?;
    let request = AGENT
        .configure_request(request)
        .https_only(url.starts_with("https://"))
        .build();
    let mut answer = AGENT.run(request).map_err(|e| e.to_string())?;
    let ok = answer.status().as_u16() < 400;
    let said = answer
        .body_mut()
        .with_config()
        .limit(MOST)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    Ok((ok, said))
}

/// What `url` answers, sent `headers`; an error where it did not succeed.
pub fn get(url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    match call("GET", url, headers, None)? {
        (true, said) => Ok(said),
        (false, _) => Err(format!("{url} could not be read")),
    }
}

/// Download `url` to `to`, saying how many megabytes have come of how
/// many, until it is done or `stop` is set.
pub fn download(url: &str, to: &Path, say: Say, stop: &AtomicBool) -> Result<(), String> {
    if stop.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    let answer: Response<Body> = AGENT
        .get(url)
        .config()
        .timeout_global(Some(LONG))
        .https_only(url.starts_with("https://"))
        .build()
        .call()
        .map_err(|e| format!("the download failed: {e}"))?;
    if answer.status().as_u16() >= 400 {
        return Err(format!("the download failed: {}", answer.status()));
    }
    let total = answer.body().content_length().filter(|n| *n > 0);
    let mut from = answer.into_body().into_reader();
    let mut file = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut chunk = vec![0; 64 << 10];
    let (mut got, mut said) = (0, None::<Instant>);
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        let n = from
            .read(&mut chunk)
            .map_err(|e| format!("the download failed: {e}"))?;
        if n == 0 {
            return file.flush().map_err(|e| e.to_string());
        }
        file.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        if said.is_some_and(|at| at.elapsed() < Duration::from_millis(250)) {
            continue;
        }
        said = Some(Instant::now());
        match total {
            Some(total) => say(
                &format!("Downloading · {} of {}", shown(got), shown(total)),
                Some((got as f64 / total as f64).min(1.) as f32),
            ),
            None => say(&format!("Downloading · {}", shown(got)), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    #[test]
    fn only_an_address_on_the_public_internet_is_reached_for_someone_else() {
        for near in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.5",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fd00:ec2::254",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::a00:1",
            "2002:7f00:1::",
            "2002:a9fe:a9fe::1",
            "2001:0:4136:e378:8000:63bf:f5ff:fffe",
            "2001:0:a00:1::1",
            "64:ff9b:1::808:808",
        ] {
            let ip: IpAddr = near.parse().unwrap();
            assert!(!super::public(ip), "{near}");
        }
        for far in [
            "140.82.112.3",
            "185.199.108.133",
            "2606:50c0:8000::154",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
            "2001:0:4136:e378:8000:63bf:f7f7:f7f7",
        ] {
            let ip: IpAddr = far.parse().unwrap();
            assert!(super::public(ip), "{far}");
        }
    }
}
