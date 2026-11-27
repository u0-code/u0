//! What a call printed, as its opened box shows it: the head and the last
//! line with how many were folded between, or on asking all of it up to a
//! limit; a line too long for the box wraps, as Claude's does; the fence
//! an agent put round it as Markdown is not part of it;
//! the four terminal colours a program is likely to mean something by are
//! kept and every other escape is taken out, and a word that says
//! something failed is red whatever it came as; and output that is not text
//! is said to be so, not drawn.

use std::ops::Range;

use gpui::{Div, FontWeight, Hsla, SharedString, StyledText, TextRun, div, prelude::*, px};

use super::kinds;
use crate::{
    fonts::Faces,
    style::{MONO, MONO_LINE, S2, meta},
    theme,
};

/// How many lines of what came back show before the rest is folded: a
/// file read shows its first twelve; anything else, if longer than eight,
/// its first two and its last.
pub fn folded_at(name: &str) -> usize {
    if name == "read" { 12 } else { 8 }
}

/// The lines a fold keeps at the head.
const HEAD: usize = 2;

/// The most lines drawn even when all are asked for. Each is laid out
/// every frame the box is on screen; past a few hundred the thread
/// stutters, and nobody reads that many in a box.
pub const MOST: usize = 400;

/// The colours a terminal's output keeps here. A program colours what
/// went wrong red, what went right green, a warning yellow and a name
/// blue; the rest of what it might colour carries less, and shown as it
/// came it would be a second palette in the thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Ink {
    #[default]
    Plain,
    Red,
    Green,
    Yellow,
    Blue,
}

impl Ink {
    fn color(self) -> Hsla {
        match self {
            Ink::Plain => theme::muted(),
            Ink::Red => theme::coral(),
            Ink::Green => theme::code_argument(),
            Ink::Yellow => theme::code_number(),
            Ink::Blue => theme::code_program(),
        }
    }

    /// The colour a basic or 256-colour index stands for, if one of ours.
    fn of_index(n: u16) -> Ink {
        match n {
            1 | 9 => Ink::Red,
            2 | 10 => Ink::Green,
            3 | 11 => Ink::Yellow,
            4 | 12 => Ink::Blue,
            _ => Ink::Plain,
        }
    }
}

/// A line of output as it is drawn: its text without escapes, and the
/// stretches of it, in bytes, that are coloured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub inks: Vec<(Range<usize>, Ink)>,
}

impl Line {
    fn push(&mut self, c: char, ink: Ink) {
        let at = self.text.len();
        self.text.push(c);
        let end = self.text.len();
        if ink == Ink::Plain {
            return;
        }
        match self.inks.last_mut() {
            Some((r, last)) if *last == ink && r.end == at => r.end = end,
            _ => self.inks.push((at..end, ink)),
        }
    }
}

/// What a terminal would show of `text`, line by line: the colours it
/// knows kept, every other escape sequence and control character taken
/// out, and a line written over after a carriage return (a progress bar)
/// left as it ended.
pub fn lines(text: &str) -> Vec<Line> {
    let mut read = Read::default();
    vte::Parser::new().advance(&mut read, text.as_bytes());
    if !read.back && !read.line.text.is_empty() {
        read.out.push(read.line);
    }
    read.out
}

/// What `lines` holds as the parser reads: the lines done, the one being
/// written, its colour, and whether a carriage return is to start it over.
#[derive(Default)]
struct Read {
    out: Vec<Line>,
    line: Line,
    ink: Ink,
    back: bool,
}

impl vte::Perform for Read {
    fn print(&mut self, c: char) {
        if std::mem::take(&mut self.back) {
            self.line = Line::default();
        }
        self.line.push(c, self.ink);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.back = false;
                self.out.push(std::mem::take(&mut self.line));
            }
            b'\r' => self.back = true,
            b'\t' => self.print('\t'),
            _ => {}
        }
    }

    /// Of the control sequences only `m` sets colours; one with nothing
    /// in it resets them.
    fn csi_dispatch(&mut self, params: &vte::Params, between: &[u8], _: bool, action: char) {
        if action == 'm' && between.is_empty() {
            self.ink = if params.is_empty() {
                Ink::Plain
            } else {
                sgr(params.iter().flatten().copied(), self.ink)
            };
        }
    }
}

/// The colour a Select Graphic Rendition sequence leaves, from the one
/// before it. Bold, underline and the like are let go; a colour that is
/// not one of ours is plain; a background changes nothing.
fn sgr(mut codes: impl Iterator<Item = u16>, mut ink: Ink) -> Ink {
    while let Some(code) = codes.next() {
        match code {
            0 | 39 => ink = Ink::Plain,
            30..=37 => ink = Ink::of_index(code - 30),
            90..=97 => ink = Ink::of_index(code - 90 + 8),
            38 | 48 => {
                let set = match codes.next() {
                    Some(5) => codes.next().map_or(Ink::Plain, Ink::of_index),
                    Some(2) => {
                        codes.nth(2);
                        Ink::Plain
                    }
                    _ => Ink::Plain,
                };
                if code == 38 {
                    ink = set;
                }
            }
            _ => {}
        }
    }
    ink
}

