//! What an agent says, drawn the way it was written: standard Markdown with
//! GitHub's tables and ~~strikes~~, read by `pulldown-cmark` and folded
//! into the few blocks drawn here. Paragraphs, headings, list items (by
//! how deep they are nested), quotes, code and tables, and inside a line
//! `code`, **bold**, *italic*, strikes and links. An address written on
//! its own is a link too.
//!
//! It is set in the desk's type (`style.rs`): body text with room
//! between its lines, blocks a step apart and the items of a list closer,
//! inline code in mono on a faint ground, and a code block in a well that
//! scrolls sideways rather than wrapping a command, as a table wider than
//! the column does rather than widening it.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    mem::take,
    ops::Range,
    rc::Rc,
};

use gpui::{
    AnyElement, Context, Div, FontStyle, FontWeight, Hsla, InteractiveText, SharedString,
    StyledText, TextRun, div, prelude::*, px,
};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::{
    desk::Desk,
    fonts::{self, Faces},
    style::{H1, H2, H3, MONO, MONO_LINE, PROSE, PROSE_LINE, S2, S4, SMALL, WELL_RADIUS},
    theme,
    thread::select,
    ui,
};

/// One block of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Block {
    Paragraph(Inline),
    Heading(u8, Inline),
    /// A list item: its marker (`•` for an unordered one, none for a
    /// second paragraph of the item), its words, and how deep it is
    /// nested, from 0.
    Item {
        marker: String,
        text: Inline,
        depth: u8,
    },
    Quote(Inline),
    Code {
        lang: String,
        text: String,
    },
    /// A table: its head, how each column is set (to the right or not),
    /// and its rows.
    Table {
        head: Vec<Inline>,
        right: Vec<bool>,
        rows: Vec<Vec<Inline>>,
    },
}

/// A line's words with their marks taken out, and how each stretch of
/// them is set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Inline {
    text: String,
    /// Each stretch of the text and how it is set, in order and leaving
    /// nothing out: what a run of text is drawn from.
    spans: Vec<(Range<usize>, Mark)>,
    /// Links: where each is in the text, and where it goes.
    links: Vec<(Range<usize>, String)>,
}

impl Inline {
    fn add(&mut self, words: &str, mark: Mark) {
        let start = self.text.len();
        self.text.push_str(words);
        match self.spans.last_mut() {
            Some((r, m)) if *m == mark && r.end == start => r.end = self.text.len(),
            _ => self.spans.push((start..self.text.len(), mark)),
        }
    }
}

/// How a stretch of a line is set. Where marks nest, the stretch takes
/// code over the others, then a link, bold, italic and a strike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Plain,
    Code,
    Bold,
    Italic,
    Strike,
    Link,
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

/// Split a message into blocks. A line begun with `• `, as models write a
/// list, is a list item as one begun with `- ` is, outside fenced code.
fn parse(text: &str) -> Vec<Block> {
    let mut fenced = false;
    let text: String = text
        .split_inclusive('\n')
        .map(|line| {
            let words = line.trim_start();
            fenced ^= words.starts_with("```") || words.starts_with("~~~");
            match words.strip_prefix("• ") {
                Some(rest) if !fenced => std::borrow::Cow::Owned(format!(
                    "{}- {rest}",
                    &line[..line.len() - words.len()]
                )),
                _ => std::borrow::Cow::Borrowed(line),
            }
        })
        .collect();
    let mut read = Reader::default();
    for event in Parser::new_ext(
        &text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    ) {
        read.event(event);
    }
    read.blocks
}

/// How many messages' blocks are kept before they are all let go: more
/// than a window shows at once.
const KEPT: usize = 256;

/// A message's text, and the blocks read from it.
type Read = (String, Rc<Vec<Block>>);

thread_local! {
    /// What each message drawn was read as, by its key.
    static READ: RefCell<HashMap<String, Read>> = RefCell::new(HashMap::new());
}

