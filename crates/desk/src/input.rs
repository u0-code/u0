//! A text input: the line you speak into, and the fields of the desk's
//! forms. GPUI draws text but has no input of its own, so this is the
//! usual one, trimmed to what a prompt needs. Return submits; the owner
//! hears it as an [`Submitted`] event, every edit as [`Changed`], and Esc
//! as [`Cancelled`], so that a field which filters as you type can follow
//! it and close itself.
//!
//! By default it is one line, and a pasted newline becomes a space;
//! [`TextInput::wrapping`] lets it hold paragraphs (⇧Return begins one),
//! wrap and grow with what is typed up to a number of lines, and scroll
//! past them. With the line empty, ↑ and ↓ go back through what was handed
//! over before. How its text is shaped and painted is in `input/element.rs`.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, SharedString, UTF16Selection, Window, actions, div, point, prelude::*, px,
};
use unicode_segmentation::UnicodeSegmentation;

mod element;

use element::{Shaped, TextElement};

use crate::style::BODY;
use crate::theme;

actions!(
    desk_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Submit,
        SubmitAside,
        Newline,
        Up,
        Down,
        Tab,
        Cancel,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DeleteWord,
        DeleteLine,
        Undo,
        Redo,
        Stash,
        SubmitAsideToo,
    ]
);

/// The context key bindings apply under.
pub const CONTEXT: &str = "DeskInput";

/// What Return does with the line: clears it into its history, or keeps
/// it (and hands it over even empty, for a list's choice).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Keeps {
    No,
    Text,
    EvenEmpty,
}

/// The line was submitted with Return.
pub struct Submitted(pub String);

/// What is typed, handed over with ⌘Return: to go on in the background
/// while a new line is begun.
pub struct SubmittedAside(pub String);

/// What is typed changed under the user's hand.
pub struct Changed;

/// Esc was pressed in the line.
pub struct Cancelled;

/// ↑ (-1) or ↓ (1) was pressed in a line with nothing handed over before
/// to go back through: a list beside it can move its choice.
pub struct Stepped(pub isize);

/// What is typed, put aside (⌘S) to come back to.
pub struct Stashed(pub String);

/// An image was pasted, not text.
pub struct PastedImage(pub gpui::Image);

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<Shaped>,
    last_bounds: Option<Bounds<Pixels>>,
    /// How many lines it may grow to before it scrolls: one unless it wraps.
    most: usize,
    /// How many lines what is typed took when it was last drawn, and the
    /// first of them in view.
    lines: usize,
    scroll: usize,
    is_selecting: bool,
    /// Room above and below the text: a prompt breathes, a field in a
    /// list row sits as tall as its neighbours.
    pad_y: f32,
    /// The size of the text and of a line: a field in a list row reads as
    /// the row's own words.
    size: f32,
    line: f32,
    /// What was handed over before, the latest last, and which of it is in
    /// the line while you go back through it with the arrows.
    history: Vec<String>,
    recalled: Option<usize>,
    /// What is typed stays when it is handed over: a search, not a message.
    keeps: Keeps,
    /// Return breaks the line, and ⌘Return hands it over (⇧⌘Return aside),
    /// where lines wrap and you asked for it.
    enter_breaks: bool,
    /// What it held before each change, to go back to (⌘Z) and forward
    /// again (⇧⌘Z); typing in a run is one change.
    undone: Vec<(SharedString, Range<usize>)>,
    redone: Vec<(SharedString, Range<usize>)>,
    last_change: Option<std::time::Instant>,
}

impl EventEmitter<Submitted> for TextInput {}
impl EventEmitter<SubmittedAside> for TextInput {}
impl EventEmitter<Changed> for TextInput {}
impl EventEmitter<Cancelled> for TextInput {}
impl EventEmitter<Stepped> for TextInput {}
impl EventEmitter<PastedImage> for TextInput {}
impl EventEmitter<Stashed> for TextInput {}

