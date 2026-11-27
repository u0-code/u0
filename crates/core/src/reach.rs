//! How a u0 is reached from elsewhere, and how a desk reaches it: on
//! this machine (127.0.0.1, plain), through a cloudflared tunnel (its own
//! TLS), or straight over the local network or Tailscale, where u0
//! speaks TLS itself. Its certificate is made once and kept secret
//! (`secrets`), and a pairing link names it by its SHA-256 (`pin`): a
//! desk trusts the certificate that link pinned, and no other, so nothing
//! between them can read or stand in for either.

use std::{net::IpAddr, sync::Arc};

use base64::Engine as _;
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, ring::default_provider},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
};
use serde::{Deserialize, Serialize};

/// Where u0 listens beyond 127.0.0.1, with TLS of its own, on the port
/// after its own (`Kept::port` + 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bind {
    /// Nowhere: this machine, and a tunnel, alone.
    #[default]
    Here,
    /// Its Tailscale address alone.
    Tailscale,
    /// Every network it is on, Tailscale too.
    Network,
}

/// A way to this u0 a pairing link can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Route {
    /// 127.0.0.1: this machine, or an `ssh -L` to it.
    Here,
    /// An address on the local network.
    Lan,
    Tailscale,
    /// The cloudflared tunnel.
    Tunnel,
}

impl Route {
    pub fn name(self) -> &'static str {
        match self {
            Self::Here => "This Mac",
            Self::Lan => "Local network",
            Self::Tailscale => "Tailscale",
            Self::Tunnel => "Tunnel",
        }
    }
}

/// How a desk reaches a server: its host, and the certificate it pins
/// there, where the server speaks TLS itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Way {
    pub host: String,
    pub pin: Option<String>,
}

impl Way {
    /// Its address: plain on this machine, else TLS (the tunnel's, or the
    /// server's own, pinned). Which machine is the host as a URL reads
    /// it, never how its text begins.
    pub fn url(&self) -> String {
        let here = bare(&self.host)
            .is_some_and(|h| ["127.0.0.1", "localhost", "[::1]"].contains(&h.as_str()));
        let plain = here && self.pin.is_none();
        format!("{}://{}", if plain { "ws" } else { "wss" }, self.host)
    }
}

/// The machine `host` names, where it is a host alone (a port after it or
/// not): never one with a user before it (`127.0.0.1:1@elsewhere`, which a
/// URL reads as elsewhere), a path or more; and only where it is written
/// as a URL writes it, so that the name read here is the name dialled
/// (`127.0.0.1.` is read as 127.0.0.1, but dialled as a name).
fn bare(host: &str) -> Option<String> {
    let url = url::Url::parse(&format!("ws://{host}")).ok()?;
    let name = match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    let alone = url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && !host.contains(['/', '?', '#', '@', '\\'])
        && url.host_str().is_some_and(|h| h.eq_ignore_ascii_case(name));
    alone.then(|| url.host_str().map(str::to_owned)).flatten()
}

/// The way and code of a pairing link, `u0://pair?host=…&pin=…#code`.
pub fn pairing(link: &str) -> Option<(Way, String)> {
    let url = url::Url::parse(link).ok()?;
    if url.scheme() != "u0" || url.host_str() != Some("pair") {
        return None;
    }
    let query =
        |name: &str| (url.query_pairs()).find_map(|(k, v)| (k == name).then(|| v.into_owned()));
    let host = query("host").filter(|h| bare(h).is_some())?;
    let code = url.fragment().filter(|c| !c.is_empty())?;
    let way = Way {
        host,
        pin: query("pin"),
    };
    Some((way, code.to_owned()))
}

/// The pairing link for `code` at `way`.
pub fn pairing_link(way: &Way, code: &str) -> String {
    let mut url = url::Url::parse("u0://pair").expect("a link");
    url.query_pairs_mut().append_pair("host", &way.host);
    if let Some(pin) = &way.pin {
        url.query_pairs_mut().append_pair("pin", pin);
    }
    url.set_fragment(Some(code));
    url.into()
}

/// This machine's addresses on its networks (IPv4), each by its route:
/// Tailscale's are in 100.64.0.0/10, the local network's private.
pub fn addresses() -> Vec<(Route, IpAddr)> {
    let Ok(networks) = sysinfo::Networks::new_with_refreshed_list() else {
        return Vec::new();
    };
    let mut found: Vec<(Route, IpAddr)> = (networks.values())
        .flat_map(|n| n.ip_networks().iter().map(|ip| ip.addr))
        .filter_map(|addr| route_of(addr).map(|route| (route, addr)))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The route of the address `ip`: none for one of IPv6, or the internet.
pub(crate) fn route_of(ip: IpAddr) -> Option<Route> {
    let IpAddr::V4(ip) = ip else { return None };
    let [a, b, ..] = ip.octets();
    if a == 100 && (64..128).contains(&b) {
        Some(Route::Tailscale)
    } else if ip.is_private() {
        Some(Route::Lan)
    } else {
        None
    }
}

/// Where the certificate and its key are kept.
const IDENTITY: &str = "share/tls";

/// This u0's certificate and its key: made once, then kept; and its
/// pin.
fn identity() -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>, String), String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let read = |kept: &str| {
        let (cert, key) = kept.split_once('.')?;
        Some((b64.decode(cert).ok()?, b64.decode(key).ok()?))
    };
    let kept = crate::secrets::get(IDENTITY)?;
    let (cert, key) = if let Some(kept) = kept.as_deref().and_then(read) {
        kept
    } else {
        let made =
            rcgen::generate_simple_self_signed(vec!["u0".to_owned()]).map_err(|e| e.to_string())?;
        let (cert, key) = (made.cert.der().to_vec(), made.signing_key.serialize_der());
        let kept = format!("{}.{}", b64.encode(&cert), b64.encode(&key));
        crate::secrets::set(IDENTITY, Some(&kept))?;
        (cert, key)
    };
    let pin = pin_of(&cert);
    let key = PrivateKeyDer::try_from(key).map_err(String::from)?;
    Ok((CertificateDer::from(cert), key, pin))
}

