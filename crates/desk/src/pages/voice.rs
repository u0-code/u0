//! Dictation's settings, in the Chat tab: how the key listens, the
//! language, your dictionary, filler words and clean-up.

use gpui::component::{Selectable as _, scroll::ScrollableElement as _};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::prefs::{group, row, said};
use super::writing::instructions;
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    settings::VoiceKey,
    ui, voice,
};

impl Desk {
    /// The dictation group; none when what is looked for, `q`, is in none
    /// of it.
    pub(super) fn dictation_group(
        &self,
        faces: &Faces,
        q: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let v = &self.settings.voice;
        group(
            faces,
            "Dictation",
            vec![
                row(
                    faces,
                    q,
                    "The dictation key (⇧⌘D)",
                    "Auto: a press begins and a press ends, or hold it while you talk. Toggle: a press begins, a press ends. Hold: it listens while held. Outside the composer, macOS's dictation types instead.",
                    Self::pick(
                        "voice-key",
                        [
                            ("Auto", VoiceKey::Auto),
                            ("Toggle", VoiceKey::Toggle),
                            ("Hold", VoiceKey::Hold),
                        ],
                        v.key,
                        |s, v| s.voice.key = v,
                        cx,
                    ),
                ),
                row(
                    faces,
                    q,
                    "Language",
                    "Heard by macOS's speech recognition on this Mac alone: a language it can't hear on the device is not sent away.",
                    self.language_pick(faces, cx),
                ),
                row(
                    faces,
                    q,
                    "Filler words",
                    "“Um”, “uh” and the like.",
                    Self::pick(
                        "voice-fillers",
                        [("Left out", true), ("Kept", false)],
                        v.no_fillers,
                        |s, v| s.voice.no_fillers = v,
                        cx,
                    ),
                ),
                row(
                    faces,
                    q,
                    "Clean up",
                    "The writer corrects what was misheard and punctuates it before it goes in; Skip puts it in as heard.",
                    Self::pick(
                        "voice-clean",
                        [("As heard", false), ("Cleaned up", true)],
                        v.clean,
                        |s, v| s.voice.clean = v,
                        cx,
                    ),
                ),
                said(q, &["Dictionary", "words", "dictation"]).then(|| {
                    instructions(
                        faces,
                        "Dictionary",
                        "Names and terms to listen for, one to a line; “heard = written” writes what is heard otherwise.",
                        &self.pages.dictionary,
                    )
                }),
            ],
        )
    }

    /// The language dictation listens for, and the menu of those macOS
    /// hears.
    fn language_pick(&self, faces: &Faces, cx: &mut Context<Self>) -> impl IntoElement {
        let now = self.settings.voice.language.clone();
        let shown = now.as_ref().map_or_else(
            || "As the system".to_owned(),
            |id| {
                voice::languages()
                    .iter()
                    .find(|l| l.tag() == id)
                    .map_or_else(|| id.clone(), |l| l.name().to_string())
            },
        );
        let trigger = ui::opener("voice-language")
            .selected(self.popup == Popup::Language)
            .child(shown);
        let faces = faces.clone();
        self.menu(
            Popup::Language,
            trigger,
            move |d, _, cx| {
                let now = d.settings.voice.language.clone();
                let pick = |id: Option<String>, name: String, cx: &mut Context<Self>| {
                    let on = id == now;
                    ui::menu_row(
                        SharedString::from(format!("language-{}", id.as_deref().unwrap_or(""))),
                        on,
                    )
                    .child(ui::one_line(ui::text(&faces, name).text_size(px(13.))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings.voice.language.clone_from(&id);
                        this.save_settings();
                        this.popup = Popup::Closed;
                        cx.notify();
                    }))
                };
                let mut list = div()
                    .id("language-menu")
                    .flex()
                    .flex_col()
                    .w(px(260.))
                    .max_h(px(420.))
                    .child(pick(None, "As the system".to_owned(), cx));
                for l in voice::languages() {
                    list = list.child(pick(Some(l.tag().to_string()), l.name().to_string(), cx));
                }
                list.overflow_y_scrollbar()
            },
            cx,
        )
    }
}
