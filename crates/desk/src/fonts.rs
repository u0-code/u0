//! Font tokens and CJK fallbacks, backed by `u0ui`'s bundled IBM Plex
//! faces (`IBM Plex Sans`, `IBM Plex Mono`, `IBM Plex Serif` and
//! `u0 Inline Mono`, registered by `u0ui::init`).
//!
//! Chinese and Japanese text falls back through Carbon's stack (`PingFang SC`
//! at the weight of the surrounding run, then `Hiragino Sans GB` and
//! `Noto Sans CJK SC`, and `Apple Color Emoji`).

use gpui::{App, FontFallbacks, FontWeight, Global, SharedString, Styled};
use u0ui::tokens::FontFamilyKey;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Faces {
    pub ui: SharedString,
    pub mono: SharedString,
}

impl Global for Faces {}

/// Install the default Carbon font families (`IBM Plex Sans` and
/// `IBM Plex Mono`). Call after `u0ui::init(cx)` has registered the bundled
/// font files.
pub fn load(cx: &mut App) {
    cx.set_global(Faces {
        ui: FontFamilyKey::Sans.primary().into(),
        mono: FontFamilyKey::Mono.primary().into(),
    });
}

/// The fallback stack for characters outside IBM Plex: Chinese in
/// `PingFang SC` at the weight of the surrounding text (so semibold headings
/// keep semibold CJK glyphs on macOS), then `Hiragino Sans GB`,
/// `Noto Sans CJK SC` and `Apple Color Emoji`.
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
        "Noto Sans CJK SC".to_owned(),
        "Apple Color Emoji".to_owned(),
    ])
}

/// `el`, with the CJK and emoji fallbacks set on it.
pub fn with_fallbacks<E: Styled>(mut el: E) -> E {
    el.text_style().font_fallbacks = Some(fallbacks(FontWeight::NORMAL));
    el
}

/// Setting text's weight sets its CJK fallback weight with it.
pub trait Weighted: Styled + Sized {
    fn weight(mut self, weight: FontWeight) -> Self {
        let style = self.text_style();
        style.font_weight = Some(weight);
        style.font_fallbacks = Some(fallbacks(weight));
        self
    }
}

impl<E: Styled> Weighted for E {}

/// Set code in `mono`, where this Mac has it (bundled `IBM Plex Mono` is
/// always registered by `u0ui::init`); else keep `IBM Plex Mono`.
pub fn set_mono(cx: &mut App, mono: crate::settings::CodeFont) {
    let wanted = mono.family();
    let chosen = if wanted == FontFamilyKey::Mono.primary()
        || cx
            .text_system()
            .all_font_names()
            .iter()
            .any(|n| n == wanted)
    {
        wanted
    } else {
        FontFamilyKey::Mono.primary()
    };
    let mut current = faces(cx).clone();
    current.mono = chosen.into();
    cx.set_global(current);
}

pub fn faces(cx: &App) -> &Faces {
    cx.global::<Faces>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carbon_plex_families_and_cjk_fallbacks_are_configured() {
        assert_eq!(FontFamilyKey::Sans.primary(), "IBM Plex Sans");
        assert_eq!(FontFamilyKey::Mono.primary(), "IBM Plex Mono");
        assert_eq!(u0ui::typography::FONT_FILES.len(), 8);
        let bold = fallbacks(FontWeight::SEMIBOLD);
        assert_eq!(bold.fallback_list()[0], "PingFangSC-Semibold");
        let normal = fallbacks(FontWeight::NORMAL);
        assert_eq!(normal.fallback_list()[0], "PingFangSC-Regular");
    }
}
