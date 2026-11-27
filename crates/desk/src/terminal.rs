//! A terminal of yours in the drawer under the thread, as t3code's: a real
//! shell in a pseudo-terminal, run by the server (u0-core's
//! `terminals`), in the session's folder, whose screen is kept here by a
//! VT parser and drawn by the desk. What you type goes to it as a
//! terminal sends keys (a `Call::TermInput`); what it prints comes back
//! as `Event::TermOutput`.

use std::{
    num::NonZeroU16,
    sync::{Arc, Mutex},
};

use gpui::{Hsla, Keystroke};
use u0ui::tokens::ColorRole;

use crate::{proto::Call, theme};

/// How many lines scrolled off the top are kept.
const SCROLLBACK: usize = 5_000;

/// A terminal's screen, of a shell the server runs.
pub struct Terminal {
    pub id: u64,
    /// The server its shell runs on: this Mac's, unless it is given
    /// another.
    pub host: crate::hub::Host,
    /// The session it was opened beside.
    pub session: u64,
    /// What it is called in its tab: the shell, or the script it was
    /// opened for; nothing until it has opened.
    pub title: String,
    /// The name of the repository's script it runs, if it runs one.
    pub script: Option<String>,
    screen: vt100::Parser,
    /// How it ended, once it has.
    pub ended: Option<String>,
    size: (u16, u16),
    /// The cells chosen with the mouse, from where the press was to where
    /// it is now, each a row and column on the screen.
    pub selection: Option<((u16, u16), (u16, u16))>,
    /// Where on the window its first cell was last drawn, to tell which
    /// cell the mouse is over.
    pub origin: Arc<Mutex<gpui::Point<gpui::Pixels>>>,
}

/// A stretch of a row drawn one way.
#[derive(Debug, Clone, PartialEq)]
pub struct Stretch {
    pub text: String,
    pub color: Hsla,
    pub background: Option<Hsla>,
    pub bold: bool,
}

impl Terminal {
    /// The screen of the terminal `id`, beside the session `session`,
    /// `rows` by `cols`, blank until it prints.
    pub fn new(id: u64, session: u64, (rows, cols): (u16, u16)) -> Self {
        Self {
            id,
            host: crate::hub::Host::HERE,
            session,
            title: String::new(),
            script: None,
            screen: vt100::Parser::new(
                NonZeroU16::new(rows).unwrap_or(NonZeroU16::MIN),
                NonZeroU16::new(cols).unwrap_or(NonZeroU16::MIN),
                SCROLLBACK,
            ),
            ended: None,
            size: (rows, cols),
            selection: None,
            origin: Arc::default(),
        }
    }

    /// Take what it printed onto its screen.
    pub fn printed(&mut self, bytes: &[u8]) {
        self.screen.process(bytes);
    }

    /// Its rows and columns.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// What was typed or pasted, to be sent to it.
    pub fn send(&mut self, bytes: Vec<u8>) -> Call {
        // Typing brings the screen back to its bottom.
        self.screen.screen_mut().set_scrollback(0);
        self.selection = None;
        Call::TermInput { id: self.id, bytes }
    }

    /// Paste `text`, marked as a paste where the program asked for that.
    pub fn paste(&mut self, text: &str) -> Call {
        let bracketed = self.screen.screen().bracketed_paste();
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if bracketed {
            self.send(format!("\x1b[200~{text}\x1b[201~").into_bytes())
        } else {
            self.send(text.into_bytes())
        }
    }