impl TextInput {
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            most: 1,
            lines: 1,
            scroll: 0,
            is_selecting: false,
            pad_y: 12.,
            history: Vec::new(),
            recalled: None,
            keeps: Keeps::No,
            enter_breaks: false,
            undone: Vec::new(),
            redone: Vec::new(),
            last_change: None,
            size: BODY,
            line: 22.,
        }
    }

    /// Wrap what is typed, and grow with it up to `most` lines; past that
    /// it scrolls to keep the cursor in view.
    #[must_use]
    pub fn wrapping(mut self, most: usize) -> Self {
        self.most = most.max(1);
        self
    }

    /// Keep what is typed when Return hands it over.
    #[must_use]
    pub fn keeping(mut self) -> Self {
        self.keeps = Keeps::Text;
        self
    }

    /// Return hands over the line even when it is empty, as a list's
    /// choice can be taken with nothing typed.
    #[must_use]
    pub fn submitting_empty(mut self) -> Self {
        self.keeps = Keeps::EvenEmpty;
        self
    }

    /// Let it grow to `most` lines before it scrolls.
    pub fn set_most(&mut self, most: usize, cx: &mut Context<Self>) {
        self.most = most.max(1);
        cx.notify();
    }

    /// Text `size` on lines `line` tall, in pixels.
    #[must_use]
    pub fn sized(mut self, size: f32, line: f32) -> Self {
        self.size = size;
        self.line = line;
        self
    }

    /// Room above and below the text, in pixels.
    #[must_use]
    pub fn padded(mut self, pad_y: f32) -> Self {
        self.pad_y = pad_y;
        self
    }

    /// Tab is not typed: what the line offers takes it, before it gets
    /// here (see `Desk::offer_keys`).
    fn tab(_: &mut Self, _: &Tab, _: &mut Window, _: &mut Context<Self>) {}

    /// A new paragraph, where the line wraps (⇧Return).
    /// A new line, where lines wrap; on one line, ⇧Return hands it over
    /// the other way (Find goes back).
    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.most > 1 {
            self.break_line(window, cx);
        } else {
            cx.emit(SubmittedAside(self.text()));
        }
    }

    /// Back through what was handed over before, while the line is empty
    /// or holds one of those.
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        // Up a line first, where what is typed takes several.
        if let Some(at) = self.line_above(false) {
            self.move_to(at, cx);
            return;
        }
        if self.history.is_empty() {
            cx.emit(Stepped(-1));
            return;
        }
        if !self.content.is_empty() && self.recalled.is_none() {
            return;
        }
        let at = self
            .recalled
            .map_or(self.history.len() - 1, |n| n.saturating_sub(1));
        self.recalled = Some(at);
        let text = self.history[at].clone();
        self.set_text(&text, cx);
        self.recalled = Some(at);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(at) = self.line_above(true) {
            self.move_to(at, cx);
            return;
        }
        if self.history.is_empty() {
            cx.emit(Stepped(1));
            return;
        }
        let Some(at) = self.recalled else { return };
        if at + 1 < self.history.len() {
            let text = self.history[at + 1].clone();
            self.set_text(&text, cx);
            self.recalled = Some(at + 1);
        } else {
            self.set_text("", cx);
        }
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(word_before(&self.content, self.cursor_offset()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(word_after(&self.content, self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(word_before(&self.content, self.cursor_offset()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(word_after(&self.content, self.cursor_offset()), cx);
    }

    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(line_of(&self.content, self.cursor_offset()).start, cx);
    }

    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(line_of(&self.content, self.cursor_offset()).end, cx);
    }

    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(line_of(&self.content, self.cursor_offset()).start, cx);
    }

    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(line_of(&self.content, self.cursor_offset()).end, cx);
    }

    fn delete_word(&mut self, _: &DeleteWord, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(word_before(&self.content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_line(&mut self, _: &DeleteLine, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(line_of(&self.content, self.cursor_offset()).start, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    /// Put what is typed aside, and clear the line.
    fn stash(&mut self, _: &Stash, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.text();
        if !text.is_empty() {
            self.reset();
            cx.emit(Stashed(text));
            cx.emit(Changed);
            cx.notify();
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((text, at)) = self.undone.pop() {
            self.redone
                .push((self.content.clone(), self.selected_range.clone()));
            self.content = text;
            self.selected_range = self.within(at);
            self.last_change = None;
            cx.emit(Changed);
            cx.notify();
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((text, at)) = self.redone.pop() {
            self.undone
                .push((self.content.clone(), self.selected_range.clone()));
            self.content = text;
            self.selected_range = self.within(at);
            self.last_change = None;
            cx.emit(Changed);
            cx.notify();
        }
    }

    /// Keep what it holds before a change, to undo it; typing in a run
    /// (each change a moment after the last, putting text in) is one.
    fn before_change(&mut self, typing: bool) {
        let run = typing
            && self
                .last_change
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(800));
        if !run {
            self.undone
                .push((self.content.clone(), self.selected_range.clone()));
            if self.undone.len() > 200 {
                self.undone.remove(0);
            }
        }
        self.redone.clear();
        self.last_change = typing.then(std::time::Instant::now);
    }

    /// The line above the cursor, where what is typed wraps onto several;
    /// `None` on the first (or with one line).
    fn line_above(&self, down: bool) -> Option<usize> {
        let layout = self.last_layout.as_ref()?;
        if layout.lines() < 2 {
            return None;
        }
        let now = layout.at(self.cursor_offset());
        let h = layout.height();
        let y = if down { now.y + h } else { now.y - h };
        if y < px(0.) || y >= h * layout.lines() as f32 {
            return None;
        }
        Some(layout.index_at(point(now.x, y + h / 2.)))
    }

    fn cancel(_: &mut Self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Cancelled);
    }

    /// Return hands the line over, unless it breaks the line (see
    /// `enter_breaks`).
    pub fn set_enter_breaks(&mut self, breaks: bool) {
        self.enter_breaks = breaks && self.most > 1;
    }

    /// A new line; in a list, its next item begun, or after an item left
    /// empty, the list ended there.
    fn break_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.selected_range.start;
        let begins = self.content[..at].rfind('\n').map_or(0, |i| i + 1);
        match next_item(&self.content[begins..at]) {
            Some(Item::Next(marker)) => {
                self.replace_text_in_range(None, &format!("\n{marker}"), window, cx);
            }
            Some(Item::End) => {
                self.selected_range = begins..self.selected_range.end;
                self.replace_text_in_range(None, "", window, cx);
            }
            None => self.replace_text_in_range(None, "\n", window, cx),
        }
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if self.enter_breaks {
            self.break_line(window, cx);
        } else {
            self.send(cx);
        }
    }

    fn submit_aside(&mut self, _: &SubmitAside, window: &mut Window, cx: &mut Context<Self>) {
        if self.enter_breaks {
            self.send(cx);
        } else {
            self.aside(window, cx);
        }
    }

    fn submit_aside_too(
        &mut self,
        _: &SubmitAsideToo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.aside(window, cx);
    }

    fn aside(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.content.trim().to_owned();
        if text.is_empty() {
            return;
        }
        if self.history.last() != Some(&text) {
            self.history.push(text.clone());
        }
        self.reset();
        cx.emit(SubmittedAside(text));
        cx.notify();
    }

    /// Hand over what is typed, if anything, and clear the line.
    pub fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.content.trim().to_owned();
        if text.is_empty() && self.keeps != Keeps::EvenEmpty {
            return;
        }
        if self.keeps != Keeps::No {
            cx.emit(Submitted(text));
            return;
        }
        if self.history.last() != Some(&text) {
            self.history.push(text.clone());
        }
        self.reset();
        cx.emit(Submitted(text));
        cx.notify();
    }

    /// Say something else in the line while it is empty.
    pub fn set_placeholder(&mut self, placeholder: &str, cx: &mut Context<Self>) {
        if self.placeholder != placeholder {
            self.placeholder = SharedString::from(placeholder.to_owned());
            cx.notify();
        }
    }

    /// What is typed, as it is.
    pub fn text(&self) -> String {
        self.content.trim().to_owned()
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }

    /// Put text in the line, replacing what was there.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.reset();
        self.content = text.to_owned().into();
        self.selected_range = self.content.len()..self.content.len();
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_everything(cx);
    }

    /// Select the whole text, so that typing replaces it.
    pub fn select_everything(&mut self, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let item = cx.read_from_clipboard();
        if let Some(image) = item.as_ref().and_then(|item| {
            item.entries().iter().find_map(|e| {
                if let gpui::ClipboardEntry::Image(i) = e {
                    Some(i.clone())
                } else {
                    None
                }
            })
        }) {
            cx.emit(PastedImage(image));
            return;
        }
        if let Some(text) = item.and_then(|item| item.text()) {
            let text = if self.most > 1 {
                text
            } else {
                text.replace('\n', " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    /// Copy what is chosen in it; with nothing chosen, ⌘C goes on to
    /// what else it copies (the words chosen in the thread).
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            cx.propagate();
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.within(self.selected_range.clone())].to_string(),
            ));
        }
    }

    /// What is chosen in the line, if anything is.
    pub fn chosen(&self) -> Option<String> {
        (!self.selected_range.is_empty())
            .then(|| self.content[self.within(self.selected_range.clone())].to_string())
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.within(self.selected_range.clone())].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.index_at(position - self.origin(bounds))
    }

    /// Where the text starts: the top left of its bounds, less the lines
    /// scrolled past.
    fn origin(&self, bounds: &Bounds<Pixels>) -> Point<Pixels> {
        let line = self.last_layout.as_ref().map_or(px(20.), Shaped::height);
        point(bounds.left(), bounds.top() - line * self.scroll as f32)
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    /// `range` made safe to cut the text with: inside it, on character
    /// boundaries, and in order. What an input method hands over can be
    /// stale by the time it arrives.
    fn within(&self, range: Range<usize>) -> Range<usize> {
        within(&self.content, range)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn reset(&mut self) {
        self.recalled = None;
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.scroll = 0;
        self.is_selecting = false;
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.within(self.range_from_utf16(&range_utf16));
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let range = self.within(range);
        self.before_change(range.is_empty() && !new_text.is_empty() && !new_text.contains('\n'));
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.emit(Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let range = self.within(range);
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if new_text.is_empty() {
            self.marked_range = None;
        } else {
            self.marked_range = Some(range.start..range.start + new_text.len());
        }
        // Where the input method puts the cursor is given within what it is
        // composing, in UTF-16: it is measured in that text, not in the
        // whole, and starts where that text does.
        self.selected_range =
            marked_selection(new_text, range.start, new_selected_range_utf16.as_ref());
        cx.emit(Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let origin = self.origin(&bounds);
        let start = origin + last_layout.at(range.start);
        let end = origin + last_layout.at(range.end);
        Some(Bounds::from_corners(
            start,
            point(end.x.max(start.x), end.y + last_layout.height()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        bounds.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_at(point - self.origin(&bounds));
        Some(self.offset_to_utf16(utf8_index))
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .min_w(px(0.))
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::submit_aside))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::delete_word))
            .on_action(cx.listener(Self::delete_line))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::stash))
            .on_action(cx.listener(Self::submit_aside_too))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .line_height(px(self.line))
            .text_size(px(self.size))
            .text_color(theme::text())
            .child(
                div()
                    .w_full()
                    .py(px(self.pad_y))
                    .child(TextElement { input: cx.entity() }),
            )
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// `range` inside `text`, on character boundaries, start before end.
fn within(text: &str, range: Range<usize>) -> Range<usize> {
    let at = |i: usize| {
        let mut i = i.min(text.len());
        while !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (a, b) = (at(range.start), at(range.end));
    a.min(b)..a.max(b)
}

/// A UTF-16 offset into `text` as a byte offset, at most its length.
fn utf8_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}

/// Where the cursor goes while an input method composes `new_text` at
/// byte `start`: the range it asks for within `new_text`, in UTF-16, or the
/// end of `new_text`.
fn marked_selection(new_text: &str, start: usize, asked: Option<&Range<usize>>) -> Range<usize> {
    match asked {
        Some(r) => start + utf8_offset(new_text, r.start)..start + utf8_offset(new_text, r.end),
        None => start + new_text.len()..start + new_text.len(),
    }
}

/// Where the word before `at` begins in `text`: past any spaces, then
/// back to the start of the word.
fn word_before(text: &str, at: usize) -> usize {
    text.split_word_bound_indices()
        .rfind(|(i, w)| *i < at && !w.trim().is_empty())
        .map_or(0, |(i, _)| i)
}

/// Where the word after `at` ends in `text`.
fn word_after(text: &str, at: usize) -> usize {
    text.split_word_bound_indices()
        .find(|(i, w)| i + w.len() > at && !w.trim().is_empty())
        .map_or(text.len(), |(i, w)| i + w.len())
}

/// The line `at` is on in `text`, between its newlines.
fn line_of(text: &str, at: usize) -> Range<usize> {
    let at = at.min(text.len());
    let start = text[..at].rfind('\n').map_or(0, |n| n + 1);
    let end = text[at..].find('\n').map_or(text.len(), |n| at + n);
    start..end
}

#[cfg(test)]
mod ime_tests {
    use super::*;

    #[test]
    fn words_and_lines_are_found_from_the_cursor() {
        let t = "fix the  upload\nthen test";
        assert_eq!(word_before(t, 9), 4);
        assert_eq!(word_before(t, 4), 0);
        assert_eq!(word_after(t, 0), 3);
        assert_eq!(word_after(t, 3), 7);
        assert_eq!(line_of(t, 5), 0..15);
        assert_eq!(line_of(t, 18), 16..25);
    }

    #[test]
    fn a_cursor_an_input_method_places_is_measured_in_what_it_composes() {
        // Composing 你好 after "ab": the cursor it asks for at the end of
        // the two characters (UTF-16 2) is byte 6 into them, byte 8 in all.
        assert_eq!(marked_selection("你好", 2, Some(&(2..2))), 8..8);
        assert_eq!(marked_selection("你好", 2, Some(&(1..2))), 5..8);
        assert_eq!(marked_selection("ni", 0, None), 2..2);
        // Asked past its end, it stays at the end.
        assert_eq!(marked_selection("哦", 0, Some(&(4..4))), 3..3);
    }

    #[test]
    fn a_range_to_cut_with_is_kept_inside_the_text_and_on_its_characters() {
        assert_eq!(
            within("哦", 4..4),
            3..3,
            "the range of the crash: past the end"
        );
        assert_eq!(within("哦a", 1..4), 0..4);
        let (end, start) = (1, 2);
        assert_eq!(within("abc", start..end), 1..2, "put in order");
    }
}

/// What a new line after a list's item does.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    /// Begins the next item with this.
    Next(String),
    /// The item was left empty: its mark goes, and the list ends.
    End,
}

/// What a new line after `line` does, where it is a list's item: the same
/// mark ("- ", "* ", "> ") or the next number ("3. "), under the same
/// indent; none where it is not a list.
fn next_item(line: &str) -> Option<Item> {
    let rest = line.trim_start();
    let indent = &line[..line.len() - rest.len()];
    let (mark, after) = if let Some(found) = ["- ", "* ", "> "]
        .iter()
        .find_map(|m| rest.strip_prefix(m).map(|a| ((*m).to_owned(), a)))
    {
        found
    } else {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let after = rest[digits..].strip_prefix(". ")?;
        let n: u64 = rest[..digits].parse().ok()?;
        (format!("{}. ", n + 1), after)
    };
    Some(if after.trim().is_empty() {
        Item::End
    } else {
        Item::Next(format!("{indent}{mark}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{Item, next_item};

    #[test]
    fn a_list_goes_on_to_its_next_item_and_ends_at_an_empty_one() {
        let next = |m: &str| Some(Item::Next(m.into()));
        assert_eq!(next_item("- milk"), next("- "));
        assert_eq!(next_item("  * eggs"), next("  * "));
        assert_eq!(next_item("9. ten"), next("10. "));
        assert_eq!(next_item("> said"), next("> "));
        assert_eq!(next_item("- "), Some(Item::End));
        assert_eq!(next_item("3. "), Some(Item::End));
        assert_eq!(next_item("plain"), None);
        assert_eq!(next_item("-not a list"), None);
        assert_eq!(next_item(". no number"), None);
    }
}
