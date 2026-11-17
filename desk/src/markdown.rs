//! What the solver says, drawn the way it was written: the small part of
//! Markdown a model actually writes. Paragraphs, headings, lists (nested
//! by their indent), quotes, fenced code, tables, and inside a line
//! `code`, **bold** and *italic* (with asterisks only). Anything else is
//! left as the text it is.
//!
//! It is set in the thread's type (`thread/style.rs`): body text with room
//! between its lines, blocks a step apart and the items of a list closer,
//! inline code in mono on a faint ground, and a code block in a well that
//! scrolls sideways rather than wrapping a command.

use std::ops::Range;

use gpui::{
    Div, FontStyle, FontWeight, Hsla, SharedString, StyledText, TextRun, div, prelude::*, px,
    relative,
};

use crate::{
    fonts::{self, Faces},
    theme,
    thread::style::{
        BOX_RADIUS, H1, H2, H3, MONO, MONO_LINE, PROSE, PROSE_LINE, S1, S2, S3, S4, SMALL,
        SMALL_LINE, WELL_RADIUS,
    },
    ui,
};

/// One block of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(String),
    Heading(u8, String),
    /// A list item: its marker as written (`•` for an unordered one), its
    /// text, and how deep it is nested, from 0.
    Item {
        marker: String,
        text: String,
        depth: u8,
    },
    Quote(String),
    Code {
        lang: String,
        text: String,
    },
    /// A table: its head, how each column is set, and its rows, each cut
    /// or padded to as many cells as the head has.
    Table {
        head: Vec<String>,
        right: Vec<bool>,
        rows: Vec<Vec<String>>,
    },
}

/// The cells of a table's line: what is between its bars, trimmed.
fn cells(line: &str) -> Vec<String> {
    let inner = line.trim();
    let inner = inner.strip_prefix('|').unwrap_or(inner);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(|c| c.trim().to_owned()).collect()
}

/// Whether a line is the rule under a table's head, `|---|:--:|`; and if
/// so, which of its columns are set to the right.
fn rule(line: &str) -> Option<Vec<bool>> {
    let t = line.trim();
    if !t.contains('-') || !t.contains('|') && !t.starts_with('-') {
        return None;
    }
    cells(t)
        .iter()
        .map(|c| {
            let dashes = c.trim_matches(':');
            (!dashes.is_empty() && dashes.chars().all(|ch| ch == '-'))
                .then(|| c.ends_with(':') && !c.starts_with(':'))
        })
        .collect()
}

/// How deep a line is nested, by its indent: two spaces, or a tab, a level.
fn depth(line: &str) -> u8 {
    let spaces: usize = line
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 2 } else { 1 })
        .sum();
    u8::try_from((spaces / 2).min(3)).unwrap_or(3)
}

/// Whether a character belongs to a script written without spaces between
/// words: lines of it are joined without one.
fn unspaced(c: char) -> bool {
    matches!(c,
        '\u{3000}'..='\u{30FF}'   // CJK punctuation, kana
        | '\u{3400}'..='\u{4DBF}' // CJK extension A
        | '\u{4E00}'..='\u{9FFF}' // CJK unified ideographs
        | '\u{F900}'..='\u{FAFF}' // compatibility ideographs
        | '\u{FF00}'..='\u{FFEF}' // full-width forms
    )
}

