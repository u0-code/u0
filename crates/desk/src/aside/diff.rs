//! The Diff tab, as t3code's diff panel has it: the changes the scope picks,
//! every file in one list as gpui-kit's diff view reads their patch (folded
//! or open, one column or side by side, wrapped or not), a tree of the files
//! beside it, words found and lit across all of them, lines commented on for
//! the agent, and the commit under it. An edit in the thread is drawn by the
//! same view.

use std::borrow::Cow;
use std::path::Path;

use gpui::component::{
    Sizable as _,
    checkbox::Checkbox,
    diff::{
        Diff, DiffAnnotation, DiffChangeIndicator, DiffFile, DiffLinePosition, DiffMode, DiffSide,
        DiffSpanHighlight, DiffState,
    },
    input::Input,
    list::ListItem,
    tree::{TreeItem, tree},
};
use gpui::{AnyElement, App, Context, Div, SharedString, div, prelude::*, px};

use super::{empty, fine};
use crate::{
    desk::Desk,
    fonts::Faces,
    git::{self, FileDiff, Scope},
    session::{Item, Session, State},
    theme,
    ui::{self, Weight},
};

/// How tall a line of an edit stands in the thread, and how many show
/// before the rest scrolls.
const ROW: f32 = 20.;
const LINES_SHOWN: usize = 24;

/// The files `scope` changed in `s`: as git read them, or, for one call
/// and where git reads nothing (no repository), as the agent's edits were.
pub fn files_of<'a>(s: &'a Session, scope: &Scope) -> Option<Cow<'a, [FileDiff]>> {
    let from_git = match scope {
        Scope::All => s.total.as_ref(),
        Scope::Turn(n) => s.turns.get(*n).and_then(|t| t.diff.as_ref()),
        Scope::Call(item) => return Some(Cow::Owned(edits(s, Some(*item)))),
        scope => s
            .git
            .scoped
            .as_ref()
            .filter(|(at, _)| at == scope)
            .map(|(_, files)| files),
    };
    match from_git {
        Some(files) => Some(Cow::Borrowed(files)),
        None if !s.repo && *scope == Scope::All => Some(Cow::Owned(edits(s, None))),
        None => None,
    }
}

/// The agent's edits, each a file of its own, its path from the folder; a
/// file edited again is named with which edit it is.
fn edits(s: &Session, only: Option<usize>) -> Vec<FileDiff> {
    let mut out: Vec<FileDiff> = Vec::new();
    for (i, item) in s.items.iter().enumerate() {
        let Item::Tool(t) = item else { continue };
        if only.is_some_and(|o| o != i) {
            continue;
        }
        let diffs = t.call.content.iter().filter_map(|c| match c {
            agent_client_protocol::v1::schema::ToolCallContent::Diff(d) => Some(d),
            _ => None,
        });
        for (n, d) in diffs.enumerate() {
            let mut d = d.clone();
            d.path = d
                .path
                .strip_prefix(&s.meta.folder)
                .unwrap_or(&d.path)
                .to_path_buf();
            let mut file = git::edit_diff(&d, t.starts.get(n).copied().flatten());
            let again = out
                .iter()
                .filter(|f| f.path.starts_with(&file.path))
                .count();
            if again > 0 {
                let named = format!("{} · edit {}", file.path, again + 1);
                file.patch = file
                    .patch
                    .replace(&format!("b/{}\n", file.path), &format!("b/{named}\n"));
                file.patch = file
                    .patch
                    .replace(&format!("a/{}\n", file.path), &format!("a/{named}\n"));
                file.path = named;
            }
            out.push(file);
        }
    }
    out
}

/// What gpui-kit's view reads of `files`: each patch parsed, its language
/// from its own name (a second edit of a file is named for it).
fn parsed(files: &[&FileDiff]) -> Vec<DiffFile> {
    files
        .iter()
        .filter_map(|f| DiffFile::parse(&f.patch).ok())
        .flatten()
        .map(|d| {
            let path = d.path().to_string();
            match path.split_once(" · edit ") {
                Some((real, _)) => {
                    let ext = Path::new(real)
                        .extension()
                        .map(|e| e.to_string_lossy().into_owned());
                    d.with_language(ext.unwrap_or_default())
                }
                None => d,
            }
        })
        .collect()
}

