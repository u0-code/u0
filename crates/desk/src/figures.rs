//! What Markdown shows as a picture rather than as words: a Mermaid
//! diagram (a code block in `mermaid`), drawn by mermaid-rs-renderer, and,
//! where the settings ask for it, a formula in TeX (`$…$`, `$$…$$`, a code
//! block in `math`), set by `ratex` (a port of `KaTeX`). Each is drawn as an SVG, natively, and
//! drawn again only when the look's text colour changes. One that cannot
//! be drawn shows what was written, and says why.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use gpui::{
    App, IntoElement, Pixels, SharedString, Window,
    component::text::{
        InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
        markdown_ast::Node,
    },
    div,
    prelude::*,
    px,
};

use crate::{style::PROSE, theme};

/// Whether `$…$` and `$$…$$` are set as formulas: the settings' choice,
/// set as the window is drawn, as t3code's "Render math".
static MATH: AtomicBool = AtomicBool::new(false);

pub fn set_math(on: bool) {
    MATH.store(on, Ordering::Relaxed);
}

pub fn math() -> bool {
    MATH.load(Ordering::Relaxed)
}

/// What a figure is made of.
enum Kind {
    Mermaid,
    /// TeX, set apart on its own line or within one.
    Math {
        display: bool,
    },
}

/// A figure drawn: its picture and its size, in points.
#[derive(Clone)]
struct Drawn {
    image: Arc<gpui::Image>,
    width: f32,
    height: f32,
    /// How far down from its top a formula's baseline is.
    baseline: f32,
}

/// A figure as parsed: what was written, and the picture last made of it
/// with the text colour it was made in; an error where none could be.
struct Figure {
    kind: Kind,
    source: String,
    drawn: Mutex<(u32, Result<Drawn, String>)>,
}

impl Figure {
    fn new(kind: Kind, source: String) -> Self {
        let colour = colour();
        let drawn = draw(&kind, &source, colour);
        Self {
            kind,
            source,
            drawn: Mutex::new((key(colour), drawn)),
        }
    }

    /// The picture for the look now, drawn again if the look changed.
    fn now(&self) -> Result<Drawn, String> {
        let colour = colour();
        let mut drawn = self
            .drawn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if drawn.0 != key(colour) {
            *drawn = (key(colour), draw(&self.kind, &self.source, colour));
        }
        drawn.1.clone()
    }
}

fn colour() -> gpui::Rgba {
    theme::text().into()
}

fn key(c: gpui::Rgba) -> u32 {
    u32::from(c)
}

/// `source` drawn as `kind` is, in `colour`.
fn draw(kind: &Kind, source: &str, colour: gpui::Rgba) -> Result<Drawn, String> {
    let (svg, baseline) = match kind {
        Kind::Mermaid => (mermaid(source, colour)?, 0.),
        Kind::Math { display } => tex(source, *display, colour)?,
    };
    let (width, height) = size(&svg).ok_or("the drawing has no size")?;
    Ok(Drawn {
        image: Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Svg,
            svg.into_bytes(),
        )),
        width,
        height,
        baseline,
    })
}

/// A Mermaid diagram as an SVG, in the dark or the light look as the text
/// is light or dark.
fn mermaid(source: &str, colour: gpui::Rgba) -> Result<String, String> {
    let light = colour.r + colour.g + colour.b < 1.5;
    let theme = if light {
        mermaid_rs_renderer::Theme::modern()
    } else {
        mermaid_rs_renderer::Theme::dark()
    };
    let options = mermaid_rs_renderer::RenderOptions {
        theme,
        ..mermaid_rs_renderer::RenderOptions::default()
    };
    mermaid_rs_renderer::render_with_options(source, options).map_err(|e| e.to_string())
}

/// A formula as an SVG at prose's size, its glyphs drawn as outlines, and
/// how far down its baseline is.
fn tex(source: &str, display: bool, colour: gpui::Rgba) -> Result<(String, f32), String> {
    const PAD: f64 = 1.;
    let nodes = ratex_parser::parse(source).map_err(|e| e.to_string())?;
    let options = ratex_layout::LayoutOptions {
        style: if display {
            ratex_types::MathStyle::Display
        } else {
            ratex_types::MathStyle::Text
        },
        color: ratex_types::Color {
            r: colour.r,
            g: colour.g,
            b: colour.b,
            a: colour.a,
        },
        ..ratex_layout::LayoutOptions::default()
    };
    let laid = ratex_layout::layout(&nodes, &options);
    let size = f64::from(PROSE);
    let svg = ratex_svg::render_to_svg(
        &ratex_layout::to_display_list(&laid),
        &ratex_svg::SvgOptions {
            font_size: size,
            padding: PAD,
            embed_glyphs: true,
            ..ratex_svg::SvgOptions::default()
        },
    );
    Ok((svg, (PAD + laid.height * size) as f32))
}

