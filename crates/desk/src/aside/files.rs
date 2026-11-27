//! The Files tab (`FileBrowserPanel`) and a file's own tab (`FileViewers` and
//! `FilesDevices` boards):
//! - `Files` tab: `u0ui::t3::workspace::FileBrowserPanel` over the repository's
//!   tracked and changed paths, with file-type glyphs and git status badges.
//! - `File(path)` tab: `FileBreadcrumbs`, saved/unsaved status, line-range
//!   commenting, optional `Outside <project>` `Alert` banner, and native viewers
//!   for Markdown/Mermaid (`Preview`/`Source`), HTML (`Open in your browser`),
//!   minified/large text (`Pretty-print` / `Load full file`), CSV/TSV
//!   (`DataTable` with hex color swatches), `.zip` (`TreeTable` + `Extract…`),
//!   PDF (`PdfViewer`), Image (`Fit`/`100%`), Video/Audio (`Open with the system
//!   player`), Binary (`FileSurfaceState`), and deleted/unreadable files.
//! - Split `Device` pane (`FilesDevices`): 316px side-by-side simulator/emulator
//!   stream with `Select` picker, `Streaming` badge, hardware frame, and
//!   `DeviceControlsRail`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Instant,
};

use gpui::component::{
    Sizable as _,
    input::{Editor, EditorState, Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, Div, Entity, FontWeight, ImageFormat, SharedString,
    Window, div, img, prelude::*, px, rgb,
};
use u0ui::{
    components::{
        Badge, BadgeSize, BadgeVariant, Button, ButtonSize, ButtonVariant, FieldSize, Select,
        SelectType, ToggleGroup, ToggleGroupItem,
        alert::{Alert, AlertAction, AlertVariant},
        data_table::{DataTable, DataTableColumn, DataTableRow},
        list_box::ListItem as SelectItem,
        table::TableSize,
        toggle::ToggleSize,
    },
    icons::{CarbonIcon, Icon},
    t3::{
        review::FileStatus,
        workspace::{
            DeviceAction, DeviceControlsRail, FileBreadcrumbs, FileBrowserPanel, FileBrowserTexts,
            FileNode, FileSurfaceKind, FileSurfaceState,
        },
    },
    theme::LayerLevel,
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
    u0::{
        panels::{PdfViewer, PdfViewerTexts},
        primitives::{CellAlign, TreeTable, TreeTableColumn, TreeTableRow},
    },
};

use super::{
    diff::{file_status_of, files_of},
    fine,
};
use crate::{
    desk::{Desk, Surface},
    devices::{Act, Key, Toward},
    fonts::Faces,
    git::Scope,
    markdown,
    proto::Call,
    repo::{Body, ZipEntry},
    session::Session,
    theme, ui,
};

/// What a file's tab shows once read.
pub enum Shown {
    /// Text, in the editor; what it held when read or last saved, and truncation metadata.
    Text {
        original: String,
        total_bytes: u64,
        truncated: bool,
        minified: bool,
    },
    /// CSV or TSV table, with raw text for switching to the text editor.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        total_rows: usize,
        raw: String,
    },
    /// A `.zip` archive's entries without extracting.
    Archive {
        bytes: u64,
        entries: Vec<ZipEntry>,
    },
    Image {
        width: u32,
        height: u32,
        bytes: u64,
    },
    /// Video or audio file opened with the system player.
    Media {
        audio: bool,
        bytes: u64,
    },
    /// A PDF's first pages, and how many it has.
    Pages(Vec<Arc<gpui::Image>>, usize),
    Binary,
    Trouble(String),
}

/// State of the Files browser, open file viewer, and split Device pane.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each toggle controls an independent view option in the file viewer and split device pane"
)]
pub struct Viewer {
    pub listed_for: Option<crate::desk::Place>,
    pub path: Option<PathBuf>,
    pub shown: Option<Shown>,
    pub editor: Entity<EditorState>,
    pub drawn: bool,
    pub raw_table: bool,
    pub zoom_fit: bool,
    pub pdf_page: usize,
    pub wrap: bool,
    pub beside_device: bool,
    pub lines: Option<(usize, usize)>,
    pub note: Entity<InputState>,
}

impl Viewer {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let note = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::tr("Say what of these lines"))
        });
        cx.subscribe_in(
            &note,
            window,
            |this: &mut Desk, _, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::PressEnter { .. }) {
                    this.comment_lines(window, cx);
                }
            },
        )
        .detach();
        Self {
            listed_for: None,
            path: None,
            shown: None,
            editor: cx.new(|cx| {
                EditorState::new(window, cx)
                    .line_number(true)
                    .searchable(true)
                    .soft_wrap(true)
            }),
            drawn: true,
            raw_table: false,
            zoom_fit: true,
            pdf_page: 0,
            wrap: true,
            beside_device: false,
            lines: None,
            note,
        }
    }
}

/// Format a byte count cleanly (`4 KB`, `2.1 MB`).
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        let tenths = (bytes * 10) / (1024 * 1024);
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else {
        format!("{} KB", bytes.div_ceil(1024).max(1))
    }
}

/// Parse `#RGB` or `#RRGGBB` into `gpui::Hsla` for CSV color swatches.
fn parse_hex_color(s: &str) -> Option<gpui::Hsla> {
    let hex = s.trim().strip_prefix('#')?;
    let val = match hex.len() {
        3 => {
            let r = u32::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u32::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u32::from_str_radix(&hex[2..3], 16).ok()?;
            (r * 17) << 16 | (g * 17) << 8 | (b * 17)
        }
        6 => u32::from_str_radix(hex, 16).ok()?,
        _ => return None,
    };
    Some(rgb(val).into())
}

/// Intermediate folder builder for `FileNode` trees.
#[derive(Default)]
struct BrowserDir {
    dirs: BTreeMap<String, BrowserDir>,
    files: Vec<(String, String, Option<FileStatus>)>,
}

fn build_file_nodes(paths: &[String], statuses: &BTreeMap<String, FileStatus>) -> Vec<FileNode> {
    fn emit(dir: BrowserDir, prefix: &str, depth: usize) -> Vec<FileNode> {
        let mut out = Vec::new();
        for (name, sub) in dir.dirs {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let children = emit(sub, &path, depth + 1);
            out.push(FileNode::dir(path, name, children).open(depth == 0));
        }
        for (path, name, status) in dir.files {
            let mut node = FileNode::file(path, name);
            if let Some(st) = status {
                node = node.status(st);
            }
            out.push(node);
        }
        out
    }

    let mut root = BrowserDir::default();
    for path in paths {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let Some((&name, parents)) = parts.split_last() else {
            continue;
        };
        let mut cur = &mut root;
        for seg in parents {
            cur = cur.dirs.entry((*seg).to_owned()).or_default();
        }
        cur.files
            .push((path.clone(), name.to_owned(), statuses.get(path).copied()));
    }
    emit(root, "", 0)
}