/// The blocks of the message `key`, read once for each text it has, not
/// at every frame it is drawn in: the window draws many while an answer
/// streams.
fn blocks(key: &str, text: &str) -> Rc<Vec<Block>> {
    READ.with_borrow_mut(|read| {
        if let Some((was, blocks)) = read.get(key)
            && was == text
        {
            return blocks.clone();
        }
        if read.len() >= KEPT {
            read.clear();
        }
        let blocks = Rc::new(parse(text));
        read.insert(key.to_owned(), (text.to_owned(), blocks.clone()));
        blocks
    })
}

/// What the words being read are inside of.
enum Within {
    Quote,
    /// A list, and the number its next item takes if it counts.
    List(Option<u64>),
    /// A list item, and its marker until its first words take it.
    Item(Option<String>),
}

/// Markdown's events folded into blocks as they come.
#[derive(Default)]
struct Reader {
    blocks: Vec<Block>,
    within: Vec<Within>,
    heading: Option<u8>,
    /// A block of code being read: its language and its text.
    code: Option<(String, String)>,
    /// The words being read, and whether a line ended before the next.
    line: Inline,
    broken: bool,
    strong: u8,
    emphasis: u8,
    struck: u8,
    /// A link being read: where its words began, and where it goes.
    link: Option<(usize, String)>,
    /// A table being read: how its columns are set, its head, its rows
    /// and the row being read.
    right: Vec<bool>,
    head: Vec<Inline>,
    rows: Vec<Vec<Inline>>,
    row: Vec<Inline>,
}

impl Reader {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => match &mut self.code {
                Some((_, code)) => code.push_str(&t),
                None => self.push(&t, self.mark()),
            },
            Event::Code(t) => self.push(&t, Mark::Code),
            Event::Html(t) | Event::InlineHtml(t) => {
                for piece in t.split_inclusive('\n') {
                    self.push(piece.trim_end_matches('\n'), Mark::Plain);
                    self.broken |= piece.ends_with('\n');
                }
            }
            Event::SoftBreak | Event::HardBreak => self.broken = true,
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level as u8);
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.within.push(Within::Quote);
            }
            Tag::List(first) => {
                self.flush();
                self.within.push(Within::List(first));
            }
            Tag::Item => {
                self.flush();
                let marker = match self.within.last_mut() {
                    Some(Within::List(Some(n))) => {
                        *n += 1;
                        format!("{}.", *n - 1)
                    }
                    _ => "•".to_owned(),
                };
                self.within.push(Within::Item(Some(marker)));
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::Table(columns) => {
                self.right = columns.iter().map(|a| *a == Alignment::Right).collect();
            }
            Tag::Strong => self.strong += 1,
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strikethrough => self.struck += 1,
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.link = Some((self.line.text.len(), dest_url.to_string()));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::HtmlBlock => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
            }
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item => {
                self.flush();
                self.within.pop();
            }
            TagEnd::CodeBlock => {
                if let Some((lang, mut text)) = self.code.take() {
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    self.blocks.push(Block::Code { lang, text });
                }
            }
            TagEnd::TableCell => {
                let cell = self.take_line();
                self.row.push(cell);
            }
            TagEnd::TableHead => self.head = take(&mut self.row),
            TagEnd::TableRow => self.rows.push(take(&mut self.row)),
            TagEnd::Table => self.blocks.push(Block::Table {
                head: take(&mut self.head),
                right: take(&mut self.right),
                rows: take(&mut self.rows),
            }),
            TagEnd::Strong => self.strong -= 1,
            TagEnd::Emphasis => self.emphasis -= 1,
            TagEnd::Strikethrough => self.struck -= 1,
            TagEnd::Link | TagEnd::Image => {
                if let Some((start, to)) = self.link.take()
                    && start < self.line.text.len()
                {
                    self.line.links.push((start..self.line.text.len(), to));
                }
            }
            _ => {}
        }
    }

    /// How the words read now are set.
    fn mark(&self) -> Mark {
        if self.link.is_some() {
            Mark::Link
        } else if self.strong > 0 {
            Mark::Bold
        } else if self.emphasis > 0 {
            Mark::Italic
        } else if self.struck > 0 {
            Mark::Strike
        } else {
            Mark::Plain
        }
    }

    /// Words onto the line: after a line's end, a space first, except
    /// where both sides are Chinese or Japanese, which a line break does
    /// not space.
    fn push(&mut self, words: &str, mark: Mark) {
        if words.is_empty() {
            return;
        }
        let line = &mut self.line;
        if take(&mut self.broken)
            && !line.text.is_empty()
            && !(line.text.ends_with(unspaced) && words.starts_with(unspaced))
        {
            line.add(" ", mark);
        }
        line.add(words, mark);
    }

    /// The line read so far, with each address written on its own in it
    /// made a link; a new line begins.
    fn take_line(&mut self) -> Inline {
        self.broken = false;
        let mut line = take(&mut self.line);
        let mut spans = Vec::with_capacity(line.spans.len());
        for (r, mark) in take(&mut line.spans) {
            let mut at = r.start;
            while mark == Mark::Plain
                && let Some(found) = address(&line.text[at..r.end])
            {
                let found = at + found.start..at + found.end;
                if found.start > at {
                    spans.push((at..found.start, Mark::Plain));
                }
                spans.push((found.clone(), Mark::Link));
                line.links
                    .push((found.clone(), line.text[found.clone()].to_owned()));
                at = found.end;
            }
            if at < r.end {
                spans.push((at..r.end, mark));
            }
        }
        line.spans = spans;
        line
    }

    /// The line read so far as a block of its own, by what it is inside.
    fn flush(&mut self) {
        let line = self.take_line();
        if line.text.is_empty() {
            return;
        }
        let lists = self
            .within
            .iter()
            .filter(|w| matches!(w, Within::List(_)))
            .count();
        let depth = u8::try_from(lists.saturating_sub(1).min(3)).unwrap_or(3);
        let block = match (self.heading, self.within.last_mut()) {
            (Some(level), _) => Block::Heading(level, line),
            (None, Some(Within::Item(marker))) => Block::Item {
                marker: marker.take().unwrap_or_default(),
                text: line,
                depth,
            },
            (None, Some(Within::Quote)) => Block::Quote(line),
            _ => Block::Paragraph(line),
        };
        self.blocks.push(block);
    }
}

