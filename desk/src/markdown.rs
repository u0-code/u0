//! What the solver says, drawn the way it was written: the small part of
//! Markdown a model actually writes. Paragraphs, headings, lists (nested
//! by their indent), quotes, fenced code, tables, and inside a line
//! `code`, **bold** and *italic* (with asterisks only). Anything else is
//! left as the text it is.
//!
//! It is set in the thread's type (`thread/style.rs`): body text with room
//! between its lines, blocks a step apart and the items of a list closer,
//! inline code in mono on a faint ground, and a code block in a well that
//! scrolls sideways rather than wrapping a command, as a table wider than
//! the column does rather than widening it.

use std::{collections::HashSet, ops::Range};

use gpui::{
    AnyElement, Context, Div, FontStyle, FontWeight, Hsla, InteractiveText, SharedString,
    StyledText, TextRun, div, prelude::*, px,
};

use crate::{
    desk::Desk,
    fonts::{self, Faces},
    theme,
    thread::style::{
        BODY_LINE, BOX_RADIUS, H1, H2, H3, MONO, MONO_LINE, PROSE, PROSE_LINE, S2, S3, S4, SMALL,
        WELL_RADIUS,
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
    /// Links: where each is in the text, and where it goes.
    pub links: Vec<(Range<usize>, String)>,
}

/// Take `code`, **bold** and *italic* out of a line. An unclosed mark is
/// left as the character it is.
pub fn inline(source: &str) -> Inline {
    let mut out = Inline {
        text: String::with_capacity(source.len()),
        code: Vec::new(),
        bold: Vec::new(),
        italic: Vec::new(),
        links: Vec::new(),
    };
    let mut rest = source;
    while let Some(c) = rest.chars().next() {
        // A link written `[words](address)`, or an address on its own.
        if let Some((words, address, used)) = link(rest) {
            let start = out.text.len();
            out.text.push_str(words);
            out.links.push((start..out.text.len(), address.to_owned()));
            rest = &rest[used..];
            continue;
        }
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

/// A link at the start of `s`: its words, its address and how many bytes
/// the whole took. `[words](https://…)` shows its words; an address on its
/// own shows itself, up to a space or a mark of Chinese punctuation, and
/// without a full stop or comma it ends a sentence with.
fn link(s: &str) -> Option<(&str, &str, usize)> {
    if let Some(after) = s.strip_prefix('[') {
        let close = after.find("](")?;
        let words = &after[..close];
        let rest = &after[close + 2..];
        let end = rest.find(')')?;
        let address = &rest[..end];
        let web = address.starts_with("http://") || address.starts_with("https://");
        return (web && !words.is_empty() && !words.contains('[')).then_some((
            words,
            address,
            1 + close + 2 + end + 1,
        ));
    }
    if !(s.starts_with("https://") || s.starts_with("http://")) {
        return None;
    }
    let end = s
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || unspaced(*c) || "，。）、；：》」』\"'<>`".contains(*c))
        .map_or(s.len(), |(i, _)| i);
    let address = s[..end].trim_end_matches(['.', ',', ')', ';', ':', '!', '?']);
    (address.len() > "https://".len()).then_some((address, address, address.len()))
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
    Link,
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
        .chain(line.links.iter().map(|(r, _)| (r.clone(), Mark::Link)))
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

/// The weight bold is set in: a semibold, as the design sets it.
fn strong_weight(_: &Faces, _: &SharedString) -> FontWeight {
    FontWeight::SEMIBOLD
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
            Setting::Small => (SMALL + 1., BODY_LINE),
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
            Setting::Small => theme::text2(),
        }
    }
}

/// A line of text with its marks drawn: inline code in mono on a faint
/// ground, bold and italic in the face of the line.
fn styled(faces: &Faces, line: &str, base: &Base) -> StyledText {
    styled_from(faces, &padded(inline(line)), base)
}

/// What stands either side of inline code, inside its shade: a thin space
/// in the line's own face, for the little room the design gives a name in
/// code before its shade ends. A run of text can be shaded but not padded.
const PAD: char = '\u{2009}';

/// `line` with a thin space put either side of each stretch of code, and
/// every stretch after it moved along to match.
fn padded(line: Inline) -> Inline {
    if line.code.is_empty() {
        return line;
    }
    let w = PAD.len_utf8();
    // Bold, italic and links never overlap code, so a boundary of theirs
    // moves by two pads for each stretch of code that ends before it.
    let shift = |at: usize| 2 * w * line.code.iter().filter(|c| c.end <= at).count();
    let moved = |r: &Range<usize>| (r.start + shift(r.start))..(r.end + shift(r.end));
    let mut text = String::with_capacity(line.text.len() + line.code.len() * 2 * w);
    let mut at = 0;
    let mut code = Vec::with_capacity(line.code.len());
    for c in &line.code {
        text.push_str(&line.text[at..c.start]);
        let start = text.len();
        text.push(PAD);
        text.push_str(&line.text[c.clone()]);
        text.push(PAD);
        code.push(start..text.len());
        at = c.end;
    }
    text.push_str(&line.text[at..]);
    Inline {
        text,
        code,
        bold: line.bold.iter().map(moved).collect(),
        italic: line.italic.iter().map(moved).collect(),
        links: line
            .links
            .iter()
            .map(|(r, a)| (moved(r), a.clone()))
            .collect(),
    }
}

/// A line whose links open in the browser when clicked; one without links
/// is plain styled text.
fn linked(faces: &Faces, id: String, line: &str, base: &Base) -> AnyElement {
    let parsed = padded(inline(line));
    let text = styled_from(faces, &parsed, base);
    if parsed.links.is_empty() {
        return text.into_any_element();
    }
    let ranges: Vec<Range<usize>> = parsed.links.iter().map(|(r, _)| r.clone()).collect();
    let addresses: Vec<String> = parsed.links.into_iter().map(|(_, a)| a).collect();
    InteractiveText::new(SharedString::from(id), text)
        .on_click(ranges, move |i, _, cx| {
            if let Some(address) = addresses.get(i) {
                cx.open_url(address);
            }
        })
        .into_any_element()
}

fn styled_from(faces: &Faces, parsed: &Inline, base: &Base) -> StyledText {
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
    // Inline code is three runs: its thin spaces in the line's face, its
    // words in mono between them, all on the one shade.
    let pieces = spans(parsed).into_iter().flat_map(|(r, mark)| {
        let w = PAD.len_utf8();
        if mark == Mark::Code && r.len() > 2 * w {
            vec![
                (r.start..r.start + w, Mark::Plain, true),
                (r.start + w..r.end - w, Mark::Code, true),
                (r.end - w..r.end, Mark::Plain, true),
            ]
        } else {
            vec![(r, mark, false)]
        }
    });
    let runs = pieces
        .map(|(r, mark, shaded)| {
            let (font, color, background) = match mark {
                Mark::Plain => (
                    face(&base.face, base.weight, FontStyle::Normal),
                    base.color,
                    None,
                ),
                // A name in code sits on the bubble's shade, in the text's
                // colour, wherever it is written: in her prose and in what
                // she says between steps alike.
                Mark::Code => (
                    face(&faces.mono, FontWeight::NORMAL, FontStyle::Normal),
                    theme::code_text(),
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
                Mark::Link => (
                    face(&base.face, base.weight, FontStyle::Normal),
                    theme::link(),
                    None,
                ),
            };
            TextRun {
                len: r.len(),
                font,
                color,
                background_color: if shaded {
                    Some(theme::bubble())
                } else {
                    background
                },
                // A link is underlined a shade darker than its words.
                underline: (mark == Mark::Link).then(|| gpui::UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme::link_line()),
                    wavy: false,
                }),
                strikethrough: None,
            }
        })
        .collect();
    StyledText::new(SharedString::from(parsed.text.clone())).with_runs(runs)
}

