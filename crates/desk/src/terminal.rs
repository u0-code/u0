//! A terminal of yours in the drawer under the thread, as t3code's: a real
//! shell in a pseudo-terminal (ghost-core's `pty`), in the session's folder
//! (or on the machine a remote project is on, over SSH), whose screen is
//! kept by a VT parser and drawn by the desk. What you type goes to it as a
//! terminal sends keys; what it prints wakes the window through the hub's
//! channel.

use std::{
    io::Read as _,
    num::NonZeroU16,
    path::Path,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use ghost_core::pty::Pty;
use gpui::{Hsla, Keystroke};

use crate::{
    hub::{Msg, Sender},
    theme,
};

/// How many lines scrolled off the top are kept.
const SCROLLBACK: usize = 5_000;

/// A shell in a pseudo-terminal, and its screen.
pub struct Terminal {
    pub id: u64,
    /// The session it was opened beside.
    pub session: u64,
    /// What it is called in its tab: the shell, or the script it was
    /// opened for.
    pub title: String,
    screen: Arc<Mutex<vt100::Parser>>,
    /// Set by the reader when the screen changed and the window has not
    /// been told since.
    pub changed: Arc<AtomicBool>,
    pty: Pty,
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
    /// Open a shell as [`Pty::open`] does, with a screen for it. Blocking:
    /// it starts a process. What it prints comes from the reader handed
    /// back with it (see `reader`).
    pub fn open(
        id: u64,
        session: u64,
        dir: &Path,
        host: Option<&str>,
        first: Option<&str>,
        (rows, cols): (u16, u16),
    ) -> Result<(Self, Box<dyn std::io::Read + Send>), String> {
        let (pty, title, reader) = Pty::open(dir, host, first, (rows, cols))?;
        let screen = Arc::new(Mutex::new(vt100::Parser::new(
            NonZeroU16::new(rows).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(cols).unwrap_or(NonZeroU16::MIN),
            SCROLLBACK,
        )));
        let t = Self {
            id,
            session,
            title,
            screen,
            changed: Arc::new(AtomicBool::new(false)),
            pty,
            ended: None,
            size: (rows, cols),
            selection: None,
            origin: Arc::default(),
        };
        Ok((t, reader))
    }

    /// What reads its output (`from`) onto its screen until its shell ends,
    /// telling the window over `tx`: run on a thread of its own once the
    /// terminal is the window's.
    pub fn reader(
        &self,
        mut from: Box<dyn std::io::Read + Send>,
    ) -> impl FnOnce(&Sender) + Send + 'static {
        let (id, into, told) = (self.id, self.screen.clone(), self.changed.clone());
        move |tx| {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match from.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        into.lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .process(&buf[..n]);
                        // Once per drawing, however much comes before it.
                        if !told.swap(true, Ordering::AcqRel) {
                            let _ = tx.send(Msg::Terminal(id));
                        }
                    }
                }
            }
            let _ = tx.send(Msg::TerminalEnded(id));
        }
    }

    /// Its rows and columns.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// Send what was typed or pasted.
    pub fn send(&mut self, bytes: &[u8]) {
        // Typing brings the screen back to its bottom.
        self.parser().screen_mut().set_scrollback(0);
        self.selection = None;
        self.pty.send(bytes);
    }

    /// Paste `text`, marked as a paste where the program asked for that.
    pub fn paste(&mut self, text: &str) {
        let bracketed = self.parser().screen().bracketed_paste();
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if bracketed {
            self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        } else {
            self.send(text.as_bytes());
        }
    }

    /// Make it `rows` by `cols`, if it is not already.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(2), cols.max(10));
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        self.pty.resize(rows, cols);
        self.parser().screen_mut().set_size(
            NonZeroU16::new(rows).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(cols).unwrap_or(NonZeroU16::MIN),
        );
    }

    /// Scroll back through what went off the top (`by` above zero) or
    /// down again.
    pub fn scroll(&mut self, by: isize) {
        let mut parser = self.parser();
        let screen = parser.screen_mut();
        let at = screen.scrollback().saturating_add_signed(by);
        screen.set_scrollback(at);
        drop(parser);
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

    /// Whether something runs in it besides its shell.
    pub fn busy(&self) -> bool {
        self.pty.busy()
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

    /// Note how it ended.
    pub fn end(&mut self) {
        self.ended = Some(self.pty.ended());
    }

    fn parser(&self) -> std::sync::MutexGuard<'_, vt100::Parser> {
        self.screen.lock().unwrap_or_else(PoisonError::into_inner)
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
                    (fg, bg) = (bg.unwrap_or_else(theme::bg), Some(fg));
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
        vt100::Color::Default => theme::text2(),
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
        0 | 8 => theme::faint(),
        1 | 9 => theme::coral(),
        2 | 10 => theme::code_argument(),
        3 | 11 => theme::code_number(),
        4 | 12 => theme::code_program(),
        5 | 13 => rgb(0xC6, 0x9C, 0xE6),
        6 | 14 => rgb(0x7F, 0xC8, 0xC8),
        7 => theme::text2(),
        15 => theme::text(),
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

    fn key(text: &str) -> Keystroke {
        Keystroke::parse(text).unwrap()
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
            ("red", theme::coral())
        );
        assert_eq!(rows[1][0].background, Some(theme::text2()));
        assert_eq!(cursor, Some((1, 3)));
    }

    #[test]
    fn a_shell_opens_in_a_terminal_and_answers() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let dir = std::env::temp_dir();
        let (mut t, from) = Terminal::open(1, 1, &dir, None, Some("echo ghost-$((40+2))"), (6, 40))
            .expect("a terminal opens");
        let read = t.reader(from);
        std::thread::spawn(move || read(&tx));
        let mut seen = false;
        for _ in 0..100 {
            while rx.try_recv().is_ok() {
                t.changed.store(false, Ordering::Release);
            }
            if t.text().contains("ghost-42") {
                seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(seen, "{}", t.text());
        assert!(!t.busy(), "the shell waits again");
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
                    == "ghost-42"
            })
            .expect("the answer on a line of its own");
        let row = u16::try_from(row).expect("a row on the screen");
        t.selection = Some(((row, 7), (row, 0)));
        assert_eq!(t.chosen_text().as_deref(), Some("ghost-42"));
        assert_eq!(t.word_at((row, 3)), ((row, 0), (row, 7)));
        t.send(b" ");
        assert_eq!(t.chosen(), None);
        t.resize(10, 50);
        assert_eq!(t.rows().0.len(), 10);
    }
}
