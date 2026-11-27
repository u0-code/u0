//! u0's one HTTP client (`ureq`), for all it reads from the web: the
//! registry and the archives it names, the forges' APIs, and the releases
//! the updater reads. Blocking: each caller is already off the window's
//! thread. A proxy set in the environment (`https_proxy`, ...) is used, and
//! certificates are checked as the system checks them.

use std::{
    io::{Read as _, Write as _},
    path::Path,
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ureq::{
    Agent, Body,
    http::{Request, Response},
    tls::{RootCerts, TlsConfig},
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

static AGENT: LazyLock<Agent> = LazyLock::new(|| {
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
        .redirect_auth_headers(ureq::config::RedirectAuthHeaders::Never)
        // What a host said went wrong is read from its answer.
        .http_status_as_error(false)
        .build()
        .into()
});

/// What `method` at `url` answers, sent `headers` and `body`: whether it
/// succeeded (a status under 400), and what it said, or why nothing was
/// heard. A request made over HTTPS is followed only to HTTPS, and its
/// `Authorization` is never carried to where it is redirected.
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