/// A message, drawn in her prose. `key` tells its code blocks and tables
/// apart from every other message's, for their scrolling and for what of
/// them is opened, which `expanded` holds.
pub fn render(
    faces: &Faces,
    key: &str,
    text: &str,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    render_as(faces, key, text, Setting::Prose, expanded, cx)
}

/// A message, drawn in the setting given.
pub fn render_as(
    faces: &Faces,
    key: &str,
    text: &str,
    set: Setting,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let mut column = div().flex().flex_col().w_full().min_w(px(0.));
    let mut before: Option<&Block> = None;
    let blocks = parse(text);
    // Blocks stand apart by 14, a list's items by 6; a heading has 24
    // above it (20 for the third level) and 14 under it.
    for (i, block) in blocks.iter().enumerate() {
        let gap = match (before, block) {
            (None, _) => 0.,
            (Some(Block::Item { .. }), Block::Item { .. }) => ITEM_GAP,
            (Some(_), Block::Heading(level, _)) if *level <= 2 => 24.,
            (Some(_), Block::Heading(..)) => 20.,
            _ => BLOCK_GAP,
        };
        let el = block_element(faces, &format!("{key}-{i}"), block, set, expanded, cx);
        column = column.child(el.mt(px(gap)));
        before = Some(block);
    }
    column
}

