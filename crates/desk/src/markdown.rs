//! What an agent says, drawn the way it was written: gpui-kit's Markdown
//! view reads it, sets it, colours its code and lets its words be chosen
//! and copied. u0 adds what only the desk can say: where a link may go,
//! that a name in code which reads as a file opens it in your editor (its
//! menu copies where it is), GitHub's alerts (`> [!NOTE]`) set apart, and
//! what the buttons on a code block and a table do; and the pictures a
//! pull request or an issue shows, fetched by its server.

use std::{
    borrow::Cow,
    collections::{HashMap, hash_map::Entry},
    fmt::Write as _,
    sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError},
};

use gpui::{
    App, ClickEvent, Context, Div, IntoElement, Overflow, SharedString, StyleRefinement,
    WeakEntity, Window,
    component::{
        menu::{ContextMenuExt as _, PopupMenuItem},
        text::{
            MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, TableData,
            TextView, TextViewStyle, markdown_ast::Node,
        },
    },
    div,
    prelude::*,
    px,
};

use crate::{
    desk::Desk,
    figures,
    fonts::Faces,
    session::Picture,
    style::{H1, H2, H3, PROSE, prose_line},
    theme, ui,
};

/// A message, drawn as prose: the sans, in prose's size, headings in
/// theirs. `key` tells this message's view apart from every other's.
pub fn render(faces: &Faces, key: &str, text: &str, cx: &mut Context<Desk>) -> impl IntoElement {
    let desk = cx.entity().downgrade();
    let (links, blocks) = (desk.clone(), desk.clone());
    // A code block wraps: the kit's cannot scroll sideways, its lines laid
    // out no wider than the block. Its first line starts under the buttons
    // in its corner, as under the kit's own label there.
    let code_block = StyleRefinement::default().pt(px(30.));
    // A table fits the column: gpui-kit keeps its narrow columns (a
    // number, a name) whole and wraps its widest, and scrolls sideways only
    // past what wrapping can give. Cells kept on one line scrolled at once,
    // with no scrollbar to say so, and the last column was out of reach.
    let mut table = StyleRefinement::default();
    table.overflow.x = Some(Overflow::Scroll);
    let style = TextViewStyle::default()
        .heading_font_size(|level, _| {
            px(match level {
                1 => H1,
                2 => H2,
                _ => H3,
            })
        })
        .code_block(code_block)
        .table(table);
    let came = came(text);
    let view = match came {
        Some(n) => format!("{key}-words-{n}"),
        None => format!("{key}-words"),
    };
    div()
        .w_full()
        .min_w(px(0.))
        .font_family(faces.ui.clone())
        .text_size(px(PROSE))
        .line_height(px(prose_line()))
        .text_color(theme::text())
        .child(
            TextView::markdown(SharedString::from(view), bulleted(text))
                .style(style)
                .selectable(true)
                .markdown_extensions(extensions())
                .when(came.is_some(), |v| v.image_source(fetched))
                .plugin(Paths(desk))
                .plugin(figures::Figures(figures::math()))
                .when(figures::math(), |v| v.plugin(figures::Formulas))
                .on_link_click(move |to, click, window, cx| {
                    follow(&links, to, click.modifiers().platform, window, cx);
                })
                .code_block_actions(move |block, _, _| {
                    code_actions(&blocks, &block.lang().unwrap_or_default(), &block.code())
                })
                .table_actions(|table, _, _| table_actions(table)),
        )
}

/// The pictures asked of a server for what Markdown shows, and the texts
/// that show them.
#[derive(Default)]
struct Pictures {
    /// By address: none while one has not come; and when it was last drawn.
    kept: HashMap<String, (Option<Arc<Picture>>, u64)>,
    /// The addresses each text shows, by a hash of the text.
    texts: HashMap<u64, Vec<String>>,
    /// The pictures drawn, counted: which was drawn longest ago.
    drawn: u64,
}

/// The most bytes of pictures kept: past it, those drawn longest ago go,
/// to be asked for again when they are next shown.
const KEPT_BYTES: usize = 128 << 20;

/// The most texts whose pictures are known at once.
const TEXTS: usize = 1024;

static PICTURES: LazyLock<Mutex<Pictures>> = LazyLock::new(Mutex::default);

