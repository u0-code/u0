//! The `ghost` command's words, as t3code's `t3` has them: Ghost's server
//! with no window (`serve`), kept running from login by launchd
//! (`service`), and what this home holds (`status`, `sessions`, `tasks`).
//! A folder named opens in Ghost: that is the script the desk puts on the
//! PATH, which hands every other word here.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use ssh_key::sha2::{Digest as _, Sha256};

use crate::{access, key::Key, servers, settings, store, tasks, when};

const HELP: &str = "\
ghost <folder>…            open each folder in Ghost, as a project (none: this one)
ghost serve                Ghost's server with no window, for desks elsewhere to pair with
ghost service install      run that server from login (launchd), the window opening on it
ghost service uninstall    stop it, and take it out of login
ghost service status       whether it is installed and running, and where it logs
ghost status               this home: the agents on, the server, the tasks, the service
ghost sessions             the sessions kept, the latest first
ghost tasks                the scheduled tasks, and when each runs next
ghost --version            this build: its version, commit and channel";

/// Do what the command line asks, and exit, when it names one of the
/// words above; else go on to open the window.
pub fn when_asked() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(word) = args.first() else { return };
    let home = settings::home();
    let done = match word.as_str() {
        "serve" => crate::link::serve_here(),
        "service" => service(&home, args.get(1).map(String::as_str)),
        "status" => status(&home).map(|s| print!("{s}")),
        "sessions" => sessions(&home).map(|s| print!("{s}")),
        "tasks" => tasks_of(&home).map(|s| print!("{s}")),
        "version" | "--version" | "-V" => {
            println!("{}", crate::update::this_build());
            Ok(())
        }
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            Ok(())
        }
        _ => return,
    };
    match done {
        Ok(()) => std::process::exit(0),
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1)
        }
    }
}

/// The launchd label of the service of `home`: Ghost's own, or one of
/// its own for another home, so two Ghosts on one Mac keep apart.
pub fn label(home: &Path) -> String {
    let default = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ghost"));
    if default.as_deref() == Some(home) {
        return "local.ghost.server".into();
    }
    let hash = Sha256::digest(home.as_os_str().as_encoded_bytes());
    let short = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]);
    format!("local.ghost.server.{short:08x}")
}

/// What the window calls the service, its way to this Mac's own Ghost.
const SERVICE: &str = "This Mac's service";

fn plist_of(label: &str) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist")))
}

/// The launchd agent that runs `exe serve` for `home` from login, again
/// if it stops, logging to `log`.
fn agent_plist(label: &str, exe: &Path, home: &Path, log: &Path) -> String {
    let x = |p: &Path| {
        p.display()
            .to_string()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key><array><string>{}</string><string>serve</string></array>
  <key>EnvironmentVariables</key><dict><key>GHOST_HOME</key><string>{}</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Background</string>
  <key>StandardOutPath</key><string>{}</string>
  <key>StandardErrorPath</key><string>{}</string>
</dict>
</plist>
"#,
        x(exe),
        x(home),
        x(log),
        x(log),
    )
}

/// `launchctl` with `args`: what it printed, or why it failed.
fn launchctl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("launchctl")
        .args(args)
        .output()
        .map_err(|e| format!("launchctl could not be run: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// The launchd domain of the one running this.
fn domain() -> Result<String, String> {
    let out = Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "gui/{}",
        String::from_utf8_lossy(&out.stdout).trim()
    ))
}

/// Where launchd has the service: "running (pid 12)", "loaded, not
/// running"; none, not loaded.
fn standing(label: &str) -> Option<String> {
    let said = launchctl(&["print", &format!("{}/{label}", domain().ok()?)]).ok()?;
    let pid = said.lines().find_map(|l| l.trim().strip_prefix("pid = "));
    Some(pid.map_or_else(
        || "loaded, not running".into(),
        |p| format!("running (pid {p})"),
    ))
}

fn service(home: &Path, word: Option<&str>) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("The service is launchd's, on macOS; elsewhere run `ghost serve` under your own supervisor (systemd, ...).".into());
    }
    let label = label(home);
    let plist = plist_of(&label)?;
    let log = home.join("logs").join("service.log");
    match word {
        Some("install") => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(home.join("logs")).map_err(|e| e.to_string())?;
            pair_window(home)?;
            crate::whole::write(&plist, agent_plist(&label, &exe, home, &log).as_bytes())
                .map_err(|e| e.to_string())?;
            let _ = launchctl(&["bootout", &format!("{}/{label}", domain()?)]);
            launchctl(&["bootstrap", &domain()?, &plist.display().to_string()])?;
            println!(
                "Ghost's server runs from login as {label}; the window opens on it.\nIts log: {}",
                log.display()
            );
            Ok(())
        }
        Some("uninstall") => {
            let _ = launchctl(&["bootout", &format!("{}/{label}", domain()?)]);
            let _ = std::fs::remove_file(&plist);
            unpair_window(home)?;
            println!(
                "{label} is stopped and out of login; the window opens on its own server again."
            );
            Ok(())
        }
        Some("status") => {
            let installed = plist.is_file();
            let state = standing(&label).unwrap_or_else(|| "not loaded".into());
            println!(
                "{label}: {}, {state}\nIts log: {}",
                if installed {
                    "installed"
                } else {
                    "not installed"
                },
                log.display()
            );
            Ok(())
        }
        _ => Err("ghost service install | uninstall | status".into()),
    }
}