/// Whether a file is a test's, by the names the common languages give
/// them: in a folder of tests, or named for one.
fn is_test(path: &str) -> bool {
    let mut parts = path.split('/').rev();
    let name = parts.next().unwrap_or_default();
    let stem = name.split('.').next().unwrap_or_default();
    parts.any(|d| matches!(d, "test" | "tests" | "__tests__" | "spec" | "fixtures"))
        || name.contains(".test.")
        || name.contains(".spec.")
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with("Test")
        || stem.ends_with("Tests")
}

/// Where `words` (in any case) are in `files`' lines, as gpui-kit's view
/// reads them: each line's place and the byte span found, to light and step
/// through.
fn found(files: &[DiffFile], words: &str) -> Vec<(DiffLinePosition, std::ops::Range<usize>)> {
    let needle = words.to_lowercase();
    let mut hits = Vec::new();
    for (at, text) in files.iter().flat_map(DiffFile::source_lines) {
        let lower = text.to_lowercase();
        // Only where lowering kept every byte where it was.
        if lower.len() == text.len() {
            for (from, _) in lower.match_indices(&needle) {
                hits.push((at.clone(), from..from + needle.len()));
            }
        }
    }
    hits
}

/// A folder of a tree being built: its folders by name, and its files.
#[derive(Default)]
struct Node {
    dirs: std::collections::BTreeMap<String, Node>,
    files: Vec<String>,
}

/// The tree's items under `node`, whose folder is `prefix`: its folders
/// first, open, then its files.
fn items(node: &Node, prefix: &str) -> Vec<TreeItem> {
    let mut out: Vec<TreeItem> = node
        .dirs
        .iter()
        .map(|(name, sub)| {
            let id = format!("{prefix}{name}/");
            TreeItem::new(id.clone(), name.clone())
                .children(items(sub, &id))
                .expanded(true)
        })
        .collect();
    out.extend(node.files.iter().map(|path| {
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        TreeItem::new(path.clone(), name)
    }));
    out
}

/// A tree of `paths` by folder, each folder open.
pub fn tree_of<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<TreeItem> {
    let mut root = Node::default();
    for path in paths {
        let mut at = &mut root;
        let parts: Vec<&str> = path.split('/').collect();
        for dir in &parts[..parts.len() - 1] {
            at = at.dirs.entry((*dir).to_owned()).or_default();
        }
        at.files.push(path.to_owned());
    }
    items(&root, "")
}

impl Desk {
    /// Give the column's diff view the files the scope picks now, when they
    /// are not what it holds: drawn each frame, so set only on a change.
    pub(crate) fn sync_diff(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let files = files_of(s, &self.review.scope).unwrap_or_default();
        let shown: Vec<&FileDiff> = files
            .iter()
            .filter(|f| !(self.review.no_tests && is_test(&f.path)))
            .collect();
        let held = (
            s.key,
            self.review.scope.clone(),
            shown.len(),
            shown.iter().map(|f| f.patch.len()).sum::<usize>(),
        );
        let mode = if self.settings.side_by_side {
            DiffMode::Split
        } else {
            DiffMode::Unified
        };
        let state = self.review.diff.clone();
        if self.review.shown.as_ref() != Some(&held) {
            let parsed = parsed(&shown);
            let folded = self.settings.diff_folded;
            let paths: Vec<String> = shown.iter().map(|f| f.path.clone()).collect();
            let words = self.review.find.read(cx).value().to_string();
            self.review.hits = if words.trim().is_empty() {
                Vec::new()
            } else {
                found(&parsed, &words)
            };
            state.update(cx, |st, cx| {
                st.set_files(parsed, cx);
                st.set_all_files_collapsed(folded, cx);
            });
            self.review.tree.update(cx, |t, cx| {
                t.set_items(tree_of(paths.iter().map(String::as_str)), cx);
            });
            self.review.shown = Some(held);
        }
        let reveal = self.review.reveal.take();
        state.update(cx, |st, cx| {
            st.set_mode(mode, cx);
            if let Some(path) = reveal.filter(|p| st.files().iter().any(|f| f.path() == p)) {
                st.set_file_collapsed(&path, false, cx);
                st.scroll_to_file(&path, cx);
            }
        });
    }

