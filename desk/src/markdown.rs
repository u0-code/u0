//! What the solver says, drawn the way it was written: the small part of
//! Markdown a model actually writes. Paragraphs, headings, lists, quotes,
//! fenced code, and inside a line `code`, **bold** and *italic* (with
//! asterisks only). Anything else is left as the text it is.

use std::ops::Range;

use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, SharedString, StyledText, div,
    prelude::*, px,
};

use crate::{fonts::Faces, theme, ui};

/// One block of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(String),
    Heading(u8, String),
    /// A list item: its marker as written (`•` for an unordered one) and
    /// its text.
    Item(String, String),
    Quote(String),
    Code {
        lang: String,
        text: String,
    },
}

/// Split a message into blocks.
pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut lines = text.lines();
    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(paragraph.join(" ")));
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
            blocks.push(Block::Item("•".to_owned(), rest.to_owned()));
        } else if let Some((n, rest)) = numbered(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Item(format!("{n}."), rest.to_owned()));
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

/// A line of text with its marks drawn.
fn styled(line: &str) -> StyledText {
    let parsed = inline(line);
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for r in parsed.code {
        highlights.push((
            r,
            HighlightStyle {
                color: Some(theme::amber2()),
                background_color: Some(theme::panel2()),
                ..HighlightStyle::default()
            },
        ));
    }
    for r in parsed.bold {
        highlights.push((
            r,
            HighlightStyle {
                font_weight: Some(FontWeight::SEMIBOLD),
                color: Some(theme::text()),
                ..HighlightStyle::default()
            },
        ));
    }
    for r in parsed.italic {
        highlights.push((
            r,
            HighlightStyle {
                font_style: Some(FontStyle::Italic),
                ..HighlightStyle::default()
            },
        ));
    }
    highlights.sort_by_key(|(r, _)| r.start);
    StyledText::new(SharedString::from(parsed.text)).with_highlights(highlights)
}

/// A message, drawn.
pub fn render(faces: &Faces, text: &str) -> Div {
    let mut column = div().flex().flex_col().gap(px(10.)).min_w(px(0.));
    for block in parse(text) {
        column = column.child(block_element(faces, &block));
    }
    column
}

fn block_element(faces: &Faces, block: &Block) -> AnyElement {
    match block {
        Block::Paragraph(text) => div()
            .text_color(theme::text())
            .line_height(px(23.))
            .child(styled(text))
            .into_any_element(),
        Block::Heading(level, text) => div()
            .pt(px(4.))
            .text_size(px(if *level <= 2 { 16. } else { 14.5 }))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text())
            .line_height(px(22.))
            .child(styled(text))
            .into_any_element(),
        Block::Item(marker, text) => div()
            .flex()
            .gap(px(10.))
            .child(
                ui::mono(faces, marker.clone())
                    .w(px(16.))
                    .flex_none()
                    .text_right()
                    .text_color(theme::muted())
                    .line_height(px(23.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_color(theme::text())
                    .line_height(px(23.))
                    .child(styled(text)),
            )
            .into_any_element(),
        Block::Quote(text) => div()
            .pl(px(12.))
            .border_l_2()
            .border_color(theme::border2())
            .text_color(theme::text2())
            .line_height(px(22.))
            .child(styled(text))
            .into_any_element(),
        Block::Code { lang, text } => div()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(theme::well())
            .border_1()
            .border_color(theme::border())
            .overflow_hidden()
            .when(!lang.is_empty(), |d| {
                d.child(
                    ui::mono(faces, lang.clone())
                        .text_size(px(10.5))
                        .text_color(theme::faint())
                        .px(px(12.))
                        .pt(px(8.)),
                )
            })
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .font_family(faces.mono.clone())
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(theme::text2())
                    .child(text.clone()),
            )
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_splits_into_the_blocks_it_was_written_as() {
        let text = "# Done\n\nCleared the cache\nfrom two places.\n\n- one\n- two\n1. first\n> quoted\n```sh\ndu -sh ~\n```";
        assert_eq!(
            parse(text),
            vec![
                Block::Heading(1, "Done".into()),
                Block::Paragraph("Cleared the cache from two places.".into()),
                Block::Item("•".into(), "one".into()),
                Block::Item("•".into(), "two".into()),
                Block::Item("1.".into(), "first".into()),
                Block::Quote("quoted".into()),
                Block::Code {
                    lang: "sh".into(),
                    text: "du -sh ~".into()
                },
            ]
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
    fn a_mark_that_is_not_closed_or_is_spaced_stays_as_written() {
        assert_eq!(inline("2 * 3 = 6").text, "2 * 3 = 6");
        assert_eq!(inline("a `b").text, "a `b");
        assert_eq!(inline("snake_case_name").text, "snake_case_name");
    }
}
