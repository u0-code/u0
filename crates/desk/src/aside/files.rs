//! The Files tab and a file's own tab, as t3code's file browser and
//! preview have them: the project's files as git sees them in gpui-kit's
//! tree (found by name, folded or opened all at once, read again), each
//! with what can be done with it; and one file read: text in gpui-kit's
//! editor with its syntax (wrapped or not, changed and saved), Markdown
//! drawn or as written, a picture, a PDF's pages as the server drew them.

use std::{path::PathBuf, sync::Arc};

use gpui::component::{
    Sizable as _,
    input::{Editor, EditorState, Input, InputEvent, InputState},
    list::ListItem,
    menu::PopupMenuItem,
    scroll::ScrollableElement as _,
    tree::{TreeState, tree},
};
use gpui::{AppContext as _, Context, Div, Entity, ImageFormat, Window, div, img, prelude::*, px};

use super::{empty, fine};
use crate::{
    desk::{Desk, Surface},
    fonts::Faces,
    markdown,
    repo::Body,
    session::Session,
    theme,
    ui::{self, Weight},
};

/// What a file's tab shows once read.
pub enum Shown {
    /// Text, in the editor; what it held when read or last saved.
    Text(String),
    Image,
    /// A PDF's first pages, and how many it has.
    Pages(Vec<Arc<gpui::Image>>, usize),
    Binary(u64),
    Trouble(String),
}

/// The Files tab and the file read: the tree and what it holds (folder,
/// how many files, the words looked for, all opened), the line to find a
/// file by name, whether every folder is open; the file asked for, what
/// came back, its editor, and whether Markdown is drawn.
pub struct Viewer {
    pub tree: Entity<TreeState>,
    pub held: Option<(PathBuf, usize, String, bool)>,
    pub search: Entity<InputState>,
    pub open_all: bool,
    pub path: Option<PathBuf>,
    pub shown: Option<Shown>,
    pub editor: Entity<EditorState>,
    pub drawn: bool,
    pub wrap: bool,
}

impl Viewer {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find a file"));
        cx.subscribe(&search, |this: &mut Desk, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.viewer.held = None;
                cx.notify();
            }
        })
        .detach();
        Self {
            tree: cx.new(|cx| TreeState::new(cx)),
            held: None,
            search,
            open_all: false,
            path: None,
            shown: None,
            editor: cx.new(|cx| {
                EditorState::new(window, cx)
                    .line_number(true)
                    .searchable(true)
                    .soft_wrap(true)
            }),
            drawn: true,
            wrap: true,
        }
    }
}

/// What a line of a file's menu does.
type Act = Box<dyn Fn(&mut Desk, &mut Window, &mut Context<Desk>)>;

/// Every folder of `items` opened, or folded.
fn fold(items: &mut [gpui::component::tree::TreeItem], open: bool) {
    for i in items {
        if i.is_folder() {
            *i = i.clone().expanded(open);
            fold(&mut i.children, open);
        }
    }
}

impl Desk {
    /// Keep the Files tab's tree and the file's tab as the session asks:
    /// its files listed once, the tree built again when they or the words
    /// change, the file shown read when it is not yet.
    pub(crate) fn sync_files(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let folder = s.meta.folder.clone();
        let surface = self.surface();
        if surface == Some(Surface::Files) {
            let Some(files) = self.files.get(&folder) else {
                if self.viewer.held.as_ref().is_none_or(|h| h.0 != folder) {
                    self.viewer.held = Some((folder.clone(), usize::MAX, String::new(), false));
                    self.list_files(folder);
                }
                return;
            };
            let words = self.viewer.search.read(cx).value().trim().to_lowercase();
            let held = (
                folder.clone(),
                files.len(),
                words.clone(),
                self.viewer.open_all,
            );
            if self.viewer.held.as_ref() != Some(&held) {
                let found = files
                    .iter()
                    .filter(|f| words.is_empty() || f.to_lowercase().contains(&words))
                    .map(String::as_str);
                let mut items = super::diff::tree_of(found);
                fold(&mut items, self.viewer.open_all || !words.is_empty());
                self.viewer.tree.update(cx, |t, cx| t.set_items(items, cx));
                self.viewer.held = Some(held);
            }
        }
        if let Some(Surface::File(path)) = surface {
            let full = folder.join(&path);
            if self.viewer.path.as_ref() != Some(&full) {
                self.viewer.path = Some(full.clone());
                self.viewer.shown = None;
                self.read_file(full);
            }
        }
    }