/// Where the first address written on its own in `s` is: up to a space or
/// a mark of Chinese punctuation, and without a full stop or comma it ends
/// a sentence with.
fn address(s: &str) -> Option<Range<usize>> {
    let start = s
        .find("https://")
        .into_iter()
        .chain(s.find("http://"))
        .min()?;
    let rest = &s[start..];
    let end = rest
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || unspaced(*c) || "，。）、；：》」』\"'<>`".contains(*c))
        .map_or(rest.len(), |(i, _)| i);
    let address = rest[..end].trim_end_matches(['.', ',', ')', ';', ':', '!', '?']);
    (address.len() > "https://".len()).then(|| start..start + address.len())
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
    /// The face its words are set in: the serif for an agent's prose, the
    /// interface's for what is said about a step.
    face: SharedString,
}

/// A line of text with its marks drawn: inline code in mono on a faint
/// ground, bold and italic in the face of the line.
fn styled(faces: &Faces, id: &str, line: &Inline, base: &Base) -> StyledText {
    styled_from(faces, id, &padded(line), base)
}

/// `text`, which draws `words`, drawn so that they can be chosen with the
/// mouse, as the paragraph `id`.
fn choosable(id: &str, text: StyledText, words: &str) -> AnyElement {
    let layout = text.layout().clone();
    select::Selectable::new(
        id.to_owned(),
        layout,
        SharedString::from(words.to_owned()),
        text,
    )
    .into_any_element()
}

/// A line with its marks drawn, its words choosable, as the paragraph `id`.
fn choosable_line(faces: &Faces, id: &str, line: &Inline, base: &Base) -> AnyElement {
    let parsed = padded(line);
    let text = styled_from(faces, id, &parsed, base);
    choosable(id, text, &parsed.text)
}

/// What stands either side of inline code, inside its shade: a thin space
/// in the line's own face, for the little room the design gives a name in
/// code before its shade ends. A run of text can be shaded but not padded.
const PAD: char = '\u{2009}';

