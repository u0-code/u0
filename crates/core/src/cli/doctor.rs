//! `ghost doctor`: what of this home is not as Ghost needs it, each
//! thing looked at said in a line, with what to do where it is not.

use std::{fmt::Write as _, path::Path};

use crate::{access, mcp, registry, servers, settings, store, tasks, workflows};

/// Each thing looked at, and whether it is well; or what to do.
pub fn doctor(home: &Path) -> (String, bool) {
    let mut checks: Vec<(String, Result<String, String>)> = Vec::new();
    let mut check =
        |what: &str, found: Result<String, String>| checks.push((what.to_owned(), found));
    let (kept, refused) = settings::Settings::load(home);
    check("settings.json", refused.map_or(Ok("reads".into()), Err));
    check(
        "sessions.json",
        store::load(home).map(|i| format!("{} sessions", i.sessions.len())),
    );
    check(
        "tasks.json",
        tasks::load(home).map(|t| format!("{} tasks", t.tasks.len())),
    );
    check(
        "runs.json",
        workflows::load_runs(home).map(|r| format!("{} runs", r.runs.len())),
    );
    let sharing = access::Access::new(home).kept();
    check(
        "devices.json",
        sharing
            .as_ref()
            .map(|k| format!("{} devices", k.devices.len()))
            .map_err(Clone::clone),
    );
    let paired = servers::Servers::load(home);
    check(
        "servers.json",
        paired
            .as_ref()
            .map(|s| format!("{} servers", s.servers.len()))
            .map_err(Clone::clone),
    );
    // A server runs with this home where its tools' door answers.
    let door = mcp::door(home).ok();
    check(
        "Ghost running",
        match door {
            Some(d) if std::net::TcpStream::connect(("127.0.0.1", d.port)).is_ok() => {
                Ok(format!("its tools at 127.0.0.1:{} and :{}", d.port, d.http))
            }
            _ => Err("No Ghost runs with this home: open Ghost, or `ghost serve`.".into()),
        },
    );
    let known = registry::kept(home);
    for agent in &kept.enabled {
        let found = known.as_ref().and_then(|r| registry::entry_of(r, agent));
        check(
            &format!("agent {agent}"),
            found.map_or_else(
                || Err("Not in the registry kept: open Ghost to read it again.".into()),
                |e| Ok(format!("{} {}", e.name, e.version)),
            ),
        );
    }
    if let Ok(k) = &sharing {
        if k.tunnel {
            check(
                "cloudflared",
                which::which("cloudflared")
                    .map(|p| p.display().to_string())
                    .map_err(|_| "Not installed: brew install cloudflared".into()),
            );
        }
        if k.bind == crate::reach::Bind::Tailscale {
            let on = crate::reach::addresses()
                .into_iter()
                .find(|(r, _)| *r == crate::reach::Route::Tailscale);
            check(
                "Tailscale",
                on.map(|(_, ip)| ip.to_string())
                    .ok_or_else(|| "This Mac is not on Tailscale".into()),
            );
        }
    }
    if let Ok(paired) = &paired {
        for s in &paired.servers {
            check(
                &format!("key for {}", s.name),
                s.reach().map(|_| "in the Keychain".into()),
            );
        }
    }
    let mut out = String::new();
    let mut well = true;
    for (what, found) in checks {
        let _ = match found {
            Ok(said) => writeln!(out, "ok   {what}: {said}"),
            Err(why) => {
                well = false;
                writeln!(out, "!!   {what}: {why}")
            }
        };
    }
    (out, well)
}
