//! Every tool there is, in a conversation and at work, defined once.
//!
//! What she can do does not depend on which loop she is in: the two differ
//! in pace and in budget. So there is one list. Each tool says which loops
//! it belongs to and what it needs before it can be offered at all: a
//! machine, the web, a place that takes files. Whoever offers tools goes
//! through this list in order, and offers what belongs to its loop and
//! whose needs are met. What each tool is said to be is in `tools.toml`;
//! the shared ones run through the same [`Tool`](crate::Tool) in either
//! loop, and a conversation runs them off to the side, a few seconds each.
//!
//! A tool that belongs to one loop alone does so because of what it is.
//! Saying something, reacting, leaving, taking work on and steering it are
//! things she does with people in front of her. Planning, finishing,
//! asking whoever the work is for, telling them as it goes, handing a part
//! to a helper and writing a handoff only mean something inside a piece of
//! work.

use crate::prompts::Loop;

/// What a tool needs before it can be offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A machine to run things on.
    Machine,
    /// The web, to open pages on.
    Web,
    /// Somewhere to search the web.
    Search,
    /// A place that takes files.
    Files,
    /// A body that can take work on.
    Work,
    /// Work under way that can be steered or stopped.
    UnderWay,
    /// Her memory to search.
    Memory,
    /// Someone the work is for, who can be asked and answer.
    Someone,
    /// Someone who follows the work as it runs.
    Follower,
    /// A way to equip a helper.
    Helpers,
}

/// One tool: its name, the loops it belongs to, and what it needs.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub name: &'static str,
    pub loops: &'static [Loop],
    pub needs: &'static [Need],
}

impl Entry {
    /// Whether it is offered in `lp` where what is had is `have`.
    pub fn offered(&self, lp: Loop, have: &[Need]) -> bool {
        self.loops.contains(&lp) && self.needs.iter().all(|n| have.contains(n))
    }

    /// Whether it belongs to both loops.
    pub fn shared(&self) -> bool {
        self.loops.len() > 1
    }
}

const TALK: &[Loop] = &[Loop::Conversation];
const WORK: &[Loop] = &[Loop::Work];
const BOTH: &[Loop] = &[Loop::Conversation, Loop::Work];

const fn entry(name: &'static str, loops: &'static [Loop], needs: &'static [Need]) -> Entry {
    Entry { name, loops, needs }
}

/// Every tool, in the order a conversation is offered them.
pub const TOOLS: &[Entry] = &[
    entry("say", TALK, &[]),
    entry("react", TALK, &[]),
    entry("later", TALK, &[]),
    entry("note", BOTH, &[]),
    entry("feel", TALK, &[]),
    entry("intend", TALK, &[]),
    entry("canon", TALK, &[]),
    entry("recall", BOTH, &[Need::Memory]),
    entry("reread", TALK, &[Need::Memory]),
    entry("keep", BOTH, &[]),
    entry("let_go", TALK, &[]),
    entry("search", BOTH, &[Need::Search]),
    entry("fetch", BOTH, &[Need::Web]),
    entry("shell", BOTH, &[Need::Machine]),
    entry("read", BOTH, &[Need::Machine]),
    entry("write", BOTH, &[Need::Machine]),
    entry("edit", BOTH, &[Need::Machine]),
    entry("undertake", TALK, &[Need::Work]),
    entry("steer", TALK, &[Need::UnderWay]),
    entry("stop", TALK, &[Need::UnderWay]),
    entry("send_file", TALK, &[Need::Files]),
    entry("leave", TALK, &[]),
    entry("plan", WORK, &[]),
    entry("done", WORK, &[]),
    entry("handoff", WORK, &[]),
    entry("ask", WORK, &[Need::Someone]),
    entry("tell", WORK, &[Need::Follower]),
    entry("delegate", WORK, &[Need::Helpers]),
];

/// The tool called `name`, if there is one.
pub fn find(name: &str) -> Option<&'static Entry> {
    TOOLS.iter().find(|e| e.name == name)
}

/// The tools offered in `lp` where what is had is `have`, in order.
pub fn offered(lp: Loop, have: &[Need]) -> impl Iterator<Item = &'static Entry> + '_ {
    TOOLS.iter().filter(move |e| e.offered(lp, have))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{Prompts, machine::Local, tools, undertaking::Undertaking};

    #[test]
    fn every_tool_is_described_and_every_tool_a_worker_carries_belongs_to_work() {
        let words = Prompts::shipped();
        for e in TOOLS {
            assert!(
                !words.tool(e.name, e.loops[0]).is_empty(),
                "{} is not described",
                e.name
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let u = Arc::new(Undertaking::open(&dir.path().join("w.db"), "main", Some("x")).unwrap());
        let carried = tools::with_web(
            tools::standard(Arc::new(Local::new(dir.path())), &u),
            &tools::WebConfig {
                search: Some(tools::Source::Searxng { url: "x".into() }),
                reader_key: None,
            },
        )
        .with(tools::Keep::new(u.clone()))
        .with(tools::Tell::new(u.clone()))
        .with(tools::Ask::new(u.clone()))
        .with(tools::Handoff::new(u));
        for name in carried.names() {
            let e = find(&name).unwrap_or_else(|| panic!("{name} is not in the catalog"));
            assert!(
                e.loops.contains(&Loop::Work),
                "{name} is not a tool of work"
            );
        }
    }
}
