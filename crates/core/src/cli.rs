//! The `ghost` command's words, as t3code's `t3` has them: Ghost's server
//! with no window (`serve`), kept running from login by launchd
//! (`service`), and what this home holds (`status`, `sessions`, `tasks`).
//! A folder named opens in Ghost. One parser (clap's) reads them all:
//! the script the desk puts on the PATH hands it everything, marked as a
//! terminal's (`--shell`), and an agent's `--mcp` is read here too.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use ssh_key::sha2::{Digest as _, Sha256};

use crate::{access, key::Key, servers, settings, store, tasks, when};

mod doctor;
mod threads;

/// The `ghost` command, as clap reads it: its words, or folders to open.
#[derive(Debug, clap::Parser)]
#[command(
    name = "ghost",
    about = "Ghost from a terminal: each folder named opens in Ghost, as a project (none: this one); or one of these words."
)]
struct Cli {
    /// Ghost's tools on the standard input and output, for an agent.
    #[arg(long, hide = true)]
    mcp: bool,
    /// With --mcp: the tools of the server this Mac is paired with by
    /// this name.
    #[arg(long, hide = true, requires = "mcp")]
    on: Option<String>,
    /// Said by the script on the PATH: run from a terminal, not by macOS.
    #[arg(long, hide = true)]
    shell: bool,
    #[command(subcommand)]
    word: Option<Word>,
    /// Folders to open in Ghost, as projects.
    folders: Vec<PathBuf>,
}

#[derive(Debug, clap::Subcommand)]
enum Word {
    /// Ghost's server with no window, for desks elsewhere to pair with.
    Serve,
    /// That server run from login by launchd, the window opening on it.
    Service {
        #[command(subcommand)]
        what: Service,
    },
    /// This home: the agents on, the server, the tasks, the service.
    Status,
    /// What of this home is not as Ghost needs it, and what to do.
    Doctor,
    /// The scheduled tasks, and when each runs next.
    Tasks,
    /// The sessions kept; what was said in one; something said in one.
    Thread {
        #[command(subcommand)]
        what: Thread,
    },
    /// The workflows and their runs; a run begun.
    Workflow {
        #[command(subcommand)]
        what: Flow,
    },
}

#[derive(Debug, clap::Subcommand)]
enum Thread {
    /// The sessions kept, the latest first, each with its agent and id.
    List,
    /// What was said in a session.
    Show { agent: String, session: String },
    /// Begin a session in a project (`.`: this folder) with what to say.
    Start {
        project: PathBuf,
        prompt: String,
        /// The agent, by its registry id; none, the first on.
        #[arg(long)]
        agent: Option<String>,
        /// Wait for its answer, and print it.
        #[arg(long)]
        wait: bool,
        /// The server paired with to ask; none, this Mac's service.
        #[arg(long)]
        on: Option<String>,
    },
    /// Say something in a session, as its next turn (one going on is let
    /// end first).
    Send {
        agent: String,
        session: String,
        prompt: String,
        #[arg(long)]
        wait: bool,
        #[arg(long)]
        on: Option<String>,
    },
}

#[derive(Debug, clap::Subcommand)]
enum Flow {
    /// The workflows, and their runs.
    List,
    /// Begin a run of a workflow with an input.
    Start {
        name: String,
        #[arg(default_value = "")]
        input: String,
        #[arg(long)]
        on: Option<String>,
    },
}

#[derive(Debug, clap::Subcommand)]
enum Service {
    /// Run it from login.
    Install,
    /// Stop it, and take it out of login.
    Uninstall,
    /// Whether it is installed and running, and where it logs.
    Status,
}

