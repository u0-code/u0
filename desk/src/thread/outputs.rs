//! The files a session's work handed over, as its journal tells them:
//! those its reports name, and those it kept along the way, for the
//! Outputs down the right.

use std::collections::HashSet;

use serde_json::Value;

use super::{s, session_tasks};
use crate::model::Entry;

/// Where a file handed over by work came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A path on its machine, named by a `reported` entry.
    Report,
    /// A file it kept in its `media/` while working, from a `kept` entry.
    Kept,
}

/// A file a session's work handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// As the journal names it: a path on the machine for a report, a name
    /// in `media/` for something kept.
    pub file: String,
    pub source: Source,
}

impl Output {
    /// Its file name, without the directories.
    pub fn name(&self) -> &str {
        self.file.rsplit('/').next().unwrap_or(&self.file)
    }

    /// What sort of file it is, by its extension, and where it came from.
    pub fn kind(&self) -> String {
        let ext = self
            .name()
            .rsplit_once('.')
            .map_or("file", |(_, e)| e)
            .to_lowercase();
        let from = match self.source {
            Source::Report => "handed over",
            Source::Kept => "kept",
        };
        format!("{ext} · {from}")
    }
}

/// The files one session's work handed over, newest first and each once:
/// those its reports name, and those it kept along the way and has not
/// let go of. Read from the journal alone.
pub fn outputs(journal: &[Entry], session: &str) -> Vec<Output> {
    let tasks = session_tasks(journal, session);
    let ids: HashSet<&str> = tasks
        .iter()
        .flat_map(|t| std::iter::once(t.id.as_str()).chain(t.parts.iter().map(|p| p.id.as_str())))
        .collect();
    let let_go: HashSet<i64> = journal
        .iter()
        .filter(|e| e.kind == "let_go")
        .filter_map(|e| e.data.get("kept").and_then(Value::as_i64))
        .collect();
    let mut out: Vec<Output> = Vec::new();
    for e in journal.iter().rev() {
        let found: Vec<Output> = match e.kind.as_str() {
            "reported" if ids.contains(s(&e.data, "id")) => e
                .data
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|f| Output {
                    file: f.to_owned(),
                    source: Source::Report,
                })
                .collect(),
            "kept" if ids.contains(s(&e.data, "from")) && !let_go.contains(&e.seq) => {
                vec![Output {
                    file: s(&e.data, "file").to_owned(),
                    source: Source::Kept,
                }]
            }
            _ => Vec::new(),
        };
        for o in found {
            if !o.file.is_empty() && !out.iter().any(|x| x.file == o.file) {
                out.push(o);
            }
        }
    }
    out
}
