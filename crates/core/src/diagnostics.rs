//! What Ghost can say of itself: the processes it runs, as a tree under
//! its own (agents, what they started, terminals), each with its CPU and
//! memory, and this Mac's load; a signal sent to one of them; and what
//! went wrong, as its log says.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One process under Ghost's: its id, how deep under Ghost's it is (1, a
/// child), its CPU in percent of one core, its memory, and its program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proc {
    pub pid: u32,
    pub depth: usize,
    pub cpu: f32,
    pub kb: u64,
    pub name: String,
}

/// Ghost's processes, and this Mac: its load over one, five and fifteen
/// minutes, its cores and its memory in bytes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Processes {
    pub procs: Vec<Proc>,
    pub load: String,
    pub cores: u32,
    pub memory: u64,
}

/// The processes under `root`, as `ps` lists them, in a tree's order.
pub fn processes(root: u32) -> Processes {
    // The ps that lists them is left out of what it lists.
    let (ps, own) = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,pcpu=,rss=,comm="])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|child| {
            let own = child.id();
            child
                .wait_with_output()
                .map(|o| (String::from_utf8_lossy(&o.stdout).into_owned(), own))
        })
        .unwrap_or_default();
    let sysctl = |name: &str| {
        std::process::Command::new("sysctl")
            .args(["-n", name])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default()
    };
    Processes {
        procs: tree(&ps, root)
            .into_iter()
            .filter(|p| p.pid != own)
            .collect(),

        load: sysctl("vm.loadavg")
            .trim_matches(['{', '}', ' '])
            .to_owned(),
        cores: sysctl("hw.ncpu").parse().unwrap_or(0),
        memory: sysctl("hw.memsize").parse().unwrap_or(0),
    }
}

/// The processes `ps` printed under `root`, each after its parent.
fn tree(ps: &str, root: u32) -> Vec<Proc> {
    let all: Vec<(u32, u32, f32, u64, String)> = ps
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let pid = w.next()?.parse().ok()?;
            let parent = w.next()?.parse().ok()?;
            let cpu = w.next()?.parse().ok()?;
            let kb = w.next()?.parse().ok()?;
            let name = w.collect::<Vec<_>>().join(" ");
            let name = name.rsplit('/').next().unwrap_or(&name).to_owned();
            Some((pid, parent, cpu, kb, name))
        })
        .collect();
    let mut out = Vec::new();
    walk(&all, root, 1, &mut out);
    out
}

/// Those of `all` whose parent is `at`, each followed by those under it.
fn walk(all: &[(u32, u32, f32, u64, String)], at: u32, depth: usize, out: &mut Vec<Proc>) {
    for (pid, parent, cpu, kb, name) in all {
        if *parent == at {
            out.push(Proc {
                pid: *pid,
                depth,
                cpu: *cpu,
                kb: *kb,
                name: name.clone(),
            });
            walk(all, *pid, depth + 1, out);
        }
    }
}

/// Send the process `pid` the signal `signal` (TERM, KILL), where it is
/// one of those under `root`; what was done, or why not.
pub fn signal(root: u32, pid: u32, signal: &str) -> Result<String, String> {
    if !processes(root).procs.iter().any(|p| p.pid == pid) {
        return Err(format!("{pid} is not one of Ghost's processes"));
    }
    let done = std::process::Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .map_err(|e| e.to_string())?;
    if done.success() {
        Ok(format!("Sent {signal} to {pid}"))
    } else {
        Err(format!("kill said no to {pid}"))
    }
}

/// What went wrong lately, as Ghost's log in `home` says: its warnings
/// and errors, the last `most`, newest first.
pub fn troubles(home: &Path, most: usize) -> Vec<String> {
    let mut files: Vec<_> = std::fs::read_dir(home.join("logs"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    let mut found = Vec::new();
    for file in files.iter().rev() {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        found.extend(
            text.lines()
                .rev()
                .filter(|l| l.contains(" ERROR ") || l.contains(" WARN "))
                .map(str::to_owned),
        );
        if found.len() >= most {
            break;
        }
    }
    found.truncate(most);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_processes_under_ghost_are_listed_each_after_its_parent() {
        let ps = "  1     0  0.1  100 /sbin/launchd
 10     1  2.0 2048 /Applications/Ghost.app/Contents/MacOS/ghost-desk
 20    10 12.5 4096 /x/agents/claude-acp/claude-agent-acp
 30    20  0.0  512 node
 40    10  0.3  256 -zsh
 50     1  9.9  999 other";
        let t = tree(ps, 10);
        assert_eq!(
            t.iter().map(|p| (p.pid, p.depth)).collect::<Vec<_>>(),
            [(20, 1), (30, 2), (40, 1)]
        );
        assert_eq!(
            (t[0].name.as_str(), t[0].cpu, t[0].kb),
            ("claude-agent-acp", 12.5, 4096)
        );
    }

    #[test]
    fn a_signal_is_refused_for_a_process_not_ghosts() {
        assert!(signal(std::process::id(), 1, "TERM").is_err());
    }

    #[test]
    fn the_logs_warnings_and_errors_come_newest_first() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("logs")).unwrap();
        std::fs::write(
            home.path().join("logs/ghost.2026-11-25.log"),
            "t1  INFO ghost: started\nt2  WARN ghost: slow\nt3 ERROR ghost: broke\n",
        )
        .unwrap();
        let t = troubles(home.path(), 10);
        assert_eq!(t.len(), 2);
        assert!(t[0].contains("broke") && t[1].contains("slow"));
    }
}
