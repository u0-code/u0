//! Dictating into the composer: listening on ⇧⌘D or the microphone,
//! what is heard shown as it comes, put in once it ends (cleaned up by
//! the writer where you asked); and reading aloud.

use std::time::{Duration, Instant};

use gpui::{Context, Window};

use super::Desk;
use crate::{settings::VoiceKey, voice, voice::Heard};

/// Dictation under way.
pub struct Dictation {
    /// Listening, till it is finished; none while permission is asked or
    /// once what was heard is ending.
    pub listening: Option<voice::Listening>,
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
                self.dictation = Some(Dictation {
                    listening: None,
                    heard: String::new(),
                    since: Instant::now(),
                    held: by_key,
                    cleaning: None,
                });
                match voice::allowed() {
                    Some(true) => self.listen(cx),
                    Some(false) => self.not_allowed(),
                    None => voice::ask(self.tx.clone()),
                }
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

    fn listen(&mut self, cx: &mut Context<Self>) {
        let v = &self.settings.voice;
        let (words, _) = voice::dictionary(&v.dictionary);
        match voice::listen(v.language.as_deref(), &words, self.tx.clone()) {
            Ok(listening) => {
                if let Some(d) = &mut self.dictation {
                    d.listening = Some(listening);
                }
            }
            Err(why) => {
                self.dictation = None;
                self.trouble("Ghost couldn't listen", Some(why), None);
            }
        }
        cx.notify();
    }

    fn not_allowed(&mut self) {
        self.dictation = None;
        self.trouble(
            "Ghost may not use speech recognition",
            Some(
                "Allow it in System Settings › Privacy & Security › Speech Recognition, \
                 and the microphone under Microphone. Ghost run outside Ghost.app (from \
                 cargo) may not listen at all."
                    .to_owned(),
            ),
            None,
        );
    }

    /// Stop listening: what was heard goes in once it ends.
    pub(crate) fn finish_dictation(&mut self, cx: &mut Context<Self>) {
        match self.dictation.as_mut().map(|d| d.listening.take()) {
            Some(Some(listening)) => listening.finish(),
            // Still asked for permission: nothing was heard.
            Some(None) => self.dictation = None,
            None => {}
        }
        cx.notify();
    }

    /// Stop listening and let what was heard go; or, while the writer
    /// cleans it up, put it in as it was heard.
    pub(crate) fn cancel_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.dictation.take() {
            if let Some(listening) = d.listening {
                listening.cancel();
            }
            if let Some(heard) = d.cleaning {
                self.put_dictated(&heard, window, cx);
            }
        }
        cx.notify();
    }

    /// What speech recognition said.
    pub(super) fn heard(&mut self, heard: Heard, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &mut self.dictation else {
            return;
        };
        match heard {
            Heard::Allowed(true) => self.listen(cx),
            Heard::Allowed(false) => self.not_allowed(),
            Heard::Words(words) => d.heard = words,
            Heard::Ended(ended) => {
                // Nothing said is no news; what was heard before a fault
                // is kept.
                let heard = match ended {
                    Ok(text) => text,
                    Err(_) if !d.heard.is_empty() || d.listening.is_none() => d.heard.clone(),
                    Err(why) => {
                        self.dictation = None;
                        self.trouble("Ghost couldn't hear that", Some(why), None);
                        cx.notify();
                        return;
                    }
                };
                if let Some(listening) = d.listening.take() {
                    listening.cancel();
                }
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
