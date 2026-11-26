//! What you say, heard on this Mac by gpui-kit's speech: the system's
//! speech recognition, on the device alone, so nothing is sent away. It is
//! the system's, not a model of Ghost's own. Here, what you asked of what
//! is heard (your dictionary, the fillers left out), the languages it
//! hears; and text read aloud with the system's voice.

use std::sync::OnceLock;

use gpui::component::speech::{SpeechLocale, SystemRecognizer};

use crate::settings::Voice;

/// Words that are said while thinking, not meant: left out of what is
/// heard where you asked.
const FILLERS: [&str; 9] = ["um", "umm", "uh", "uhm", "er", "erm", "ah", "hmm", "mm"];

/// Your dictionary's lines: the words to listen for, and what to write for
/// what is heard ("heard = written"), whose written side is listened for
/// too.
pub fn dictionary(lines: &[String]) -> (Vec<String>, Vec<(String, String)>) {
    let mut words = Vec::new();
    let mut aliases = Vec::new();
    for line in lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        match line.split_once('=') {
            Some((heard, written)) if !heard.trim().is_empty() => {
                words.push(written.trim().to_owned());
                aliases.push((heard.trim().to_owned(), written.trim().to_owned()));
            }
            _ => words.push(line.to_owned()),
        }
    }
    (words, aliases)
}

/// What was `heard`, as you asked for it written: your dictionary's
/// words in place of what was heard for them, and the fillers left out.
pub fn tidy(heard: &str, voice: &Voice) -> String {
    let (_, aliases) = dictionary(&voice.dictionary);
    let mut text = heard.trim().to_owned();
    for (from, to) in &aliases {
        text = replace_words(&text, from, to);
    }
    if !voice.no_fillers {
        return text;
    }
    let mut kept: Vec<String> = Vec::new();
    let mut capital = false;
    for (n, token) in text.split_whitespace().enumerate() {
        let core = token.trim_matches(|c: char| !c.is_alphanumeric());
        if !FILLERS.contains(&core.to_lowercase().as_str()) {
            kept.push(token.to_owned());
            continue;
        }
        capital |= n == 0 && core.starts_with(char::is_uppercase);
        // A sentence that ended on the filler still ends.
        let end = token.chars().last().filter(|c| ".?!".contains(*c));
        if let (Some(end), Some(last)) = (end, kept.last_mut()) {
            let last = last.trim_end_matches(',');
            *kept.last_mut().expect("there is a last") = format!("{last}{end}");
        }
    }
    let mut text = kept.join(" ");
    if capital && let Some(first) = text.chars().next() {
        text = first.to_uppercase().chain(text.chars().skip(1)).collect();
    }
    text
}

/// `text` with each whole-word `from`, in any case, written `to`.
fn replace_words(text: &str, from: &str, to: &str) -> String {
    let lower = text.to_lowercase();
    let from = from.to_lowercase();
    // Lower-casing can change a text's length; then it is left as it is.
    if from.is_empty() || lower.len() != text.len() {
        return text.to_owned();
    }
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let mut out = String::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find(&from).map(|f| f + at) {
        let end = found + from.len();
        if word(lower[..found].chars().last()) || word(lower[end..].chars().next()) {
            out.push_str(&text[at..end]);
        } else {
            out.push_str(&text[at..found]);
            out.push_str(to);
        }
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

/// The languages speech recognition hears on this Mac alone, by name in
/// yours: read once, as reading them makes a recognizer for each.
pub fn languages() -> &'static [SpeechLocale] {
    static ALL: OnceLock<Vec<SpeechLocale>> = OnceLock::new();
    ALL.get_or_init(SystemRecognizer::supported_locales)
}

/// The system's voice, which reads aloud.
#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2_avf_audio::{AVSpeechBoundary, AVSpeechSynthesizer, AVSpeechUtterance};
    use objc2_foundation::NSString;

    thread_local! {
        /// The system's voice, made once on the main thread.
        static VOICE: Retained<AVSpeechSynthesizer> =
            // SAFETY: a plain object made on the thread that uses it.
            unsafe { AVSpeechSynthesizer::new() };
    }

    /// Read `text` aloud in the system's voice, in place of what it read.
    pub fn speak(text: &str) {
        VOICE.with(|voice| {
            // SAFETY: the synthesizer made on this thread, and an
            // utterance made here.
            unsafe {
                voice.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate);
                voice.speakUtterance(&AVSpeechUtterance::speechUtteranceWithString(
                    &NSString::from_str(text),
                ));
            }
        });
    }

    /// Stop reading aloud.
    pub fn stop_speaking() {
        // SAFETY: the synthesizer made on this thread.
        VOICE.with(|voice| unsafe { voice.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate) });
    }
}

#[cfg(target_os = "macos")]
pub use mac::{speak, stop_speaking};

/// Where there is no system voice, nothing is read aloud.
#[cfg(not(target_os = "macos"))]
pub fn speak(_: &str) {}

#[cfg(not(target_os = "macos"))]
pub fn stop_speaking() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(lines: &[&str], no_fillers: bool) -> Voice {
        Voice {
            dictionary: lines.iter().map(|l| (*l).to_owned()).collect(),
            no_fillers,
            ..Voice::default()
        }
    }

    #[test]
    fn a_dictionary_line_with_an_equals_sign_writes_what_is_heard_otherwise() {
        let (words, aliases) = dictionary(&[
            "Ghost".to_owned(),
            " gpui kit = gpui-kit ".to_owned(),
            String::new(),
        ]);
        assert_eq!(words, ["Ghost", "gpui-kit"]);
        assert_eq!(aliases, [("gpui kit".to_owned(), "gpui-kit".to_owned())]);
        let v = voice(&["gpui kit = gpui-kit"], false);
        assert_eq!(
            tidy("Use GPUI Kit for it, not gpui kitchen", &v),
            "Use gpui-kit for it, not gpui kitchen"
        );
    }

    #[test]
    fn fillers_are_left_out_only_where_asked_and_a_sentence_still_ends() {
        let said = "Um, so we should uh ship it, um. Then hmm test it";
        assert_eq!(
            tidy(said, &voice(&[], true)),
            "So we should ship it. Then test it"
        );
        assert_eq!(tidy(said, &voice(&[], false)), said);
        assert_eq!(tidy("um", &voice(&[], true)), "");
    }
}
