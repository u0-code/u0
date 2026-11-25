//! Words in the thread chosen with the mouse, as in any page: pressed on a
//! message's text and dragged, across its paragraphs and into the next
//! message; ⌘C copies them.
//!
//! GPUI's text is not selectable of itself. Each paragraph of the thread
//! is drawn through [`Selectable`], which notes where it was painted this
//! frame and with what text; the mouse is matched against those, and the
//! next frame draws the words chosen on a tint. Only what is painted can
//! be chosen: the thread draws what is in view.

use std::{cell::RefCell, ops::Range};

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, HighlightStyle, IntoElement,
    LayoutId, Pixels, Point, SharedString, StyledText, TextLayout, TextRun, Window,
};

use crate::theme;

/// A paragraph painted this frame: its id, where its text was laid out,
/// and the text.
struct Painted {
    id: SharedString,
    layout: TextLayout,
    text: SharedString,
}

/// Where the choosing began and where it is now: a paragraph and a byte in
/// its text.
#[derive(Clone)]
struct Chosen {
    anchor: (SharedString, usize),
    head: (SharedString, usize),
}

thread_local! {
    static PAINTED: RefCell<Vec<Painted>> = const { RefCell::new(Vec::new()) };
    /// The paragraphs of the frame before, in the order they were painted:
    /// the thread's order, which tells what lies between two of them.
    static ORDER: RefCell<Vec<SharedString>> = const { RefCell::new(Vec::new()) };
    static CHOSEN: RefCell<Option<Chosen>> = const { RefCell::new(None) };
}

/// A new frame of the thread begins: what was painted is the order now.
pub fn new_frame() {
    let ids: Vec<SharedString> = PAINTED.with(|p| p.borrow_mut().drain(..).map(|p| p.id).collect());
    if !ids.is_empty() {
        ORDER.with(|o| *o.borrow_mut() = ids);
    }
}

/// The paragraph at `at` and the byte under it; with `nearest`, the one
/// nearest above or below when `at` is between them.
fn hit(at: Point<Pixels>, nearest: bool) -> Option<(SharedString, usize)> {
    PAINTED.with(|p| {
        let p = p.borrow();
        let inside = |b: Bounds<Pixels>| at.y >= b.top() && at.y <= b.bottom();
        let found = p
            .iter()
            .find(|x| {
                let b = x.layout.bounds();
                inside(b) && at.x >= b.left() && at.x <= b.right()
            })
            .or_else(|| p.iter().find(|x| nearest && inside(x.layout.bounds())))
            .or_else(|| {
                if !nearest {
                    return None;
                }
                // Between paragraphs: the first below, or the last above.
                p.iter()
                    .find(|x| x.layout.bounds().top() > at.y)
                    .or_else(|| p.iter().rev().find(|x| x.layout.bounds().bottom() < at.y))
            })?;
        let b = found.layout.bounds();
        let byte = if at.y < b.top() {
            0
        } else if at.y > b.bottom() {
            found.text.len()
        } else {
            match found.layout.index_for_position(at) {
                Ok(i) | Err(i) => i.min(found.text.len()),
            }
        };
        Some((found.id.clone(), byte))
    })
}

/// The mouse pressed at `at`: choosing begins there, on a paragraph; true
/// when it did. Elsewhere, what was chosen is let go.
pub fn press(at: Point<Pixels>) -> bool {
    let start = hit(at, false);
    CHOSEN.with(|c| {
        *c.borrow_mut() = start.map(|s| Chosen {
            anchor: s.clone(),
            head: s,
        });
    });
    CHOSEN.with(|c| c.borrow().is_some())
}

/// The mouse dragged to `at` while choosing.
pub fn drag(at: Point<Pixels>) {
    let Some(head) = hit(at, true) else { return };
    CHOSEN.with(|c| {
        if let Some(chosen) = c.borrow_mut().as_mut() {
            chosen.head = head;
        }
    });
}

/// Let go of what was chosen.
pub fn clear() {
    CHOSEN.with(|c| *c.borrow_mut() = None);
}

/// The chosen stretch from the first paragraph to the last, in the
/// thread's order: (paragraph, byte) at each end.
fn ends() -> Option<((usize, usize), (usize, usize))> {
    let chosen = CHOSEN.with(|c| c.borrow().clone())?;
    ORDER.with(|o| {
        let o = o.borrow();
        let at =
            |(id, byte): &(SharedString, usize)| Some((o.iter().position(|x| x == id)?, *byte));
        let (a, h) = (at(&chosen.anchor)?, at(&chosen.head)?);
        let (from, to) = if a <= h { (a, h) } else { (h, a) };
        (from != to).then_some((from, to))
    })
}