/// The lines of a paragraph as one: a space between lines, except where
/// both sides are Chinese or Japanese, which a line break does not space.
fn joined(lines: &[&str]) -> String {
    let mut out = String::new();
    for line in lines {
        let starts_unspaced = line.chars().next().is_some_and(unspaced);
        let ends_unspaced = out.chars().next_back().is_some_and(unspaced);
        if !(out.is_empty() || starts_unspaced && ends_unspaced) {
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

/// Split a message into blocks.
pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let lines = text.lines();
    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(joined(paragraph)));
            paragraph.clear();
        }
    };
    let mut lines = lines.peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        // A table: a line of bars over a rule, then the lines of bars that
        // follow.
        if trimmed.starts_with('|')
            && let Some(right) = lines.peek().and_then(|next| rule(next))
        {
            flush(&mut paragraph, &mut blocks);
            lines.next();
            let head = cells(trimmed);
            let n = head.len();
            let mut rows = Vec::new();
            while let Some(next) = lines.peek() {
                if !next.trim_start().starts_with('|') {
                    break;
                }
                let mut row = cells(next);
                row.resize(n, String::new());
                rows.push(row);
                lines.next();
            }
            let mut right = right;
            right.resize(n, false);
            blocks.push(Block::Table { head, right, rows });
        } else if let Some(lang) = trimmed.strip_prefix("```") {
            flush(&mut paragraph, &mut blocks);
            let mut code = Vec::new();
            for inner in lines.by_ref() {
                if inner.trim_start().starts_with("```") {
                    break;
                }
                code.push(inner);
            }
            blocks.push(Block::Code {
                lang: lang.trim().to_owned(),
                text: code.join("\n"),
            });
        } else if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
        } else if let Some((level, rest)) = heading(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading(level, rest.to_owned()));
        } else if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("• "))
        {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Item {
                marker: "•".to_owned(),
                text: rest.to_owned(),
                depth: depth(line),
            });
        } else if let Some((n, rest)) = numbered(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Item {
                marker: format!("{n}."),
                text: rest.to_owned(),
                depth: depth(line),
            });
        } else if let Some(rest) = trimmed
            .strip_prefix("> ")
            .or_else(|| trimmed.strip_prefix('>'))
        {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Quote(rest.to_owned()));
        } else {
            paragraph.push(trimmed);
        }
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=4).contains(&hashes) {
        line[hashes..]
            .strip_prefix(' ')
            .map(|rest| (u8::try_from(hashes).unwrap_or(4), rest))
    } else {
        None
    }
}

fn numbered(line: &str) -> Option<(&str, &str)> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = line[digits..].strip_prefix(". ")?;
    Some((&line[..digits], rest))
}

/// A line's text with its inline marks taken out, and where each mark was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub code: Vec<Range<usize>>,
    pub bold: Vec<Range<usize>>,
    pub italic: Vec<Range<usize>>,
}