/// A line as stretches of one colour each, in bytes, covering all of it:
/// the colours its escapes gave it, and over them the words that say
/// something went wrong (`FAILED`, `error`) in red, coloured or not, as a
/// failure is told everywhere in the thread.
fn stretches(line: &Line) -> Vec<(usize, Ink)> {
    let mut each = vec![Ink::Plain; line.text.len()];
    for (r, ink) in &line.inks {
        each[r.clone()].fill(*ink);
    }
    for r in kinds::alarms(&line.text) {
        each[r].fill(Ink::Red);
    }
    let mut out: Vec<(usize, Ink)> = Vec::new();
    for ink in each {
        match out.last_mut() {
            Some((len, last)) if *last == ink => *len += 1,
            _ => out.push((1, ink)),
        }
    }
    out
}

/// Lines in mono, each stretch in its colour, one under another.
fn styled(faces: &Faces, lines: &[Line]) -> StyledText {
    let mut text = String::new();
    let mut runs: Vec<TextRun> = Vec::new();
    let mut run = |len: usize, ink: Ink| {
        if len == 0 {
            return;
        }
        let color = ink.color();
        if let Some(last) = runs.last_mut().filter(|r| r.color == color) {
            last.len += len;
            return;
        }
        runs.push(TextRun {
            len,
            font: gpui::Font {
                family: faces.mono.clone(),
                features: gpui::FontFeatures::default(),
                fallbacks: Some(crate::fonts::fallbacks(gpui::FontWeight::NORMAL)),
                weight: FontWeight::NORMAL,
                style: gpui::FontStyle::Normal,
            },
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    };
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
            run(1, Ink::Plain);
        }
        for (len, ink) in stretches(line) {
            run(len, ink);
        }
        text.push_str(&line.text);
    }
    StyledText::new(SharedString::from(text)).with_runs(runs)
}

/// One line in mono, coloured as `styled` colours a list of lines.
pub fn styled_one(faces: &Faces, line: &Line) -> StyledText {
    styled(faces, std::slice::from_ref(line))
}

/// The lines drawn when all are asked for, and how many are left out.
pub fn capped<T>(lines: &[T]) -> (&[T], usize) {
    let shown = lines.len().min(MOST);
    (&lines[..shown], lines.len() - shown)
}

/// The line under output cut at `MOST`, saying how much of it is not
/// drawn in the thread, with `Show all` to open the whole output in the
/// right column (`LongAndMany`).
fn left_out(faces: &Faces, n: usize, show_all: Option<gpui::AnyElement>) -> Div {
    let said = if n == 1 {
        crate::tr("… 1 more line not drawn")
    } else {
        crate::tr_with("… %{n} more lines not drawn", &[("n", &n)])
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(36.))
        .px(px(12.))
        .bg(u0ui::theme::color(u0ui::tokens::ColorRole::Layer02))
        .border_t_1()
        .border_b_1()
        .border_dashed()
        .border_color(u0ui::theme::color(u0ui::tokens::ColorRole::BorderStrong01))
        .child(meta(faces, said).flex_1())
        .children(show_all)
}

/// What a call printed, without the Markdown fence an agent may have
/// put round it (Claude's adapter sends "```console" … "```"): its lines.
pub fn unfenced(text: &str) -> &str {
    let t = text.trim();
    let Some(open) = t.strip_prefix("```") else {
        return text;
    };
    // The opening line is the fence and its language; the closing, the fence.
    match (open.split_once('\n'), t.len() > 3 && t.ends_with("```")) {
        (Some((_, inner)), true) => inner[..inner.len() - 3].trim_end_matches('\n'),
        _ => text,
    }
}