/// What of the paragraph `id` is chosen, `len` long.
fn range_of(id: &str, len: usize) -> Option<Range<usize>> {
    let ((p0, b0), (p1, b1)) = ends()?;
    let n = ORDER.with(|o| o.borrow().iter().position(|x| x == id))?;
    if n < p0 || n > p1 {
        return None;
    }
    let start = if n == p0 { b0.min(len) } else { 0 };
    let end = if n == p1 { b1.min(len) } else { len };
    (start < end).then_some(start..end)
}

/// The words chosen, paragraph by paragraph, as text; none when nothing is.
pub fn text() -> Option<String> {
    let ((p0, b0), (p1, b1)) = ends()?;
    let order = ORDER.with(|o| o.borrow().clone());
    let mut out = Vec::new();
    for (n, id) in order.iter().enumerate().take(p1 + 1).skip(p0) {
        let Some(text) = PAINTED.with(|p| {
            p.borrow()
                .iter()
                .find(|x| &x.id == id)
                .map(|x| x.text.clone())
        }) else {
            continue;
        };
        let start = if n == p0 { b0.min(text.len()) } else { 0 };
        let end = if n == p1 {
            b1.min(text.len())
        } else {
            text.len()
        };
        if let Some(part) = text.get(start..end) {
            out.push(part.trim_matches('\u{2009}').to_owned());
        }
    }
    (!out.is_empty()).then(|| out.join("\n"))
}

/// What the words chosen are drawn on: the accent, faint.
fn tint() -> gpui::Hsla {
    let mut tint = theme::accent();
    tint.a = 0.25;
    tint
}

/// `runs` with what of the paragraph `id` is chosen drawn on the tint.
pub fn tinted(id: &str, len: usize, runs: Vec<TextRun>) -> Vec<TextRun> {
    match range_of(id, len) {
        Some(chosen) => split(runs, &chosen, tint()),
        None => runs,
    }
}

/// `text` in the style around it, choosable, as the paragraph `id`.
pub fn plain(id: impl Into<SharedString>, text: impl Into<SharedString>) -> Selectable {
    let (id, text) = (id.into(), text.into());
    let mut styled = StyledText::new(text.clone());
    if let Some(chosen) = range_of(&id, text.len()) {
        styled = styled.with_highlights(vec![(
            chosen,
            HighlightStyle {
                background_color: Some(tint()),
                ..HighlightStyle::default()
            },
        )]);
    }
    let layout = styled.layout().clone();
    Selectable::new(id, layout, text, styled)
}

/// `runs` cut at the ends of `range`, the runs within it on `tint`.
fn split(runs: Vec<TextRun>, range: &Range<usize>, tint: gpui::Hsla) -> Vec<TextRun> {
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut at = 0;
    for run in runs {
        let (start, end) = (at, at + run.len);
        at = end;
        let cuts = [
            start,
            range.start.clamp(start, end),
            range.end.clamp(start, end),
            end,
        ];
        for w in cuts.windows(2) {
            if w[1] > w[0] {
                let mut piece = run.clone();
                piece.len = w[1] - w[0];
                if w[0] >= range.start && w[1] <= range.end {
                    piece.background_color = Some(tint);
                }
                out.push(piece);
            }
        }
    }
    out
}

/// A paragraph whose words can be chosen: `inner` draws `text`, laid out
/// in `layout`.
pub struct Selectable {
    id: SharedString,
    layout: TextLayout,
    text: SharedString,
    inner: AnyElement,
}

impl Selectable {
    pub fn new(
        id: impl Into<SharedString>,
        layout: TextLayout,
        text: SharedString,
        inner: impl IntoElement,
    ) -> Self {
        Self {
            id: id.into(),
            layout,
            text,
            inner: inner.into_any_element(),
        }
    }
}

impl Element for Selectable {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.inner.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(window, cx);
        PAINTED.with(|p| {
            p.borrow_mut().push(Painted {
                id: self.id.clone(),
                layout: self.layout.clone(),
                text: self.text.clone(),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(len: usize) -> TextRun {
        TextRun {
            len,
            font: gpui::font("Inter"),
            color: theme::text(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    #[test]
    fn the_runs_are_cut_where_the_choice_begins_and_ends() {
        let tint = theme::accent();
        let out = split(vec![run(5), run(5)], &(3..7), tint);
        let lens: Vec<_> = out.iter().map(|r| r.len).collect();
        assert_eq!(lens, [3, 2, 2, 3]);
        let tinted: Vec<_> = out.iter().map(|r| r.background_color.is_some()).collect();
        assert_eq!(tinted, [false, true, true, false]);
    }
}