/// `line` with a thin space put either side of each stretch of code, and
/// every stretch after it moved along to match.
fn padded(line: &Inline) -> Inline {
    let w = PAD.len_utf8();
    let code = |m: &Mark| *m == Mark::Code;
    // A boundary moves by two pads for each stretch of code that ends
    // before it: a stretch of code takes its own two.
    let shift = |at: usize| {
        2 * w
            * line
                .spans
                .iter()
                .filter(|(r, m)| code(m) && r.end <= at)
                .count()
    };
    let moved = |r: &Range<usize>| (r.start + shift(r.start))..(r.end + shift(r.end));
    let mut text = String::with_capacity(line.text.len() + 2 * w * line.spans.len());
    for (r, m) in &line.spans {
        let words = &line.text[r.clone()];
        if code(m) {
            text.push(PAD);
            text.push_str(words);
            text.push(PAD);
        } else {
            text.push_str(words);
        }
    }
    Inline {
        text,
        spans: line.spans.iter().map(|(r, m)| (moved(r), *m)).collect(),
        links: line
            .links
            .iter()
            .map(|(r, a)| (moved(r), a.clone()))
            .collect(),
    }
}

/// A line whose links open when clicked: an address in the browser, a
/// file (a link to one, or code that names one) in your editor; any other
/// link does nothing. A line with neither is plain styled text.
fn linked(
    faces: &Faces,
    id: String,
    line: &Inline,
    base: &Base,
    desk: &gpui::WeakEntity<Desk>,
) -> AnyElement {
    let parsed = padded(line);
    let text = styled_from(faces, &id, &parsed, base);
    let mut ranges: Vec<Range<usize>> = parsed.links.iter().map(|(r, _)| r.clone()).collect();
    let mut addresses: Vec<String> = parsed.links.iter().map(|(_, a)| a.clone()).collect();
    for (r, _) in parsed.spans.iter().filter(|(_, m)| *m == Mark::Code) {
        let named = parsed.text[r.clone()].trim_matches(PAD);
        if path_like(named) {
            ranges.push(r.clone());
            addresses.push(named.to_owned());
        }
    }
    if ranges.is_empty() {
        return choosable(&id, text, &parsed.text);
    }
    let desk = desk.clone();
    let (layout, words) = (
        text.layout().clone(),
        SharedString::from(parsed.text.clone()),
    );
    let linked = InteractiveText::new(SharedString::from(id.clone()), text).on_click(
        ranges,
        move |i, _, cx| {
            let Some(address) = addresses.get(i) else {
                return;
            };
            if web(address) {
                cx.open_url(address);
            } else if path_like(address) {
                let _ = desk.update(cx, |d, cx| d.open_named(address, cx));
            }
        },
    );
    select::Selectable::new(id, layout, words, linked).into_any_element()
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

fn styled_from(faces: &Faces, id: &str, parsed: &Inline, base: &Base) -> StyledText {
    let face = |family: &SharedString, weight, style| gpui::Font {
        family: family.clone(),
        features: gpui::FontFeatures::default(),
        fallbacks: Some(fonts::fallbacks(weight)),
        weight,
        style,
    };
    // Inline code is three runs: its thin spaces in the line's face, its
    // words in mono between them, all on the one shade.
    let pieces = parsed.spans.iter().flat_map(|(r, mark)| {
        let (r, mark) = (r.clone(), *mark);
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
                // colour, wherever it is written: in prose and in a thought
                // alike.
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
                Mark::Strike => (
                    face(&base.face, base.weight, FontStyle::Normal),
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
                strikethrough: (mark == Mark::Strike).then(|| gpui::StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(base.color),
                }),
            }
        })
        .collect();
    let runs = select::tinted(id, parsed.text.len(), runs);
    StyledText::new(SharedString::from(parsed.text.clone())).with_runs(runs)
}

