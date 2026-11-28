//! Settings › Voice (`SetVoice.dc.html`): dictation by the system's speech
//! recognition on this Mac, how its key listens and the language it hears;
//! what is done to a transcript before it goes in (fillers, the correction
//! cue, the writer); and your dictionary.

use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use u0ui::{
    components::{ButtonVariant, button::ButtonSize},
    t3::workspace::{KeybindingPill, settings::SettingsSection},
    u0::settings::{DictionaryEditor, DictionaryTexts},
};

use super::srow::{SGroup, SRow, carbon_page_with_tail};
use crate::{
    desk::Desk,
    fonts::Faces,
    settings::{Voice, VoiceKey},
    voice,
};

impl Desk {
    /// The Voice page, found among the settings by `query`.
    pub(super) fn voice_tab(
        &self,
        query: &str,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let defaults = Voice::default();
        let v = &self.settings.voice;
        let dictation = SGroup::new(
            crate::tr("Dictation"),
            vec![
                self.language_srow(cx),
                self.dictation_key_srow(cx),
                self.global_segmented_srow(
                    "voice.key",
                    "How the key works",
                    "Auto: hold to talk, or tap to start and tap again to stop. Esc cancels.",
                    [
                        ("Auto", VoiceKey::Auto),
                        ("Toggle", VoiceKey::Toggle),
                        ("Hold", VoiceKey::Hold),
                    ],
                    v.key,
                    defaults.key,
                    |s, k| s.voice.key = k,
                    false,
                    cx,
                ),
                self.voice_switch(
                    "voice.live_words",
                    "Show words while you speak",
                    "Off, the composer says it is listening until you stop.",
                    |v| &mut v.live_words,
                    cx,
                ),
            ],
        )
        .description(crate::tr(
            "Apple’s speech recognition on this Mac. Nothing is downloaded, and no audio is sent anywhere.",
        ));
        let cleanup = SGroup::new(
            crate::tr("Cleanup"),
            vec![
                self.voice_switch(
                    "voice.no_fillers",
                    "Remove filler words",
                    "Done on this Mac, by the list below.",
                    |v| &mut v.no_fillers,
                    cx,
                ),
                self.voice_text(
                    "voice.fillers",
                    "Fillers",
                    "Words taken out, commas between.",
                    &self.pages.fillers,
                    faces,
                ),
                self.voice_text(
                    "voice.correction_cue",
                    "Correction cue",
                    "Say this to take back the sentence you just said.",
                    &self.pages.cue,
                    faces,
                ),
                self.voice_switch(
                    "voice.clean",
                    "Tidy with the writer",
                    "The writer fixes punctuation and names. Off puts in what was heard.",
                    |v| &mut v.clean,
                    cx,
                ),
                self.writer_srow(faces, cx),
            ],
        )
        .description(crate::tr(
            "What happens to a transcript before it lands in the composer.",
        ));
        let dictionary = self.dictionary_section(query, cx);
        carbon_page_with_tail(
            "settings-voice",
            super::Tab::Voice.title(),
            crate::tr(
                "Dictate prompts with Apple’s speech recognition, on this Mac. Audio never leaves it; there is nothing to download.",
            ),
            self.carbon_settings_banner(super::Tab::Voice, cx),
            vec![dictation, cleanup],
            dictionary,
            query,
            faces,
        )
    }

    /// A switch for one of dictation's settings.
    fn voice_switch(
        &self,
        id: &'static str,
        title: &'static str,
        about: &'static str,
        of: fn(&mut Voice) -> &mut bool,
        cx: &mut Context<Self>,
    ) -> SRow {
        let now = *of(&mut self.settings.voice.clone());
        let default = *of(&mut Voice::default());
        let (set, reset) = (cx.entity(), cx.entity());
        self.srow(id, crate::tr(title), crate::tr(about))
            .modified(now != default, move |_, cx| {
                reset.update(cx, |d, cx| {
                    *of(&mut d.settings.voice) = default;
                    d.save_settings();
                    cx.notify();
                });
            })
            .switch(now, move |on, _, cx| {
                set.update(cx, |d, cx| {
                    *of(&mut d.settings.voice) = on;
                    d.save_settings();
                    cx.notify();
                });
            })
    }

    /// A line for one of dictation's settings, kept as it is typed.
    fn voice_text(
        &self,
        id: &'static str,
        title: &'static str,
        about: &'static str,
        field: &gpui::Entity<gpui::component::input::InputState>,
        faces: &Faces,
    ) -> SRow {
        self.srow(id, crate::tr(title), crate::tr(about))
            .text_input(field, faces, false)
    }

