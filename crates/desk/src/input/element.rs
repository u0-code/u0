//! The input's text as drawn: shaped on one line or wrapped in paragraphs,
//! laid out to the input's width, scrolled to the cursor, and painted with
//! the selection and the cursor over it.

use gpui::{
    App, Bounds, ElementId, ElementInputHandler, Entity, GlobalElementId, LayoutId, PaintQuad,
    Pixels, Point, ShapedLine, Style, TextRun, UnderlineStyle, Window, WrappedLine, fill, point,
    prelude::*, px, relative, size,
};

use super::TextInput;
use crate::theme;

/// What is typed, shaped: on one line, or in paragraphs, each wrapped to
/// the input's width and starting at its byte in the text.
pub(super) enum Shaped {
    Line(Box<ShapedLine>, Pixels),
    Wrapped(Vec<(usize, WrappedLine)>, Pixels),
}

impl Shaped {
    /// The height of one of its lines.
    pub(super) fn height(&self) -> Pixels {
        match self {
            Self::Line(_, h) | Self::Wrapped(_, h) => *h,
        }
    }

    /// How many lines it takes.
    pub(super) fn lines(&self) -> usize {
        match self {
            Self::Line(..) => 1,
            Self::Wrapped(ps, _) => ps.iter().map(|(_, w)| w.wrap_boundaries().len() + 1).sum(),
        }
    }

    /// The paragraph `index` is in, and how many lines come before it.
    fn paragraph(ps: &[(usize, WrappedLine)], index: usize) -> (usize, usize) {
        let mut rows = 0;
        for (n, (start, w)) in ps.iter().enumerate() {
            let last = n + 1 == ps.len();
            if last || index < ps[n + 1].0 {
                let _ = start;
                return (n, rows);
            }
            rows += w.wrap_boundaries().len() + 1;
        }
        (0, 0)
    }

    /// Where the character at `index` starts, from the top left of the text.
    pub(super) fn at(&self, index: usize) -> Point<Pixels> {
        match self {
            Self::Line(l, _) => point(l.x_for_index(index), px(0.)),
            Self::Wrapped(ps, h) => {
                let (n, rows) = Self::paragraph(ps, index);
                let (start, w) = &ps[n];
                let local = index.saturating_sub(*start);
                let p = w
                    .position_for_index(local, *h)
                    .unwrap_or_else(|| point(w.width(), *h * w.wrap_boundaries().len() as f32));
                point(p.x, p.y + *h * rows as f32)
            }
        }
    }

    /// The character nearest `p`, from the top left of the text.
    pub(super) fn index_at(&self, p: Point<Pixels>) -> usize {
        match self {
            Self::Line(l, _) => l.closest_index_for_x(p.x),
            Self::Wrapped(ps, h) => {
                let row = (p.y / *h).floor().clamp(0., (self.lines() - 1) as f32) as usize;
                let mut rows = 0;
                for (start, w) in ps {
                    let here = w.wrap_boundaries().len() + 1;
                    if row < rows + here {
                        let local = row - rows;
                        let q = point(p.x.max(px(0.)), *h * local as f32 + *h / 2.);
                        return start
                            + w.closest_index_for_position(q, *h)
                                .unwrap_or_else(|edge| edge);
                    }
                    rows += here;
                }
                ps.last().map_or(0, |(start, w)| start + w.len())
            }
        }
    }

    fn paint(&self, origin: Point<Pixels>, window: &mut Window, cx: &mut App) {
        match self {
            Self::Line(l, h) => {
                let _ = l.paint(origin, *h, gpui::TextAlign::Left, None, window, cx);
            }
            Self::Wrapped(ps, h) => {
                let mut y = origin.y;
                for (_, w) in ps {
                    let _ = w.paint(
                        point(origin.x, y),
                        *h,
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                    y += *h * (w.wrap_boundaries().len() + 1) as f32;
                }
            }
        }
    }
}

pub(super) struct TextElement {
    pub(super) input: Entity<TextInput>,
}

pub(super) struct PrepaintState {
    line: Option<Shaped>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    /// The first line in view, and how many lines the text took.
    scroll: usize,
    lines: usize,
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let input = self.input.read(cx);
        let shown = input.lines.clamp(1, input.most);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height() * shown as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), theme::faint())
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let height = window.line_height();
        let most = input.most;
        let line = if most > 1 {
            // One wrapped line for each paragraph, each where it starts.
            let starts = std::iter::once(0).chain(
                display_text
                    .char_indices()
                    .filter(|(_, c)| *c == '\n')
                    .map(|(i, _)| i + 1),
            );
            let wrapped = window
                .text_system()
                .shape_text(
                    display_text.clone(),
                    font_size,
                    &runs,
                    Some(bounds.size.width),
                    None,
                )
                .ok()
                .filter(|lines| !lines.is_empty())
                .map(|lines| starts.zip(lines).collect::<Vec<_>>());
            match wrapped {
                Some(w) => Shaped::Wrapped(w.into_iter().collect(), height),
                None => Shaped::Line(
                    Box::new(
                        window
                            .text_system()
                            .shape_line(display_text, font_size, &runs, None),
                    ),
                    height,
                ),
            }
        } else {
            Shaped::Line(
                Box::new(
                    window
                        .text_system()
                        .shape_line(display_text, font_size, &runs, None),
                ),
                height,
            )
        };

        // Keep the cursor's line in view: scroll as little as will do it.
        let lines = line.lines();
        let row = |index: usize| (line.at(index).y / height).round() as usize;
        let cursor_row = row(cursor);
        let mut scroll = input.scroll.min(lines.saturating_sub(most));
        if cursor_row < scroll {
            scroll = cursor_row;
        } else if cursor_row >= scroll + most {
            scroll = cursor_row + 1 - most;
        }
        let origin = point(bounds.left(), bounds.top() - height * scroll as f32);

        let cursor_at = origin + line.at(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(cursor_at, size(px(1.5), height)),
                    theme::accent(),
                )),
            )
        } else {
            let mut tint = theme::accent();
            tint.a = 0.25;
            // One band for each line the selection touches.
            let (from, to) = (line.at(selected_range.start), line.at(selected_range.end));
            let (first, last) = (row(selected_range.start), row(selected_range.end));
            let bands = (first..=last)
                .map(|r| {
                    let y = height * r as f32;
                    let left = if r == first { from.x } else { px(0.) };
                    let right = if r == last { to.x } else { bounds.size.width };
                    fill(
                        Bounds::from_corners(
                            origin + point(left, y),
                            origin + point(right, y + height),
                        ),
                        tint,
                    )
                })
                .collect();
            (bands, None)
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
            scroll,
            lines,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let (scroll, lines) = (prepaint.scroll, prepaint.lines);
        let line = prepaint.line.take().expect("a shaped line");
        let origin = point(bounds.left(), bounds.top() - line.height() * scroll as f32);
        // Lines scrolled out of view are not drawn over what is around it.
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for band in prepaint.selection.drain(..) {
                window.paint_quad(band);
            }
            line.paint(origin, window, cx);
            if focus_handle.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll = scroll;
            // A change in how many lines it takes changes its height, which
            // is laid out before it is drawn: draw it once more.
            if input.lines != lines {
                input.lines = lines;
                cx.notify();
            }
        });
    }
}
