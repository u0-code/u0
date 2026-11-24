//! Three faces: a sans for the interface, a mono for code and every
//! number, and a serif for what an agent says.
//!
//! They are the faces of the thread's design: `Inter`, `JetBrains Mono` and
//! `Source Serif 4`, open fonts shipped in `assets/fonts/` as static
//! instances (GPUI does not set a variable font's weight). They are built
//! into the binary, so that the app reads nothing from the checkout as it
//! opens: when the checkout is under a folder macOS guards, such as the
//! Desktop, a read there waits on the system asking whether it may, and
//! the window would wait with it. If a face cannot be added, each falls
//! back to a face macOS always has.
//!
//! None of them has Chinese, and what is said often is. Each falls back
//! to `PingFang SC` for it, at the weight of the text around it, the face macOS sets Chinese in, rather than to
//! whatever the system's cascade finds first for the language you set,
//! which can be a Japanese face with other shapes for the same characters.
//! The window's text carries the fallbacks, so every face under it has
//! them; a run of text set with a font of its own is given them too.

use std::borrow::Cow;

use gpui::{App, FontFallbacks, FontWeight, Global, SharedString, Styled};

#[derive(Clone)]
pub struct Faces {
    pub ui: SharedString,
    pub mono: SharedString,
    pub serif: SharedString,
}

impl Global for Faces {}

macro_rules! shipped {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../assets/fonts/", $name)) as &[u8])),*]
    };
}

/// Every face in `assets/fonts/`, by its file's name.
const SHIPPED: &[(&str, &[u8])] = shipped![
    "Inter-400.ttf",
    "Inter-500.ttf",
    "Inter-600.ttf",
    "JetBrainsMono-400.ttf",
    "SourceSerif4-400.ttf",
    "SourceSerif4-400-Italic.ttf",
    "SourceSerif4-600.ttf",
    "SourceSerif4-700.ttf",
];

/// Add what is shipped, then pick a face for each voice.
pub fn load(cx: &mut App) {
    let shipped = SHIPPED
        .iter()
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
        .collect();
    let _ = cx.text_system().add_fonts(shipped);
    let names = cx.text_system().all_font_names();
    let pick = |wanted: &[&str]| -> SharedString {
        wanted
            .iter()
            .find(|w| names.iter().any(|n| n == *w))
            .map_or_else(
                || (*wanted.last().unwrap_or(&"Helvetica")).to_owned().into(),
                |w| (*w).to_owned().into(),
            )
    };
    cx.set_global(Faces {
        ui: pick(&["Inter", ".SystemUIFont", "Helvetica Neue"]),
        mono: pick(&["JetBrains Mono", "SF Mono", "Menlo"]),
        serif: pick(&["Source Serif 4", "Georgia"]),
    });
}

/// The faces a character none of ours has is looked for in, in order:
/// Chinese in `PingFang SC` at the weight the text is set in (or, on a
/// system without it, `Hiragino Sans GB`), and emoji in colour. The weight
/// is named because macOS takes a fallback by its name alone: given the
/// family, a heading's Chinese came out as thin as the body's.
pub fn fallbacks(weight: FontWeight) -> FontFallbacks {
    let pingfang = if weight.0 >= FontWeight::SEMIBOLD.0 {
        "PingFangSC-Semibold"
    } else if weight.0 >= FontWeight::MEDIUM.0 {
        "PingFangSC-Medium"
    } else {
        "PingFangSC-Regular"
    };
    FontFallbacks::from_fonts(vec![
        pingfang.to_owned(),
        "Hiragino Sans GB".to_owned(),
        "Apple Color Emoji".to_owned(),
    ])
}

/// `el`, with the fallbacks for everything set in it.
pub fn with_fallbacks<E: Styled>(mut el: E) -> E {
    el.text_style()
        .get_or_insert_with(Default::default)
        .font_fallbacks = Some(fallbacks(FontWeight::NORMAL));
    el
}

/// Setting text's weight sets its Chinese's weight with it.
pub trait Weighted: Styled + Sized {
    fn weight(mut self, weight: FontWeight) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        style.font_weight = Some(weight);
        style.font_fallbacks = Some(fallbacks(weight));
        self
    }
}

impl<E: Styled> Weighted for E {}

pub fn faces(cx: &App) -> &Faces {
    cx.global::<Faces>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_in_the_fonts_folder_is_built_in() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
            })
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        on_disk.sort();
        let mut built_in: Vec<String> = SHIPPED.iter().map(|(n, _)| (*n).to_owned()).collect();
        built_in.sort();
        assert_eq!(built_in, on_disk);
    }
}
