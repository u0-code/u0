//! The `u0` words that ask a server, as a desk asks it: `thread` and
//! `workflow`. What only reads is read from this home; what acts goes
//! over a link to a server this Mac is paired with (`--on`, or this
//! Mac's service), signed in by this Mac's key as the window is, and is
//! checked against that device's scope there.

use std::{fmt::Write as _, path::Path};

use agent_client_protocol::v1::schema::SessionId;

use crate::{
    proto::{Call, Event},
    servers, settings, store, when, workflows,
};

/// How to reach the server named `on`, or this Mac's service: its way
/// and this Mac's key for it.
fn server(home: &Path, on: Option<&str>) -> Result<(crate::reach::Way, crate::key::Key), String> {
    let all = servers::Servers::load(home)?;
    let port = crate::access::Access::new(home).kept()?.port;
    let found = match on {
        Some(name) => all.servers.iter().find(|s| s.name == name),
        None => all.service(port),
    };
    found
        .ok_or_else(|| match on {
            Some(name) => format!("This Mac is not paired with {name}"),
            None => "Name a server this Mac is paired with (--on), or run `u0 service install`: the window's own server is not reached from a terminal.".into(),
        })?
        .reach()
}

/// Ask the server `on` (or this Mac's service) `call`, and hand what it
/// says to `heard` until it has what it waits for.
fn ask<T>(
    home: &Path,
    on: Option<&str>,
    call: Call,
    heard: impl FnMut(Event) -> Option<Result<T, String>>,
) -> Result<T, String> {
    let (way, key) = server(home, on)?;
    crate::acp::runtime().block_on(crate::link::ask(&way, &key, call, heard))
}

/// The threads kept, the latest first: when, the agent, the id, the
/// project and the title.
pub fn list(home: &Path) -> Result<String, String> {
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
        let id = m.id.as_ref().map_or("-", |id| &id.0);
        let _ = writeln!(
            out,
            "{:>5}  {:<14} {:<38} {:<20} {}",
            when::ago(m.updated, now),
            m.agent,
            id,
            project,
            m.title
        );
    }
    Ok(out)
}

/// What was said in the thread `id` of `agent`, as its record has it.
pub fn show(home: &Path, agent: &str, id: &str) -> Result<String, String> {
    let record = store::read(&store::record_path(home, agent, id))
        .ok_or_else(|| format!("u0 keeps no record of {agent} {id}"))?;
    Ok(crate::session::said(record, false))
}

/// Say `prompt` to `agent`, in its thread `session` or a new one in
/// `project`, on the server `on`: the thread it is said in; with `wait`,
/// what the agent answered.
pub fn say(
    home: &Path,
    on: Option<&str>,
    agent: Option<String>,
    session: Option<String>,
    project: &Path,
    prompt: String,
    wait: bool,
) -> Result<String, String> {
    let agent = match agent {
        Some(agent) => agent,
        None => (settings::Settings::load(home).0.enabled.first().cloned())
            .ok_or("Name the agent (--agent): none is on")?,
    };
    let tag = crate::access::number();
    let call = Call::Say {
        tag,
        agent,
        session: session.map(SessionId::new),
        project: project.to_path_buf(),
        prompt,
    };
    ask(home, on, call, |event| match event {
        Event::Turned { tag: t, .. } if t != tag => None,
        Event::Turned {
            session: Err(why), ..
        } => Some(Err(why)),
        Event::Turned {
            session: Ok(id),
            answer,
            ..
        } => match answer {
            Some(answer) if wait => Some(Ok(answer)),
            _ if wait => None,
            _ => Some(Ok(id.0.to_string())),
        },
        _ => None,
    })
}

/// The workflows in this home, and their runs, the latest last.
pub fn workflows(home: &Path) -> Result<String, String> {
    let mut out = String::new();
    for flow in workflows::load(home) {
        let _ = match flow {
            Ok(f) => writeln!(out, "{:<24} {}", f.name, f.about),
            Err((name, why)) => writeln!(out, "{name:<24} does not read: {why}"),
        };
    }
    for run in workflows::load_runs(home)?.runs {
        let _ = writeln!(
            out,
            "run {:<4} {:<24} {:?}, {} steps done",
            run.id,
            run.workflow,
            run.state,
            run.done.len()
        );
    }
    Ok(out)
}

/// Begin a run of the workflow `name` with `input` on the server `on`: the
/// run's number.
pub fn start(home: &Path, on: Option<&str>, name: String, input: String) -> Result<String, String> {
    let began = store::secs(std::time::SystemTime::now());
    let wanted = name.clone();
    let call = Call::StartFlow {
        workflow: name,
        input,
    };
    ask(home, on, call, |event| match event {
        Event::Notice(Err((what, why))) if what.contains(&wanted) => Some(Err(why)),
        Event::Workflows { runs: Ok(runs), .. } => (runs.iter())
            .filter(|r| r.workflow == wanted && r.began >= began)
            .map(|r| r.id)
            .max()
            .map(|id| Ok(format!("run {id}"))),
        _ => None,
    })
}