/// Intermediate folder builder for `.zip` `TreeTableRow` trees.
#[derive(Default)]
struct ZipDir {
    dirs: BTreeMap<String, ZipDir>,
    files: Vec<(String, String, u64)>,
    bytes: u64,
}

fn build_zip_rows(entries: &[ZipEntry]) -> Vec<TreeTableRow> {
    fn emit(dir: ZipDir, prefix: &str, depth: usize) -> Vec<TreeTableRow> {
        let mut out = Vec::new();
        for (name, sub) in dir.dirs {
            let id = format!("dir:{prefix}{name}/");
            let total = sub.bytes;
            let children = emit(sub, &format!("{prefix}{name}/"), depth + 1);
            out.push(
                TreeTableRow::new(id)
                    .cell("name", name)
                    .cell("size", format_bytes(total))
                    .open(depth == 0)
                    .children(children),
            );
        }
        for (id, name, bytes) in dir.files {
            out.push(
                TreeTableRow::new(id)
                    .cell("name", name)
                    .cell("size", format_bytes(bytes)),
            );
        }
        out
    }

    let mut root = ZipDir::default();
    for e in entries {
        if e.is_dir {
            continue;
        }
        let parts: Vec<&str> = e.path.split('/').filter(|s| !s.is_empty()).collect();
        let Some((&name, parents)) = parts.split_last() else {
            continue;
        };
        let mut cur = &mut root;
        cur.bytes += e.bytes;
        for seg in parents {
            cur = cur.dirs.entry((*seg).to_owned()).or_default();
            cur.bytes += e.bytes;
        }
        cur.files
            .push((format!("file:{}", e.path), name.to_owned(), e.bytes));
    }
    emit(root, "", 0)
}

/// `.fv-bar`: 36px `Layer01` viewer header bar (`padding: 0 4px 0 12px`, `border-b-1`).
fn fv_bar(faces: &Faces, icon: CarbonIcon, name: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .min_h(px(36.))
        .pl(px(12.))
        .pr(px(4.))
        .bg(theme::role(ColorRole::Layer01))
        .border_b_1()
        .border_color(theme::role(ColorRole::BorderSubtle01))
        .child(
            Icon::carbon(icon)
                .size_length(px(16.))
                .color(theme::role(ColorRole::IconSecondary)),
        )
        .child(
            ui::mono(faces, name)
                .flex_none()
                .type_style(TypeStyle::Code01)
                .text_color(theme::role(ColorRole::TextPrimary)),
        )
}

/// `.fv-frame`: `Layer01` card with `1px BorderSubtle01`.
fn fv_frame() -> Div {
    div()
        .flex()
        .flex_col()
        .bg(theme::role(ColorRole::Layer01))
        .border_1()
        .border_color(theme::role(ColorRole::BorderSubtle01))
}

impl Desk {
    /// Whether `full` is the open file in `self.viewer` with unsaved editor changes.
    pub(super) fn is_file_unsaved(&self, full: &Path, cx: &App) -> bool {
        if self.viewer.path.as_deref() != Some(full) {
            return false;
        }
        let original = match &self.viewer.shown {
            Some(Shown::Text {
                original,
                truncated: false,
                ..
            }) => Some(original.as_str()),
            Some(Shown::Table { raw, .. }) if self.viewer.raw_table => Some(raw.as_str()),
            _ => None,
        };
        original.is_some_and(|was| was != self.viewer.editor.read(cx).value().as_ref())
    }

    /// Keep the Files tab's listing and the open file's contents in sync.
    pub(crate) fn sync_files(&mut self, _cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let folder = crate::desk::Place::folder_of(s);
        let surface = self.surface();
        if surface == Some(Surface::Files) && !self.files.contains_key(&folder) {
            if self.viewer.listed_for.as_ref() != Some(&folder) {
                self.viewer.listed_for = Some(folder.clone());
                self.list_files(&folder);
            }
            return;
        }
        if let Some(Surface::File(path)) = surface {
            let full = if path.is_absolute() {
                path
            } else {
                folder.folder.join(&path)
            };
            if self.viewer.path.as_ref() != Some(&full) {
                self.viewer.path = Some(full.clone());
                self.viewer.lines = None;
                self.viewer.raw_table = false;
                self.viewer.pdf_page = 0;
                self.viewer.shown = None;
                self.read_file(folder.host, full);
            }
        }
    }

    /// A file read: shown, its text in the editor with its syntax.
    pub(crate) fn file_read(
        &mut self,
        path: &Path,
        body: Result<Box<Body>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.viewer.path.as_deref() != Some(path) {
            return;
        }
        let set_editor = |editor: &Entity<EditorState>,
                          text: &str,
                          window: &mut Window,
                          cx: &mut Context<Self>| {
            let language = path
                .extension()
                .map_or_else(|| "text".to_owned(), |e| e.to_string_lossy().into_owned());
            editor.update(cx, |e, cx| {
                e.set_highlighter(language, cx);
                e.set_value(text, window, cx);
            });
        };
        self.viewer.shown = Some(match body.map(|b| *b) {
            Ok(Body::Text {
                text,
                total_bytes,
                truncated,
                minified,
            }) => {
                set_editor(&self.viewer.editor, &text, window, cx);
                Shown::Text {
                    original: text,
                    total_bytes,
                    truncated,
                    minified,
                }
            }
            Ok(Body::Table {
                headers,
                rows,
                total_rows,
                raw,
            }) => {
                set_editor(&self.viewer.editor, &raw, window, cx);
                Shown::Table {
                    headers,
                    rows,
                    total_rows,
                    raw,
                }
            }
            Ok(Body::Archive { bytes, entries }) => Shown::Archive { bytes, entries },
            Ok(Body::Image {
                width,
                height,
                bytes,
            }) => Shown::Image {
                width,
                height,
                bytes,
            },
            Ok(Body::Media { audio, bytes }) => Shown::Media { audio, bytes },
            Ok(Body::Pdf { pages, of }) => Shown::Pages(
                pages
                    .into_iter()
                    .map(|png| Arc::new(gpui::Image::from_bytes(ImageFormat::Png, png)))
                    .collect(),
                of,
            ),
            Ok(Body::Binary(_)) => Shown::Binary,
            Err(e) => Shown::Trouble(e),
        });
        cx.notify();
    }