/// Pair the window of `home` with its service, as a desk pairs with any
/// server, but through the files the service has not opened yet: a key
/// for the window let in (`devices.json`), the service kept as the way
/// to this Mac's Ghost (`servers.json`, the private half in the Keychain;
/// `Servers::service`), and the server listening.
fn pair_window(home: &Path) -> Result<(), String> {
    let access = access::Access::new(home);
    let port = access.kept()?.port;
    unpair_window(home)?;
    let key = Key::generate();
    let now = store::secs(std::time::SystemTime::now());
    access.change(|k| {
        k.devices.push(access::Device {
            label: "This Mac's window".into(),
            key: key.public(),
            paired: now,
            seen: now,
        });
        k.serve = true;
    });
    servers::Servers::change(home, |s| {
        s.paired(
            servers::Server {
                name: SERVICE.into(),
                host: format!("127.0.0.1:{port}"),
                key: key.public(),
            },
            &key,
        )
    })
}

/// Undo `pair_window`: the window's key let out, the service forgotten.
fn unpair_window(home: &Path) -> Result<(), String> {
    let mut gone = None;
    servers::Servers::change(home, |s| {
        gone = (s.servers.iter())
            .find(|x| x.name == SERVICE)
            .map(|x| x.key.clone());
        s.forget(SERVICE);
        Ok(())
    })?;
    if let Some(key) = gone {
        access::Access::new(home).change(|k| k.devices.retain(|d| d.key != key));
    }
    Ok(())
}

/// What this home holds, in a few lines.
fn status(home: &Path) -> Result<String, String> {
    let (kept, refused) = settings::Settings::load(home);
    let mut out = format!("Ghost's home: {}\n", home.display());
    if let Some(why) = refused {
        let _ = writeln!(out, "settings.json does not read: {why}");
    }
    let _ = writeln!(out, "Agents on: {}", kept.enabled.join(", "));
    let port = access::Access::new(home).kept().map(|k| k.port)?;
    let up = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
    let _ = writeln!(
        out,
        "Server: {}",
        if up {
            format!("listening on 127.0.0.1:{port}")
        } else {
            "not listening (the window's own serves no one)".into()
        }
    );
    let label = label(home);
    let service = match standing(&label) {
        Some(state) => format!("{label}, {state}"),
        None if plist_of(&label).is_ok_and(|p| p.is_file()) => format!("{label}, not loaded"),
        None => "not installed (ghost service install)".into(),
    };
    let _ = writeln!(out, "Service: {service}");
    let list = tasks::load(home).map(|t| t.tasks).unwrap_or_default();
    let _ = writeln!(
        out,
        "Scheduled tasks: {} ({} paused)",
        list.len(),
        list.iter().filter(|t| t.paused).count()
    );
    Ok(out)
}

/// The sessions kept, the latest first: when, the agent, the project and
/// the title.
fn sessions(home: &Path) -> Result<String, String> {
    let mut index = store::load(home)?.sessions;
    index.retain(|m| !m.flags.archived);
    index.sort_by_key(|m| std::cmp::Reverse(m.updated));
    let now = std::time::SystemTime::now();
    let mut out = String::new();
    for m in index {
        let project = m.project.file_name().map_or_else(
            || m.project.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let _ = writeln!(
            out,
            "{:>5}  {:<14} {:<20} {}",
            when::ago(m.updated, now),
            m.agent,
            project,
            m.title
        );
    }
    Ok(out)
}

/// The scheduled tasks: each by its number, its schedule, when it runs
/// next and what it says.
fn tasks_of(home: &Path) -> Result<String, String> {
    let mut out = String::new();
    for t in tasks::load(home)?.tasks {
        let next = t.next.filter(|_| !t.paused).map_or_else(
            || "not to run".to_owned(),
            |at| when::said_at(store::time(at), std::time::SystemTime::now(), false),
        );
        let _ = writeln!(
            out,
            "#{:<3} {:<28} next: {:<18} {}",
            t.id,
            t.schedule.said(false),
            next,
            t.title
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_home_has_a_service_of_its_own() {
        let (a, b) = (label(Path::new("/tmp/a")), label(Path::new("/tmp/b")));
        assert!(a.starts_with("local.ghost.server."));
        assert_ne!(a, b);
        let p = agent_plist(
            &a,
            Path::new("/Applications/Ghost.app/x"),
            Path::new("/h&o"),
            Path::new("/l"),
        );
        assert!(p.contains("<string>serve</string>"));
        assert!(p.contains("<string>/h&amp;o</string>"), "{p}");
    }
}
