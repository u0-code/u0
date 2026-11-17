//! Three faces: a sans for the interface, a mono for the journal and every
//! number, and a serif for her own words and the one headline on a page.
//!
//! The sans and the mono the design is set in (IBM Plex Sans, IBM Plex
//! Mono) are open fonts, loaded from `assets/fonts/` when they are there.
//! Until they are, each falls back to a face macOS always has, so the desk
//! still reads as three voices. Her words are in a text serif macOS has (New
//! York, else Charter): one made to be read at 15, as Claude's app sets its
//! own answers, not a display face that thins out at that size.
//!
//! None of them has Chinese, and what she says often is. Each falls back
//! to `PingFang SC` for it, the face macOS sets Chinese in, rather than to
//! whatever the system's cascade finds first for the language you set,
//! which can be a Japanese face with other shapes for the same characters.
//! The window's text carries the fallbacks, so every face under it has
//! them; a run of text set with a font of its own is given them too.

use std::{borrow::Cow, path::PathBuf};

use gpui::{App, FontFallbacks, Global, SharedString, Styled};

#[derive(Clone)]
pub struct Faces {
    pub ui: SharedString,
    pub mono: SharedString,
    pub serif: SharedString,
}

impl Global for Faces {}

/// Load what is shipped, then pick a face for each voice.
pub fn load(cx: &mut App) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
    let mut shipped = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_font = path.extension().is_some_and(|e| e == "ttf" || e == "otf");
            if is_font && let Ok(bytes) = std::fs::read(&path) {
                shipped.push(Cow::Owned(bytes));
            }
        }
    }
    if !shipped.is_empty() {
        let _ = cx.text_system().add_fonts(shipped);
    }
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
        ui: pick(&["IBM Plex Sans", ".SystemUIFont", "Helvetica Neue"]),
        mono: pick(&["IBM Plex Mono", "SF Mono", "Menlo"]),
        serif: pick(&["New York", "Charter", "Georgia"]),
    });
}

/// The faces a character none of ours has is looked for in, in order:
/// Chinese in `PingFang SC` (or, on a system without it, `Hiragino Sans GB`),
/// and emoji in colour.
pub fn fallbacks() -> FontFallbacks {
    FontFallbacks::from_fonts(vec![
        "PingFang SC".to_owned(),
        "Hiragino Sans GB".to_owned(),
        "Apple Color Emoji".to_owned(),
    ])
}

/// The faces her prose falls back to: Chinese in a serif (`Songti SC`), so
/// her words read in one voice whatever the script, then the usual.
pub fn serif_fallbacks() -> FontFallbacks {
    FontFallbacks::from_fonts(vec![
        "Songti SC".to_owned(),
        "PingFang SC".to_owned(),
        "Hiragino Sans GB".to_owned(),
        "Apple Color Emoji".to_owned(),
    ])
}

/// `el`, with the fallbacks for everything set in it.
pub fn with_fallbacks<E: Styled>(mut el: E) -> E {
    el.text_style()
        .get_or_insert_with(Default::default)
        .font_fallbacks = Some(fallbacks());
    el
}

/// `el`, set in serif, with her prose's fallbacks.
pub fn with_serif_fallbacks<E: Styled>(mut el: E) -> E {
    el.text_style()
        .get_or_insert_with(Default::default)
        .font_fallbacks = Some(serif_fallbacks());
    el
}

pub fn faces(cx: &App) -> &Faces {
    cx.global::<Faces>()
}