fn pictures() -> MutexGuard<'static, Pictures> {
    PICTURES.lock().unwrap_or_else(PoisonError::into_inner)
}

fn hash(text: &str) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(text, &mut h);
    std::hash::Hasher::finish(&h)
}

impl Pictures {
    /// The pictures of an item read, `urls` (`pictures::of`), shown in
    /// `texts`: which each text shows is known, and those not yet asked
    /// for are given back, taken as asked.
    fn wanted<'a>(
        &mut self,
        texts: impl IntoIterator<Item = &'a str>,
        urls: Vec<String>,
    ) -> Vec<String> {
        if self.texts.len() > TEXTS {
            self.texts.clear();
        }
        for text in texts {
            let shows: Vec<_> = (crate::pictures::found(text).into_iter())
                .filter(|url| urls.contains(url))
                .collect();
            if !shows.is_empty() {
                self.texts.insert(hash(text), shows);
            }
        }
        let mut asked = urls;
        asked.retain(|url| match self.kept.entry(url.clone()) {
            Entry::Vacant(e) => {
                e.insert((None, 0));
                true
            }
            Entry::Occupied(_) => false,
        });
        asked
    }

    /// The picture at `url` come, those drawn longest ago let go past
    /// `most` bytes; one that did not come is asked for again the next time
    /// it is shown.
    fn came(&mut self, url: String, picture: Option<Arc<Picture>>, most: usize) {
        let Some(picture) = picture else {
            self.kept.remove(&url);
            return;
        };
        self.drawn += 1;
        self.kept.insert(url, (Some(picture), self.drawn));
        let bytes = |kept: &HashMap<String, (Option<Arc<Picture>>, u64)>| -> usize {
            kept.values()
                .filter_map(|(p, _)| p.as_ref())
                .map(|p| p.bytes.len())
                .sum()
        };
        while bytes(&self.kept) > most {
            let oldest = (self.kept.iter())
                .filter(|(_, (p, _))| p.is_some())
                .min_by_key(|(_, (_, at))| *at)
                .map(|(url, _)| url.clone());
            match oldest {
                Some(url) => self.kept.remove(&url),
                None => break,
            };
        }
    }

    /// How many of the pictures `text` shows have come; none, it is not a
    /// text whose pictures were asked for.
    fn shown(&self, text: &str) -> Option<usize> {
        let urls = self.texts.get(&hash(text))?;
        Some(
            (urls.iter())
                .filter(|url| matches!(self.kept.get(*url), Some((Some(_), _))))
                .count(),
        )
    }
}

/// The pictures of an item read, `urls`, shown in `texts`: those not yet
/// asked for, taken as asked.
pub fn wanted<'a>(texts: impl IntoIterator<Item = &'a str>, urls: Vec<String>) -> Vec<String> {
    pictures().wanted(texts, urls)
}

/// The picture at `url` come, or not.
pub fn fetched_at(url: String, picture: Option<Arc<Picture>>) {
    pictures().came(url, picture, KEPT_BYTES);
}

/// How many of the pictures `text` shows have come; none, it shows none
/// asked for. Only then are its pictures drawn from what came, so that the
/// view's own reading of any other (a `data:` address) is kept; and its
/// view is a new one each time another comes, as the view lays a picture
/// out once, at the size it had then.
fn came(text: &str) -> Option<usize> {
    pictures().shown(text)
}

/// What the view draws for the picture at `url`: what came, or nothing yet.
fn fetched(url: &gpui::SharedUri) -> gpui::ImageSource {
    let mut pictures = pictures();
    pictures.drawn += 1;
    let drawn = pictures.drawn;
    match pictures.kept.get_mut(url.as_ref()) {
        Some((Some(picture), at)) => {
            *at = drawn;
            crate::thread::picture(picture)
        }
        _ => gpui::ImageSource::Custom(Arc::new(|_, _| None)),
    }
}

