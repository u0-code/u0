//! What u0 can say of itself: the processes it runs, as a tree under
//! its own (agents, what they started, terminals), each with its CPU and
//! memory, and this Mac's load; a signal sent to one of them; and what
//! went wrong, as its log says.

use std::{
    path::Path,
    sync::{LazyLock, Mutex},
};

use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System};

/// One process under u0's: its id, how deep under u0's it is (1, a
/// child), its CPU in percent of one core, its memory, and its program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proc {
    pub pid: u32,
    pub depth: usize,
    pub cpu: f32,
    pub kb: u64,
    pub name: String,
}

/// u0's processes, and this Mac: its load over one, five and fifteen
/// minutes, its cores and its memory in bytes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Processes {
    pub procs: Vec<Proc>,
    pub load: String,
    pub cores: u32,
    pub memory: u64,
}

/// The machine as last read: kept between reads, since a process's CPU is
/// what it used between two of them. None where it cannot be read.
static SYSTEM: LazyLock<Option<Mutex<System>>> =
    LazyLock::new(|| System::new().ok().map(Mutex::new));

/// The machine as last read, held.
fn system() -> Option<std::sync::MutexGuard<'static, System>> {
    let held = SYSTEM.as_ref()?.lock();
    Some(held.unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// A process as read: its id, its parent's, its CPU, its memory in
/// kilobytes and its program.
type Read = (u32, u32, f32, u64, String);

/// The processes under `root`, in a tree's order, and this machine.
pub fn processes(root: u32) -> Processes {
    let Some(mut system) = system() else {
        return Processes::default();
    };
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
    system.refresh_memory();
    system.refresh_cpu_list(CpuRefreshKind::nothing());
    let all: Vec<Read> = system
        .processes()
        .iter()
        .map(|(pid, p)| {
            (
                pid.as_u32(),
                p.parent().map_or(0, Pid::as_u32),
                p.cpu_usage(),
                p.memory() / 1024,
                p.name().to_string_lossy().into_owned(),
            )
        })
        .collect();
    let mut procs = Vec::new();
    walk(&all, root, 1, &mut procs);
    Processes {
        procs,
        load: System::load_average().map_or_else(
            |_| String::new(),
            |l| format!("{:.2} {:.2} {:.2}", l.one, l.five, l.fifteen),
        ),
        cores: system.cpus().len() as u32,
        memory: system.total_memory(),
    }
}

/// Those of `all` whose parent is `at`, each followed by those under it.
fn walk(all: &[Read], at: u32, depth: usize, out: &mut Vec<Proc>) {
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
        return Err(format!("{pid} is not one of u0's processes"));
    }
    let sent = match signal {
        "KILL" => Signal::Kill,
        _ => Signal::Term,
    };
    if send(pid, sent) {
        Ok(format!("Sent {signal} to {pid}"))
    } else {
        Err(format!("{pid} could not be sent {signal}"))
    }
}

/// Ask the process `pid`, one u0 started, to end (TERM).
pub fn terminate(pid: u32) {
    send(pid, Signal::Term);
}

/// Send `pid` the signal `sent`; whether it was.
fn send(pid: u32, sent: Signal) -> bool {
    let Some(mut system) = system() else {
        return false;
    };
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing(),
    );
    system.process(pid).and_then(|p| p.kill_with(sent)) == Some(true)
}

/// What went wrong lately, as u0's log in `home` says: its warnings
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
    fn the_processes_under_u0_are_listed_each_after_its_parent() {
        let read = |pid, parent, cpu, kb, name: &str| (pid, parent, cpu, kb, name.to_owned());
        let all = [
            read(1, 0, 0.1, 100, "launchd"),
            read(10, 1, 2.0, 2048, "u0-desk"),
            read(20, 10, 12.5, 4096, "claude-agent-acp"),
            read(30, 20, 0.0, 512, "node"),
            read(40, 10, 0.3, 256, "zsh"),
            read(50, 1, 9.9, 999, "other"),
        ];
        let mut t = Vec::new();
        walk(&all, 10, 1, &mut t);
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
    fn this_machine_and_the_processes_under_this_one_are_read() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let here = processes(std::process::id());
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            here.procs
                .iter()
                .any(|p| p.pid == child.id() && p.depth == 1)
        );
        assert!(here.cores > 0 && here.memory > 0);
    }

    #[test]
    fn a_signal_is_refused_for_a_process_not_u0s() {
        assert!(signal(std::process::id(), 1, "TERM").is_err());
    }

    #[test]
    fn the_logs_warnings_and_errors_come_newest_first() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("logs")).unwrap();
        std::fs::write(
            home.path().join("logs/u0.2026-11-25.log"),
            "t1  INFO u0: started\nt2  WARN u0: slow\nt3 ERROR u0: broke\n",
        )
        .unwrap();
        let t = troubles(home.path(), 10);
        assert_eq!(t.len(), 2);
        assert!(t[0].contains("broke") && t[1].contains("slow"));
    }
}