/// A message, drawn as prose. `key` tells its code blocks and tables
/// apart from every other message's, for their scrolling and for what of
/// them is opened, which `expanded` holds.
pub fn render(
    faces: &Faces,
    key: &str,
    text: &str,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let mut column = div().flex().flex_col().w_full().min_w(px(0.));
    let mut before: Option<&Block> = None;
    let blocks = blocks(key, text);
    // Blocks stand apart by 10, a list's items by 4; a heading has 24
    // above it (20 for the third level) and 14 under it.
    for (i, block) in blocks.iter().enumerate() {
        let gap = match (before, block) {
            (None, _) => 0.,
            (Some(Block::Item { .. }), Block::Item { .. }) => ITEM_GAP,
            (Some(_), Block::Heading(level, _)) if *level <= 2 => 24.,
            (Some(_), Block::Heading(..)) => 20.,
            _ => BLOCK_GAP,
        };
        let el = block_element(faces, &format!("{key}-{i}"), block, expanded, cx);
        column = column.child(el.mt(px(gap)));
        before = Some(block);
    }
    column
}

/// The space between blocks of a message, and between the items of a list:
/// half a line, and a fifth of one, as Claude's app has them (63 px from a
/// paragraph's last line to the next one's, 49 between items, 42 a line).
const BLOCK_GAP: f32 = 10.;
const ITEM_GAP: f32 = 4.;
/// How far a list's words stand in from its marker's edge, at each level.
const LIST_INDENT: f32 = 22.;

/// Body text: what a paragraph, a list item or a cell is set in, the
/// serif, as the design sets an agent's words.
fn body(faces: &Faces) -> Base {
    Base {
        weight: FontWeight::NORMAL,
        color: theme::text(),
        strong: theme::text(),
        face: faces.serif.clone(),
    }
}

/// A block of text in prose's size: 15 on 23.
fn sized() -> Div {
    div().text_size(px(PROSE)).line_height(px(PROSE_LINE))
}

fn block_element(
    faces: &Faces,
    key: &str,
    block: &Block,
    expanded: &HashSet<String>,
    cx: &mut Context<Desk>,
) -> Div {
    let desk = cx.entity().downgrade();
    match block {
        Block::Paragraph(text) => {
            sized().child(linked(faces, key.to_owned(), text, &body(faces), &desk))
        }
        // A heading is in the interface's face, semibold, as the design
        // sets "What changes" over an agent's prose.
        Block::Heading(level, text) => {
            let (size, line) = match level {
                1 => H1,
                2 => H2,
                _ => H3,
            };
            let base = Base {
                weight: FontWeight::SEMIBOLD,
                color: body(faces).strong,
                face: faces.ui.clone(),
                ..body(faces)
            };
            div()
                .text_size(px(size))
                .line_height(px(line))
                .child(choosable_line(faces, key, text, &base))
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
                    sized()
                        .w(px(LIST_INDENT))
                        .flex_none()
                        .font_family(faces.serif.clone())
                        .text_color(theme::muted())
                        .child(marker.clone()),
                )
                .child(sized().flex_1().min_w(px(0.)).child(linked(
                    faces,
                    key.to_owned(),
                    text,
                    &body(faces),
                    &desk,
                )))
        }
        // A quote steps in and goes a shade quieter: no bar beside it.
        Block::Quote(text) => sized().pl(px(S4)).child(choosable_line(
            faces,
            key,
            text,
            &Base {
                color: theme::text2(),
                ..body(faces)
            },
        )),
        Block::Code { lang, text } => code(
            faces,
            key,
            lang,
            text,
            expanded.contains(&format!("{key}:wrap")),
            cx,
        ),
        Block::Table { head, right, rows } => table(
            faces,
            key,
            &Table { head, right, rows },
            expanded.contains(&format!("{key}:rows")),
            cx,
        ),
    }
}

/// How many rows of a table show before the rest are folded under a
/// line that opens them.
const TABLE_ROWS: usize = 20;
/// The room between a table's columns, and above and below a cell's line.
const COLUMN_GAP: f32 = 20.;
const CELL_PAD: f32 = 9.;

/// The rows a table shows, and how many it folds away.
fn table_rows<T>(rows: &[T], all: bool) -> (&[T], usize) {
    let shown = if all {
        rows.len()
    } else {
        rows.len().min(TABLE_ROWS)
    };
    (&rows[..shown], rows.len() - shown)
}

