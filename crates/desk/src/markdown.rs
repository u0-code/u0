//! What an agent says, drawn the way it was written: gpui-kit's Markdown
//! view reads it, sets it, colours its code and lets its words be chosen
//! and copied. Ghost adds what only the desk can say: where a link may go,
//! that a name in code which reads as a file opens it in your editor, and
//! what the buttons on a code block and a table do.

use std::borrow::Cow;

use gpui::{
    App, ClickEvent, Context, Div, IntoElement, Overflow, SharedString, StyleRefinement,
    WeakEntity, WhiteSpace, Window,
    component::text::{
        MarkdownNode, MarkdownParseContext, MarkdownPlugin, TableData, TextView, TextViewStyle,
        markdown_ast::Node,
    },
    div,
    prelude::*,
    px,
};

use crate::{
    desk::Desk,
    fonts::Faces,
    style::{H1, H2, H3, PROSE, PROSE_LINE},
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
    // A table keeps its cells on one line and scrolls sideways rather than
    // breaking a number.
    let mut table = StyleRefinement::default();
    table.overflow.x = Some(Overflow::Scroll);
    let mut cell = StyleRefinement::default();
    cell.text.white_space = Some(WhiteSpace::Nowrap);
    let style = TextViewStyle::default()
        .heading_font_size(|level, _| {
            px(match level {
                1 => H1,
                2 => H2,
                _ => H3,
            })
        })
        .code_block(code_block)
        .table(table)
        .table_cell(cell);
    div()
        .w_full()
        .min_w(px(0.))
        .font_family(faces.ui.clone())
        .text_size(px(PROSE))
        .line_height(px(PROSE_LINE))
        .text_color(theme::text())
        .child(
            TextView::markdown(SharedString::from(format!("{key}-words")), bulleted(text))
                .style(style)
                .selectable(true)
                .plugin(Paths(desk))
                .on_link_click(move |to, click, _, cx| {
                    follow(&links, to, click.modifiers().platform, cx);
                })
                .code_block_actions(move |block, _, _| {
                    code_actions(&blocks, &block.lang().unwrap_or_default(), &block.code())
                })
                .table_actions(|table, _, _| table_actions(table)),
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

/// A link clicked: a commit of the session's repository in its Diff tab
/// (with ⌘, on its host), an address in the browser or in mail, a file in
/// your editor; any other link does nothing.
fn follow(desk: &WeakEntity<Desk>, to: &str, outside: bool, cx: &mut App) {
    let here = commit_of(to).filter(|_| !outside).is_some_and(|id| {
        desk.update(cx, |d, cx| d.show_commit(id, cx))
            .unwrap_or(false)
    });
    if here {
        return;
    }
    if web(to) {
        cx.open_url(to);
    } else if path_like(to) {
        let _ = desk.update(cx, |d, _| d.open_named(to));
    }
}

/// The commit a host's commit link names (`…/commit/<sha>`), as its first
/// seven characters.
fn commit_of(address: &str) -> Option<&str> {
    let (_, rest) = address.split_once("/commit/")?;
    let sha = rest.split(['/', '#', '?']).next()?;
    (sha.len() >= 7 && sha.bytes().all(|b| b.is_ascii_hexdigit())).then(|| &sha[..7])
}

/// Whether a link opens in the browser or in mail. No other scheme is
/// opened: a link's words hide where it goes, and `file:` or an app's own
/// scheme would launch what it names.
fn web(address: &str) -> bool {
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|s| address.starts_with(s))
}

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
        div()
            .id(SharedString::from(format!("path-{path}")))
            .px(px(3.))
            .rounded(px(4.))
            // The shade the kit sets code on: its accent (`theme::kit`).
            .bg(theme::hover())
            .font_family(crate::fonts::faces(cx).mono.clone())
            .text_color(theme::link())
            .cursor_pointer()
            .child(path.clone())
            .on_click(move |_: &ClickEvent, _, cx| {
                let _ = desk.update(cx, |d, _| d.open_named(&path));
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
                ui::icon_button("run", "play", "Run it in a terminal of yours").on_click(
                    move |_, window, cx| {
                        let _ =
                            desk.update(cx, |d, cx| d.open_terminal(Some(&command), window, cx));
                    },
                ),
            )
        })
        .child(
            ui::icon_button("copy", "copy", "Copy").on_click(move |_, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
            }),
        )
}

/// Under a table: copy it as Markdown or as CSV.
fn table_actions(table: &TableData) -> Div {
    let copy = |id: &'static str, label: &'static str, text: String| {
        ui::button(id, label, ui::Weight::Quiet).on_click(move |_, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        })
    };
    div()
        .flex()
        .gap(px(2.))
        .child(copy("md", "Copy as Markdown", table.markdown.clone()))
        .child(copy("csv", "Copy as CSV", csv(table)))
}

/// A table's words as CSV, cells quoted where they need it.
fn csv(table: &TableData) -> String {
    let row = |cells: &[String]| {
        cells
            .iter()
            .map(|c| {
                if c.contains([',', '"', '\n']) {
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

    #[test]
    fn a_host_commit_link_names_its_commit() {
        assert_eq!(
            commit_of("https://github.com/o/r/commit/9b3f184b0c1d2e3f#diff-1"),
            Some("9b3f184")
        );
        assert_eq!(commit_of("https://github.com/o/r/pull/12"), None);
        assert_eq!(commit_of("https://github.com/o/r/commit/main"), None);
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
    fn a_link_opens_only_the_web_mail_or_a_file_never_an_app() {
        assert!(web("https://example.com") && web("http://x.y") && web("mailto:a@b.c"));
        for launches in [
            "file:///Applications/X.app",
            "x-app://run",
            "javascript:go()",
        ] {
            assert!(!web(launches) && !path_like(launches), "{launches}");
        }
        assert!(path_like("src/a.rs:12"));
    }
}