/// What of Markdown is read: HTML as gpui-kit reads it into its own nodes
/// (an `<img>` a picture, by its `src`, `alt` and size; a `<br>` a break),
/// never drawn as a page; and GitHub's alerts, each a block set apart by a line down its left in its
/// colour, under its name: `> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`,
/// `[!WARNING]`, `[!CAUTION]`, and Claude's `[!INSIGHT]`.
fn extensions() -> MarkdownExtensions {
    type Colour = fn() -> gpui::Hsla;
    let kinds: [(&str, &str, &str, Colour); 6] = [
        ("note", "Note", "alert", theme::link),
        ("tip", "Tip", "bulb", theme::ok),
        ("important", "Important", "alert", theme::accent),
        ("warning", "Warning", "alert", theme::coral),
        ("caution", "Caution", "alert", theme::gone),
        ("insight", "Insight", "bulb", theme::accent),
    ];
    kinds.into_iter().fold(
        MarkdownExtensions::default(),
        |e, (name, title, icon, colour)| {
            e.block_surface(name, move |_, _| {
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .pl(px(12.))
                    .py(px(2.))
                    .border_l_2()
                    .border_color(colour())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_color(colour())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(ui::icon(icon, 14.))
                            .child(crate::tr(title)),
                    )
            })
        },
    )
}

/// `text` with each line begun with `• `, as models write a list, made a
/// list item as one begun with `- ` is, outside fenced code.
fn bulleted(text: &str) -> SharedString {
    if !text.contains("• ") {
        return SharedString::from(text.to_owned());
    }
    let mut fenced = false;
    text.split_inclusive('\n')
        .map(|line| {
            let words = line.trim_start();
            fenced ^= words.starts_with("```") || words.starts_with("~~~");
            match words.strip_prefix("• ") {
                Some(rest) if !fenced => {
                    Cow::Owned(format!("{}- {rest}", &line[..line.len() - words.len()]))
                }
                _ => Cow::Borrowed(line),
            }
        })
        .collect::<String>()
        .into()
}

/// A link clicked: a commit of the session's repository in its Diff tab,
/// a pull request on the PRs page (with ⌘, either on its host), else as
/// any link an agent gives (`Desk::follow_link`): a web address in your
/// browser, a file of the thread's in u0's viewer, nothing else.
fn follow(
    desk: &WeakEntity<Desk>,
    to: &str,
    outside: bool,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let here = !outside
        && desk
            .update(cx, |d, cx| match (commit_of(to), pull_of(to)) {
                (Some(id), _) => d.show_commit(id, cx),
                (_, Some(n)) => d.show_pr(n, window, cx),
                _ => false,
            })
            .unwrap_or(false);
    if here {
        return;
    }
    let _ = desk.update(cx, |d, cx| d.follow_link(to, cx));
}

/// The commit a host's commit link names (`…/commit/<sha>`), as its first
/// seven characters.
fn commit_of(address: &str) -> Option<&str> {
    let (_, rest) = address.split_once("/commit/")?;
    let sha = rest.split(['/', '#', '?']).next()?;
    (sha.len() >= 7 && sha.bytes().all(|b| b.is_ascii_hexdigit())).then(|| &sha[..7])
}

/// The pull request a host's link names (`…/pull/<n>`).
fn pull_of(address: &str) -> Option<u64> {
    let (_, rest) = address.split_once("/pull/")?;
    rest.split(['/', '#', '?']).next()?.parse().ok()
}

/// Whether a link opens in the browser or in mail. No other scheme is
/// opened: a link's words hide where it goes, and `file:` or an app's own
/// scheme would launch what it names.
/// Whether code reads as a file's path: `src/a.rs`, `a.rs:12`, `./x/y`.
pub fn path_like(text: &str) -> bool {
    let name = text.split(':').next().unwrap_or(text);
    !name.is_empty()
        && name.len() < 200
        && !name.contains(char::is_whitespace)
        && !name.contains("://")
        && (name.contains('/')
            || std::path::Path::new(name).extension().is_some_and(|e| {
                (1..=5).contains(&e.len())
                    && e.to_str()
                        .is_some_and(|e| e.chars().all(char::is_alphanumeric))
            }))
        && name.chars().any(char::is_alphabetic)
}

/// Inline code that names a file, which a click opens in your editor. The
/// kit's inline code is not clickable, so such code is drawn here: in mono
/// on the shade the kit gives code, one piece that does not break.
struct Paths(WeakEntity<Desk>);

