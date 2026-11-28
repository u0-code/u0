//! A pull request reviewed on the PRs page, as t3code reviews one: its
//! files and how much of each kind they change (`core::review`), the ones
//! you viewed, comments on its lines that go out together as a review, its
//! conversations answered and resolved, and a guide to reading it, which
//! the writer writes.

use std::collections::BTreeMap;

use gpui::component::input::{InputEvent, InputState};
use gpui::{AppContext as _, Context, Entity, Window};
use u0ui::t3::review::MergeMethod;

use super::Desk;
use crate::{github::Line, review::File, writer::Ask};

/// The files you viewed: by pull request (its address) and path, the mark
/// of its changes as you viewed it.
type Viewed = BTreeMap<String, BTreeMap<String, String>>;

/// The pull request read, as it is reviewed.
pub struct Review {
    /// Its files, from its patch.
    pub files: Vec<File>,
    /// The files you viewed, kept in `viewed.json`.
    pub viewed: Viewed,
    /// Comments on its lines, to go out with the next review.
    pub lines: Vec<Line>,
    /// The line being commented on, and the field for it.
    pub commenting: Option<(String, u64)>,
    pub note: Entity<InputState>,
    /// The conversations resolved or only bots' shown too, as they are
    /// not at first.
    pub all_threads: bool,
    /// The guide to it: for which pull request, at which commit, and what
    /// the writer wrote (none while it writes).
    pub guide: Option<(u64, String, Option<Written>)>,
    /// Current stop index inside the parsed review guide.
    pub stop: usize,
    /// How it is to be merged, the project's way until another is chosen.
    pub method: MergeMethod,
}

/// One structured stop parsed from a writer's Markdown review guide: its
/// title, the first file it names and how many it names, and its note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideStopData {
    pub title: String,
    pub file: Option<String>,
    pub files: usize,
    pub note: String,
}

/// The paths a line of the guide names in backticks, each once.
fn paths_in(line: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for code in line.split('`').skip(1).step_by(2) {
        if (code.contains('/') || code.contains('.')) && !found.iter().any(|f| f == code) {
            found.push(code.to_owned());
        }
    }
    found
}

/// Parse headings or numbered list items in `markdown` into structured review stops.
pub fn parse_guide_stops(markdown: &str) -> Vec<GuideStopData> {
    let mut stops: Vec<GuideStopData> = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        let heading = trimmed
            .strip_prefix("### ")
            .or_else(|| trimmed.strip_prefix("## "))
            .or_else(|| {
                let (num, rest) = trimmed.split_once(". ")?;
                (!num.is_empty() && num.chars().all(|c| c.is_ascii_digit())).then_some(rest)
            });
        if let Some(raw) = heading {
            let title = raw.trim().trim_matches('*').to_owned();
            let paths = paths_in(&title);
            stops.push(GuideStopData {
                title,
                file: paths.first().cloned(),
                files: paths.len(),
                note: String::new(),
            });
        } else if let Some(last) = stops.last_mut()
            && !trimmed.is_empty()
            && last.note.is_empty()
        {
            let paths = paths_in(trimmed);
            if last.file.is_none() {
                last.file = paths.first().cloned();
            }
            last.files = last.files.max(paths.len());
            trimmed
                .trim_start_matches(['-', '*', ' '])
                .clone_into(&mut last.note);
        }
    }
    stops
}

/// What the writer wrote, or why it did not.
type Written = Result<String, String>;

impl Review {
    pub(super) fn new(home: &std::path::Path, window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let note =
            cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("Comment on this line")));
        cx.subscribe_in(
            &note,
            window,
            |this: &mut Desk, _, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::PressEnter { .. }) {
                    this.add_line(cx);
                }
            },
        )
        .detach();
        Self {
            files: Vec::new(),
            viewed: crate::whole::read_json(&home.join(VIEWED)).unwrap_or_default(),
            lines: Vec::new(),
            commenting: None,
            note,
            all_threads: false,
            guide: None,
            stop: 0,
            method: MergeMethod::Squash,
        }
    }
}

/// Where the files you viewed are kept, in the home.
const VIEWED: &str = "viewed.json";

impl Desk {
    /// The address of the pull request read, by which what you viewed of
    /// it is kept.
    fn read_url(&self) -> Option<String> {
        match &self.forge.detail {
            Some(Ok(d)) => Some(d.item.url.clone()),
            _ => None,
        }
    }

    /// Whether you viewed `file` of the pull request read, as it is now.
    pub fn viewed(&self, file: &File) -> bool {
        let r = &self.forge.pr;
        self.read_url()
            .and_then(|url| r.viewed.get(&url)?.get(&file.path))
            .is_some_and(|mark| *mark == file.mark)
    }