/// An SVG's size, from its `viewBox`.
fn size(svg: &str) -> Option<(f32, f32)> {
    let (_, rest) = svg.split_once("viewBox=\"")?;
    let (bounds, _) = rest.split_once('"')?;
    let mut numbers = bounds.split_whitespace().skip(2).map(str::parse::<f32>);
    let (width, height) = (numbers.next()?.ok()?, numbers.next()?.ok()?);
    (width > 0. && height > 0.).then_some((width, height))
}

/// A figure on its own: a diagram scrolls sideways where it is wider than
/// the thread; one that cannot be drawn is shown as written, under why.
fn block(figure: &Figure, cx: &App) -> gpui::AnyElement {
    match figure.now() {
        Ok(d) => div()
            .id(SharedString::from(format!("figure-{:x}", d.image.id())))
            .w_full()
            .overflow_x_scroll()
            .child(gpui::img(d.image).w(px(d.width)).h(px(d.height)))
            .into_any_element(),
        Err(why) => div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_color(theme::muted())
                    .child(format!("This can't be drawn: {why}")),
            )
            .child(
                div()
                    .font_family(crate::fonts::faces(cx).mono.clone())
                    .whitespace_normal()
                    .child(figure.source.clone()),
            )
            .into_any_element(),
    }
}

/// Mermaid diagrams, and formulas set apart where math is on.
pub struct Figures(pub bool);

impl MarkdownPlugin for Figures {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "figure"
    }

    fn parse(&self, node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let figure = match node {
            Node::Code(code) if code.lang.as_deref() == Some("mermaid") => {
                Figure::new(Kind::Mermaid, code.value.clone())
            }
            Node::Code(code) if self.0 && code.lang.as_deref() == Some("math") => {
                Figure::new(Kind::Math { display: true }, code.value.clone())
            }
            Node::Math(m) if self.0 => Figure::new(Kind::Math { display: true }, m.value.clone()),
            _ => return None,
        };
        let written = cx.node_source(node).unwrap_or(&figure.source).to_owned();
        let text = figure.source.clone();
        Some(
            MarkdownNode::new("figure", figure)
                .text(text)
                .markdown(written),
        )
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        node.data::<Figure>()
            .map_or_else(|| div().into_any_element(), |f| block(f, cx))
    }
}

/// Formulas within a line, where math is on: set on the line's baseline;
/// one that cannot be set stays as written.
pub struct Formulas;

impl MarkdownPlugin for Formulas {
    fn name(&self) -> &'static str {
        "formula"
    }

    fn parse(&self, node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let Node::InlineMath(m) = node else {
            return None;
        };
        let figure = Figure::new(Kind::Math { display: false }, m.value.clone());
        let written = cx
            .node_source(node)
            .map_or_else(|| format!("${}$", m.value), str::to_owned);
        Some(
            MarkdownNode::new("formula", figure)
                .text(written.clone())
                .markdown(written),
        )
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        _: &InlineRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> Option<InlineElement> {
        let d = node.data::<Figure>()?.now().ok()?;
        Some(
            InlineElement::new(gpui::img(d.image).w(px(d.width)).h(px(d.height)))
                .with_baseline(Pixels::from(d.baseline)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: gpui::Rgba = gpui::Rgba {
        r: 0.,
        g: 0.,
        b: 0.,
        a: 1.,
    };

    #[test]
    fn a_mermaid_flowchart_is_drawn_with_its_words() {
        let d = draw(&Kind::Mermaid, "flowchart LR\n  A[Start] --> B[End]", BLACK).unwrap();
        assert!(d.width > 0. && d.height > 0.);
        let svg = std::str::from_utf8(d.image.bytes()).unwrap();
        assert!(svg.contains("Start") && !svg.contains("foreignObject"));
    }

    #[test]
    fn a_diagram_that_does_not_read_says_why() {
        assert!(draw(&Kind::Mermaid, "not a diagram at all", BLACK).is_err());
    }

    #[test]
    fn a_formula_is_set_as_outlines_in_the_text_colour() {
        let d = draw(
            &Kind::Math { display: false },
            r"\frac{x^2}{\sqrt{y}}",
            BLACK,
        )
        .unwrap();
        let svg = std::str::from_utf8(d.image.bytes()).unwrap();
        assert!(svg.contains("<path") && !svg.contains("<text"));
        assert!(d.baseline > 0. && d.baseline < d.height);
    }

    #[test]
    fn a_formula_that_does_not_read_says_why() {
        assert!(draw(&Kind::Math { display: true }, r"\frac{x", BLACK).is_err());
    }

    #[test]
    fn an_svg_is_as_large_as_its_view_box() {
        assert_eq!(
            size(r#"<svg width="9pt" viewBox="0 0 170.5 50.25">"#),
            Some((170.5, 50.25))
        );
        assert_eq!(size("<svg>"), None);
    }
}