impl MarkdownPlugin for Paths {
    fn name(&self) -> &'static str {
        "path"
    }

    fn parse(&self, node: &Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        match node {
            Node::InlineCode(code) if path_like(&code.value) => {
                Some(MarkdownNode::new("path", ()).text(code.value.clone()))
            }
            _ => None,
        }
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (desk, path) = (self.0.clone(), node.as_text().to_owned());
        let full = (desk.upgrade())
            .map(|d| d.read(cx).full_path(&path).display().to_string())
            .unwrap_or_default();
        let (opens, written) = (desk.clone(), path.clone());
        div()
            .id(SharedString::from(format!("path-{path}")))
            .px(px(3.))
            .rounded(px(4.))
            // The shade the kit sets code on: its accent (`theme::kit`).
            .bg(theme::hover())
            .text_color(theme::link())
            .cursor_pointer()
            // The face on the words only: the menu is in the window's.
            .child(
                div()
                    .font_family(crate::fonts::faces(cx).mono.clone())
                    .child(path.clone()),
            )
            .on_click(move |_: &ClickEvent, _, cx| {
                let _ = desk.update(cx, |d, _| d.open_named(&path));
            })
            .context_menu(move |menu, _, _| {
                let copy = |label: &str, text: String| {
                    PopupMenuItem::new(crate::tr(label)).on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                    })
                };
                let (desk, path) = (opens.clone(), written.clone());
                menu.item(
                    PopupMenuItem::new(crate::tr("Open in your editor")).on_click(
                        move |_, _, cx| {
                            let _ = desk.update(cx, |d, _| d.open_named(&path));
                        },
                    ),
                )
                .item(copy("Copy the path", written.clone()))
                .item(copy("Copy the full path", full.clone()))
            })
    }
}

/// From a code block's corner: run it in a terminal of yours when it is a
/// shell's, and copy it.
fn code_actions(desk: &WeakEntity<Desk>, lang: &str, code: &str) -> Div {
    let shell = matches!(lang, "bash" | "sh" | "zsh" | "shell" | "console" | "fish");
    let command: String = code
        .lines()
        .map(|l| l.strip_prefix("$ ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n");
    let (desk, copy) = (desk.clone(), code.to_owned());
    div()
        .flex()
        .gap(px(2.))
        .when(shell, |d| {
            d.child(
                ui::icon_button("run", "play", crate::tr("Run it in a terminal of yours"))
                    .on_click(move |_, window, cx| {
                        let _ =
                            desk.update(cx, |d, cx| d.open_terminal(Some(&command), window, cx));
                    }),
            )
        })
        .child(
            ui::icon_button("copy", "copy", crate::tr("Copy")).on_click(move |_, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
            }),
        )
}

/// Under a table: copy it as Markdown or as CSV.
fn table_actions(table: &TableData) -> Div {
    let copy = |id: &'static str, label: &'static str, text: String| {
        ui::button(id, crate::tr(label), ui::Weight::Quiet).on_click(move |_, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        })
    };
    div()
        .flex()
        .gap(px(2.))
        .child(copy("md", "Copy as Markdown", table.markdown.clone()))
        .child(copy("csv", "Copy as CSV", csv(table)))
        .child(copy("html", "Copy as HTML", html(table)))
}

