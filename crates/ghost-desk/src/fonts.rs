//! Three faces: a sans for the interface, a mono for the journal and every
//! number, and a serif for her own words and the one headline on a page.
//!
//! The faces the design is set in (IBM Plex Sans, IBM Plex Mono, Instrument
//! Serif) are open fonts, loaded from `assets/fonts/` when they are there.
//! Until they are, each falls back to a face macOS always has, so the desk
//! still reads as three voices.

use std::{borrow::Cow, path::PathBuf};

use gpui::{App, Global, SharedString};

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
        serif: pick(&["Instrument Serif", "New York", "Georgia"]),
    });
}

pub fn faces(cx: &App) -> &Faces {
    cx.global::<Faces>()
}