/// What a command printed, in mono, quiet, its long lines wrapped: whole
/// when it is short or asked for (up to `MOST` lines), else its head, how
/// much was left out, and its last line; `show_all` opens the whole output
/// in the right column when lines are cut.
pub fn printed_with(
    faces: &Faces,
    text: &str,
    all: bool,
    mut show_all: Option<gpui::AnyElement>,
) -> Div {
    let lines = lines(unfenced(text));
    let mut inner = div()
        .flex()
        .flex_col()
        .w_full()
        .font_family(faces.mono.clone())
        .text_size(px(MONO))
        .line_height(px(MONO_LINE))
        .text_color(theme::muted());
    let mut left = 0;
    if all || lines.len() <= folded_at("") {
        let (shown, rest) = capped(&lines);
        left = rest;
        inner = inner.child(styled(faces, shown));
    } else {
        let gone = lines.len() - HEAD - 1;
        inner = inner
            .child(styled(faces, &lines[..HEAD]))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_color(theme::quiet())
                    .child(crate::tr_with("… %{n} lines", &[("n", &gone)]).into_any_element())
                    .children(if lines.len() > MOST {
                        show_all.take()
                    } else {
                        None
                    }),
            )
            .child(styled(faces, &lines[lines.len() - 1..]));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(S2))
        .w_full()
        .child(inner)
        .when(left > 0, |d| d.child(left_out(faces, left, show_all)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each coloured stretch of a line, as text.
    fn inked(line: &Line) -> Vec<(&str, Ink)> {
        line.inks
            .iter()
            .map(|(r, ink)| (&line.text[r.clone()], *ink))
            .collect()
    }

    #[test]
    fn red_green_yellow_and_blue_are_kept_and_reset_ends_them() {
        let got = lines(
            "\u{1b}[31merror\u{1b}[0m: \u{1b}[32mok\u{1b}[m \u{1b}[1;33mwarn\u{1b}[39m \u{1b}[94mname\u{1b}[0m",
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "error: ok warn name");
        assert_eq!(
            inked(&got[0]),
            vec![
                ("error", Ink::Red),
                ("ok", Ink::Green),
                ("warn", Ink::Yellow),
                ("name", Ink::Blue),
            ]
        );
    }

    #[test]
    fn any_other_colour_is_drawn_plain_and_a_background_changes_nothing() {
        let got = lines(
            "\u{1b}[35mpurple\u{1b}[0m \u{1b}[38;2;255;0;0mtrue\u{1b}[0m \u{1b}[32;41mgreen\u{1b}[0m \u{1b}[38;5;9mbright red",
        );
        assert_eq!(got[0].text, "purple true green bright red");
        assert_eq!(
            inked(&got[0]),
            vec![("green", Ink::Green), ("bright red", Ink::Red)]
        );
    }

    #[test]
    fn every_other_escape_is_taken_out_and_none_shows_raw() {
        let got = lines(
            "\u{1b}[2K\u{1b}[1Gdone\u{1b}[?25h\n\u{1b}]8;;https://x.dev\u{7}link\u{1b}]8;;\u{1b}\\ \u{1b}(Bplain\u{7}",
        );
        let text: Vec<&str> = got.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text, vec!["done", "link plain"]);
        assert!(got.iter().all(|l| l.inks.is_empty()));
    }

    #[test]
    fn a_colour_runs_on_to_the_next_line_until_it_is_reset() {
        let got = lines("\u{1b}[31mfirst\nsecond\u{1b}[0m\nthird");
        assert_eq!(inked(&got[0]), vec![("first", Ink::Red)]);
        assert_eq!(inked(&got[1]), vec![("second", Ink::Red)]);
        assert!(got[2].inks.is_empty());
    }

    #[test]
    fn a_word_saying_something_failed_is_red_whether_it_was_coloured_or_not() {
        let got = lines(
            "test a ... FAILED\n\u{1b}[34mnote\u{1b}[0m: error in \u{1b}[32mtest_error_cases\u{1b}[0m",
        );
        assert_eq!(
            stretches(&got[0]),
            vec![(11, Ink::Plain), (6, Ink::Red)],
            "{:?}",
            got[0]
        );
        assert_eq!(
            stretches(&got[1]),
            vec![
                (4, Ink::Blue),
                (2, Ink::Plain),
                (5, Ink::Red),
                (4, Ink::Plain),
                (16, Ink::Green),
            ]
        );
    }

    #[test]
    fn a_line_written_over_after_a_carriage_return_shows_as_it_ended() {
        let got = lines(" 10%\r 55%\r100%\r\nnext");
        let text: Vec<&str> = got.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text, vec!["100%", "next"]);
    }

    #[test]
    fn all_of_a_long_output_is_drawn_up_to_the_cap_and_the_rest_is_counted() {
        let many: Vec<usize> = (0..2_322).collect();
        let (shown, left) = capped(&many);
        assert_eq!(shown.len(), MOST);
        assert_eq!(left, 2_322 - MOST);
        let few: Vec<usize> = (0..12).collect();
        assert_eq!(capped(&few), (&few[..], 0));
    }

    #[test]
    fn the_markdown_fence_round_output_is_not_part_of_it() {
        assert_eq!(unfenced("```console\nok\nfine\n```"), "ok\nfine");
        assert_eq!(unfenced("```\nok\n```\n"), "ok");
        assert_eq!(unfenced("plain\n```"), "plain\n```");
        assert_eq!(unfenced("```"), "```");
    }
}
