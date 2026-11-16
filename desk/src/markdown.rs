//! What the solver says, drawn the way it was written: the small part of
//! Markdown a model actually writes. Paragraphs, headings, lists (nested
//! by their indent), quotes, fenced code, and inside a line `code`,
//! **bold** and *italic* (with asterisks only). Anything else is left as
//! the text it is.
//!
//! It is set in the thread's type (`thread/style.rs`): body text with room
//! between its lines, blocks a step apart and the items of a list closer,
//! inline code in mono on a faint ground, and a code block in a well that
//! scrolls sideways rather than wrapping a command.

use std::ops::Range;

use gpui::{
    Div, FontStyle, FontWeight, Hsla, SharedString, StyledText, TextRun, div, prelude::*, px,
};

use crate::{
    fonts::{self, Faces},
    theme,
    thread::style::{H1, H2, H3, MONO, MONO_LINE, PROSE, PROSE_LINE, S1, S2, S3, WELL_RADIUS},
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
    let mut lines = text.lines();
    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(joined(paragraph)));
            paragraph.clear();
        }
    };
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(lang) = trimmed.strip_prefix("```") {
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

/// How a block's text is set before its marks: its weight and its colour.
#[derive(Clone, Copy)]
struct Base {
    weight: FontWeight,
    color: Hsla,
}

/// A line of text with its marks drawn: inline code in mono on a faint
/// ground, bold and italic in the face of the line.
fn styled(faces: &Faces, line: &str, base: Base) -> StyledText {
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
                    face(&faces.serif, base.weight, FontStyle::Normal),
                    base.color,
                    None,
                ),
                Mark::Code => (
                    face(&faces.mono, FontWeight::NORMAL, FontStyle::Normal),
                    theme::amber2(),
                    Some(theme::panel2()),
                ),
                Mark::Bold => (
                    face(&faces.serif, FontWeight::SEMIBOLD, FontStyle::Normal),
                    theme::text(),
                    None,
                ),
                Mark::Italic => (
                    face(&faces.serif, base.weight, FontStyle::Italic),
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

/// A message, drawn. `key` tells its code blocks apart from every other
/// message's, for their scrolling.
pub fn render(faces: &Faces, key: &str, text: &str) -> Div {
    let mut column = div().flex().flex_col().min_w(px(0.));
    let mut before: Option<&Block> = None;
    let blocks = parse(text);
    for (i, block) in blocks.iter().enumerate() {
        let gap = match (before, block) {
            (None, _) => 0.,
            (Some(Block::Item { .. }), Block::Item { .. }) => S1 + 2.,
            (Some(Block::Heading(..)), _) => S2,
            (Some(_), Block::Heading(..)) => S3 + S2,
            _ => S3,
        };
        column = column.child(block_element(faces, &format!("{key}-{i}"), block).mt(px(gap)));
        before = Some(block);
    }
    column
}

/// Body text: what a paragraph or a list item is set in.
fn body() -> Base {
    Base {
        weight: FontWeight::NORMAL,
        color: theme::text(),
    }
}

fn block_element(faces: &Faces, key: &str, block: &Block) -> Div {
    match block {
        Block::Paragraph(text) => div()
            .text_size(px(PROSE))
            .line_height(px(PROSE_LINE))
            .child(styled(faces, text, body())),
        Block::Heading(level, text) => {
            let (size, line) = match level {
                1 => H1,
                2 => H2,
                _ => H3,
            };
            let base = Base {
                weight: FontWeight::SEMIBOLD,
                color: theme::text(),
            };
            div()
                .text_size(px(size))
                .line_height(px(line))
                .child(styled(faces, text, base))
        }
        Block::Item {
            marker,
            text,
            depth,
        } => div()
            .flex()
            .gap(px(S2))
            .pl(px(f32::from(*depth) * 20.))
            .child(
                ui::mono(faces, marker.clone())
                    .min_w(px(14.))
                    .flex_none()
                    .text_right()
                    .text_size(px(MONO))
                    .text_color(theme::muted())
                    .line_height(px(PROSE_LINE)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(PROSE))
                    .line_height(px(PROSE_LINE))
                    .child(styled(faces, text, body())),
            ),
        Block::Quote(text) => div()
            .pl(px(S3))
            .border_l_2()
            .border_color(theme::border2())
            .text_size(px(PROSE))
            .line_height(px(PROSE_LINE))
            .child(styled(
                faces,
                text,
                Base {
                    weight: FontWeight::NORMAL,
                    color: theme::text2(),
                },
            )),
        Block::Code { lang, text } => code(faces, key, lang, text),
    }
}

/// A fenced block: its language, small, over its text in mono, in a well
/// that scrolls sideways so that a long command keeps its lines.
fn code(faces: &Faces, key: &str, lang: &str, text: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(WELL_RADIUS))
        .bg(theme::well())
        .border_1()
        .border_color(theme::border())
        .overflow_hidden()
        .when(!lang.is_empty(), |d| {
            d.child(
                ui::mono(faces, lang.to_owned())
                    .text_size(px(10.5))
                    .text_color(theme::faint())
                    .px(px(S3))
                    .pt(px(S2)),
            )
        })
        .child(
            div()
                .id(SharedString::from(format!("{key}-code")))
                .flex()
                .overflow_x_scroll()
                .child(
                    div()
                        .flex_none()
                        .px(px(S3))
                        .py(px(10.))
                        .font_family(faces.mono.clone())
                        .text_size(px(MONO))
                        .line_height(px(MONO_LINE))
                        .text_color(theme::text2())
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