    /// Begin a comment on the lines chosen in the file read, or cancel it.
    fn begin_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewer.lines.take().is_some() {
            cx.notify();
            return;
        }
        let editor = self.viewer.editor.read(cx);
        let (text, range) = (editor.value(), editor.selected_range());
        let line = |at: usize| text[..at.min(text.len())].matches('\n').count() + 1;
        let end = if range.end > range.start && text[..range.end].ends_with('\n') {
            range.end - 1
        } else {
            range.end
        };
        self.viewer.lines = Some((line(range.start), line(end)));
        self.viewer.note.update(cx, |i, cx| {
            i.set_value("", window, cx);
            window.focus(&gpui::Focusable::focus_handle(&*i, cx), cx);
        });
        cx.notify();
    }

    /// Attach the chosen line-range comment to the composer.
    fn comment_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let said = self.viewer.note.read(cx).value().trim().to_owned();
        let (Some((from, to)), Some(path)) = (self.viewer.lines.take(), self.viewer.path.clone())
        else {
            return;
        };
        let path = (self.session())
            .and_then(|s| path.strip_prefix(&s.meta.folder).ok())
            .map_or_else(|| path.clone(), Path::to_path_buf);
        let value = self.viewer.editor.read(cx).value().to_string();
        let quoted: Vec<&str> = value.lines().skip(from - 1).take(to + 1 - from).collect();
        let place = if from == to {
            format!("{}:{from}", path.display())
        } else {
            format!("{}:{from}-{to}", path.display())
        };
        let text = format!("{place} \u{2014} {said}\n\n```\n{}\n```", quoted.join("\n"));
        self.add_context(crate::session::Context::Review, &text, window, cx);
    }

    /// The `Files` tab: `u0ui::t3::workspace::FileBrowserPanel` with file-type
    /// glyphs and git status badges.
    pub(super) fn files_tab(&self, _faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let folder = crate::desk::Place::folder_of(s);
        let listed = self.files.get(&folder);
        let loading = listed.is_none() && s.repo;

        let mut statuses = BTreeMap::new();
        if let Some(diffs) = files_of(s, &Scope::All) {
            for f in diffs.iter() {
                statuses.insert(f.path.clone(), file_status_of(f));
            }
        }
        let tree = listed
            .map(|paths| build_file_nodes(paths, &statuses))
            .unwrap_or_default();

        let selected = self
            .viewer
            .path
            .as_ref()
            .and_then(|p| p.strip_prefix(&s.meta.folder).ok())
            .map(|p| SharedString::from(p.to_string_lossy().into_owned()));

        let (d_sel, d_ref) = (cx.entity(), cx.entity());
        let texts = FileBrowserTexts {
            search: crate::tr("Search files"),
            refresh: crate::tr("Refresh files"),
            loading: crate::tr("Loading files…"),
            empty: if s.repo {
                crate::tr("No matching files.")
            } else {
                crate::tr("The project's files show here in a git repository.")
            },
        };

        let mut panel = FileBrowserPanel::new(
            SharedString::from(format!("files-browser-{}-{}", s.key, usize::from(!loading))),
            tree,
        )
        .loading(loading)
        .texts(texts)
        .on_select(move |path, _, cx| {
            let p = PathBuf::from(path.as_ref());
            d_sel.update(cx, |this, cx| this.open_surface(Surface::File(p), cx));
        })
        .on_refresh(move |_, cx| {
            let f = folder.clone();
            d_ref.update(cx, |this, cx| {
                this.files.remove(&f);
                this.viewer.listed_for = None;
                cx.notify();
            });
        });
        if let Some(sel) = selected {
            panel = panel.selected(sel);
        }

        div().flex().flex_col().flex_1().min_h(px(0.)).child(panel)
    }

    /// A file's tab (`FileViewers` & `FilesDevices`): `FileBreadcrumbs` + status
    /// header, optional outside-workspace `Alert`, and type-specific native viewer.
    #[allow(
        clippy::too_many_lines,
        reason = "dispatches the header bar and each native file viewer variant in one cohesive view"
    )]
    pub(super) fn file_tab(
        &self,
        faces: &Faces,
        s: &Session,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Div {
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            s.meta.folder.join(path)
        };
        let outside = !full.starts_with(&s.meta.folder);
        let rel_display = full
            .strip_prefix(&s.meta.folder)
            .map_or_else(|_| Desk::shown(&full), |p| p.display().to_string());
        let file_name = full
            .file_name()
            .map_or_else(|| rel_display.clone(), |n| n.to_string_lossy().into_owned());
        let project_name = s.meta.project.file_name().map_or_else(
            || Desk::shown(&s.meta.project),
            |n| n.to_string_lossy().into_owned(),
        );

        let ext = full
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mermaid = matches!(ext.as_str(), "mmd" | "mermaid");
        let markdown = mermaid || matches!(ext.as_str(), "md" | "markdown");
        let is_html = matches!(ext.as_str(), "html" | "htm");

        let value = self.viewer.editor.read(cx).value().to_string();
        let editable = match &self.viewer.shown {
            Some(Shown::Text {
                original,
                truncated: false,
                ..
            }) => Some(original.clone()),
            Some(Shown::Table { raw, .. }) if self.viewer.raw_table => Some(raw.clone()),
            _ => None,
        };
        let changed = editable.as_ref().is_some_and(|was| *was != value);

        let desk = cx.entity();
        let (edit_p, reveal_p, saved_p) = (full.clone(), full.clone(), full.clone());
        let (d_crumb, d_comment, d_wrap, d_edit, d_save, d_dev) = (
            desk.clone(),
            desk.clone(),
            desk.clone(),
            desk.clone(),
            desk.clone(),
            desk.clone(),
        );

        let crumbs = FileBreadcrumbs::new(
            SharedString::from(format!("file-crumbs-{rel_display}")),
            rel_display.clone(),
        )
        .label(crate::tr("Path"))
        .on_select(move |_, _, cx| {
            d_crumb.update(cx, |this, cx| this.open_surface(Surface::Files, cx));
        });

        let showing_lines = editable.is_some() && !(markdown && self.viewer.drawn);
        let head = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(36.))
            .pl(px(12.))
            .pr(px(4.))
            .bg(theme::role(ColorRole::Background))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(div().flex_1().min_w(px(0.)).overflow_hidden().child(crumbs))
            .when(changed, |d| {
                let save_label = crate::tr_with("Save %{name}", &[("name", &file_name)]);
                d.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(6.))
                        .pr(px(4.))
                        .child(
                            div()
                                .size(px(8.))
                                .rounded_full()
                                .bg(theme::role(ColorRole::SupportWarning)),
                        )
                        .child(
                            ui::text(faces, crate::tr("Unsaved"))
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary)),
                        ),
                )
                .child(
                    Button::new("file-save", save_label)
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            let p = saved_p.clone();
                            d_save.update(cx, |this, cx| {
                                let text = this.viewer.editor.read(cx).value().to_string();
                                let total_bytes = text.len() as u64;
                                this.write(p, text.clone());
                                this.viewer.shown = Some(Shown::Text {
                                    original: text,
                                    total_bytes,
                                    truncated: false,
                                    minified: false,
                                });
                                cx.notify();
                            });
                        }),
                )
            })
            .when(!changed && editable.is_some() && !outside, |d| {
                d.child(
                    ui::text(faces, crate::tr("Saved"))
                        .pr(px(6.))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextHelper)),
                )
            })
            .when(showing_lines, |d| {
                let wrap_now = self.viewer.wrap;
                d.child(
                    Button::new("file-comment", crate::tr("Comment on the lines chosen"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::Chat)
                        .icon_only(true)
                        .on_click(move |_, window, cx| {
                            d_comment.update(cx, |this, cx| this.begin_lines(window, cx));
                        }),
                )
                .child(
                    Button::new("file-wrap", crate::tr("Wrap long lines"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::ArrowRight)
                        .icon_only(true)
                        .selected(wrap_now)
                        .on_click(move |_, window, cx| {
                            d_wrap.update(cx, |this, cx| {
                                this.viewer.wrap = !this.viewer.wrap;
                                let w = this.viewer.wrap;
                                this.viewer
                                    .editor
                                    .update(cx, |e, cx| e.set_soft_wrap(w, window, cx));
                            });
                        }),
                )
            })
            .when(editable.is_some(), |d| {
                let copy_val = value.clone();
                d.child(
                    Button::new("file-copy", crate::tr("Copy what it holds"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::Copy)
                        .icon_only(true)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                copy_val.clone(),
                            ));
                        }),
                )
            })
            .child(
                Button::new("file-editor", crate::tr("Open in your editor"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Edit)
                    .icon_only(true)
                    .on_click(move |_, _, cx| {
                        let p = edit_p.clone();
                        d_edit.update(cx, |this, _| this.edit(p, None));
                    }),
            )
            .child(
                Button::new("file-reveal", crate::tr("Reveal in Finder"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::FolderOpen)
                    .icon_only(true)
                    .on_click(move |_, _, cx| cx.reveal_path(&reveal_p)),
            )
            .child(
                Button::new(
                    "file-device-split",
                    if self.viewer.beside_device {
                        crate::tr("Close device pane")
                    } else {
                        crate::tr("Show device beside file")
                    },
                )
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .icon(CarbonIcon::Mobile)
                .icon_only(true)
                .selected(self.viewer.beside_device)
                .on_click(move |_, _, cx| {
                    d_dev.update(cx, |this, cx| {
                        this.viewer.beside_device = !this.viewer.beside_device;
                        if this.viewer.beside_device
                            && !this.panes.whole
                            && this.settings.right < 720
                        {
                            this.settings.right = 740;
                        }
                        this.watch_device();
                        cx.notify();
                    });
                }),
            );

        let outside_banner = outside.then(|| {
            let d_copy = desk.clone();
            let from_p = full.clone();
            let to_rel = PathBuf::from(&file_name);
            let to_p = s.meta.folder.join(&to_rel);
            let host = s.host;
            div()
                .flex_none()
                .p(px(12.))
                .border_b_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(
                    Alert::new("file-outside-alert")
                        .variant(AlertVariant::Warning)
                        .title(crate::tr_with(
                            "Outside %{project}",
                            &[("project", &project_name)],
                        ))
                        .description(crate::tr_with(
                            "%{path} is read only here, and the agent can read it. To let the agent edit it, copy it into the project.",
                            &[("path", &rel_display)],
                        ))
                        .action(
                            AlertAction::new(crate::tr_with(
                                "Copy into %{project}…",
                                &[("project", &project_name)],
                            ))
                            .on_click(move |_, cx| {
                                let (from, to, rel) =
                                    (from_p.clone(), to_p.clone(), to_rel.clone());
                                d_copy.update(cx, |this, cx| {
                                    this.call(host, Call::CopyFile { from, to });
                                    this.open_surface(Surface::File(rel), cx);
                                });
                            }),
                        ),
                )
        });

        let body: AnyElement = match &self.viewer.shown {
            None => div()
                .p(px(16.))
                .child(
                    FileSurfaceState::new("file-reading", FileSurfaceKind::Loading)
                        .loading_title(crate::tr("Reading…")),
                )
                .into_any_element(),

            Some(Shown::Text { .. }) if markdown => {
                let d_md = desk.clone();
                let mode = if self.viewer.drawn {
                    "preview"
                } else {
                    "source"
                };
                let switcher = ToggleGroup::new("file-md-mode")
                    .size(ToggleSize::Sm)
                    .value([mode])
                    .item(ToggleGroupItem::new("preview").child(crate::tr("Preview")))
                    .item(ToggleGroupItem::new("source").child(crate::tr("Source")))
                    .on_value_change(move |vals, _, cx| {
                        let preview = vals.first().is_some_and(|v| v.as_ref() == "preview");
                        d_md.update(cx, |this, cx| {
                            this.viewer.drawn = preview;
                            cx.notify();
                        });
                    });
                let bar = fv_bar(faces, CarbonIcon::Document, rel_display.clone())
                    .child(div().flex_1())
                    .child(div().flex_none().w(px(160.)).mr(px(4.)).child(switcher));

                let content: AnyElement = if self.viewer.drawn {
                    let rendered = if mermaid {
                        format!("```mermaid\n{value}\n```")
                    } else {
                        value
                    };
                    div()
                        .id("file-drawn")
                        .flex_1()
                        .min_h(px(0.))
                        .bg(theme::role(ColorRole::Layer01))
                        .child(
                            div()
                                .px(px(24.))
                                .pt(px(20.))
                                .pb(px(24.))
                                .child(markdown::render(faces, "file-drawn", &rendered, cx)),
                        )
                        .overflow_y_scrollbar()
                        .into_any_element()
                } else {
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .bg(theme::role(ColorRole::Background))
                        .child(Editor::new(&self.viewer.editor).bordered(false).h_full())
                        .into_any_element()
                };

                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .child(bar)
                    .child(content)
                    .into_any_element()
            }

            Some(Shown::Text { total_bytes, .. }) if is_html => {
                let size_str = format_bytes(*total_bytes);
                let picked = full.clone();
                let bar = fv_bar(faces, CarbonIcon::Document, rel_display.clone())
                    .child(
                        ui::one_line(
                            ui::text(
                                faces,
                                crate::tr_with(
                                    "HTML · %{size} · shown as source; u0 never draws a web page",
                                    &[("size", &size_str)],
                                ),
                            )
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .child(
                        Button::new("html-open-browser", crate::tr("Open in your browser"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Launch)
                            .on_click(move |_, _, cx| crate::aside::rules::yours(&picked, cx)),
                    );
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(theme::role(ColorRole::Background))
                    .child(bar)
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .child(Editor::new(&self.viewer.editor).bordered(false).h_full()),
                    )
                    .into_any_element()
            }

            Some(Shown::Text {
                total_bytes,
                minified,
                truncated,
                ..
            }) if *minified || *truncated => {
                let (total_bytes, minified, truncated) = (*total_bytes, *minified, *truncated);
                let size_str = format_bytes(total_bytes);
                let warn_detail = match (minified, truncated) {
                    (true, true) => crate::tr_with(
                        " · %{size}, minified · showing the first 100 KB",
                        &[("size", &size_str)],
                    ),
                    (true, false) => crate::tr_with(" · %{size}, minified", &[("size", &size_str)]),
                    _ => crate::tr_with(
                        " · %{size} · showing the first 100 KB",
                        &[("size", &size_str)],
                    ),
                };
                let (d_pretty, d_load) = (desk.clone(), desk.clone());
                let ext_owned = ext.clone();
                let load_path = full.clone();
                let host = s.host;
                let remaining = total_bytes.saturating_sub(100 * 1024);

                let warn = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(10.))
                    .bg(theme::role(ColorRole::Layer01))
                    .border_l(px(3.))
                    .border_color(theme::role(ColorRole::SupportWarning))
                    .border_b_1()
                    .child(
                        Icon::carbon(CarbonIcon::WarningAltFilled)
                            .size_length(px(16.))
                            .color(theme::role(ColorRole::SupportWarning)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                ui::mono(faces, file_name.clone())
                                    .type_style(TypeStyle::Code01)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::role(ColorRole::TextPrimary)),
                            )
                            .child(
                                ui::text(faces, warn_detail)
                                    .type_style(TypeStyle::BodyCompact01)
                                    .text_color(theme::role(ColorRole::TextPrimary)),
                            ),
                    );

                let foot = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .min_h(px(36.))
                    .pl(px(12.))
                    .pr(px(4.))
                    .bg(theme::role(ColorRole::Layer01))
                    .border_t_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
                    .when(truncated && remaining > 0, |d| {
                        d.child(
                            ui::text(
                                faces,
                                crate::tr_with(
                                    "%{size} not loaded",
                                    &[("size", &format_bytes(remaining))],
                                ),
                            )
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                        )
                    })
                    .child(div().flex_1())
                    .when(minified, |d| {
                        d.child(
                            Button::new("file-pretty", crate::tr("Pretty-print"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, window, cx| {
                                    let ext_c = ext_owned.clone();
                                    d_pretty.update(cx, |this, cx| {
                                        let cur = this.viewer.editor.read(cx).value().to_string();
                                        let pretty = crate::repo::pretty_print(&ext_c, &cur);
                                        this.viewer.editor.update(cx, |e, cx| {
                                            e.set_value(&pretty, window, cx);
                                        });
                                    });
                                }),
                        )
                    })
                    .when(truncated, |d| {
                        d.child(
                            Button::new("file-load-full", crate::tr("Load full file"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    let p = load_path.clone();
                                    d_load.update(cx, |this, _| this.read_file_full(host, p));
                                }),
                        )
                    });

                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(theme::role(ColorRole::Background))
                    .child(warn)
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .child(Editor::new(&self.viewer.editor).bordered(false).h_full()),
                    )
                    .child(foot)
                    .into_any_element()
            }

            Some(Shown::Text { .. }) => div()
                .flex_1()
                .min_h(px(0.))
                .bg(theme::role(ColorRole::Background))
                .child(Editor::new(&self.viewer.editor).bordered(false).h_full())
                .into_any_element(),

            Some(Shown::Table {
                headers,
                rows,
                total_rows,
                ..
            }) => {
                let d_tbl = desk.clone();
                let raw_mode = self.viewer.raw_table;
                let dim = crate::tr_with(
                    "%{rows} rows · %{cols} columns",
                    &[("rows", total_rows), ("cols", &headers.len())],
                );
                let bar = fv_bar(faces, CarbonIcon::Grid, file_name.clone())
                    .child(
                        ui::text(faces, dim)
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new(
                            "csv-toggle-raw",
                            if raw_mode {
                                crate::tr("Show as table")
                            } else {
                                crate::tr("Open as text")
                            },
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            d_tbl.update(cx, |this, cx| {
                                this.viewer.raw_table = !this.viewer.raw_table;
                                cx.notify();
                            });
                        }),
                    );

                if raw_mode {
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.))
                        .bg(theme::role(ColorRole::Background))
                        .child(bar)
                        .child(
                            div()
                                .flex_1()
                                .min_h(px(0.))
                                .child(Editor::new(&self.viewer.editor).bordered(false).h_full()),
                        )
                        .into_any_element()
                } else {
                    let faces_cell = faces.clone();
                    let cols: Vec<DataTableColumn> = headers
                        .iter()
                        .enumerate()
                        .map(|(ci, h)| {
                            let key = format!("c{ci}");
                            let f = faces_cell.clone();
                            DataTableColumn::new(key, h.clone()).render(move |val, _| {
                                let swatch = parse_hex_color(val).map(|fill| {
                                    div()
                                        .flex_none()
                                        .size(px(12.))
                                        .bg(fill)
                                        .border_1()
                                        .border_color(gpui::hsla(0., 0., 0.55, 0.6))
                                });
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .children(swatch)
                                    .child(
                                        ui::mono(&f, val.clone())
                                            .type_style(TypeStyle::Code01)
                                            .text_color(theme::role(ColorRole::TextPrimary)),
                                    )
                                    .into_any_element()
                            })
                        })
                        .collect();
                    let table_rows: Vec<DataTableRow> = rows
                        .iter()
                        .enumerate()
                        .map(|(ri, r)| {
                            let mut row = DataTableRow::new(format!("r{ri}"));
                            for (ci, cell) in r.iter().enumerate() {
                                row = row.cell(format!("c{ci}"), cell.clone());
                            }
                            row
                        })
                        .collect();
                    let table = DataTable::new("file-csv-table")
                        .columns(cols)
                        .rows(table_rows)
                        .size(TableSize::Xs)
                        .search(false)
                        .sortable(false)
                        .page_size(100);
                    div()
                        .id("file-csv-scroll")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.))
                        .child(bar)
                        .child(
                            div()
                                .id("file-csv-body")
                                .flex_1()
                                .min_h(px(0.))
                                .child(table)
                                .overflow_y_scrollbar(),
                        )
                        .into_any_element()
                }
            }

            Some(Shown::Archive { bytes, entries }) => {
                let d_zip = desk.clone();
                let archive_p = full.clone();
                let stem = full.file_stem().map_or_else(
                    || "extracted".to_owned(),
                    |s| s.to_string_lossy().into_owned(),
                );
                let target_dir_p = full.parent().unwrap_or(&s.meta.folder).join(stem);
                let host = s.host;
                let folder_place = crate::desk::Place::folder_of(s);
                let file_count = entries.iter().filter(|e| !e.is_dir).count();
                let dim = crate::tr_with(
                    "%{n} files · %{size}",
                    &[("n", &file_count), ("size", &format_bytes(*bytes))],
                );
                let bar = fv_bar(faces, CarbonIcon::Archive, file_name.clone())
                    .child(
                        ui::text(faces, dim)
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("zip-extract", crate::tr("Extract…"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::FolderOpen)
                            .on_click(move |_, _, cx| {
                                let (archive, target_dir, f) = (
                                    archive_p.clone(),
                                    target_dir_p.clone(),
                                    folder_place.clone(),
                                );
                                d_zip.update(cx, |this, cx| {
                                    this.call(
                                        host,
                                        Call::ExtractArchive {
                                            archive,
                                            dest: target_dir,
                                        },
                                    );
                                    this.files.remove(&f);
                                    this.viewer.listed_for = None;
                                    cx.notify();
                                });
                            }),
                    );
                let cols = [
                    TreeTableColumn::new("name", crate::tr("Name")),
                    TreeTableColumn::new("size", crate::tr("Size"))
                        .width(6.)
                        .align(CellAlign::End),
                ];
                let tree_rows = build_zip_rows(entries);
                let tree = TreeTable::new("file-zip-tree", cols, tree_rows)
                    .label(file_name.clone())
                    .toggle_labels(crate::tr("Expand"), crate::tr("Collapse"));
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .child(bar)
                    .child(
                        div()
                            .id("file-zip-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .child(tree)
                            .overflow_y_scrollbar(),
                    )
                    .into_any_element()
            }

            Some(Shown::Image {
                width,
                height,
                bytes,
            }) => {
                let d_img = desk.clone();
                let fit = self.viewer.zoom_fit;
                let size_str = format_bytes(*bytes);
                let meta = if *width > 0 && *height > 0 {
                    format!("{width} \u{00d7} {height} \u{00b7} {size_str}")
                } else {
                    size_str
                };
                let mode = if fit { "fit" } else { "actual" };
                let switcher = ToggleGroup::new("file-img-zoom")
                    .size(ToggleSize::Sm)
                    .value([mode])
                    .item(ToggleGroupItem::new("fit").child(crate::tr("Fit")))
                    .item(ToggleGroupItem::new("actual").child("100%"))
                    .on_value_change(move |vals, _, cx| {
                        let next_fit = vals.first().is_some_and(|v| v.as_ref() == "fit");
                        d_img.update(cx, |this, cx| {
                            this.viewer.zoom_fit = next_fit;
                            cx.notify();
                        });
                    });
                let bar = fv_bar(faces, CarbonIcon::Image, file_name.clone())
                    .child(
                        ui::one_line(
                            ui::text(faces, meta)
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary)),
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .child(div().flex_none().w(px(144.)).mr(px(4.)).child(switcher));
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .child(bar)
                    .child(
                        div()
                            .id("file-image")
                            .flex_1()
                            .min_h(px(0.))
                            .p(px(16.))
                            .bg(theme::role(ColorRole::LayerAccent01))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(img(full).when(fit, |i| i.max_w_full().max_h_full()))
                            .overflow_y_scrollbar(),
                    )
                    .into_any_element()
            }

            Some(Shown::Media { audio, bytes }) => {
                let is_audio = *audio;
                let picked = full.clone();
                let card = fv_frame()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .min_h(px(36.))
                            .pl(px(12.))
                            .pr(px(4.))
                            .py(px(2.))
                            .border_b_1()
                            .border_color(theme::role(ColorRole::BorderSubtle01))
                            .child(
                                Icon::carbon(if is_audio {
                                    CarbonIcon::Volume
                                } else {
                                    CarbonIcon::Video
                                })
                                .size_length(px(16.))
                                .color(theme::role(ColorRole::IconSecondary)),
                            )
                            .child(
                                ui::mono(faces, file_name.clone())
                                    .type_style(TypeStyle::Code01)
                                    .text_color(theme::role(ColorRole::TextPrimary)),
                            )
                            .child(
                                ui::mono(faces, format_bytes(*bytes))
                                    .type_style(TypeStyle::Code01)
                                    .text_color(theme::role(ColorRole::TextSecondary)),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("media-open", crate::tr("Open with the system player"))
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .icon(CarbonIcon::Launch)
                                    .on_click(move |_, _, cx| {
                                        crate::aside::rules::yours(&picked, cx);
                                    }),
                            ),
                    )
                    .child(
                        ui::text(
                            faces,
                            if is_audio {
                                crate::tr("u0 plays no audio itself.")
                            } else {
                                crate::tr("u0 plays no video itself.")
                            },
                        )
                        .px(px(12.))
                        .pt(px(8.))
                        .pb(px(10.))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                    );
                div().p(px(12.)).child(card).into_any_element()
            }

            Some(Shown::Pages(pages, of)) => {
                let d_pdf = desk.clone();
                let pages_arc = pages.clone();
                let total = (*of).max(1) as u32;
                let cur_page = ((self.viewer.pdf_page + 1) as u32).clamp(1, total);
                let texts = PdfViewerTexts {
                    previous: crate::tr("Previous page"),
                    next: crate::tr("Next page"),
                    zoom_out: crate::tr("Zoom out"),
                    zoom_in: crate::tr("Zoom in"),
                    page: Rc::new(|n| crate::tr_with("Page %{n}", &[("n", &n)])),
                };
                let faces_pdf = faces.clone();
                let viewer = PdfViewer::new("file-pdf-viewer", file_name.clone(), total)
                    .page(cur_page)
                    .texts(texts)
                    .on_page_change(move |n, _, cx| {
                        d_pdf.update(cx, |this, cx| {
                            this.viewer.pdf_page = (n.saturating_sub(1)) as usize;
                            cx.notify();
                        });
                    })
                    .render_page(move |n, _, _| {
                        if let Some(page_img) = pages_arc.get((n.saturating_sub(1)) as usize) {
                            img(page_img.clone()).size_full().into_any_element()
                        } else {
                            ui::text(
                                &faces_pdf,
                                crate::tr("Open in default app to read remaining pages."),
                            )
                            .type_style(TypeStyle::Label01)
                            .into_any_element()
                        }
                    });
                div()
                    .id("file-pdf-wrap")
                    .flex_1()
                    .min_h(px(0.))
                    .p(px(12.))
                    .child(fv_frame().child(viewer))
                    .overflow_y_scrollbar()
                    .into_any_element()
            }

            Some(Shown::Binary) => {
                let picked = full.clone();
                let d_ask = desk.clone();
                let agent_name = self.name_of(&s.meta.agent);
                let mention = format!("@{rel_display} ");
                div()
                    .p(px(12.))
                    .child(
                        fv_frame().child(
                            FileSurfaceState::new("file-binary", FileSurfaceKind::Binary)
                                .title(crate::tr_with(
                                    "%{name} is a binary file",
                                    &[("name", &file_name)],
                                ))
                                .body(crate::tr("u0 can’t show it here."))
                                .action(
                                    Button::new("bin-open", crate::tr("Open in default app"))
                                        .variant(ButtonVariant::Secondary)
                                        .size(ButtonSize::Sm)
                                        .on_click(move |_, _, cx| {
                                            crate::aside::rules::yours(&picked, cx);
                                        }),
                                )
                                .action(
                                    Button::new(
                                        "bin-ask",
                                        crate::tr_with(
                                            "Ask %{name} to inspect it",
                                            &[("name", &agent_name)],
                                        ),
                                    )
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(
                                        move |_, window, cx| {
                                            let m = mention.clone();
                                            d_ask.update(cx, |this, cx| {
                                                this.put_line(&m, window, cx);
                                            });
                                        },
                                    ),
                                ),
                        ),
                    )
                    .into_any_element()
            }

            Some(Shown::Trouble(e)) => {
                let deleted_turn = s.turns.iter().enumerate().rev().find_map(|(i, t)| {
                    let diffs = t.diff.as_ref()?;
                    diffs
                        .iter()
                        .any(|d| d.path == rel_display && file_status_of(d) == FileStatus::Deleted)
                        .then_some(i)
                });
                let card = if let Some(turn_ix) = deleted_turn {
                    let turn_num = turn_ix + 1;
                    let agent_name = self.name_of(&s.meta.agent);
                    let (d_undo, d_diff) = (desk.clone(), desk.clone());
                    FileSurfaceState::new("file-deleted", FileSurfaceKind::Binary)
                        .title(crate::tr_with("Deleted in turn %{n}", &[("n", &turn_num)]))
                        .body(crate::tr_with(
                            "%{agent} removed %{path}. Restore this turn or see the change in the turn’s diff.",
                            &[("agent", &agent_name), ("path", &rel_display)],
                        ))
                        .action(
                            Button::new("del-restore", crate::tr("Restore this turn"))
                                .variant(ButtonVariant::Secondary)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    d_undo.update(cx, |this, cx| this.undo(turn_ix, cx));
                                }),
                        )
                        .action(
                            Button::new(
                                "del-diff",
                                crate::tr_with("View turn %{n} diff", &[("n", &turn_num)]),
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                d_diff.update(cx, |this, cx| {
                                    this.show_changes(Scope::Turn(turn_ix), cx);
                                    this.open_surface(Surface::Diff, cx);
                                });
                            }),
                        )
                } else {
                    FileSurfaceState::new("file-trouble", FileSurfaceKind::Failure)
                        .title(crate::tr_with("Couldn't read it: %{e}", &[("e", e)]))
                };
                div()
                    .p(px(12.))
                    .child(fv_frame().child(card))
                    .into_any_element()
            }
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .bg(theme::role(ColorRole::Background))
            .child(head)
            .children(outside_banner)
            .children(self.viewer.lines.map(|(from, to)| {
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(6.))
                    .bg(theme::role(ColorRole::Layer01))
                    .border_b_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
                    .child(fine(
                        faces,
                        if from == to {
                            crate::tr_with("Line %{from}", &[("from", &from)])
                        } else {
                            crate::tr_with("Lines %{from}–%{to}", &[("from", &from), ("to", &to)])
                        },
                    ))
                    .child(div().flex_1().child(Input::new(&self.viewer.note).small()))
            }))
            .child(body)
    }

    /// 316px right `Device` split pane beside the file editor (`FilesDevices` board):
    /// `Select` device picker + `Streaming` badge + `Close` button, rounded hardware
    /// frame with interactive touch stream, and `DeviceControlsRail`.
    pub(super) fn device_split_pane(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity();
        let found = self
            .sims
            .listed
            .as_ref()
            .map_or(&[][..], |(list, _)| list.as_slice());
        let active = found
            .iter()
            .find(|d| Some(&d.id) == self.sims.on.as_ref())
            .or_else(|| found.first())
            .cloned();

        let select_items: Vec<SelectItem> = if found.is_empty() {
            vec![SelectItem::labeled(
                "none",
                crate::tr("No simulator or emulator"),
            )]
        } else {
            found
                .iter()
                .map(|d| SelectItem::labeled(d.id.clone(), d.name.clone()))
                .collect()
        };
        let selected_val = active
            .as_ref()
            .map_or_else(|| "none".to_owned(), |d| d.id.clone());

        let d_pick = desk.clone();
        let picker = Select::new("fd-device-select")
            .kind(SelectType::Inline)
            .size(FieldSize::Sm)
            .layer(LayerLevel::One)
            .items(select_items)
            .value(Some(selected_val))
            .disabled(found.is_empty())
            .on_value_change(move |val, _, cx| {
                let Some(val) = val.filter(|v| v.as_ref() != "none") else {
                    return;
                };
                d_pick.update(cx, |this, cx| {
                    this.sims.on = Some(val.to_string());
                    this.sims.screen = None;
                    this.watch_device();
                    cx.notify();
                });
            });

        let streaming = active.as_ref().is_some_and(|d| d.running);
        let badge = if streaming {
            Badge::new(crate::tr("Streaming"))
                .variant(BadgeVariant::Green)
                .size(BadgeSize::Sm)
        } else {
            Badge::new(crate::tr("Shut down"))
                .variant(BadgeVariant::Gray)
                .size(BadgeSize::Sm)
        };

        let d_close = desk.clone();
        let bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(36.))
            .px(px(4.))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(div().flex_1().min_w(px(0.)).child(picker))
            .child(div().flex_none().child(badge))
            .child(
                Button::new("fd-device-close", crate::tr("Close device pane"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Close)
                    .icon_only(true)
                    .on_click(move |_, _, cx| {
                        d_close.update(cx, |this, cx| {
                            this.viewer.beside_device = false;
                            cx.notify();
                        });
                    }),
            );

        let screen_frame = self
            .sims
            .screen
            .as_ref()
            .zip(active.as_ref())
            .filter(|((id, ..), d)| *id == d.id && d.running);

        let inner_screen: AnyElement = if let Some(((_, image, (img_w, img_h)), dev)) = screen_frame
        {
            let (img_w, img_h) = (*img_w, *img_h);
            let origin = self.sims.origin.clone();
            let (at, touched, scrolled) = (self.sims.origin.clone(), dev.clone(), dev.clone());
            let host = self.session().map_or(crate::hub::Host::HERE, |s| s.host);
            let pixel = move |pt: gpui::Point<gpui::Pixels>| {
                let orig = *at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let rel = pt - orig;
                let px_x = f32::from(rel.x) * (img_w as f32 / 226.);
                let px_y = f32::from(rel.y) * (img_h as f32 / 490.);
                (px_x.max(0.) as u32, px_y.max(0.) as u32)
            };
            div()
                .id("fd-live-screen")
                .relative()
                .size_full()
                .rounded(px(29.))
                .overflow_hidden()
                .bg(theme::role(ColorRole::Background))
                .child(img(image.clone()).size_full())
                .child(
                    gpui::canvas(
                        move |bounds, _, _| {
                            *origin
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = bounds.origin;
                        },
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .cursor_pointer()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, e: &gpui::MouseDownEvent, _, _| {
                        this.sims.pressed = Some((e.position, Instant::now()));
                    }),
                )
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, e: &gpui::MouseUpEvent, _, _| {
                        let Some((down, started)) = this.sims.pressed.take() else {
                            return;
                        };
                        let moved = e.position - down;
                        let ((x, y), (to_x, to_y)) = (pixel(down), pixel(e.position));
                        let act = if f32::from(moved.x).hypot(f32::from(moved.y)) > 6. {
                            Act::Swipe { x, y, to_x, to_y }
                        } else if started.elapsed().as_millis() >= 500 {
                            Act::Press { x, y }
                        } else {
                            Act::Tap { x, y }
                        };
                        this.call(
                            host,
                            Call::Device {
                                device: touched.clone(),
                                act,
                            },
                        );
                    }),
                )
                .on_scroll(cx.listener(move |this, e: &gpui::ScrollEvent, _, _| {
                    let delta = e.delta.pixel_delta(px(16.));
                    let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
                    let toward = match (dx.abs() >= dy.abs(), dx > 0., dy > 0.) {
                        _ if dx.abs().max(dy.abs()) < 1. => return,
                        (false, _, true) => Toward::Up,
                        (false, _, false) => Toward::Down,
                        (true, true, _) => Toward::Left,
                        (true, false, _) => Toward::Right,
                    };
                    this.call(
                        host,
                        Call::Device {
                            device: scrolled.clone(),
                            act: Act::Scroll { toward },
                        },
                    );
                }))
                .into_any_element()
        } else {
            let (title, sub) = match &active {
                Some(d) if d.running => (d.name.clone(), crate::tr("Reading its screen…")),
                Some(d) => (d.name.clone(), crate::tr("Shut down")),
                None => (
                    crate::tr("No simulator running").into(),
                    crate::tr("Open Devices to start or install one."),
                ),
            };
            let d_act = desk.clone();
            let dev_start = active.clone().filter(|d| !d.running);
            let host = self.session().map_or(crate::hub::Host::HERE, |s| s.host);
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .size_full()
                .rounded(px(29.))
                .p(px(16.))
                .bg(theme::role(ColorRole::Layer02))
                .text_center()
                .child(
                    Icon::carbon(CarbonIcon::Mobile)
                        .size_length(px(24.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
                .child(
                    ui::text(faces, title)
                        .type_style(TypeStyle::HeadingCompact01)
                        .text_color(theme::role(ColorRole::TextPrimary)),
                )
                .child(
                    ui::text(faces, sub)
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                )
                .when_some(dev_start, |d, dev| {
                    d.child(
                        Button::new("fd-start-dev", crate::tr("Start"))
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                let target = dev.clone();
                                d_act.update(cx, |this, cx| {
                                    this.call(
                                        host,
                                        Call::Device {
                                            device: target,
                                            act: Act::Start,
                                        },
                                    );
                                    cx.notify();
                                });
                            }),
                    )
                })
                .into_any_element()
        };

        let phone_shell = div()
            .w(px(236.))
            .h(px(500.))
            .flex_none()
            .rounded(px(34.))
            .p(px(5.))
            .bg(rgb(0x0026_2626))
            .border_1()
            .border_color(theme::role(ColorRole::BorderStrong01))
            .child(inner_screen);

        let d_rail = desk;
        let rail_dev = active;
        let host = self.session().map_or(crate::hub::Host::HERE, |s| s.host);
        let rail = DeviceControlsRail::new("fd-device-rail")
            .actions([
                DeviceAction::new(crate::tr("Home")).icon(CarbonIcon::Home),
                DeviceAction::new(crate::tr("Rotate")).icon(CarbonIcon::Rotate),
                DeviceAction::new(crate::tr("Screenshot")).icon(CarbonIcon::Image),
                DeviceAction::new(crate::tr("Record")).icon(CarbonIcon::Recording),
                DeviceAction::new(crate::tr("Keyboard")).icon(CarbonIcon::Keyboard),
            ])
            .on_action(move |ix, _, cx| {
                let target = rail_dev.clone();
                d_rail.update(cx, |this, cx| match (ix, target) {
                    (0, Some(device)) => {
                        this.call(
                            host,
                            Call::Device {
                                device,
                                act: Act::Key { key: Key::Home },
                            },
                        );
                    }
                    (1, Some(device)) => {
                        this.call(
                            host,
                            Call::Device {
                                device,
                                act: Act::Rotate { right: true },
                            },
                        );
                    }
                    (2, Some(device)) => {
                        this.call(host, Call::Screen { device });
                    }
                    _ => {
                        this.open_surface(Surface::Devices, cx);
                    }
                });
            });

        div()
            .w(px(316.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme::role(ColorRole::Layer01))
            .border_l_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(bar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .gap(px(8.))
                    .pt(px(24.))
                    .pb(px(16.))
                    .px(px(12.))
                    .items_start()
                    .justify_center()
                    .child(phone_shell)
                    .child(
                        div()
                            .border_1()
                            .border_color(theme::role(ColorRole::BorderSubtle01))
                            .child(rail),
                    ),
            )
    }
}
