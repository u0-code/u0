//! Dictation's settings, in the Chat tab: how the key listens, the
//! language, your dictionary, filler words and clean-up.

use gpui::component::setting::{SettingField, SettingGroup, SettingItem};
use gpui::{IntoElement, SharedString, WeakEntity};

use super::prefs::{group, item};
use super::writing::instructions;
use crate::{desk::Desk, settings::VoiceKey, voice};

impl Desk {
    /// The dictation group.
    pub(super) fn dictation_group(w: &WeakEntity<Desk>) -> SettingGroup {
        group(
            crate::tr("Dictation"),
            vec![
                item(
                    w,
                    crate::tr("The dictation key (⇧⌘D)"),
                    crate::tr(
                        "Auto: press or hold. Toggle: press to start and stop. Hold: hold to talk.",
                    ),
                    |d, _, cx| {
                        d.pick(
                            "voice-key",
                            [
                                (crate::tr("Auto"), VoiceKey::Auto),
                                (crate::tr("Toggle"), VoiceKey::Toggle),
                                (crate::tr("Hold"), VoiceKey::Hold),
                            ],
                            d.settings.voice.key,
                            |s, v| s.voice.key = v,
                            cx,
                        )
                        .into_any_element()
                    },
                ),
                Self::language_item(w),
                item(
                    w,
                    crate::tr("Filler words"),
                    crate::tr("“Um”, “uh” and the like."),
                    |d, _, cx| {
                        d.pick(
                            "voice-fillers",
                            [(crate::tr("Left out"), true), (crate::tr("Kept"), false)],
                            d.settings.voice.no_fillers,
                            |s, v| s.voice.no_fillers = v,
                            cx,
                        )
                        .into_any_element()
                    },
                ),
                item(
                    w,
                    crate::tr("Clean up"),
                    crate::tr(
                        "The writer fixes mishearings and punctuation; Skip inserts it as heard.",
                    ),
                    |d, _, cx| {
                        d.pick(
                            "voice-clean",
                            [
                                (crate::tr("As heard"), false),
                                (crate::tr("Cleaned up"), true),
                            ],
                            d.settings.voice.clean,
                            |s, v| s.voice.clean = v,
                            cx,
                        )
                        .into_any_element()
                    },
                ),
                instructions(
                    w,
                    "Dictionary",
                    "Names and terms, one per line; “heard = written” to correct.",
                    |d| &d.pages.dictionary,
                )
                .keywords(["words", "dictation"]),
            ],
        )
    }

    /// The language dictation listens for, among those macOS hears: as the
    /// system's (none), or one chosen.
    fn language_item(w: &WeakEntity<Desk>) -> SettingItem {
        let mut choices = vec![(SharedString::default(), crate::tr("As the system"))];
        choices.extend(voice::languages().iter().map(|l| {
            (
                SharedString::from(l.tag().to_string()),
                SharedString::from(l.name().to_string()),
            )
        }));
        let (read, write) = (w.clone(), w.clone());
        SettingItem::new(
            crate::tr("Language"),
            SettingField::scrollable_dropdown(
                choices,
                move |cx| {
                    let now = read
                        .upgrade()
                        .and_then(|d| d.read(cx).settings.voice.language.clone());
                    now.map(SharedString::from).unwrap_or_default()
                },
                move |tag, cx| {
                    let _ = write.update(cx, |d, cx| {
                        d.settings.voice.language = (!tag.is_empty()).then(|| tag.to_string());
                        d.save_settings();
                        cx.notify();
                    });
                },
            ),
        )
        .description(crate::tr(
            "Recognized on this Mac only; nothing is sent away.",
        ))
    }
}