    /// Show `path`'s changes: the Diff tab, its file open and in view.
    pub fn reveal_in_diff(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(folder) = self.session().map(|s| s.meta.folder.clone()) else {
            return;
        };
        let named = path
            .strip_prefix(&folder)
            .unwrap_or(path)
            .display()
            .to_string();
        self.review.reveal = Some(named);
        self.open_surface(crate::desk::Surface::Diff, cx);
    }

    /// Show the commit `id` in the Diff tab, when the session on screen works
    /// in a repository: true when it does.
    pub fn show_commit(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        if !self.session().is_some_and(|s| s.repo) {
            return false;
        }
        self.show_changes(Scope::Commit(id.to_owned()), cx);
        true
    }

    /// What was found again, the view told to read it afresh.
    pub(crate) fn find_in_diff(&mut self, cx: &mut Context<Self>) {
        self.review.shown = None;
        self.review.hit = 0;
        cx.notify();
    }

    /// Bring the `by`th next found line into view, its file opened.
    pub(crate) fn step_hit(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.review.hits.len();
        if n == 0 {
            return;
        }
        let at = (self.review.hit.cast_signed() + by)
            .rem_euclid(n.cast_signed())
            .cast_unsigned();
        self.review.hit = at;
        let (place, _) = self.review.hits[at].clone();
        self.review.diff.update(cx, |st, cx| {
            st.set_file_collapsed(place.path(), false, cx);
            st.scroll_to_line(place, cx);
        });
        cx.notify();
    }