/// What a certificate is pinned by: its SHA-256, URL-safe base64.
fn pin_of(cert: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, cert);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.as_ref())
}

/// The TLS this u0 speaks where it listens on a network, and the pin
/// a link to it names.
pub(crate) fn acceptor() -> Result<(tokio_rustls::TlsAcceptor, String), String> {
    let (cert, key, pin) = identity()?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .and_then(|c| c.with_no_client_auth().with_single_cert(vec![cert], key))
        .map_err(|e| e.to_string())?;
    Ok((tokio_rustls::TlsAcceptor::from(Arc::new(config)), pin))
}

/// The TLS a desk speaks to a server whose certificate is pinned (`pin`): that
/// certificate, and no other, whatever name or dates it bears.
pub(crate) fn pinned(pin: &str) -> rustls::ClientConfig {
    let provider = Arc::new(default_provider());
    rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring's protocols")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned {
            pin: pin.to_owned(),
            provider,
        }))
        .with_no_client_auth()
}

/// A certificate trusted by its pin alone; what it signs checked as any.
#[derive(Debug)]
struct Pinned {
    pin: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if pin_of(cert) == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "not the u0 this Mac paired with: its certificate changed".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algorithms = &self.provider.signature_verification_algorithms;
        rustls::crypto::verify_tls12_signature(message, cert, dss, algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algorithms = &self.provider.signature_verification_algorithms;
        rustls::crypto::verify_tls13_signature(message, cert, dss, algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        (self.provider.signature_verification_algorithms).supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    #[test]
    fn a_pairing_link_names_the_way_its_code_and_its_pin() {
        let tunnel = Way {
            host: "abc.trycloudflare.com".into(),
            pin: None,
        };
        let (way, code) = pairing(&pairing_link(&tunnel, "c0de")).unwrap();
        assert_eq!(
            (way.url().as_str(), code.as_str()),
            ("wss://abc.trycloudflare.com", "c0de")
        );
        let lan = Way {
            host: "192.168.1.5:4318".into(),
            pin: Some("p+n".into()),
        };
        let (way, _) = pairing(&pairing_link(&lan, "a+b%2Fc")).unwrap();
        assert_eq!(way, lan);
        assert_eq!(way.url(), "wss://192.168.1.5:4318");
        let here = Way {
            host: "127.0.0.1:4317".into(),
            pin: None,
        };
        assert_eq!(here.url(), "ws://127.0.0.1:4317");
        assert_eq!(pairing("u0://pair?host=#x"), None);
        assert_eq!(pairing("u0://new?host=h#x"), None);
    }

    #[test]
    fn a_host_with_a_user_before_it_or_not_as_a_url_writes_it_is_no_way_nor_this_machine() {
        let link = "u0://pair?host=127.0.0.1%3A1%40evil.example#c0de";
        assert_eq!(pairing(link), None);
        for host in [
            "127.0.0.1:1@evil.example",
            "localhost@evil.example",
            "127.0.0.1/../x",
            "127.0.0.1.",
            "127.0.0.1.:4317",
            "127.1:4317",
        ] {
            assert_eq!(
                pairing(&format!("u0://pair?host={host}#c0de")),
                None,
                "{host}"
            );
            let way = Way {
                host: host.into(),
                pin: None,
            };
            assert!(way.url().starts_with("wss://"), "{host}");
        }
        let here = Way {
            host: "127.0.0.1:4317".into(),
            pin: None,
        };
        assert_eq!(here.url(), "ws://127.0.0.1:4317");
        assert!(pairing("u0://pair?host=[::1]:4317#c0de").is_some());
        assert!(pairing("u0://pair?host=LOCALHOST:4317#c0de").is_some());
    }

    #[test]
    fn an_address_is_known_for_its_network() {
        assert_eq!(
            route_of(IpAddr::V4(Ipv4Addr::new(100, 101, 2, 3))),
            Some(Route::Tailscale)
        );
        assert_eq!(
            route_of(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5))),
            Some(Route::Lan)
        );
        assert_eq!(
            route_of(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))),
            Some(Route::Lan)
        );
        assert_eq!(route_of(IpAddr::V4(Ipv4Addr::new(100, 1, 2, 3))), None);
        assert_eq!(route_of(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))), None);
    }

    #[test]
    fn the_certificate_is_made_once_and_kept_its_pin_with_it() {
        let (first, _, pin) = identity().unwrap();
        let (again, _, same) = identity().unwrap();
        assert_eq!((first, pin.clone()), (again, same));
        assert_eq!(pin.len(), 43);
    }
}