/// What a table is made of, as it was parsed.
struct Table<'a> {
    head: &'a [Inline],
    right: &'a [bool],
    rows: &'a [Vec<Inline>],
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
fn table(faces: &Faces, key: &str, t: &Table, all: bool, cx: &mut Context<Desk>) -> Div {
    let line = PROSE_LINE;
    let (shown, folded) = table_rows(t.rows, all);
    let strong = Base {
        weight: strong_weight(faces, &faces.serif),
        color: body(faces).strong,
        ..body(faces)
    };
    let plain = body(faces);
    let empty = Inline::default();
    let mut grid = div().flex().flex_none();
    for (i, title) in t.head.iter().enumerate() {
        let gap = i + 1 < t.head.len();
        let right = t.right.get(i).copied().unwrap_or(false);
        let cell = || {
            sized()
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
                .child(styled(faces, "", title, &strong)),
        );
        for (r, cells) in shown.iter().enumerate() {
            let ruled = r + 1 < shown.len();
            let text = cells.get(i).unwrap_or(&empty);
            column = column.child(
                cell()
                    .h(px(line + 2. * CELL_PAD + if ruled { 1. } else { 0. }))
                    .py(px(CELL_PAD))
                    .when(ruled, |d| d.border_b_1().border_color(theme::panel()))
                    .child(styled(faces, "", text, &plain)),
            );
        }
        grid = grid.child(column);
    }
    let toggle = format!("{key}:rows");
    let group = SharedString::from(format!("{key}-table-group"));
    let (as_md, as_csv) = (table_text(t, false), table_text(t, true));
    let copy = |what: &str, label: &'static str, text: String| {
        crate::ui::button(
            faces,
            SharedString::from(format!("{key}-{what}")),
            label,
            crate::ui::Weight::Quiet,
        )
        .h(px(22.))
        .on_click(move |_, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        })
    };
    div()
        .relative()
        .group(group.clone())
        .flex()
        .flex_col()
        .gap(px(S2))
        .w_full()
        .min_w(px(0.))
        .child(
            div()
                .absolute()
                .top(px(-26.))
                .right(px(0.))
                .flex()
                .gap(px(2.))
                .invisible()
                .group_hover(group, gpui::Styled::visible)
                .child(copy("md", "Copy as Markdown", as_md))
                .child(copy("csv", "Copy as CSV", as_csv)),
        )
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