/// Do what the command line asks, and exit, where it names a word, or is
/// a terminal's; else (macOS starting the app) go on to open the window.
pub fn when_asked() {
    use clap::{CommandFactory as _, FromArgMatches as _};
    let command = Cli::command()
        .bin_name("ghost")
        .version(crate::update::this_build());
    let shell = std::env::args().any(|a| a == "--shell");
    let cli = match command
        .try_get_matches()
        .and_then(|m| Cli::from_arg_matches(&m))
    {
        Ok(cli) => cli,
        // Help, the version, and what does not read, from a terminal.
        Err(e) if shell || !e.use_stderr() => e.exit(),
        Err(_) => return,
    };
    let done = if cli.mcp {
        crate::mcp::pipe(cli.on.as_deref())
    } else if let Some(word) = cli.word {
        crate::path::widen();
        let home = settings::home();
        crate::logs::start(settings::Settings::load(&home).0.log_days);
        match word {
            Word::Serve => crate::link::serve_here(),
            Word::Service { what } => service(&home, &what),
            Word::Status => status(&home).map(|s| print!("{s}")),
            Word::Doctor => {
                let (said, well) = doctor::doctor(&home);
                print!("{said}");
                if well {
                    Ok(())
                } else {
                    Err("Some of it is not well: see above.".into())
                }
            }
            Word::Tasks => tasks_of(&home).map(|s| print!("{s}")),
            Word::Thread { what } => thread(&home, what).map(|s| println!("{}", s.trim_end())),
            Word::Workflow { what } => match what {
                Flow::List => threads::workflows(&home),
                Flow::Start { name, input, on } => {
                    threads::start(&home, on.as_deref(), name, input)
                }
            }
            .map(|s| println!("{}", s.trim_end())),
        }
    } else if cli.shell {
        open(&cli.folders)
    } else {
        return;
    };
    match done {
        Ok(()) => std::process::exit(0),
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1)
        }
    }
}