/// The space between blocks of a message, and between the items of a list.
const BLOCK_GAP: f32 = 14.;
const ITEM_GAP: f32 = 6.;
/// How far a list's words stand in from its marker's edge, at each level.
const LIST_INDENT: f32 = 22.;

/// Body text in a setting: what a paragraph, a list item or a cell is set in.
fn body(faces: &Faces, set: Setting) -> Base {
    Base {
        weight: FontWeight::NORMAL,
        color: set.color(),
        strong: match set {
            Setting::Prose | Setting::Small => theme::text(),
        },
        face: set.face(faces),
    }
}

/// A block of text in a setting's size.
fn sized(set: Setting) -> Div {
    let (size, line) = set.size();
    div().text_size(px(size)).line_height(px(line))
}

fn block_element(
    faces: &Faces,
    key: &str,
    block: &Block,
    set: Setting,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    match block {
        Block::Paragraph(text) => {
            sized(set).child(linked(faces, key.to_owned(), text, &body(faces, set)))
        }
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
            // The marker sits in the indent, grey, so the words line up
            // with each other whatever the marker is.
            div()
                .flex()
                .pl(px(f32::from(*depth) * LIST_INDENT))
                .child(
                    sized(set)
                        .w(px(LIST_INDENT))
                        .flex_none()
                        .font_family(set.face(faces))
                        .text_color(theme::muted())
                        .child(marker.clone()),
                )
                .child(sized(set).flex_1().min_w(px(0.)).child(linked(
                    faces,
                    key.to_owned(),
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
        Block::Table { head, right, rows } => table(
            faces,
            key,
            &Table { head, right, rows },
            set,
            expanded.contains(&format!("{key}:rows")),
            cx,
        ),
    }
}

/// How wide a cell's text is, in columns: a Chinese or Japanese character
/// takes two.
pub fn width(text: &str) -> usize {
    text.chars().map(|c| if unspaced(c) { 2 } else { 1 }).sum()
}

/// How many rows of a table show before the rest are folded under a
/// line that opens them.
const TABLE_ROWS: usize = 20;
/// The room between a table's columns, and above and below a cell's line.
const COLUMN_GAP: f32 = 20.;
const CELL_PAD: f32 = 9.;

/// The rows a table shows, and how many it folds away.
fn table_rows(rows: &[Vec<String>], all: bool) -> (&[Vec<String>], usize) {
    let shown = if all {
        rows.len()
    } else {
        rows.len().min(TABLE_ROWS)
    };
    (&rows[..shown], rows.len() - shown)
}

/// What a table is made of, as it was parsed.
struct Table<'a> {
    head: &'a [String],
    right: &'a [bool],
    rows: &'a [Vec<String>],
}

/// A table, set the way a careful page sets one: the head in the bold of
/// the text over a rule, a hairline between rows and none around, each
/// column as wide as its widest cell and no cell wrapped, so a number is
/// never broken. A table wider than the column scrolls sideways in its
/// own box; the column never widens. Past twenty rows the rest wait under
/// a line that shows them.
///
/// It is laid out by columns, each a stack of its cells, so a column takes
/// the width of its widest cell without anything being measured here;
/// every cell of a row is given the same height in points, so the rows
/// line up across the columns.
fn table(
    faces: &Faces,
    key: &str,
    t: &Table,
    set: Setting,
    all: bool,
    cx: &mut Context<Desk>,
) -> Div {
    let (_, line) = set.size();
    let (shown, folded) = table_rows(t.rows, all);
    let strong = Base {
        weight: strong_weight(faces, &set.face(faces)),
        color: body(faces, set).strong,
        ..body(faces, set)
    };
    let plain = body(faces, set);
    let mut grid = div().flex().flex_none();
    for (i, title) in t.head.iter().enumerate() {
        let gap = i + 1 < t.head.len();
        let right = t.right.get(i).copied().unwrap_or(false);
        let cell = || {
            sized(set)
                .flex()
                .flex_none()
                .whitespace_nowrap()
                .when(gap, |d| d.pr(px(COLUMN_GAP)))
                .when(right, gpui::Styled::justify_end)
        };
        // A hairline takes a point of its cell's height: a height here
        // holds the border as well as the padding.
        let mut column = div().flex().flex_col().flex_none().child(
            cell()
                .h(px(line + CELL_PAD + 1.))
                .pb(px(CELL_PAD))
                .border_b_1()
                .border_color(theme::rule())
                .child(styled(faces, title, &strong)),
        );
        for (r, cells) in shown.iter().enumerate() {
            let ruled = r + 1 < shown.len();
            let text = cells.get(i).map_or("", String::as_str);
            column = column.child(
                cell()
                    .h(px(line + 2. * CELL_PAD + if ruled { 1. } else { 0. }))
                    .py(px(CELL_PAD))
                    .when(ruled, |d| d.border_b_1().border_color(theme::panel()))
                    .child(styled(faces, text, &plain)),
            );
        }
        grid = grid.child(column);
    }
    let toggle = format!("{key}:rows");
    div()
        .flex()
        .flex_col()
        .gap(px(S2))
        .w_full()
        .min_w(px(0.))
        .child(
            ui::sideways(
                div()
                    .id(SharedString::from(format!("{key}-table")))
                    .flex()
                    .w_full(),
            )
            .child(grid),
        )
        .when(folded > 0, |d| {
            let rows = if folded == 1 { "row" } else { "rows" };
            d.child(
                div()
                    .id(SharedString::from(toggle.clone()))
                    .cursor_pointer()
                    .child(
                        ui::text(faces, format!("Show {folded} more {rows}"))
                            .text_size(px(SMALL))
                            .text_color(theme::muted())
                            .hover(|s| s.text_color(theme::text())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx))),
            )
        })
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
        .child({
            let copy = text.to_owned();
            div()
                .relative()
                .w_full()
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
                                .pl(px(14.))
                                .pr(px(44.))
                                .py(px(10.))
                                .text_size(px(MONO))
                                .line_height(px(MONO_LINE))
                                .whitespace_nowrap()
                                .child(crate::syntax::styled(faces, text, lang)),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{key}-copy")))
                        .absolute()
                        .top(px(S2))
                        .right(px(S2))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bubble()))
                        .child(ui::icon("copy", 15.).text_color(theme::text()))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                        }),
                )
        })
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
    fn a_table_past_twenty_rows_shows_twenty_until_all_are_asked_for() {
        let rows: Vec<Vec<String>> = (0..68).map(|i| vec![i.to_string()]).collect();
        let (shown, folded) = table_rows(&rows, false);
        assert_eq!((shown.len(), folded), (20, 48));
        assert_eq!(shown[19], vec!["19".to_owned()]);
        let (shown, folded) = table_rows(&rows, true);
        assert_eq!((shown.len(), folded), (68, 0));
        let few = &rows[..20];
        assert_eq!(table_rows(few, false), (few, 0));
    }

    #[test]
    fn chinese_lines_of_a_paragraph_join_without_a_space_and_english_with_one() {
        assert_eq!(
            parse("我看了一下\n磁盘的情况\nthen ran\ndu"),
            vec![Block::Paragraph("我看了一下磁盘的情况 then ran du".into())]
        );
    }

    #[test]
    fn a_link_shows_its_words_and_an_address_shows_itself() {
        let line =
            inline("见 [PR 147](https://github.com/a/b/pull/147)，或 https://example.com/x。");
        assert_eq!(line.text, "见 PR 147，或 https://example.com/x。");
        assert_eq!(line.links.len(), 2);
        assert_eq!(&line.text[line.links[0].0.clone()], "PR 147");
        assert_eq!(line.links[0].1, "https://github.com/a/b/pull/147");
        assert_eq!(line.links[1].1, "https://example.com/x");
        assert!(inline("[not](a link)").links.is_empty());
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
    fn inline_code_is_padded_inside_its_shade_and_what_follows_moves_along() {
        let line = padded(inline("a `ls` then **b** [c](https://x.y)"));
        assert_eq!(&line.text[line.code[0].clone()], "\u{2009}ls\u{2009}");
        assert_eq!(&line.text[line.bold[0].clone()], "b");
        assert_eq!(&line.text[line.links[0].0.clone()], "c");
    }

    #[test]
    fn a_mark_that_is_not_closed_or_is_spaced_stays_as_written() {
        assert_eq!(inline("2 * 3 = 6").text, "2 * 3 = 6");
        assert_eq!(inline("a `b").text, "a `b");
        assert_eq!(inline("snake_case_name").text, "snake_case_name");
    }
}