    /// The Diff tab.
    pub(super) fn diff_tab(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let files = files_of(s, &self.review.scope);
        let reading =
            matches!(self.review.scope, Scope::Branch | Scope::Commit(_)) && files.is_none();
        let files = files.unwrap_or_default();
        let (added, gone) = files
            .iter()
            .fold((0, 0), |(a, g), f| (a + f.added, g + f.gone));
        let tests = files.iter().filter(|f| is_test(&f.path)).count();
        let shown = files.len() - if self.review.no_tests { tests } else { 0 };
        let head = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(40.))
            .px(px(12.))
            .when(s.repo, |d| d.child(self.scope_tabs(faces, s, cx)))
            .child(div().flex_1())
            .when(!files.is_empty(), |d| {
                d.child(ui::counts(faces, added, gone, 11.5))
            });
        let tool = |id: &'static str, icon: &'static str, tip: &'static str, on: bool| {
            ui::icon_button(id, icon, tip)
                .size(px(26.))
                .when(on, |b| b.text_color(theme::text()))
        };
        let folded = self
            .review
            .diff
            .read(cx)
            .files()
            .iter()
            .all(|f| self.review.diff.read(cx).is_file_collapsed(f.path()));
        let key = s.key;
        let tools = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .h(px(32.))
            .px(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                tool("diff-tree", "list", "File tree", self.review.tree_open).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.review.tree_open = !this.review.tree_open;
                        cx.notify();
                    }),
                ),
            )
            .child(
                tool(
                    "diff-fold",
                    if folded {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    if folded {
                        "Expand all files"
                    } else {
                        "Collapse all files"
                    },
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.review
                        .diff
                        .update(cx, |st, cx| st.set_all_files_collapsed(!folded, cx));
                })),
            )
            .child(
                tool(
                    "diff-split",
                    "panel-right",
                    "Side by side",
                    self.settings.side_by_side,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings.side_by_side = !this.settings.side_by_side;
                    this.save_settings();
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "diff-wrap",
                    "arrow-right",
                    "Wrap long lines",
                    self.settings.diff_wrap,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings.diff_wrap = !this.settings.diff_wrap;
                    this.save_settings();
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "diff-spacing",
                    "minus",
                    "Leave out changes only in spacing",
                    self.settings.no_spacing,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings.no_spacing = !this.settings.no_spacing;
                    this.save_settings();
                    this.refresh_diffs(key);
                    cx.notify();
                })),
            )
            .when(tests > 0, |d| {
                d.child(
                    tool(
                        "diff-tests",
                        "file",
                        "Hide test files",
                        self.review.no_tests,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.review.no_tests = !this.review.no_tests;
                        cx.notify();
                    })),
                )
            })
            .child(
                tool(
                    "diff-find",
                    "search",
                    "Find in the diff",
                    self.review.finding,
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_diff_find(window, cx))),
            )
            .child(div().flex_1())
            .child(
                tool("diff-refresh", "retry", "Read the changes again", false).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.refresh_diffs(key);
                        this.refresh_git(key);
                        cx.notify();
                    }),
                ),
            );
        let find = self.review.finding.then(|| {
            let n = self.review.hits.len();
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(6.))
                .border_b_1()
                .border_color(theme::border())
                .child(div().flex_1().child(Input::new(&self.review.find).small()))
                .child(fine(
                    faces,
                    match n {
                        0 => "none".to_owned(),
                        n => format!("{} of {n}", self.review.hit + 1),
                    },
                ))
                .child(
                    ui::icon_button("hit-up", "arrow-up", "The one before")
                        .size(px(22.))
                        .on_click(cx.listener(|this, _, _, cx| this.step_hit(-1, cx))),
                )
                .child(
                    ui::icon_button("hit-down", "arrow-down", "The next")
                        .size(px(22.))
                        .on_click(cx.listener(|this, _, _, cx| this.step_hit(1, cx))),
                )
        });
        let body = if reading {
            div()
                .flex_1()
                .p(px(16.))
                .child(empty(faces, "Reading git…"))
        } else if shown == 0 {
            div().flex_1().p(px(16.)).child(empty(
                faces,
                if s.repo {
                    "Nothing changed here."
                } else {
                    "Files the agent changes show here."
                },
            ))
        } else {
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .when(self.review.tree_open, |d| d.child(self.diff_tree(cx)))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .child(self.diff_view(s, cx)),
                )
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(head)
            .child(tools)
            .children(find)
            .child(body)
            .child(self.diff_foot(faces, s, cx))
    }

    /// The tree of the files shown: a click brings one into view.
    fn diff_tree(&self, cx: &mut Context<Self>) -> Div {
        let state = self.review.diff.clone();
        let faces = crate::fonts::faces(cx).clone();
        div()
            .flex_none()
            .w(px(180.))
            .h_full()
            .border_r_1()
            .border_color(theme::border())
            .py(px(4.))
            .child(tree(&self.review.tree, move |ix, entry, selected, _, _| {
                let item = entry.item();
                let path = item.id.clone();
                let state = state.clone();
                let folder = entry.is_folder();
                ListItem::new(ix)
                    .selected(selected)
                    .h(px(24.))
                    .pl(px(8. + 12. * entry.depth() as f32))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                ui::icon(
                                    match (folder, entry.is_expanded()) {
                                        (false, _) => "file",
                                        (true, true) => "chevron-down",
                                        (true, false) => "chevron-right",
                                    },
                                    12.,
                                )
                                .text_color(theme::faint()),
                            )
                            .child(ui::one_line(
                                ui::mono(&faces, item.label.clone())
                                    .text_size(px(12.))
                                    .text_color(theme::text2()),
                            )),
                    )
                    .when(!folder, |d| {
                        d.on_click(move |_, _, cx| {
                            state.update(cx, |st, cx| {
                                st.set_file_collapsed(&path, false, cx);
                                st.scroll_to_file(&path, cx);
                            });
                        })
                    })
            }))
    }

    /// gpui-kit's view of the files: each file's head with whether it goes
    /// in the next commit and its path to copy; a line's + to say something
    /// of it to the agent, what was said under it.
    fn diff_view(&self, s: &Session, cx: &mut Context<Self>) -> impl IntoElement {
        let desk = cx.entity();
        let commenting = self.review.commenting.clone();
        let comments = self.review.comments.clone();
        let mut notes: Vec<DiffAnnotation> = comments
            .iter()
            .map(|(path, line, _)| {
                let path = path.display().to_string();
                DiffAnnotation::line(
                    SharedString::from(format!("said-{path}-{line}")),
                    DiffLinePosition::new(path, DiffSide::Modified, *line),
                )
            })
            .collect();
        if let Some((path, line)) = &commenting {
            let path = path.display().to_string();
            notes.push(DiffAnnotation::line(
                "saying",
                DiffLinePosition::new(path, DiffSide::Modified, *line),
            ));
        }
        let lit = self
            .review
            .hits
            .iter()
            .map(|(place, span)| DiffSpanHighlight::new(place.clone(), span.clone()));
        let input = self.review.comment.clone();
        let (repo, left_out) = (s.repo, s.git.left_out.clone());
        let key = s.key;
        let faces = crate::fonts::faces(cx).clone();
        Diff::new(&self.review.diff)
            .size_full()
            .soft_wrap(self.settings.diff_wrap)
            .change_indicator(DiffChangeIndicator::Bars)
            .span_highlights(lit)
            .annotations(notes)
            .render_annotation({
                let (desk, faces) = (desk.clone(), faces.clone());
                move |note, _, _| -> AnyElement {
                    let Some(at) = note.position() else {
                        return div().into_any_element();
                    };
                    let (path, line) = (std::path::PathBuf::from(at.path().as_ref()), at.line());
                    if *note.id() == gpui::ElementId::from("saying") {
                        return div()
                            .px(px(12.))
                            .py(px(6.))
                            .child(Input::new(&input).small())
                            .into_any_element();
                    }
                    let said = comments
                        .iter()
                        .find(|c| c.0 == path && c.1 == line)
                        .map(|c| c.2.clone())
                        .unwrap_or_default();
                    let desk = desk.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mx(px(12.))
                        .my(px(4.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .bg(theme::panel())
                        .border_1()
                        .border_color(theme::border2())
                        .child(ui::text(&faces, said).flex_1().text_size(px(12.5)))
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("unsay-{line}")),
                                "x",
                                "Take it back",
                            )
                            .size(px(20.))
                            .on_click(move |_, _, cx| {
                                desk.update(cx, |d, cx| d.drop_comment(&path, line, cx));
                            }),
                        )
                        .into_any_element()
                }
            })
            .on_add_annotation({
                let desk = desk.clone();
                move |range, window, cx| {
                    let (path, line) =
                        (std::path::PathBuf::from(range.path().as_ref()), range.end());
                    desk.update(cx, |d, cx| d.begin_comment(path, line, window, cx));
                }
            })
            .render_header_suffix(move |file, _, _| {
                let path = file.path().to_string();
                let copied = path.clone();
                let goes = !left_out.contains(&path);
                let desk = desk.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("copy-{path}")),
                            "copy",
                            "Copy its path",
                        )
                        .size(px(20.))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
                        }),
                    )
                    .when(repo, |d| {
                        d.child(
                            Checkbox::new(SharedString::from(format!("goes-{path}")))
                                .small()
                                .checked(goes)
                                .tooltip(if goes {
                                    "In the next commit: click to leave it out"
                                } else {
                                    "Left out of the next commit"
                                })
                                .on_click(move |_: &bool, _, cx: &mut App| {
                                    desk.update(cx, |d, cx| {
                                        if let Some(s) = d.sessions.at_mut(key)
                                            && !s.git.left_out.remove(&path)
                                        {
                                            s.git.left_out.insert(path.clone());
                                        }
                                        cx.notify();
                                    });
                                }),
                        )
                    })
            })
    }

    /// Under the diff: the comments to hand the agent, the commit, the step
    /// a git action is at, and the branch's way on (pull, publish).
    fn diff_foot(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let said = self.review.comments.len();
        let (behind, origin) = s.git.upstream;
        let gh = self.on_path.contains(&"gh");
        let idle = !matches!(s.state, State::Working | State::Opening);
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(8.))
            .p(px(12.))
            .border_t_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .when(said > 0, |d| {
                        let name = self.name_of(&s.meta.agent);
                        d.child(
                            ui::button(
                                "send-comments",
                                format!(
                                    "Put {said} comment{} to {name} in the line",
                                    if said == 1 { "" } else { "s" }
                                ),
                                Weight::Secondary,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.send_comments(window, cx)),
                            ),
                        )
                    })
                    .when(s.repo, |d| {
                        d.child(
                            ui::button(
                                "commit",
                                "Commit…",
                                if idle {
                                    Weight::Primary
                                } else {
                                    Weight::Secondary
                                },
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.toggle_commit(window, cx)),
                            ),
                        )
                    })
                    .child(div().flex_1())
                    .when(behind > 0, |d| {
                        d.child(fine(faces, format!("{behind} behind its upstream")))
                            .child(
                                ui::button("pull", "Pull", Weight::Quiet)
                                    .h(px(24.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.git_action(crate::repo::GitAction::Pull, cx);
                                    })),
                            )
                    })
                    .when(s.repo && !origin && gh, |d| {
                        d.children([("Publish, private", false), ("Public", true)].map(
                            |(label, public)| {
                                ui::button(label, label, Weight::Quiet).h(px(24.)).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        let publish = crate::repo::GitAction::Publish(public);
                                        this.git_action(publish, cx);
                                    }),
                                )
                            },
                        ))
                    }),
            )
            .when(self.review.committing && s.repo, |d| {
                d.child(self.commit_form(faces, s, cx))
            })
            .children(s.git.step.as_ref().map(|step| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::live_dot())
                    .child(fine(faces, format!("{step}…")).text_color(theme::accent()))
            }))
    }

    /// Open or close the line to find words in the diff with.
    pub fn toggle_diff_find(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.review.finding = !self.review.finding;
        if self.review.finding {
            window.focus(
                &gpui::Focusable::focus_handle(self.review.find.read(cx), cx),
                cx,
            );
        } else {
            self.review
                .find
                .update(cx, |i, cx| i.set_value("", window, cx));
            self.find_in_diff(cx);
        }
        cx.notify();
    }

    /// An edit in the thread, `key` naming its card: gpui-kit's view of it,
    /// as tall as its lines up to `LINES_SHOWN`, the rest scrolled to.
    pub(crate) fn edit_view(
        &self,
        key: &str,
        file: &FileDiff,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = self
            .edits
            .borrow_mut()
            .entry(key.to_owned())
            .or_insert_with(|| cx.new(|cx| DiffState::new(parsed(&[file]), cx)))
            .clone();
        let rows = file
            .patch
            .lines()
            .filter(|l| !l.starts_with("---") && !l.starts_with("+++"))
            .count();
        div().h(px(ROW * rows.min(LINES_SHOWN) as f32 + 8.)).child(
            Diff::new(&state)
                .size_full()
                .header_visible(false)
                .soft_wrap(self.settings.diff_wrap)
                .change_indicator(DiffChangeIndicator::Bars),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_files_are_known_by_their_folder_or_name() {
        for named in [
            "tests/hub.rs",
            "src/__tests__/a.tsx",
            "web/a.test.ts",
            "web/a.spec.js",
            "py/test_store.py",
            "go/store_test.go",
            "App/StoreTests.swift",
        ] {
            assert!(is_test(named), "{named}");
        }
        for named in ["src/hub.rs", "src/testing.md", "contest/a.rs", "README.md"] {
            assert!(!is_test(named), "{named}");
        }
    }

    #[test]
    fn words_are_found_on_the_side_and_line_they_are_on() {
        let patch = "--- a/a.rs\n+++ b/a.rs\n@@ -10,2 +10,2 @@\n keep Foo\n-old foo\n+new FOO\n";
        let at = |side, line| DiffLinePosition::new("a.rs", side, line);
        assert_eq!(
            found(&DiffFile::parse(patch).unwrap(), "foo"),
            [
                (at(DiffSide::Modified, 10), 5..8),
                (at(DiffSide::Original, 11), 4..7),
                (at(DiffSide::Modified, 11), 4..7),
            ]
        );
    }

    #[test]
    fn an_edit_reads_as_gpui_kit_reads_a_file_it_numbered_from_where_it_starts() {
        let d = agent_client_protocol::v1::schema::Diff::new("src/a.rs", "fn a() {\n    2\n}\n")
            .old_text(Some("fn a() {\n    1\n}\n".to_owned()));
        let file = git::edit_diff(&d, Some(10));
        let read = parsed(&[&file]);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].path().as_ref(), "src/a.rs");
        assert_eq!((read[0].additions(), read[0].deletions()), (1, 1));
        let at = DiffLinePosition::new("src/a.rs", DiffSide::Modified, 11);
        assert!(found(&read, "2").contains(&(at, 4..5)));
    }

    #[test]
    fn a_tree_holds_each_folder_once_above_its_files() {
        let items = tree_of(["src/a.rs", "src/ui/b.rs", "README.md"]);
        let names: Vec<&str> = items.iter().map(|i| i.label.as_ref()).collect();
        assert_eq!(names, ["src", "README.md"]);
        assert_eq!(items[0].children.len(), 2);
    }
}