/// Open each of `folders` (none: this one) in Ghost, as a project, through
/// macOS, which hands it to the Ghost running or starts one.
fn open(folders: &[PathBuf]) -> Result<(), String> {
    let here = [PathBuf::from(".")];
    for folder in if folders.is_empty() {
        &here[..]
    } else {
        folders
    } {
        let opened = Command::new("open")
            .args(["-b", crate::update::IDENTIFIER])
            .arg(folder)
            .status()
            .map_err(|e| e.to_string())?;
        if !opened.success() {
            return Err(format!("{} did not open", folder.display()));
        }
    }
    Ok(())
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
/// if it stops, logging to `log`: as launchd reads it.
#[derive(serde::Serialize)]
#[serde(rename_all = "PascalCase")]
struct Agent<'a> {
    label: &'a str,
    program_arguments: [&'a Path; 2],
    environment_variables: std::collections::BTreeMap<&'a str, &'a Path>,
    run_at_load: bool,
    keep_alive: bool,
    process_type: &'a str,
    standard_out_path: &'a Path,
    standard_error_path: &'a Path,
}

/// The plist of the launchd agent `label` that runs `exe serve` for
/// `home` from login, again if it stops, logging to `log`.
fn agent_plist(label: &str, exe: &Path, home: &Path, log: &Path) -> Vec<u8> {
    let agent = Agent {
        label,
        program_arguments: [exe, Path::new("serve")],
        environment_variables: [("GHOST_HOME", home)].into(),
        run_at_load: true,
        keep_alive: true,
        process_type: "Background",
        standard_out_path: log,
        standard_error_path: log,
    };
    let mut xml = Vec::new();
    plist::to_writer_xml(&mut xml, &agent).expect("a plist writes");
    xml
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
fn domain() -> String {
    // SAFETY: getuid only reads the process's user id; it cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

/// Where launchd has the service: "running (pid 12)", "loaded, not
/// running"; none, not loaded.
fn standing(label: &str) -> Option<String> {
    let said = launchctl(&["print", &format!("{}/{label}", domain())]).ok()?;
    let pid = said.lines().find_map(|l| l.trim().strip_prefix("pid = "));
    Some(pid.map_or_else(
        || "loaded, not running".into(),
        |p| format!("running (pid {p})"),
    ))
}

fn service(home: &Path, what: &Service) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("The service is launchd's, on macOS; elsewhere run `ghost serve` under your own supervisor (systemd, ...).".into());
    }
    let label = label(home);
    let plist = plist_of(&label)?;
    let log = home.join("logs").join("service.log");
    match what {
        Service::Install => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            crate::settings::keep_private(home).map_err(|e| e.to_string())?;
            pair_window(home)?;
            crate::whole::write(&plist, &agent_plist(&label, &exe, home, &log))
                .map_err(|e| e.to_string())?;
            let _ = launchctl(&["bootout", &format!("{}/{label}", domain())]);
            launchctl(&["bootstrap", &domain(), &plist.display().to_string()])?;
            println!(
                "Ghost's server runs from login as {label}; the window opens on it.\nIts log: {}",
                log.display()
            );
            Ok(())
        }
        Service::Uninstall => {
            let _ = launchctl(&["bootout", &format!("{}/{label}", domain())]);
            let _ = std::fs::remove_file(&plist);
            unpair_window(home)?;
            println!(
                "{label} is stopped and out of login; the window opens on its own server again."
            );
            Ok(())
        }
        Service::Status => {
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
            scope: access::Scope::Full,
            until: None,
            waiting: false,
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
                pin: None,
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

/// What `ghost thread` asks.
fn thread(home: &Path, what: Thread) -> Result<String, String> {
    let here = |p: PathBuf| std::fs::canonicalize(&p).map_err(|e| format!("{}: {e}", p.display()));
    match what {
        Thread::List => threads::list(home),
        Thread::Show { agent, session } => threads::show(home, &agent, &session),
        Thread::Start {
            project,
            prompt,
            agent,
            wait,
            on,
        } => threads::say(
            home,
            on.as_deref(),
            agent,
            None,
            &here(project)?,
            prompt,
            wait,
        ),
        Thread::Send {
            agent,
            session,
            prompt,
            wait,
            on,
        } => {
            let index = store::load(home)?;
            let project = (index.sessions.iter())
                .find(|m| m.agent == agent && m.id.as_ref().is_some_and(|id| *id.0 == *session))
                .map(|m| m.project.clone())
                .ok_or_else(|| format!("Ghost keeps no session {session} of {agent}"))?;
            threads::say(
                home,
                on.as_deref(),
                Some(agent),
                Some(session),
                &project,
                prompt,
                wait,
            )
        }
    }
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
    fn one_parser_reads_the_words_the_folders_and_an_agents_mcp() {
        use clap::Parser as _;
        let read = |args: &[&str]| {
            Cli::try_parse_from(std::iter::once("ghost").chain(args.iter().copied()))
        };
        let cli = read(&["--shell", "service", "install"]).unwrap();
        assert!(
            cli.shell
                && matches!(
                    cli.word,
                    Some(Word::Service {
                        what: Service::Install
                    })
                )
        );
        let cli = read(&["--shell", "a", "b"]).unwrap();
        assert_eq!(cli.folders, [PathBuf::from("a"), PathBuf::from("b")]);
        let cli = read(&[
            "--shell",
            "thread",
            "send",
            "a",
            "s1",
            "Now fix it",
            "--wait",
            "--on",
            "box",
        ])
        .unwrap();
        assert!(matches!(
            cli.word,
            Some(Word::Thread {
                what: Thread::Send {
                    wait: true,
                    on: Some(_),
                    ..
                }
            })
        ));
        assert!(read(&["--shell", "workflow", "start", "release"]).is_ok());
        let cli = read(&["--mcp", "--on", "box"]).unwrap();
        assert!(cli.mcp && cli.on.as_deref() == Some("box"));
        assert!(read(&["--on", "box"]).is_err(), "--on is --mcp's");
        // What macOS may pass an app it starts does not read: the window
        // opens.
        assert!(read(&["-psn_0_12345"]).is_err());
    }

    #[test]
    fn each_home_has_a_service_of_its_own() {
        let (a, b) = (label(Path::new("/tmp/a")), label(Path::new("/tmp/b")));
        assert!(a.starts_with("local.ghost.server."));
        assert_ne!(a, b);
        let p = String::from_utf8(agent_plist(
            &a,
            Path::new("/Applications/Ghost.app/x"),
            Path::new("/h&o"),
            Path::new("/l"),
        ))
        .unwrap();
        assert!(p.contains("<string>serve</string>"));
        assert!(p.contains("<string>/h&amp;o</string>"), "{p}");
    }
}
