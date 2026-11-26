//! Dictating into the composer: listening on ⇧⌘D or the microphone,
//! what is heard shown as it comes, put in once it ends (cleaned up by
//! the writer where you asked); and reading aloud.

use std::time::{Duration, Instant};

use gpui::component::speech::{
    SpeechError, SpeechEvent, SpeechState, SpeechStatus, SystemRecognizer,
};
use gpui::{AppContext as _, Context, Entity, Subscription, Window};

use super::Desk;
use crate::{settings::VoiceKey, voice};

/// Dictation under way.
pub struct Dictation {
    /// The system's speech recognition, hearing till it is finished.
    pub speech: Entity<SpeechState>,
    _heard: Subscription,
    /// All heard so far.
    pub heard: String,
    /// When it began, which tells a tap of the key from a hold.
    since: Instant,
    /// The key that began it is still down.
    held: bool,
    /// What was heard is with the writer to be cleaned up.
    pub cleaning: Option<String>,
}

/// Held this long, the dictation key listens till it is let go.
const HOLD: Duration = Duration::from_millis(400);

impl Desk {
    /// The dictation key, or the microphone: begin listening for the
    /// composer, or finish. Elsewhere than the composer, macOS's own
    /// dictation types into the field that has the keys.
    pub(crate) fn dictate(&mut self, by_key: bool, window: &mut Window, cx: &mut Context<Self>) {
        let composer = gpui::Focusable::focus_handle(self.composer.input.read(cx), cx);
        if self.dictation.is_none() && by_key && !composer.contains_focused(window, cx) {
            crate::macos::dictate();
            return;
        }
        match &mut self.dictation {
            // A key held down repeats; only a new press finishes.
            Some(d) if by_key && d.held => {}
            Some(_) => self.finish_dictation(cx),
            None => {
                window.focus(&composer, cx);
                let v = &self.settings.voice;
                let (words, _) = voice::dictionary(&v.dictionary);
                let mut recognizer = SystemRecognizer::new().expected_phrases(words);
                if let Some(language) = &v.language {
                    recognizer = recognizer.locale(language.clone());
                }
                let speech = cx.new(|cx| SpeechState::new(cx).recognizer(recognizer));
                let heard = cx.subscribe_in(&speech, window, |this, _, e, window, cx| {
                    this.heard(e, window, cx);
                });
                self.dictation = Some(Dictation {
                    speech: speech.clone(),
                    _heard: heard,
                    heard: String::new(),
                    since: Instant::now(),
                    held: by_key,
                    cleaning: None,
                });
                speech.update(cx, SpeechState::start);
            }
        }
        cx.notify();
    }

    /// The dictation key let go: where it was held to talk, that is the
    /// end.
    pub(crate) fn dictate_key_up(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &mut self.dictation else {
            return;
        };
        let was_held = std::mem::replace(&mut d.held, false);
        let hold = match self.settings.voice.key {
            VoiceKey::Hold => true,
            VoiceKey::Auto => d.since.elapsed() >= HOLD,
            VoiceKey::Toggle => false,
        };
        if was_held && hold {
            self.finish_dictation(cx);
        }
    }

    fn not_allowed(&mut self) {
        self.dictation = None;
        self.trouble(
            "Ghost may not use speech recognition",
            Some(
                "Allow it in System Settings › Privacy & Security › Speech Recognition, \
                 and the microphone under Microphone."
                    .to_owned(),
            ),
            None,
        );
    }

    /// Stop listening: what was heard goes in once it ends. Asked again,
    /// or before it listened, it is let go.
    pub(crate) fn finish_dictation(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.dictation else { return };
        let speech = d.speech.clone();
        if speech.read(cx).status() == SpeechStatus::Recording {
            speech.update(cx, SpeechState::stop);
        } else {
            speech.update(cx, SpeechState::cancel);
            self.dictation = None;
        }
        cx.notify();
    }

    /// Stop listening and let what was heard go; or, while the writer
    /// cleans it up, put it in as it was heard.
    pub(crate) fn cancel_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.dictation.take() {
            d.speech.update(cx, SpeechState::cancel);
            if let Some(heard) = d.cleaning {
                self.put_dictated(&heard, window, cx);
            }
        }
        cx.notify();
    }

    /// What speech recognition said.
    fn heard(&mut self, said: &SpeechEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &mut self.dictation else {
            return;
        };
        let heard = match said {
            SpeechEvent::Started | SpeechEvent::Cancelled => return,
            SpeechEvent::Partial(words) => {
                d.heard = words.to_string();
                cx.notify();
                return;
            }
            SpeechEvent::Final(text) => text.to_string(),
            SpeechEvent::Error(SpeechError::PermissionDenied) => return self.not_allowed(),
            // What was heard before a fault is kept.
            SpeechEvent::Error(_) if !d.heard.is_empty() => d.heard.clone(),
            SpeechEvent::Error(why) => {
                let why = match why {
                    SpeechError::Unsupported => "macOS can't hear this language on this Mac \
                        alone: choose another, or add it under System Settings › Keyboard › \
                        Dictation. Ghost run outside Ghost.app (from cargo) can't listen at all."
                        .to_owned(),
                    why => why.to_string(),
                };
                self.dictation = None;
                self.trouble("Ghost couldn't hear that", Some(why), None);
                cx.notify();
                return;
            }
        };
        let text = voice::tidy(&heard, &self.settings.voice);
        match self.current {
            Some(key) if self.settings.voice.clean && !text.is_empty() => {
                d.cleaning = Some(text.clone());
                self.ask_writer(key, crate::writer::Ask::Transcript { heard: text });
            }
            _ => {
                self.dictation = None;
                self.put_dictated(&text, window, cx);
            }
        }
        cx.notify();
    }

    /// The writer's clean copy of `heard`, put in unless you went on
    /// without it.
    pub(super) fn cleaned(
        &mut self,
        heard: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .dictation
            .as_ref()
            .is_some_and(|d| d.cleaning.as_deref() == Some(heard))
        {
            self.dictation = None;
            self.put_dictated(if text.is_empty() { heard } else { text }, window, cx);
        }
    }

    /// `text` typed in the composer where its cursor is, a space before it
    /// where it follows a word.
    fn put_dictated(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        self.composer.input.update(cx, |i, cx| {
            let at = i.selected_range().start;
            let before = i.value();
            let space = before[..at.min(before.len())]
                .chars()
                .last()
                .is_some_and(|c| !c.is_whitespace());
            i.insert(
                if space {
                    format!(" {text}")
                } else {
                    text.to_owned()
                },
                window,
                cx,
            );
        });
    }

    /// Read aloud what is chosen in the composer, else the agent's last
    /// answer in the session on screen.
    pub(crate) fn read_aloud(&self, cx: &Context<Self>) {
        let chosen = self.composer.input.read(cx).selected_value().to_string();
        let text = if chosen.trim().is_empty() {
            self.session()
                .and_then(|s| {
                    s.items.iter().rev().find_map(|i| match i {
                        crate::session::Item::Agent(t) => Some(t.clone()),
                        _ => None,
                    })
                })
                .unwrap_or_default()
        } else {
            chosen
        };
        if !text.trim().is_empty() {
            voice::speak(&text);
        }
    }
}