    /// The language dictation listens for, among those macOS hears: as the
    /// system's (none), or one chosen.
    fn language_srow(&self, cx: &mut Context<Self>) -> SRow {
        let mut choices = vec![(crate::tr("As the system"), None)];
        choices.extend(voice::languages().iter().map(|l| {
            (
                SharedString::from(l.name().to_string()),
                Some(l.tag().to_string()),
            )
        }));
        let now = self.settings.voice.language.clone();
        let (pick, reset) = (cx.entity(), cx.entity());
        self.srow(
            "voice.language",
            crate::tr("Language"),
            crate::tr(
                "The languages macOS has for dictation; add one in System Settings › Keyboard.",
            ),
        )
        .modified(now.is_some(), move |_, cx| {
            reset.update(cx, |d, cx| {
                d.settings.voice.language = None;
                d.save_settings();
                cx.notify();
            });
        })
        .select(choices, &now, move |tag, _, cx| {
            pick.update(cx, |d, cx| {
                d.settings.voice.language = tag;
                d.save_settings();
                cx.notify();
            });
        })
    }

    /// The dictation key as it is bound now, and the way to change it.
    fn dictation_key_srow(&self, cx: &mut Context<Self>) -> SRow {
        let keys = crate::keys::bound(cx, &self.keys_yours)
            .into_iter()
            .find(|b| b.name == "dictate")
            .map(|b| b.keys)
            .unwrap_or_default();
        let (desk, now) = (cx.entity(), keys.clone());
        let change = u0ui::components::Button::new("voice-key-change", crate::tr("Change"))
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Sm)
            .on_click(move |_, window, cx| {
                let keys = now.clone();
                desk.update(cx, |d, cx| {
                    d.pick_settings_tab(super::Tab::Keybindings, window, cx);
                    d.pages.editing_keys = Some(("dictate", keys.into()));
                    d.pages
                        .keys_when
                        .update(cx, |i, cx| i.set_value("", window, cx));
                    cx.notify();
                });
            });
        let shown = (!keys.is_empty()).then(|| KeybindingPill::new(keys));
        self.srow(
            "voice.dictation_key",
            crate::tr("Dictation key"),
            crate::tr(
                "Elsewhere than the composer, macOS's own dictation types where the keys are.",
            ),
        )
        .control(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .children(shown)
                .child(change),
        )
    }

    /// The writer that tidies a transcript: chosen in one place, Writing.
    fn writer_srow(&self, faces: &Faces, cx: &mut Context<Self>) -> SRow {
        let desk = cx.entity();
        self.srow(
            "voice.writer",
            crate::tr("Writer"),
            crate::tr("Chosen in one place, Writing."),
        )
        .control(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    crate::ui::text(faces, self.writer_shown()).text_size(px(crate::style::BODY)),
                )
                .child(
                    u0ui::components::Button::new("voice-writer", crate::tr("Change in Writing"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            desk.update(cx, |d, cx| {
                                d.pick_settings_tab(super::Tab::Writing, window, cx);
                            });
                        }),
                ),
        )
    }

    /// Your dictionary: words dictation listens for, a "heard = written"
    /// writing what is heard otherwise.
    fn dictionary_section(&self, query: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let title = crate::tr("Dictionary");
        let about = crate::tr(
            "Names dictation should spell your way; “heard = written” writes what is heard otherwise.",
        );
        let q = query.to_lowercase();
        if !q.is_empty() && !title.to_lowercase().contains(&q) && !about.to_lowercase().contains(&q)
        {
            return None;
        }
        let w = cx.entity().downgrade();
        let editor = DictionaryEditor::new("voice-dictionary")
            .label(crate::tr("Your words"))
            .words(self.settings.voice.dictionary.iter().cloned())
            .texts(DictionaryTexts {
                placeholder: crate::tr("Add a word or name dictation should know"),
                input: crate::tr("New word"),
                add: crate::tr("Add"),
                remove: std::rc::Rc::new(|w| crate::tr_with("Remove %{word}", &[("word", &w)])),
                count: std::rc::Rc::new(|n| crate::tr_with("%{n} words", &[("n", &n.to_string())])),
            })
            .on_change(move |words, _, cx| {
                let _ = w.update(cx, |d, cx| {
                    d.settings.voice.dictionary = words.iter().map(ToString::to_string).collect();
                    d.save_settings();
                    cx.notify();
                });
            });
        Some(
            SettingsSection::new(title)
                .description(about)
                .child(div().px(px(20.)).py(px(16.)).child(editor))
                .into_any_element(),
        )
    }
}
