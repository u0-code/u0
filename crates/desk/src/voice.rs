//! What you say, heard on this Mac by gpui-kit's speech: the system's
//! speech recognition, on the device alone, so nothing is sent away. It is
//! the system's, not a model of u0's own. Here, what you asked of what
//! is heard (your dictionary, what you took back, the fillers left out),
//! the languages it hears; and text read aloud with the system's voice.

use std::sync::OnceLock;

use gpui::component::speech::{SpeechLocale, SystemRecognizer};

use crate::settings::Voice;

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
/// words in place of what was heard for them, each sentence you took back
/// with your correction cue gone, and your filler words left out.
pub fn tidy(heard: &str, voice: &Voice) -> String {
    let (_, aliases) = dictionary(&voice.dictionary);
    let mut text = heard.trim().to_owned();
    for (from, to) in &aliases {
        text = replace_words(&text, from, to);
    }
    text = take_back(&text, &voice.correction_cue);
    if !voice.no_fillers {
        return text;
    }
    let fillers: Vec<String> = (voice.fillers.split(','))
        .map(|f| f.trim().to_lowercase())
        .filter(|f| !f.is_empty())
        .collect();
    let mut kept: Vec<String> = Vec::new();
    let mut capital = false;
    for (n, token) in text.split_whitespace().enumerate() {
        let core = token.trim_matches(|c: char| !c.is_alphanumeric());
        if !fillers.contains(&core.to_lowercase()) {
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

/// Where `from`, lower case, is found in `lower` from `at` on as whole
/// words.
fn find_words(lower: &str, from: &str, mut at: usize) -> Option<usize> {
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    while let Some(found) = lower[at..].find(from).map(|f| f + at) {
        let end = found + from.len();
        if !word(lower[..found].chars().last()) && !word(lower[end..].chars().next()) {
            return Some(found);
        }
        at = end;
    }
    None
}

/// `text` with each whole-word `from`, in any case, written `to`.
fn replace_words(text: &str, from: &str, to: &str) -> String {
    let lower = text.to_lowercase();
    let from = from.to_lowercase();
    // Lower-casing can change a text's length; then it is left as it is.
    if from.is_empty() || lower.len() != text.len() {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut at = 0;
    while let Some(found) = find_words(&lower, &from, at) {
        out.push_str(&text[at..found]);
        out.push_str(to);
        at = found + from.len();
    }
    out.push_str(&text[at..]);
    out
}

/// `text` with each `cue` said in it taken out, and the sentence said
/// before it: what you take back as you dictate ("scratch that").
fn take_back(text: &str, cue: &str) -> String {
    let cue = cue.trim().to_lowercase();
    let ends = ['.', '!', '?'];
    let mut text = text.to_owned();
    // Lower-casing can change a text's length; then it is left as it is.
    while !cue.is_empty() && text.to_lowercase().len() == text.len() {
        let Some(at) = find_words(&text.to_lowercase(), &cue, 0) else {
            break;
        };
        // The sentence before the cue, even one that ended as it was said.
        let said = text[..at].trim_end().trim_end_matches([',', '.', '!', '?']);
        let kept = said.rfind(ends).map_or("", |end| &text[..=end]);
        let rest = text[at + cue.len()..]
            .trim_start_matches(|c: char| c.is_whitespace() || c == ',' || ends.contains(&c));
        let mut rest = rest.to_owned();
        if let Some(first) = rest.chars().next() {
            rest = first.to_uppercase().chain(rest.chars().skip(1)).collect();
        }
        text = [kept.trim(), &rest]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }
    text
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
    fn the_correction_cue_takes_back_the_sentence_said_before_it() {
        let v = voice(&[], false);
        assert_eq!(
            tidy(
                "Fix the login bug. Rename it scratch that, rename the theme",
                &v
            ),
            "Fix the login bug. Rename the theme"
        );
        assert_eq!(
            tidy("Fix the login bug. Scratch that. Ship it", &v),
            "Ship it"
        );
        assert_eq!(tidy("scratch that", &v), "");
        assert_eq!(tidy("Don't scratch thatch", &v), "Don't scratch thatch");
        let off = Voice {
            correction_cue: String::new(),
            ..v
        };
        assert_eq!(
            tidy("Rename it scratch that", &off),
            "Rename it scratch that"
        );
    }

    #[test]
    fn a_dictionary_line_with_an_equals_sign_writes_what_is_heard_otherwise() {
        let (words, aliases) = dictionary(&[
            "u0".to_owned(),
            " gpui kit = gpui-kit ".to_owned(),
            String::new(),
        ]);
        assert_eq!(words, ["u0", "gpui-kit"]);
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
        // Your list is the one left out.
        let yours = Voice {
            fillers: "so, Then".into(),
            ..voice(&[], true)
        };
        assert_eq!(
            tidy(said, &yours),
            "Um, we should uh ship it, um. hmm test it"
        );
    }
}