/// A table's words as Markdown, or as CSV (cells quoted where they need
/// it).
fn table_text(t: &Table, csv: bool) -> String {
    let row = |cells: &[Inline]| {
        let cells: Vec<&str> = cells.iter().map(|c| c.text.as_str()).collect();
        if csv {
            cells
                .iter()
                .map(|c| {
                    if c.contains([',', '"', '\n']) {
                        format!("\"{}\"", c.replace('"', "\"\""))
                    } else {
                        (*c).to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        } else {
            format!("| {} |", cells.join(" | "))
        }
    };
    let mut out = vec![row(t.head)];
    if !csv {
        out.push(format!("|{}|", vec!["---"; t.head.len()].join("|")));
    }
    out.extend(t.rows.iter().map(|r| row(r)));
    out.join("\n")
}

/// A fenced block: its text in mono, coloured by its language, in a
/// hairline well that scrolls sideways so that a long line keeps its shape,
/// or wraps when you ask. From its corner, on hover: wrap, run it in a
/// terminal of yours (a shell's), copy.
fn code(
    faces: &Faces,
    key: &str,
    lang: &str,
    text: &str,
    wrap: bool,
    cx: &mut Context<Desk>,
) -> Div {
    let group = SharedString::from(format!("{key}-code-group"));
    let copy = text.to_owned();
    let shell = matches!(lang, "bash" | "sh" | "zsh" | "shell" | "console" | "fish");
    let command: String = text
        .lines()
        .map(|l| l.strip_prefix("$ ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n");
    let toggle = format!("{key}:wrap");
    let button = |what: &str, icon: &str, tip: &'static str| {
        crate::ui::icon_button(SharedString::from(format!("{key}-{what}")), icon, tip)
            .size(px(24.))
            .bg(theme::panel())
    };
    div()
        .w_full()
        .relative()
        .group(group.clone())
        .child(
            div()
                .absolute()
                .top(px(6.))
                .right(px(6.))
                .flex()
                .gap(px(2.))
                .invisible()
                .group_hover(group, gpui::Styled::visible)
                .child(
                    button(
                        "wrap",
                        "list",
                        if wrap {
                            "Keep lines whole"
                        } else {
                            "Wrap lines"
                        },
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(&toggle, cx))),
                )
                .when(shell, |d| {
                    d.child(
                        button("run", "play", "Run it in a terminal of yours").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open_terminal(Some(&command), window, cx);
                            }),
                        ),
                    )
                })
                .child(button("copy", "copy", "Copy").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                })),
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
                        .when(!wrap, |d| d.flex_none().whitespace_nowrap())
                        .when(wrap, |d| d.flex_1().min_w(px(0.)))
                        .px(px(14.))
                        .py(px(10.))
                        .font_family(faces.mono.clone())
                        .text_size(px(MONO))
                        .line_height(px(MONO_LINE))
                        .text_color(theme::text2())
                        .child(choosable(
                            &format!("{key}-code"),
                            crate::syntax::styled(faces, &format!("{key}-code"), text, lang),
                            text,
                        )),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words with no marks.
    fn plain(text: &str) -> Inline {
        let mut line = Inline::default();
        line.add(text, Mark::Plain);
        line
    }

    /// The words of a message of one paragraph.
    fn inline(text: &str) -> Inline {
        match parse(text).pop() {
            Some(Block::Paragraph(line)) => line,
            None => Inline::default(),
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    /// The words of a line set with `mark`.
    fn marked(line: &Inline, mark: Mark) -> Vec<&str> {
        line.spans
            .iter()
            .filter(|(_, m)| *m == mark)
            .map(|(r, _)| &line.text[r.clone()])
            .collect()
    }

    #[test]
    fn a_table_copies_as_markdown_or_csv_and_code_naming_a_file_is_a_path() {
        let head = vec![plain("a"), plain("b")];
        let rows = vec![vec![plain("1"), plain("x, y")]];
        let t = Table {
            head: &head,
            right: &[],
            rows: &rows,
        };
        assert_eq!(table_text(&t, false), "| a | b |\n|---|---|\n| 1 | x, y |");
        assert_eq!(table_text(&t, true), "a,b\n1,\"x, y\"");
        assert!(path_like("src/upload/s3.rs"));
        assert!(path_like("main.rs:12"));
        assert!(!path_like("Uploader::send"));
        assert!(!path_like("let x = 1"));
        assert!(!path_like("1.5"));
    }

    fn item(marker: &str, text: &str, depth: u8) -> Block {
        Block::Item {
            marker: marker.into(),
            text: plain(text),
            depth,
        }
    }

    #[test]
    fn a_line_begun_with_a_bullet_is_a_list_item_but_not_in_code() {
        assert_eq!(
            parse("Found:\n\n• one\n  • under one\n```\n• kept\n```"),
            vec![
                Block::Paragraph(plain("Found:")),
                item("•", "one", 0),
                item("•", "under one", 1),
                Block::Code {
                    lang: String::new(),
                    text: "• kept".into(),
                },
            ]
        );
    }

    #[test]
    fn a_message_splits_into_the_blocks_it_was_written_as() {
        let text = "# Done\n\nCleared the cache\nfrom two places.\n\n- one\n  - under one\n- two\n1. first\n> quoted\n```sh\ndu -sh ~\n```";
        assert_eq!(
            parse(text),
            vec![
                Block::Heading(1, plain("Done")),
                Block::Paragraph(plain("Cleared the cache from two places.")),
                item("•", "one", 0),
                item("•", "under one", 1),
                item("•", "two", 0),
                item("1.", "first", 0),
                Block::Quote(plain("quoted")),
                Block::Code {
                    lang: "sh".into(),
                    text: "du -sh ~".into()
                },
            ]
        );
    }

    #[test]
    fn a_list_counts_from_its_first_number_and_an_item_keeps_its_second_paragraph() {
        assert_eq!(
            parse("3. three\n4. four\n\n   more of four"),
            vec![
                item("3.", "three", 0),
                item("4.", "four", 0),
                item("", "more of four", 0),
            ]
        );
    }

    #[test]
    fn a_table_is_its_head_its_rule_and_the_lines_of_bars_after() {
        let text = "统计好了：\n\n| crate | 行数 |\n|---|---:|\n| ghost-mind | 18,856 |\n| ghost-net |\n\n最大的三个";
        assert_eq!(
            parse(text),
            vec![
                Block::Paragraph(plain("统计好了：")),
                Block::Table {
                    head: vec![plain("crate"), plain("行数")],
                    right: vec![false, true],
                    rows: vec![
                        vec![plain("ghost-mind"), plain("18,856")],
                        vec![plain("ghost-net"), Inline::default()],
                    ],
                },
                Block::Paragraph(plain("最大的三个")),
            ]
        );
    }

    #[test]
    fn a_line_of_bars_with_no_rule_under_it_is_text() {
        assert_eq!(
            parse("| not | a table |\nreally"),
            vec![Block::Paragraph(plain("| not | a table | really"))]
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
            vec![Block::Paragraph(plain("我看了一下磁盘的情况 then ran du"))]
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
        assert_eq!(
            marked(&line, Mark::Link),
            ["PR 147", "https://example.com/x"]
        );
        assert!(inline("[not](a link)").links.is_empty());
    }

    #[test]
    fn inline_marks_come_out_of_the_text_and_are_remembered() {
        let line = inline("run `ls` then **look**, *then* _too_ ~~gone~~ rest");
        assert_eq!(line.text, "run ls then look, then too gone rest");
        assert_eq!(marked(&line, Mark::Code), ["ls"]);
        assert_eq!(marked(&line, Mark::Bold), ["look"]);
        assert_eq!(marked(&line, Mark::Italic), ["then", "too"]);
        assert_eq!(marked(&line, Mark::Strike), ["gone"]);
    }

    #[test]
    fn a_mark_inside_another_takes_its_own_stretch() {
        let line = inline("**see `a.rs` now**");
        assert_eq!(marked(&line, Mark::Bold), ["see ", " now"]);
        assert_eq!(marked(&line, Mark::Code), ["a.rs"]);
    }

    #[test]
    fn a_line_is_set_in_stretches_that_cover_all_of_it_in_order() {
        let line = inline("run `ls` then **看看**");
        let marks: Vec<(&str, Mark)> = line
            .spans
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
        let total: usize = line.spans.iter().map(|(r, _)| r.len()).sum();
        assert_eq!(total, line.text.len());
        assert!(inline("").spans.is_empty());
    }

    #[test]
    fn inline_code_is_padded_inside_its_shade_and_what_follows_moves_along() {
        let line = padded(&inline("a `ls` then **b** [c](https://x.y)"));
        assert_eq!(marked(&line, Mark::Code), ["\u{2009}ls\u{2009}"]);
        assert_eq!(marked(&line, Mark::Bold), ["b"]);
        assert_eq!(&line.text[line.links[0].0.clone()], "c");
    }

    #[test]
    fn a_mark_that_is_not_closed_or_is_spaced_stays_as_written() {
        assert_eq!(inline("2 * 3 = 6").text, "2 * 3 = 6");
        assert_eq!(inline("a `b").text, "a `b");
        assert_eq!(inline("snake_case_name").text, "snake_case_name");
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

    #[test]
    fn a_message_is_read_again_only_when_its_text_changes() {
        let first = blocks("s-1", "Reading the");
        assert!(Rc::ptr_eq(&first, &blocks("s-1", "Reading the")));
        let grown = blocks("s-1", "Reading the code");
        assert!(!Rc::ptr_eq(&first, &grown));
        assert_eq!(*grown, parse("Reading the code"));
        assert!(!Rc::ptr_eq(&grown, &blocks("s-2", "Reading the code")));
    }
}