    /// Make it `rows` by `cols`: what tells its shell, unless it is so
    /// already.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Option<Call> {
        let (rows, cols) = (rows.max(2), cols.max(10));
        if self.size == (rows, cols) {
            return None;
        }
        self.size = (rows, cols);
        self.screen.screen_mut().set_size(
            NonZeroU16::new(rows).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(cols).unwrap_or(NonZeroU16::MIN),
        );
        Some(Call::TermResize {
            id: self.id,
            size: (rows, cols),
        })
    }

    /// Every cell of the screen chosen.
    pub fn choose_all(&mut self) {
        let (rows, cols) = self.size;
        self.selection = Some(((0, 0), (rows.saturating_sub(1), cols.saturating_sub(1))));
    }

    /// What it printed forgotten, the screen and what went off its top;
    /// what its shell prints next begins it again.
    pub fn clear(&mut self) {
        let (rows, cols) = self.size;
        self.screen = vt100::Parser::new(
            NonZeroU16::new(rows).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(cols).unwrap_or(NonZeroU16::MIN),
            SCROLLBACK,
        );
        self.selection = None;
    }

    /// Back to its newest line, from scrolling back.
    pub fn latest(&mut self) {
        self.screen.screen_mut().set_scrollback(0);
    }

    /// Scroll back through what went off the top (`by` above zero) or
    /// down again.
    pub fn scroll(&mut self, by: isize) {
        let screen = self.screen.screen_mut();
        let at = screen.scrollback().saturating_add_signed(by);
        screen.set_scrollback(at);
        self.selection = None;
    }

    /// The cells chosen, first to last, the last one in; `None` when
    /// nothing is, or only the cell pressed on.
    pub fn chosen(&self) -> Option<((u16, u16), (u16, u16))> {
        let (a, b) = self.selection?;
        (a != b).then(|| if a <= b { (a, b) } else { (b, a) })
    }

    /// The cells of the word at `(row, col)`: the run of cells around it
    /// with something other than a space in them.
    pub fn word_at(&self, (row, col): (u16, u16)) -> ((u16, u16), (u16, u16)) {
        let parser = self.parser();
        let screen = parser.screen();
        let cols = screen.size().1.get();
        let filled = |c: u16| {
            screen
                .cell(row, c)
                .is_some_and(|x| x.is_wide_continuation() || !x.contents().trim().is_empty())
        };
        if !filled(col) {
            return ((row, col), (row, col));
        }
        let (mut from, mut to) = (col, col);
        while from > 0 && filled(from - 1) {
            from -= 1;
        }
        while to + 1 < cols && filled(to + 1) {
            to += 1;
        }
        ((row, from), (row, to))
    }

    /// The word at `(row, col)`, without what stands around it in a
    /// sentence (brackets, quotes, a full stop).
    pub fn word_text(&self, at: (u16, u16)) -> String {
        let ((row, from), (_, to)) = self.word_at(at);
        let word = self
            .parser()
            .screen()
            .contents_between(row, from, row, to.saturating_add(1));
        word.trim_matches(|c: char| "()<>[]\"'`,;".contains(c))
            .trim_end_matches('.')
            .to_owned()
    }

    /// The address at `(row, col)`, when the word there is one.
    pub fn link_at(&self, at: (u16, u16)) -> Option<String> {
        let word = self.word_text(at);
        (word.starts_with("https://") || word.starts_with("http://")).then_some(word)
    }

    /// The text of the cells chosen.
    pub fn chosen_text(&self) -> Option<String> {
        let ((r0, c0), (r1, c1)) = self.chosen()?;
        let text = self
            .parser()
            .screen()
            .contents_between(r0, c0, r1, c1.saturating_add(1));
        Some(text.trim_end().to_owned()).filter(|t| !t.is_empty())
    }

    /// The screen as rows of stretches, and the cursor where it shows.
    pub fn rows(&self) -> (Vec<Vec<Stretch>>, Option<(u16, u16)>) {
        drawn(self.parser().screen())
    }

    /// Whether the program asked for the cursor keys' other codes.
    pub fn app_cursor(&self) -> bool {
        self.parser().screen().application_cursor()
    }

    /// What the screen shows, as text: to hand to an agent, or to find an
    /// address in.
    pub fn text(&self) -> String {
        self.parser().screen().contents()
    }

    fn parser(&self) -> &vt100::Parser {
        &self.screen
    }
}