/// Take `code`, **bold** and *italic* out of a line. An unclosed mark is
/// left as the character it is.
pub fn inline(source: &str) -> Inline {
    let mut out = Inline {
        text: String::with_capacity(source.len()),
        code: Vec::new(),
        bold: Vec::new(),
        italic: Vec::new(),
    };
    let mut rest = source;
    while let Some(c) = rest.chars().next() {
        let marked = match c {
            '`' => take(rest, "`").map(|(inner, used)| (inner, used, 0)),
            '*' if rest.starts_with("**") => take(rest, "**").map(|(inner, used)| (inner, used, 1)),
            // Only `*`: an underscore is far more often part of a name.
            '*' => take(rest, "*").map(|(inner, used)| (inner, used, 2)),
            _ => None,
        };
        match marked {
            Some((inner, used, which)) if !inner.is_empty() => {
                let start = out.text.len();
                out.text.push_str(inner);
                let range = start..out.text.len();
                match which {
                    0 => out.code.push(range),
                    1 => out.bold.push(range),
                    _ => out.italic.push(range),
                }
                rest = &rest[used..];
            }
            _ => {
                out.text.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// The text between an opening `mark` at the start of `s` and the next
/// one, and how many bytes the whole took.
fn take<'a>(s: &'a str, mark: &str) -> Option<(&'a str, usize)> {
    let after = &s[mark.len()..];
    let end = after.find(mark)?;
    // `* ` is a bullet or a product, not emphasis.
    if mark != "`" && (after.starts_with(' ') || after[..end].ends_with(' ')) {
        return None;
    }
    Some((&after[..end], mark.len() + end + mark.len()))
}

/// How a stretch of a line is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Plain,
    Code,
    Bold,
    Italic,
}

/// The whole of a line's text in stretches, each with how it is set, in
/// order and leaving nothing out: what a run of text is drawn from.
pub fn spans(line: &Inline) -> Vec<(Range<usize>, Mark)> {
    let mut marked: Vec<(Range<usize>, Mark)> = line
        .code
        .iter()
        .map(|r| (r.clone(), Mark::Code))
        .chain(line.bold.iter().map(|r| (r.clone(), Mark::Bold)))
        .chain(line.italic.iter().map(|r| (r.clone(), Mark::Italic)))
        .collect();
    marked.sort_by_key(|(r, _)| r.start);
    let mut out = Vec::new();
    let mut at = 0;
    for (r, mark) in marked {
        if r.start > at {
            out.push((at..r.start, Mark::Plain));
        }
        at = r.end;
        out.push((r, mark));
    }
    if at < line.text.len() {
        out.push((at..line.text.len(), Mark::Plain));
    }
    out
}

/// The weight bold is set in: in the serif a true bold, since the Chinese
/// it falls back to (Songti) has a bold and no semibold, and a semibold
/// would leave Chinese words beside bold Latin ones looking plain.
fn strong_weight(faces: &Faces, face: &SharedString) -> FontWeight {
    if face == &faces.serif {
        FontWeight::BOLD
    } else {
        FontWeight::SEMIBOLD
    }
}

/// How a block's text is set before its marks: its weight and its colour.
#[derive(Clone)]
struct Base {
    weight: FontWeight,
    color: Hsla,
    /// What is set in bold is set in.
    strong: Hsla,
    /// The face its words are set in: the serif for her prose, the
    /// interface's for what is said about a step.
    face: SharedString,
}

/// How a message is set: in her prose, the serif at body size, or small
/// and quiet, as what she said along the way at a step of her work.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Prose,
    Small,
}

impl Setting {
    fn size(self) -> (f32, f32) {
        match self {
            Setting::Prose => (PROSE, PROSE_LINE),
            Setting::Small => (SMALL, SMALL_LINE),
        }
    }

    fn face(self, faces: &Faces) -> SharedString {
        match self {
            Setting::Prose => faces.serif.clone(),
            Setting::Small => faces.ui.clone(),
        }
    }

    fn color(self) -> Hsla {
        match self {
            Setting::Prose => theme::text(),
            Setting::Small => theme::muted(),
        }
    }
}

/// A line of text with its marks drawn: inline code in mono on a faint
/// ground, bold and italic in the face of the line.
fn styled(faces: &Faces, line: &str, base: &Base) -> StyledText {
    let parsed = inline(line);
    let face = |family: &SharedString, weight, style| gpui::Font {
        family: family.clone(),
        features: gpui::FontFeatures::default(),
        fallbacks: Some(if family == &faces.serif {
            fonts::serif_fallbacks()
        } else {
            fonts::fallbacks()
        }),
        weight,
        style,
    };
    let runs = spans(&parsed)
        .into_iter()
        .map(|(r, mark)| {
            let (font, color, background) = match mark {
                Mark::Plain => (
                    face(&base.face, base.weight, FontStyle::Normal),
                    base.color,
                    None,
                ),
                // In her prose a name in code sits on a light shade of its
                // own, in the text's colour; in the small print of a step,
                // where there are many, it is only set in mono.
                Mark::Code if base.face == faces.ui => (
                    face(&faces.mono, FontWeight::NORMAL, FontStyle::Normal),
                    base.strong,
                    None,
                ),
                Mark::Code => (
                    face(&faces.mono, FontWeight::NORMAL, FontStyle::Normal),
                    theme::text(),
                    Some(theme::bubble()),
                ),
                Mark::Bold => (
                    face(
                        &base.face,
                        strong_weight(faces, &base.face),
                        FontStyle::Normal,
                    ),
                    base.strong,
                    None,
                ),
                Mark::Italic => (
                    face(&base.face, base.weight, FontStyle::Italic),
                    base.color,
                    None,
                ),
            };
            TextRun {
                len: r.len(),
                font,
                color,
                background_color: background,
                underline: None,
                strikethrough: None,
            }
        })
        .collect();
    StyledText::new(SharedString::from(parsed.text)).with_runs(runs)
}

/// A message, drawn in her prose. `key` tells its code blocks apart from
/// every other message's, for their scrolling.
pub fn render(faces: &Faces, key: &str, text: &str) -> Div {
    render_as(faces, key, text, Setting::Prose)
}

/// A message, drawn in the setting given.
pub fn render_as(faces: &Faces, key: &str, text: &str, set: Setting) -> Div {
    let mut column = div().flex().flex_col().w_full().min_w(px(0.));
    let mut before: Option<&Block> = None;
    let blocks = parse(text);
    let (_, line) = set.size();
    // Blocks stand apart by most of a line; a list's items by a little.
    let apart = (line * 0.6).round();
    for (i, block) in blocks.iter().enumerate() {
        let gap = match (before, block) {
            (None, _) => 0.,
            (Some(Block::Item { .. }), Block::Item { .. }) => S1 + 2.,
            (Some(Block::Heading(..)), _) => S2,
            (Some(_), Block::Heading(..)) => apart + S2,
            _ => apart,
        };
        let el = block_element(faces, &format!("{key}-{i}"), block, set);
        column = column.child(el.mt(px(gap)));
        before = Some(block);
    }
    column
}

/// Body text in a setting: what a paragraph, a list item or a cell is set in.
fn body(faces: &Faces, set: Setting) -> Base {
    Base {
        weight: FontWeight::NORMAL,
        color: set.color(),
        strong: match set {
            Setting::Prose => theme::text(),
            Setting::Small => theme::text2(),
        },
        face: set.face(faces),
    }
}

/// A block of text in a setting's size.
fn sized(set: Setting) -> Div {
    let (size, line) = set.size();
    div().text_size(px(size)).line_height(px(line))
}

fn block_element(faces: &Faces, key: &str, block: &Block, set: Setting) -> Div {
    match block {
        Block::Paragraph(text) => sized(set).child(styled(faces, text, &body(faces, set))),
        Block::Heading(level, text) => {
            let (size, line) = match (set, level) {
                (Setting::Small, _) => set.size(),
                (_, 1) => H1,
                (_, 2) => H2,
                _ => H3,
            };
            let base = Base {
                weight: strong_weight(faces, &set.face(faces)),
                color: body(faces, set).strong,
                ..body(faces, set)
            };
            div()
                .text_size(px(size))
                .line_height(px(line))
                .child(styled(faces, text, &base))
        }
        Block::Item {
            marker,
            text,
            depth,
        } => {
            // The marker hangs in front, in the face of the text, so that
            // a wrapped line comes back under the words and not under it.
            let bullet = marker == "•";
            div()
                .flex()
                .pl(px(f32::from(*depth) * 22.))
                .child(
                    sized(set)
                        .w(px(if bullet { 18. } else { 24. }))
                        .flex_none()
                        .font_family(set.face(faces))
                        .when(bullet, |d| d.font_weight(FontWeight::BOLD))
                        .text_color(theme::muted())
                        .child(marker.clone()),
                )
                .child(sized(set).flex_1().min_w(px(0.)).child(styled(
                    faces,
                    text,
                    &body(faces, set),
                )))
        }
        // A quote steps in and goes a shade quieter: no bar beside it.
        Block::Quote(text) => sized(set).pl(px(S4)).child(styled(
            faces,
            text,
            &Base {
                color: theme::text2(),
                ..body(faces, set)
            },
        )),
        Block::Code { lang, text } => code(faces, key, lang, text),
        Block::Table { head, right, rows } => table(faces, head, right, rows, set),
    }
}

/// How wide a cell's text is, in columns: a Chinese or Japanese character
/// takes two.
fn width(text: &str) -> usize {
    text.chars().map(|c| if unspaced(c) { 2 } else { 1 }).sum()
}

/// Each column's share of a table's width, by the widest thing in it, so
/// that a column of short numbers stays narrow and one of words gets room.
pub fn shares(head: &[String], rows: &[Vec<String>]) -> Vec<f32> {
    let widths: Vec<usize> = (0..head.len())
        .map(|i| {
            std::iter::once(&head[i])
                .chain(rows.iter().filter_map(|r| r.get(i)))
                .map(|c| width(&inline(c).text))
                .max()
                .unwrap_or(0)
                .clamp(4, 48)
        })
        .collect();
    let total: usize = widths.iter().sum::<usize>().max(1);
    widths.iter().map(|w| *w as f32 / total as f32).collect()
}

/// A table, set the way a careful page sets one: the head in the bold of
/// the text over a rule, a hairline between rows and none around, each
/// column as wide as it needs to be in the whole width.
fn table(
    faces: &Faces,
    head: &[String],
    right: &[bool],
    rows: &[Vec<String>],
    set: Setting,
) -> Div {
    let shares = shares(head, rows);
    let row = |cells: &[String], strong: bool| {
        let base = if strong {
            Base {
                weight: strong_weight(faces, &set.face(faces)),
                color: body(faces, set).strong,
                ..body(faces, set)
            }
        } else {
            body(faces, set)
        };
        let mut line = div().flex().w_full().items_start();
        for (i, cell) in cells.iter().enumerate() {
            let last = i + 1 == cells.len();
            line = line.child(
                sized(set)
                    .w(relative(shares.get(i).copied().unwrap_or(0.)))
                    .flex_none()
                    .min_w(px(0.))
                    .py(px(S2 + 1.))
                    .when(!last, |d| d.pr(px(S4)))
                    .when(right.get(i).copied().unwrap_or(false), |d| {
                        d.flex().justify_end()
                    })
                    .child(styled(faces, cell, &base)),
            );
        }
        line
    };
    let mut column = div()
        .flex()
        .flex_col()
        .w_full()
        .child(row(head, true).border_b_1().border_color(theme::rule()));
    for (i, cells) in rows.iter().enumerate() {
        let last = i + 1 == rows.len();
        column = column
            .child(row(cells, false).when(!last, |d| d.border_b_1().border_color(theme::rule())));
    }
    column
}

/// A fenced block, drawn as a command is when opened: a box with its
/// language at the head, and its text in mono in a well under it that
/// scrolls sideways so that a long line keeps its shape. Both are hairline
/// frames on the thread's ground.
fn code(faces: &Faces, key: &str, lang: &str, text: &str) -> Div {
    let lang = if lang.is_empty() { "text" } else { lang };
    div()
        .flex()
        .flex_col()
        .gap(px(S3))
        .px(px(S3))
        .pt(px(14.))
        .pb(px(S4))
        .rounded(px(BOX_RADIUS))
        .border_1()
        .border_color(theme::border())
        .child(
            ui::text(faces, lang.to_owned())
                .px(px(10.))
                .text_size(px(14.))
                .text_color(theme::muted()),
        )
        .child(
            div()
                .id(SharedString::from(format!("{key}-code")))
                .flex()
                .rounded(px(WELL_RADIUS))
                .border_1()
                .border_color(theme::border())
                .overflow_x_scroll()
                .child(
                    div()
                        .flex_none()
                        .px(px(14.))
                        .py(px(10.))
                        .font_family(faces.mono.clone())
                        .text_size(px(MONO))
                        .line_height(px(MONO_LINE))
                        .text_color(theme::text())
                        .whitespace_nowrap()
                        .child(text.to_owned()),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(marker: &str, text: &str, depth: u8) -> Block {
        Block::Item {
            marker: marker.into(),
            text: text.into(),
            depth,
        }
    }

    #[test]
    fn a_message_splits_into_the_blocks_it_was_written_as() {
        let text = "# Done\n\nCleared the cache\nfrom two places.\n\n- one\n  - under one\n- two\n1. first\n> quoted\n```sh\ndu -sh ~\n```";
        assert_eq!(
            parse(text),
            vec![
                Block::Heading(1, "Done".into()),
                Block::Paragraph("Cleared the cache from two places.".into()),
                item("•", "one", 0),
                item("•", "under one", 1),
                item("•", "two", 0),
                item("1.", "first", 0),
                Block::Quote("quoted".into()),
                Block::Code {
                    lang: "sh".into(),
                    text: "du -sh ~".into()
                },
            ]
        );
    }

    #[test]
    fn a_table_is_its_head_its_rule_and_the_lines_of_bars_after() {
        let text = "统计好了：\n\n| crate | 行数 |\n|---|---:|\n| ghost-mind | 18,856 |\n| ghost-net |\n\n最大的三个";
        assert_eq!(
            parse(text),
            vec![
                Block::Paragraph("统计好了：".into()),
                Block::Table {
                    head: vec!["crate".into(), "行数".into()],
                    right: vec![false, true],
                    rows: vec![
                        vec!["ghost-mind".into(), "18,856".into()],
                        vec!["ghost-net".into(), String::new()],
                    ],
                },
                Block::Paragraph("最大的三个".into()),
            ]
        );
    }

    #[test]
    fn a_line_of_bars_with_no_rule_under_it_is_text() {
        assert_eq!(
            parse("| not | a table |\nreally"),
            vec![Block::Paragraph("| not | a table | really".into())]
        );
    }

    #[test]
    fn a_column_of_short_numbers_gets_less_of_a_table_than_one_of_words() {
        let head = vec!["crate".to_owned(), "行数".to_owned()];
        let rows = vec![vec![
            "ghost-protocol 和它的说明".to_owned(),
            "1,339".to_owned(),
        ]];
        let shares = shares(&head, &rows);
        assert!(shares[0] > shares[1], "{shares:?}");
        assert!((shares.iter().sum::<f32>() - 1.).abs() < 1e-4);
    }

    #[test]
    fn chinese_lines_of_a_paragraph_join_without_a_space_and_english_with_one() {
        assert_eq!(
            parse("我看了一下\n磁盘的情况\nthen ran\ndu"),
            vec![Block::Paragraph("我看了一下磁盘的情况 then ran du".into())]
        );
    }

    #[test]
    fn inline_marks_come_out_of_the_text_and_are_remembered() {
        let line = inline("run `ls` then **look**, *then* rest");
        assert_eq!(line.text, "run ls then look, then rest");
        assert_eq!(&line.text[line.code[0].clone()], "ls");
        assert_eq!(&line.text[line.bold[0].clone()], "look");
        assert_eq!(&line.text[line.italic[0].clone()], "then");
    }

    #[test]
    fn a_line_is_set_in_stretches_that_cover_all_of_it_in_order() {
        let line = inline("run `ls` then **看看**");
        let spans = spans(&line);
        let marks: Vec<(&str, Mark)> = spans
            .iter()
            .map(|(r, m)| (&line.text[r.clone()], *m))
            .collect();
        assert_eq!(
            marks,
            [
                ("run ", Mark::Plain),
                ("ls", Mark::Code),
                (" then ", Mark::Plain),
                ("看看", Mark::Bold)
            ]
        );
        let total: usize = spans.iter().map(|(r, _)| r.len()).sum();
        assert_eq!(total, line.text.len());
        assert!(super::spans(&inline("")).is_empty());
    }

    #[test]
    fn a_mark_that_is_not_closed_or_is_spaced_stays_as_written() {
        assert_eq!(inline("2 * 3 = 6").text, "2 * 3 = 6");
        assert_eq!(inline("a `b").text, "a `b");
        assert_eq!(inline("snake_case_name").text, "snake_case_name");
    }
}