/// A table's words as an HTML table, what HTML gives meaning escaped.
fn html(table: &TableData) -> String {
    let escape = |c: &str| {
        c.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let row = |cells: &[String], tag: &str| {
        let mut out = String::from("<tr>");
        for c in cells {
            let _ = write!(out, "<{tag}>{}</{tag}>", escape(c));
        }
        out + "</tr>"
    };
    let body: String = table.rows.iter().map(|r| row(r, "td")).collect();
    format!(
        "<table><thead>{}</thead><tbody>{body}</tbody></table>",
        row(&table.headers, "th")
    )
}

/// A table's words as CSV, cells quoted where they need it.
fn csv(table: &TableData) -> String {
    let row = |cells: &[String]| {
        cells
            .iter()
            .map(|c| {
                if c.contains([',', '"', '\n', '\r']) {
                    format!("\"{}\"", c.replace('"', "\"\""))
                } else {
                    c.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    std::iter::once(row(&table.headers))
        .chain(table.rows.iter().map(|r| row(r)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(side: u8, extra: usize) -> Option<Arc<Picture>> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0".to_vec();
        b.extend([side, 0, 0, 0, side, 8, 6, 0, 0, 0]);
        b.resize(b.len() + extra, 0);
        Picture::read(b)
    }

    #[test]
    fn a_picture_is_asked_for_once_and_again_only_after_it_failed() {
        let mut p = Pictures::default();
        let (body, url) = (
            "![a](https://example.com/once.png)",
            "https://example.com/once.png",
        );
        assert_eq!(p.wanted([body], vec![url.into()]), [url]);
        assert!(p.wanted([body], vec![url.into()]).is_empty());
        assert_eq!(p.shown(body), Some(0));
        p.came(url.into(), None, KEPT_BYTES);
        assert_eq!(p.wanted([body], vec![url.into()]), [url]);
    }

    #[test]
    fn a_text_is_drawn_anew_as_each_of_its_pictures_comes_its_references_read() {
        let mut p = Pictures::default();
        let body =
            "<img src=\"https://example.com/a.png?x=1&amp;y=2\"> ![b](https://example.com/b.png)";
        let urls = vec![
            "https://example.com/a.png?x=1&y=2".to_owned(),
            "https://example.com/b.png".to_owned(),
        ];
        p.wanted([body], urls.clone());
        assert_eq!(p.shown(body), Some(0));
        p.came(urls[0].clone(), png(1, 0), KEPT_BYTES);
        assert_eq!(p.shown(body), Some(1));
        assert_eq!(p.shown("no pictures here"), None);
    }

    #[test]
    fn the_pictures_drawn_longest_ago_go_past_the_most_kept() {
        let mut p = Pictures::default();
        let urls: Vec<String> = (0..3)
            .map(|n| format!("https://example.com/{n}.png"))
            .collect();
        let body = urls.iter().fold(String::new(), |mut body, u| {
            let _ = write!(body, "![]({u}) ");
            body
        });
        p.wanted([body.as_str()], urls.clone());
        for url in &urls {
            p.came(url.clone(), png(1, 1000), 2500);
        }
        assert_eq!(p.shown(&body), Some(2));
        assert!(!matches!(p.kept.get(&urls[0]), Some((Some(_), _))));
        assert_eq!(p.wanted([body.as_str()], urls.clone()), [urls[0].clone()]);
    }

    #[test]
    fn a_host_commit_link_names_its_commit() {
        assert_eq!(
            commit_of("https://github.com/o/r/commit/9b3f184b0c1d2e3f#diff-1"),
            Some("9b3f184")
        );
        assert_eq!(commit_of("https://github.com/o/r/pull/12"), None);
        assert_eq!(commit_of("https://github.com/o/r/commit/main"), None);
        assert_eq!(
            pull_of("https://github.com/o/r/pull/12#issuecomment-1"),
            Some(12)
        );
        assert_eq!(pull_of("https://github.com/o/r/issues/12"), None);
    }

    #[test]
    fn a_table_copies_as_csv_quoting_what_needs_it() {
        let table = TableData {
            headers: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "x, y".into()]],
            ..TableData::default()
        };
        assert_eq!(csv(&table), "a,b\n1,\"x, y\"");
    }

    #[test]
    fn a_table_copies_as_html_escaping_its_cells() {
        let table = TableData {
            headers: vec!["a".into()],
            rows: vec![vec!["<b> & c".into()]],
            ..TableData::default()
        };
        assert_eq!(
            html(&table),
            "<table><thead><tr><th>a</th></tr></thead><tbody><tr><td>&lt;b&gt; &amp; c</td></tr></tbody></table>"
        );
    }

    #[test]
    fn code_naming_a_file_is_a_path() {
        assert!(path_like("src/upload/s3.rs"));
        assert!(path_like("main.rs:12"));
        assert!(!path_like("Uploader::send"));
        assert!(!path_like("let x = 1"));
        assert!(!path_like("1.5"));
    }

    #[test]
    fn a_line_begun_with_a_bullet_is_a_list_item_but_not_in_code() {
        assert_eq!(
            bulleted("Found:\n\n• one\n  • under one\n```\n• kept\n```"),
            "Found:\n\n- one\n  - under one\n```\n• kept\n```"
        );
    }

    #[test]
    fn a_name_reads_as_a_path_but_an_app_s_address_does_not() {
        for launches in [
            "file:///Applications/X.app",
            "x-app://run",
            "javascript:go()",
        ] {
            assert!(!path_like(launches), "{launches}");
        }
        assert!(path_like("src/a.rs:12"));
    }
}