/// A screen as rows of stretches, and the cursor where it shows.
fn drawn(screen: &vt100::Screen) -> (Vec<Vec<Stretch>>, Option<(u16, u16)>) {
    {
        let (rows, cols) = (screen.size().0.get(), screen.size().1.get());
        let mut out = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            let mut line: Vec<Stretch> = Vec::new();
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let (mut fg, mut bg) = (color(cell.fgcolor()), background(cell.bgcolor()));
                if cell.inverse() {
                    (fg, bg) = (
                        bg.unwrap_or_else(|| theme::role(ColorRole::TerminalBackground)),
                        Some(fg),
                    );
                }
                let text = if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                };
                match line.last_mut() {
                    Some(s) if s.color == fg && s.background == bg && s.bold == cell.bold() => {
                        s.text.push_str(text);
                    }
                    _ => line.push(Stretch {
                        text: text.to_owned(),
                        color: fg,
                        background: bg,
                        bold: cell.bold(),
                    }),
                }
            }
            // Spaces at a row's end with nothing behind them are not drawn.
            if let Some(last) = line.last_mut().filter(|s| s.background.is_none()) {
                let kept = last.text.trim_end().len();
                last.text.truncate(kept);
            }
            out.push(line);
        }
        let cursor =
            (!screen.hide_cursor() && screen.scrollback() == 0).then(|| screen.cursor_position());
        (out, cursor)
    }
}

/// The bytes a terminal sends for `key`; `None` for one it does not send
/// (a key of the window's, with ⌘).
pub fn key_bytes(key: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let m = &key.modifiers;
    if m.platform {
        return None;
    }
    let arrow = |c: char| {
        if app_cursor {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
    };
    let named = match key.key.as_str() {
        "enter" => Some("\r".to_owned()),
        "backspace" => Some("\x7f".to_owned()),
        "tab" if m.shift => Some("\x1b[Z".to_owned()),
        "tab" => Some("\t".to_owned()),
        "escape" => Some("\x1b".to_owned()),
        "up" => Some(arrow('A')),
        "down" => Some(arrow('B')),
        "right" => Some(arrow('C')),
        "left" => Some(arrow('D')),
        "home" => Some("\x1b[H".to_owned()),
        "end" => Some("\x1b[F".to_owned()),
        "delete" => Some("\x1b[3~".to_owned()),
        "pageup" => Some("\x1b[5~".to_owned()),
        "pagedown" => Some("\x1b[6~".to_owned()),
        _ => None,
    };
    if let Some(s) = named {
        let s = if m.alt { format!("\x1b{s}") } else { s };
        return Some(s.into_bytes());
    }
    if m.control {
        let c = key.key.chars().next().filter(|_| key.key.len() == 1)?;
        let b = match c {
            'a'..='z' => c as u8 - b'a' + 1,
            '@' | ' ' => 0,
            '[' => 0x1b,
            '\\' => 0x1c,
            ']' => 0x1d,
            _ => return None,
        };
        return Some(vec![b]);
    }
    // ⌥ with a letter is Meta, as a shell's line editor reads it.
    if m.alt && key.key.len() == 1 {
        return Some(format!("\x1b{}", key.key).into_bytes());
    }
    let typed = key
        .key_char
        .as_deref()
        .or(if key.key == "space" { Some(" ") } else { None })?;
    Some(typed.as_bytes().to_vec())
}

/// The colour a cell's foreground stands for: the desk's own for the
/// sixteen named ones, the xterm cube and greys for the rest.
fn color(c: vt100::Color) -> Hsla {
    match c {
        vt100::Color::Default => theme::role(ColorRole::TerminalText),
        vt100::Color::Idx(n) => indexed(n),
        vt100::Color::Rgb(r, g, b) => rgb(r, g, b),
    }
}

fn background(c: vt100::Color) -> Option<Hsla> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(0) => None,
        other => Some(color(other)),
    }
}

