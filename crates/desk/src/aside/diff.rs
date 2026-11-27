//! The Diff tab, as Carbon's right panel has it (`RightColumnStates` Columns 1 & 4):
//! the changes the scope picks, every file in one list as gpui-kit's diff view
//! reads their patch (folded or open, one column or side by side, wrapped or
//! not), a tree of the files beside it, words found and lit across all of them,
//! lines commented on for the agent, and the contextual git footer cards under
//! it. An edit in the thread is drawn by the same view.

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
use u0ui::{
    components::{Badge, BadgeSize, BadgeVariant, Button, ButtonSize, ButtonVariant},
    icons::{CarbonIcon, Icon},
    t3::review::DiffStat,
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
};

use super::{empty, fine};
use crate::{
    desk::Desk,
    fonts::Faces,
    git::{self, FileDiff, Scope},
    session::{Item, Session, State},
    theme, ui,
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
        None if s.git_dir().is_none() && *scope == Scope::All => Some(Cow::Owned(edits(s, None))),
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
                let named = format!("{} \u{00b7} edit {}", file.path, again + 1);
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
            match path.split_once(" \u{00b7} edit ") {
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

/// One `.ft` contextual git card under the diff (`RightColumnStates` Column 4).
fn ft_card(
    faces: &Faces,
    heading: impl Into<SharedString>,
    line: Div,
    actions: Option<Div>,
    note: impl Into<SharedString>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .bg(theme::role(ColorRole::Background))
        .border_1()
        .border_color(theme::role(ColorRole::BorderSubtle01))
        .child(
            div().px(px(16.)).pt(px(12.)).pb(px(4.)).child(
                ui::text(faces, heading)
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary)),
            ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .px(px(16.))
                .pt(px(8.))
                .pb(px(16.))
                .child(line)
                .children(actions)
                .child(
                    ui::text(faces, note)
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                ),
        )
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
        if self.session().is_none_or(|s| s.git_dir().is_none()) {
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
        let reading = matches!(
            self.review.scope,
            Scope::Branch | Scope::Commit(_) | Scope::Uncommitted
        ) && files.is_none();
        let files = files.unwrap_or_default();
        let (added, gone) = files
            .iter()
            .fold((0, 0), |(a, g), f| (a + f.added, g + f.gone));
        let tests = files.iter().filter(|f| is_test(&f.path)).count();
        let shown = files.len() - if self.review.no_tests { tests } else { 0 };
        let head = self.diff_bar(faces, s, cx);

        let tool = |id: &'static str, icon: &'static str, tip: &'static str, on: bool| {
            ui::icon_button(id, icon, crate::tr(tip))
                .size(px(26.))
                .when(on, |b| b.text_color(theme::role(ColorRole::TextPrimary)))
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
            .border_color(theme::role(ColorRole::BorderSubtle01))
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
            .when(!files.is_empty(), |d| {
                d.child(div().pr(px(4.)).child(DiffStat::new(added, gone)))
            })
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
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(div().flex_1().child(Input::new(&self.review.find).small()))
                .child(fine(
                    faces,
                    match n {
                        0 => crate::tr("none"),
                        n => crate::tr_with(
                            "%{hit} of %{n}",
                            &[("hit", &(self.review.hit + 1)), ("n", &n)],
                        ),
                    },
                ))
                .child(
                    ui::icon_button("hit-up", "arrow-up", crate::tr("The one before"))
                        .size(px(22.))
                        .on_click(cx.listener(|this, _, _, cx| this.step_hit(-1, cx))),
                )
                .child(
                    ui::icon_button("hit-down", "arrow-down", crate::tr("The next"))
                        .size(px(22.))
                        .on_click(cx.listener(|this, _, _, cx| this.step_hit(1, cx))),
                )
        });
        let body = if reading {
            div()
                .flex_1()
                .p(px(16.))
                .child(empty(faces, crate::tr("Reading git…")))
        } else if shown == 0 {
            div().flex_1().p(px(16.)).child(empty(
                faces,
                if s.git_dir().is_some() {
                    crate::tr("Nothing changed here.")
                } else {
                    crate::tr("Files the agent changes show here.")
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
            .children(self.diff_foot(faces, s, cx))
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
            .border_color(theme::role(ColorRole::BorderSubtle01))
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
                                Icon::carbon(match (folder, entry.is_expanded()) {
                                    (false, _) => CarbonIcon::Document,
                                    (true, true) => CarbonIcon::ChevronDown,
                                    (true, false) => CarbonIcon::ChevronRight,
                                })
                                .size_length(px(12.))
                                .color(theme::role(ColorRole::IconSecondary)),
                            )
                            .child(ui::one_line(
                                ui::mono(&faces, item.label.clone())
                                    .text_size(px(crate::style::CAPTION))
                                    .text_color(theme::role(ColorRole::TextSecondary)),
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

    /// gpui-kit's view of the files.
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
        let (repo, left_out) = (s.git_dir().is_some(), s.git.left_out.clone());
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
                        .bg(theme::role(ColorRole::Layer01))
                        .border_1()
                        .border_color(theme::role(ColorRole::BorderSubtle01))
                        .child(
                            ui::text(&faces, said)
                                .flex_1()
                                .text_size(px(crate::style::CAPTION)),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("unsay-{line}")),
                                "x",
                                crate::tr("Take it back"),
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
                            crate::tr("Copy its path"),
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
                                    crate::tr("In the next commit: click to leave it out")
                                } else {
                                    crate::tr("Left out of the next commit")
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

    /// Under the diff (`RightColumnStates` Column 4): contextual `.ft` cards
    /// offering what git can do for the repository's current state.
    fn diff_foot(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        let said = self.review.comments.len();
        let (ahead, behind, origin) = s.git.upstream;
        let gh = self.on_path.contains(&"gh");
        let idle = !matches!(s.state, State::Working | State::Opening);
        let file_count = files_of(s, &Scope::All).map_or(0, |f| f.len());
        let clean = file_count == 0;
        let has_repo = s.git_dir().is_some();
        let desk = cx.entity();

        let mut cards: Vec<Div> = Vec::new();

        if said > 0 {
            let name = self.name_of(&s.meta.agent);
            let d_send = desk.clone();
            let label = crate::tr_with(
                if said == 1 {
                    "Put %{n} comment to %{name} in the line"
                } else {
                    "Put %{n} comments to %{name} in the line"
                },
                &[("n", &said), ("name", &name)],
            );
            cards.push(
                div().flex().child(
                    Button::new("send-comments", label)
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::Chat)
                        .on_click(move |_, window, cx| {
                            d_send.update(cx, |this, cx| this.send_comments(window, cx));
                        }),
                ),
            );
        }

        if let Some(pr) = s.git.pr.as_ref().filter(|p| p.state == "merged") {
            let base = if pr.base.is_empty() {
                "main"
            } else {
                pr.base.as_str()
            };
            let line = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    Badge::new(crate::tr("Merged"))
                        .variant(BadgeVariant::Purple)
                        .size(BadgeSize::Sm),
                )
                .child(
                    ui::text(
                        faces,
                        crate::tr_with(
                            "#%{n} merged into %{base}",
                            &[("n", &pr.number), ("base", &base)],
                        ),
                    )
                    .type_style(TypeStyle::Body01)
                    .text_color(theme::role(ColorRole::TextPrimary)),
                );
            cards.push(ft_card(
                faces,
                crate::tr("Its pull request merged"),
                line,
                None,
                crate::tr(
                    "The thread settles on its own (Threads \u{203a} Auto-settle merged threads). Its worktree stays until you archive it.",
                ),
            ));
        } else if has_repo && !origin && gh {
            let project = s.meta.project.file_name().map_or_else(
                || Desk::shown(&s.meta.project),
                |n| n.to_string_lossy().into_owned(),
            );
            let line = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    Icon::carbon(CarbonIcon::Earth)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
                .child(
                    ui::text(faces, crate::tr("Not on any host"))
                        .type_style(TypeStyle::Body01)
                        .text_color(theme::role(ColorRole::TextPrimary)),
                );
            let (d_priv, d_pub) = (desk.clone(), desk.clone());
            let acts = div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(
                    Button::new("pub-private", crate::tr("Publish, private"))
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            d_priv.update(cx, |this, cx| {
                                this.git_action(crate::repo::GitAction::Publish(false), cx);
                            });
                        }),
                )
                .child(
                    Button::new("pub-public", crate::tr("Public"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            d_pub.update(cx, |this, cx| {
                                this.git_action(crate::repo::GitAction::Publish(true), cx);
                            });
                        }),
                );
            cards.push(ft_card(
                faces,
                crate::tr_with("No remote yet \u{00b7} %{project}", &[("project", &project)]),
                line,
                Some(acts),
                crate::tr(
                    "A new GitHub repository under your gh login, pushed. Offered where gh is installed.",
                ),
            ));
        } else if has_repo && s.git.pr.is_none() && (!clean || ahead > 0) {
            let branch = s.branch.as_deref().unwrap_or("HEAD");
            let summary = crate::tr_with(
                "%{branch} \u{00b7} %{files} files, %{ahead} commits ahead",
                &[
                    ("branch", &branch),
                    ("files", &file_count),
                    ("ahead", &ahead),
                ],
            );
            let line = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    Icon::carbon(CarbonIcon::Branch)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
                .child(
                    ui::mono(faces, summary)
                        .type_style(TypeStyle::Code01)
                        .text_color(theme::role(ColorRole::TextPrimary)),
                );
            let (d_commit, d_pr, key) = (desk.clone(), desk.clone(), s.key);
            let acts = div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(
                    Button::new("commit", crate::tr("Commit…"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .disabled(clean || !idle)
                        .on_click(move |_, window, cx| {
                            d_commit.update(cx, |this, cx| this.toggle_commit(window, cx));
                        }),
                )
                .child(
                    Button::new("create-pr-ft", crate::tr("Create PR"))
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::PullRequest)
                        .disabled(!idle)
                        .on_click(move |_, _, cx| {
                            d_pr.update(cx, |this, cx| this.ask_pr(key, cx));
                        }),
                );
            cards.push(ft_card(
                faces,
                crate::tr("On its own branch, no pull request yet"),
                line,
                Some(acts),
                crate::tr(
                    "Create PR has the writer write the title and description, then commits, pushes and opens it with gh pr create.",
                ),
            ));
        }

        if behind > 0 {
            let base = s.git.compare_to.as_deref().unwrap_or("main");
            let line = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    Icon::carbon(CarbonIcon::ArrowDown)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
                .child(
                    ui::text(
                        faces,
                        crate::tr_with(
                            "%{n} behind origin/%{base}",
                            &[("n", &behind), ("base", &base)],
                        ),
                    )
                    .type_style(TypeStyle::Body01)
                    .text_color(theme::role(ColorRole::TextPrimary)),
                );
            let d_pull = desk.clone();
            let acts = div().flex().gap(px(8.)).child(
                Button::new("pull", crate::tr("Pull"))
                    .variant(ButtonVariant::Tertiary)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, _, cx| {
                        d_pull.update(cx, |this, cx| {
                            this.git_action(crate::repo::GitAction::Pull, cx);
                        });
                    }),
            );
            cards.push(ft_card(
                faces,
                crate::tr("Behind its upstream"),
                line,
                Some(acts),
                crate::tr("Fast-forward only: nothing of yours is rewritten."),
            ));
        }

        if self.review.committing && has_repo {
            cards.push(self.commit_form(faces, s, cx));
        }
        if let Some(step) = &s.git.step {
            cards.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::live_dot())
                    .child(
                        fine(faces, format!("{step}…"))
                            .text_color(theme::role(ColorRole::LinkPrimary)),
                    ),
            );
        }

        if cards.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .gap(px(12.))
                .p(px(8.))
                .border_t_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .children(cards),
        )
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