    /// A file read: shown, its text in the editor with its syntax.
    pub(crate) fn file_read(
        &mut self,
        path: &std::path::Path,
        body: Result<Body, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.viewer.path.as_deref() != Some(path) {
            return;
        }
        self.viewer.shown = Some(match body {
            Ok(Body::Text(text)) => {
                let language = path
                    .extension()
                    .map_or_else(|| "text".to_owned(), |e| e.to_string_lossy().into_owned());
                self.viewer.editor.update(cx, |e, cx| {
                    e.set_highlighter(language, cx);
                    e.set_value(&text, window, cx);
                });
                Shown::Text(text)
            }
            Ok(Body::Image) => Shown::Image,
            Ok(Body::Pdf { pages, of }) => Shown::Pages(
                pages
                    .into_iter()
                    .map(|png| Arc::new(gpui::Image::from_bytes(ImageFormat::Png, png)))
                    .collect(),
                of,
            ),
            Ok(Body::Binary(size)) => Shown::Binary(size),
            Err(e) => Shown::Trouble(e),
        });
        cx.notify();
    }

    /// The Files tab: the line to find a file by name, folding, reading
    /// again, and the tree.
    pub(super) fn files_tab(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let listed = self.files.get(&s.meta.folder).map_or(0, Vec::len);
        let folder = s.meta.folder.clone();
        let head = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(10.))
            .py(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&self.viewer.search).small()),
            )
            .child(
                ui::icon_button(
                    "files-fold",
                    if self.viewer.open_all {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    if self.viewer.open_all {
                        "Fold every folder"
                    } else {
                        "Open every folder"
                    },
                )
                .size(px(24.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.viewer.open_all = !this.viewer.open_all;
                    cx.notify();
                })),
            )
            .child(
                ui::icon_button("files-refresh", "retry", "Read the files again")
                    .size(px(24.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.files.remove(&folder);
                        this.viewer.held = None;
                        cx.notify();
                    })),
            );
        let body = if listed == 0 {
            div().p(px(16.)).child(empty(
                faces,
                if s.repo {
                    "Reading the files…"
                } else {
                    "The project's files show here in a git repository."
                },
            ))
        } else {
            div()
                .flex_1()
                .min_h(px(0.))
                .py(px(4.))
                .child(self.file_tree(s, cx))
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(head)
            .child(body)
    }

    /// The tree of the project's files: a click opens one in its tab; a
    /// right-click, what else can be done with it.
    fn file_tree(&self, s: &Session, cx: &mut Context<Self>) -> impl IntoElement {
        let desk = cx.entity();
        let faces = crate::fonts::faces(cx).clone();
        let folder = s.meta.folder.clone();
        let opener = desk.clone();
        tree(&self.viewer.tree, move |ix, entry, selected, _, _| {
            let item = entry.item();
            let (folder_row, path) = (entry.is_folder(), item.id.to_string());
            let desk = opener.clone();
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
                                match (folder_row, entry.is_expanded()) {
                                    (false, _) => "file",
                                    (true, true) => "chevron-down",
                                    (true, false) => "chevron-right",
                                },
                                12.,
                            )
                            .text_color(theme::faint()),
                        )
                        .child(ui::one_line(
                            ui::text(&faces, item.label.clone())
                                .text_size(px(12.5))
                                .text_color(theme::text2()),
                        )),
                )
                .when(!folder_row, |d| {
                    d.on_click(move |_, _, cx| {
                        let path = PathBuf::from(&path);
                        desk.update(cx, |d, cx| d.open_surface(Surface::File(path), cx));
                    })
                })
        })
        .context_menu(move |_, entry, menu, _, _| {
            let named = entry.item().id.to_string();
            let full = folder.join(named.trim_end_matches('/'));
            let item = |label: &str, act: Act| {
                let desk = desk.clone();
                PopupMenuItem::new(label.to_owned())
                    .on_click(move |_, window, cx| desk.update(cx, |d, cx| act(d, window, cx)))
            };
            let (open, edit, reveal, add) =
                (named.clone(), full.clone(), full.clone(), full.clone());
            let mut menu = menu;
            if !entry.is_folder() {
                menu = menu.item(item(
                    "Open",
                    Box::new(move |d, _, cx| {
                        d.open_surface(Surface::File(PathBuf::from(&open)), cx);
                    }),
                ));
            }
            let (path, mention) = (named.clone(), format!("@{}", named.trim_end_matches('/')));
            menu.item(item(
                "Open in your editor",
                Box::new(move |d, _, _| d.edit(edit.clone(), None)),
            ))
            .item(item(
                "Reveal in Finder",
                Box::new(move |_, _, cx| cx.reveal_path(&reveal)),
            ))
            .separator()
            .item(item(
                "Add to chat",
                Box::new(move |d, window, cx| d.drop_files(std::slice::from_ref(&add), window, cx)),
            ))
            .item(item(
                "Copy path",
                Box::new(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
                }),
            ))
            .item(item(
                "Copy as mention",
                Box::new(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(mention.clone()));
                }),
            ))
        })
    }

    /// A file's tab: what it is and what can be done with it, then what it
    /// holds.
    pub(super) fn file_tab(
        &self,
        faces: &Faces,
        s: &Session,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Div {
        let full = s.meta.folder.join(path);
        let markdown = path
            .extension()
            .is_some_and(|e| e == "md" || e == "markdown");
        let value = self.viewer.editor.read(cx).value().to_string();
        let text = match &self.viewer.shown {
            Some(Shown::Text(was)) => Some(was.clone()),
            _ => None,
        };
        let changed = text.as_ref().is_some_and(|was| *was != value);
        let (edit, reveal, saved) = (full.clone(), full.clone(), full.clone());
        let head = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(36.))
            .px(px(12.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                ui::one_line(
                    ui::mono(faces, path.display().to_string())
                        .text_size(px(12.))
                        .text_color(theme::text2()),
                )
                .flex_1(),
            )
            .when(markdown && text.is_some(), |d| {
                d.child(
                    ui::button(
                        "md-drawn",
                        if self.viewer.drawn { "Source" } else { "Drawn" },
                        Weight::Quiet,
                    )
                    .h(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.viewer.drawn = !this.viewer.drawn;
                        cx.notify();
                    })),
                )
            })
            .when(text.is_some(), |d| {
                d.child(
                    ui::icon_button("file-wrap", "arrow-right", "Wrap long lines")
                        .size(px(24.))
                        .when(self.viewer.wrap, |b| b.text_color(theme::text()))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.viewer.wrap = !this.viewer.wrap;
                            let wrap = this.viewer.wrap;
                            this.viewer
                                .editor
                                .update(cx, |e, cx| e.set_soft_wrap(wrap, window, cx));
                        })),
                )
                .child(
                    ui::icon_button("file-copy", "copy", "Copy what it holds")
                        .size(px(24.))
                        .on_click({
                            let value = value.clone();
                            move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    value.clone(),
                                ));
                            }
                        }),
                )
            })
            .child(
                ui::icon_button("file-editor", "pencil", "Open in your editor")
                    .size(px(24.))
                    .on_click(cx.listener(move |this, _, _, _| this.edit(edit.clone(), None))),
            )
            .child(
                ui::icon_button("file-reveal", "folder", "Reveal in Finder")
                    .size(px(24.))
                    .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
            )
            .when(changed, |d| {
                d.child(
                    ui::button("file-save", "Save", Weight::Primary)
                        .h(px(24.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let text = this.viewer.editor.read(cx).value().to_string();
                            this.write(saved.clone(), text.clone());
                            this.viewer.shown = Some(Shown::Text(text));
                            cx.notify();
                        })),
                )
            });
        let body: gpui::AnyElement = match &self.viewer.shown {
            None => div()
                .p(px(16.))
                .child(empty(faces, "Reading…"))
                .into_any_element(),
            Some(Shown::Text(_)) if markdown && self.viewer.drawn => div()
                .id("file-drawn")
                .flex_1()
                .min_h(px(0.))
                .px(px(16.))
                .py(px(12.))
                .child(markdown::render(faces, "file-drawn", &value, cx))
                .overflow_y_scrollbar()
                .into_any_element(),
            Some(Shown::Text(_)) => div()
                .flex_1()
                .min_h(px(0.))
                .child(Editor::new(&self.viewer.editor).bordered(false).h_full())
                .into_any_element(),
            Some(Shown::Image) => div()
                .flex_1()
                .min_h(px(0.))
                .p(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .child(img(full).max_w_full().max_h_full())
                .into_any_element(),
            Some(Shown::Pages(pages, of)) => {
                let mut list = div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .p(px(16.));
                for page in pages {
                    list = list.child(img(page.clone()).w_full().rounded(px(4.)));
                }
                if *of > pages.len() {
                    list = list.child(fine(
                        faces,
                        format!(
                            "The first {} of {of} pages: open it to read the rest.",
                            pages.len()
                        ),
                    ));
                }
                div()
                    .id("file-pages")
                    .flex_1()
                    .min_h(px(0.))
                    .bg(theme::well())
                    .child(list)
                    .overflow_y_scrollbar()
                    .into_any_element()
            }
            Some(Shown::Binary(size)) => div()
                .p(px(16.))
                .child(empty(
                    faces,
                    format!(
                        "Not text: {} KB. Open it in your editor or reveal it in Finder.",
                        size.div_ceil(1024)
                    ),
                ))
                .into_any_element(),
            Some(Shown::Trouble(e)) => div()
                .p(px(16.))
                .child(empty(faces, format!("Couldn't read it: {e}")))
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(head)
            .child(body)
    }
}