fn indexed(n: u8) -> Hsla {
    match n {
        0 | 8 => rgb(0x8D, 0x8D, 0x8D),
        1 | 9 => rgb(0xFF, 0x83, 0x89),
        2 | 10 => rgb(0x42, 0xBE, 0x65),
        3 | 11 => rgb(0xF1, 0xC2, 0x1B),
        4 | 12 => rgb(0x78, 0xA9, 0xFF),
        5 | 13 => rgb(0xBE, 0x95, 0xFF),
        6 | 14 => rgb(0x3D, 0xDB, 0xD9),
        7 | 15 => theme::role(ColorRole::TerminalText),
        16..=231 => {
            let n = n - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            rgb(level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        232..=255 => {
            let v = 8 + (n - 232) * 10;
            rgb(v, v, v)
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Hsla {
    gpui::rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &'static str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    #[test]
    fn a_cleared_terminal_forgets_what_it_printed_and_all_can_be_chosen() {
        let mut t = Terminal::new(1, 1, (3, 10));
        t.printed(b"hello\r\nworld");
        t.choose_all();
        assert_eq!(t.chosen(), Some(((0, 0), (2, 9))));
        assert!(t.text().contains("hello"));
        t.clear();
        assert!(!t.text().contains("hello"));
        assert_eq!(t.chosen(), None);
    }

    #[test]
    fn keys_are_sent_as_a_terminal_sends_them() {
        assert_eq!(key_bytes(&key("enter"), false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(&key("backspace"), false), Some(vec![0x7f]));
        assert_eq!(key_bytes(&key("ctrl-c"), false), Some(vec![3]));
        assert_eq!(key_bytes(&key("up"), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(&key("up"), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(&key("alt-b"), false), Some(b"\x1bb".to_vec()));
        assert_eq!(key_bytes(&key("cmd-k"), false), None);
    }

    #[test]
    fn a_screen_is_drawn_in_stretches_of_one_colour_without_trailing_space() {
        let four = NonZeroU16::new(4).unwrap();
        let mut p = vt100::Parser::new(four, NonZeroU16::new(20).unwrap(), 0);
        p.process(b"ok \x1b[31mred\x1b[0m\r\n\x1b[7minv\x1b[0m");
        let (rows, cursor) = drawn(p.screen());
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0][0].text, "ok ");
        assert_eq!(
            (rows[0][1].text.as_str(), rows[0][1].color),
            ("red", rgb(0xFF, 0x83, 0x89))
        );
        assert_eq!(
            rows[1][0].background,
            Some(theme::role(ColorRole::TerminalText))
        );
        assert_eq!(cursor, Some((1, 3)));
    }

    /// What the server says of the terminal 1 into `t`, until its screen
    /// shows `text`.
    fn until(
        t: &mut Terminal,
        rx: &std::sync::mpsc::Receiver<crate::proto::Event>,
        text: &str,
    ) -> bool {
        for _ in 0..100 {
            while let Ok(e) = rx.try_recv() {
                match e {
                    crate::proto::Event::TermOpened { result, .. } => {
                        t.title = result.expect("a terminal opens");
                    }
                    crate::proto::Event::TermOutput { bytes, .. } => t.printed(&bytes),
                    _ => {}
                }
            }
            if t.text().contains(text) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn a_shell_opens_in_a_terminal_and_answers_and_opened_again_shows_it_still() {
        let (tx, rx) = std::sync::mpsc::channel();
        let send: Arc<dyn Fn(crate::proto::Event) + Send + Sync> = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        let shells = u0_core::terminals::Terminals::default();
        let at = (std::env::temp_dir(), None);
        let first = Some("echo u0-$((40+2))".to_owned());
        shells.open(1, at.clone(), first.clone(), (6, 40), send.clone());
        let mut t = Terminal::new(1, 1, (6, 40));
        assert!(until(&mut t, &rx, "u0-42"), "{}", t.text());
        assert_eq!(t.title, "echo u0-$((40+2))");
        // The answer's own line, chosen with the mouse, is its text.
        let row = t
            .rows()
            .0
            .iter()
            .position(|r| {
                r.iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    == "u0-42"
            })
            .expect("the answer on a line of its own");
        let row = u16::try_from(row).expect("a row on the screen");
        t.selection = Some(((row, 4), (row, 0)));
        assert_eq!(t.chosen_text().as_deref(), Some("u0-42"));
        assert_eq!(t.word_at((row, 3)), ((row, 0), (row, 4)));
        let typed = t.send(b" ".to_vec());
        assert!(matches!(typed, Call::TermInput { id: 1, .. }));
        assert_eq!(t.chosen(), None);
        assert!(t.resize(10, 50).is_some());
        assert!(t.resize(10, 50).is_none(), "it is that size already");
        assert_eq!(t.rows().0.len(), 10);
        // Opened again (a desk come back), it shows what it printed.
        shells.open(1, at, first, (6, 40), send);
        let mut again = Terminal::new(1, 1, (6, 40));
        assert!(until(&mut again, &rx, "u0-42"), "{}", again.text());
        assert!(!shells.close(1, false), "the shell waits again: it closes");
    }
}
