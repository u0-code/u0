//! Dictation's settings, in the Chat tab: how the key listens, the
//! language, your dictionary, filler words and clean-up.

use gpui::component::{Selectable as _, scroll::ScrollableElement as _, setting::SettingGroup};
use gpui::{Context, IntoElement, SharedString, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item};
use super::writing::instructions;
use crate::{
    desk::{Desk, Popup},
    fonts::Faces,
    settings::VoiceKey,
    ui, voice,
};

impl Desk {
    /// The dictation group.
    pub(super) fn dictation_group(w: &WeakEntity<Desk>) -> SettingGroup {
        group("Dictation", vec![
                item(w, "The dictation key (⇧⌘D)", "Auto: a press begins and a press ends, or hold it while you talk. Toggle: a press begins, a press ends. Hold: it listens while held. Outside the composer, macOS's dictation types instead.", |d, _, cx| Self::pick(
                        "voice-key",
                        [
                            ("Auto", VoiceKey::Auto),
                            ("Toggle", VoiceKey::Toggle),
                            ("Hold", VoiceKey::Hold),
                        ],
                        d.settings.voice.key,
                        |s, v| s.voice.key = v,
                        cx,
                    ).into_any_element()),
                item(w, "Language", "Heard by macOS's speech recognition on this Mac alone: a language it can't hear on the device is not sent away.", |d, faces, cx| d.language_pick(faces, cx).into_any_element()),
                item(w, "Filler words", "“Um”, “uh” and the like.", |d, _, cx| Self::pick(
                        "voice-fillers",
                        [("Left out", true), ("Kept", false)],
                        d.settings.voice.no_fillers,
                        |s, v| s.voice.no_fillers = v,
                        cx,
                    ).into_any_element()),
                item(w, "Clean up", "The writer corrects what was misheard and punctuates it before it goes in; Skip puts it in as heard.", |d, _, cx| Self::pick(
                        "voice-clean",
                        [("As heard", false), ("Cleaned up", true)],
                        d.settings.voice.clean,
                        |s, v| s.voice.clean = v,
                        cx,
                    ).into_any_element()),
                instructions(
                    w,
                    "Dictionary",
                    "Names and terms to listen for, one to a line; “heard = written” writes what is heard otherwise.",
                    |d| &d.pages.dictionary,
                )
                .keywords(["words", "dictation"]),
            ])
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