    /// Mark the file at `path` viewed, or not; a viewed one is folded.
    pub fn toggle_viewed(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(url) = self.read_url() else {
            return;
        };
        let Some(file) = self.forge.pr.files.iter().find(|f| f.path == path).cloned() else {
            return;
        };
        let now = !self.viewed(&file);
        let of = self.forge.pr.viewed.entry(url.clone()).or_default();
        if now {
            of.insert(file.path.clone(), file.mark.clone());
        } else {
            of.remove(&file.path);
            if of.is_empty() {
                self.forge.pr.viewed.remove(&url);
            }
        }
        self.forge
            .diff
            .update(cx, |d, cx| d.set_file_collapsed(path, now, cx));
        if let Ok(text) = serde_json::to_vec_pretty(&self.forge.pr.viewed) {
            crate::store::queue(crate::store::Job::Write(self.home.join(VIEWED), text));
        }
        cx.notify();
    }

    /// The patch of the pull request read came: its files, those you
    /// viewed folded.
    pub(super) fn pr_patch(&mut self, patch: &str, cx: &mut Context<Self>) {
        self.forge.pr.files = crate::review::files(patch);
        let folded: Vec<String> = (self.forge.pr.files.iter())
            .filter(|f| self.viewed(f))
            .map(|f| f.path.clone())
            .collect();
        self.forge.diff.update(cx, |d, cx| {
            for path in &folded {
                d.set_file_collapsed(path, true, cx);
            }
        });
    }

    /// Another pull request is read: what was begun on the last is put
    /// away, and it is to be merged the project's way.
    pub(super) fn pr_changed(&mut self) {
        let how = self.settings_in(self.forge.folder.as_ref()).merge.gh();
        self.forge.pr.method = match how {
            "merge" => MergeMethod::Merge,
            "rebase" => MergeMethod::Rebase,
            _ => MergeMethod::Squash,
        };
        let r = &mut self.forge.pr;
        r.files.clear();
        r.lines.clear();
        r.commenting = None;
        r.stop = 0;
    }

    /// Begin a comment on line `line` of `path`.
    pub fn begin_line(
        &mut self,
        path: String,
        line: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let said = (self.forge.pr.lines.iter())
            .find(|l| l.path == path && l.line == line)
            .map(|l| l.body.clone())
            .unwrap_or_default();
        self.forge.pr.commenting = Some((path, line));
        let note = self.forge.pr.note.clone();
        note.update(cx, |i, cx| {
            i.set_value(said, window, cx);
            window.focus(&gpui::Focusable::focus_handle(&*i, cx), cx);
        });
        cx.notify();
    }

    /// The comment written, kept with its line for the next review.
    fn add_line(&mut self, cx: &mut Context<Self>) {
        let body = self.forge.pr.note.read(cx).value().trim().to_owned();
        let r = &mut self.forge.pr;
        if let Some((path, line)) = r.commenting.take() {
            r.lines.retain(|l| !(l.path == path && l.line == line));
            if !body.is_empty() {
                r.lines.push(Line { path, line, body });
            }
        }
        cx.notify();
    }

    pub fn drop_line(&mut self, path: &str, line: u64, cx: &mut Context<Self>) {
        (self.forge.pr.lines).retain(|l| !(l.path == path && l.line == line));
        cx.notify();
    }

    /// Have the writer write a guide to the pull request read.
    pub fn write_guide(&mut self, cx: &mut Context<Self>) {
        let (Some(Ok(d)), Some(p)) = (&self.forge.detail, self.forge.folder.clone()) else {
            return;
        };
        let (n, head) = (d.item.number, d.head.clone());
        self.forge.pr.guide = Some((n, head.clone(), None));
        self.ask_writer(
            0,
            Ask::Guide {
                dir: p.folder,
                n,
                head,
            },
        );
        cx.notify();
    }

    /// The guide written to the pull request `n` at `head`, or why not.
    pub(super) fn guide_written(&mut self, n: u64, head: String, text: Result<String, String>) {
        self.forge.pr.guide = Some((n, head, Some(text)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_headings_and_numbered_stops_extract_titles_files_and_notes() {
        let md = "## 1. Range state\n- `src/date-picker/state.ts` anchors the end date.\n2. **Inputs & labels**\nAria labels in `src/a.tsx` and `src/b.tsx`, `src/a.tsx` first.";
        let stops = parse_guide_stops(md);
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].title, "1. Range state");
        assert_eq!(stops[0].file.as_deref(), Some("src/date-picker/state.ts"));
        assert_eq!(
            stops[0].note,
            "`src/date-picker/state.ts` anchors the end date."
        );
        assert_eq!(stops[1].title, "Inputs & labels");
        assert_eq!(
            stops[1].note,
            "Aria labels in `src/a.tsx` and `src/b.tsx`, `src/a.tsx` first."
        );
        assert_eq!((stops[0].files, stops[1].files), (1, 2));
    }
}
