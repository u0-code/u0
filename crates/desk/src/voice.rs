//! What you say, heard on this Mac: the microphone through `AVAudioEngine`
//! into Apple's speech recognition (`SFSpeechRecognizer`), on the device
//! alone, so nothing is sent away. It is the
//! system's, not a model of Ghost's own. What it hears comes back to the
//! window as `Msg::Heard`, put in the composer once it ends; and Ghost
//! reads text aloud with the system's voice.

use crate::hub::{Msg, Sender};
use crate::settings::Voice;

/// What speech recognition says, as it hears.
pub enum Heard {
    /// Whether you let Ghost use speech recognition, once asked.
    Allowed(bool),
    /// All heard so far.
    Words(String),
    /// It ended: all it heard, or why it could not hear.
    Ended(Result<String, String>),
}

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

#[cfg(target_os = "macos")]
pub use mac::{Listening, allowed, ask, languages, listen, speak, stop_speaking};

#[cfg(target_os = "macos")]
mod mac {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::{AllocAnyThread as _, rc::Retained};
    use objc2_avf_audio::{
        AVAudioEngine, AVAudioPCMBuffer, AVAudioTime, AVSpeechBoundary, AVSpeechSynthesizer,
        AVSpeechUtterance,
    };
    use objc2_foundation::{NSArray, NSBundle, NSError, NSLocale, NSString, ns_string};
    use objc2_speech::{
        SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
        SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus as Status,
    };

    use super::{Heard, Msg, Sender};

    /// Hearing now: the microphone's sound going to a recognition task.
    pub struct Listening {
        engine: Retained<AVAudioEngine>,
        request: Retained<SFSpeechAudioBufferRecognitionRequest>,
        task: Retained<SFSpeechRecognitionTask>,
    }

    impl Listening {
        /// Stop listening; what was heard still ends as `Heard::Ended`.
        pub fn finish(&self) {
            self.stop();
            // SAFETY: the request is the one the task reads, made here.
            unsafe { self.request.endAudio() };
        }

        /// Stop listening, and let what was heard go.
        pub fn cancel(&self) {
            self.stop();
            // SAFETY: the task made by `listen`.
            unsafe { self.task.cancel() };
        }

        fn stop(&self) {
            // SAFETY: the engine made by `listen`, on the main thread; the
            // tap on bus 0 is the one `listen` installed.
            unsafe {
                self.engine.stop();
                self.engine.inputNode().removeTapOnBus(0);
            }
        }
    }

    /// Whether you let Ghost use speech recognition; none, not asked yet.
    /// Run outside its bundle, Ghost has not said why it would listen,
    /// and macOS would end it for asking: then it may not.
    pub fn allowed() -> Option<bool> {
        let said = NSBundle::mainBundle()
            .objectForInfoDictionaryKey(ns_string!("NSSpeechRecognitionUsageDescription"));
        if said.is_none() {
            return Some(false);
        }
        // SAFETY: a class method that reads a status.
        match unsafe { SFSpeechRecognizer::authorizationStatus() } {
            Status::NotDetermined => None,
            status => Some(status == Status::Authorized),
        }
    }

    /// Ask macOS whether Ghost may use speech recognition: the answer
    /// comes as `Heard::Allowed`.
    pub fn ask(tx: Sender) {
        let answered = RcBlock::new(move |status: Status| {
            let _ = tx.send(Msg::Heard(Heard::Allowed(status == Status::Authorized)));
        });
        // SAFETY: the block is copied by the framework and only sends on a
        // channel, which may be done from any thread.
        unsafe { SFSpeechRecognizer::requestAuthorization(&answered) };
    }

    /// Begin listening in `language` (a locale; none, the system's), for
    /// `words` among others: what is heard comes as `Heard::Words`, then
    /// `Heard::Ended`.
    pub fn listen(
        language: Option<&str>,
        words: &[String],
        tx: Sender,
    ) -> Result<Listening, String> {
        // SAFETY: Speech and AVFAudio objects made and used here on the
        // main thread. The tap's block runs on an audio thread and only
        // appends buffers to the request, which Apple documents as safe
        // from any thread; the result block only sends on a channel.
        unsafe {
            let recognizer = match language {
                Some(id) => SFSpeechRecognizer::initWithLocale(
                    SFSpeechRecognizer::alloc(),
                    &NSLocale::localeWithLocaleIdentifier(&NSString::from_str(id)),
                ),
                None => SFSpeechRecognizer::init(SFSpeechRecognizer::alloc()),
            }
            .ok_or("Speech recognition does not hear this language")?;
            if !recognizer.isAvailable() {
                return Err("Speech recognition is not available just now".to_owned());
            }
            // On this Mac alone: a language heard only on Apple's servers
            // is not heard at all.
            if !recognizer.supportsOnDeviceRecognition() {
                return Err("macOS can't hear this language on this Mac alone: choose \
                            another, or add it under System Settings › Keyboard › Dictation"
                    .to_owned());
            }
            let request = SFSpeechAudioBufferRecognitionRequest::new();
            request.setShouldReportPartialResults(true);
            request.setAddsPunctuation(true);
            request.setRequiresOnDeviceRecognition(true);
            let words: Vec<Retained<NSString>> =
                words.iter().map(|w| NSString::from_str(w)).collect();
            request.setContextualStrings(&NSArray::from_retained_slice(&words));

            let engine = AVAudioEngine::new();
            let input = engine.inputNode();
            let format = input.outputFormatForBus(0);
            let fed = request.clone();
            let tap = RcBlock::new(
                move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
                    fed.appendAudioPCMBuffer(buffer.as_ref());
                },
            );
            input.installTapOnBus_bufferSize_format_block(
                0,
                1024,
                Some(&format),
                RcBlock::as_ptr(&tap),
            );
            engine.prepare();
            engine
                .startAndReturnError()
                .map_err(|e| e.localizedDescription().to_string())?;

            let heard = RcBlock::new(
                move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                    let heard = if let Some(result) = result.as_ref() {
                        let text = result.bestTranscription().formattedString().to_string();
                        if result.isFinal() {
                            Heard::Ended(Ok(text))
                        } else {
                            Heard::Words(text)
                        }
                    } else if let Some(error) = error.as_ref() {
                        Heard::Ended(Err(error.localizedDescription().to_string()))
                    } else {
                        return;
                    };
                    let _ = tx.send(Msg::Heard(heard));
                },
            );
            let task = recognizer.recognitionTaskWithRequest_resultHandler(&request, &heard);
            Ok(Listening {
                engine,
                request,
                task,
            })
        }
    }

    /// The languages speech recognition hears: each locale with its name
    /// in yours, by name.
    pub fn languages() -> Vec<(String, String)> {
        // SAFETY: class methods and Foundation objects read here.
        let mut all: Vec<(String, String)> = unsafe {
            let here = NSLocale::currentLocale();
            SFSpeechRecognizer::supportedLocales()
                .allObjects()
                .to_vec()
                .into_iter()
                .map(|locale| {
                    let id = locale.localeIdentifier();
                    let name = here.localizedStringForLocaleIdentifier(&id).to_string();
                    let id = id.to_string();
                    // A locale macOS has no name for goes by its own.
                    let name = if name.trim().is_empty() {
                        id.clone()
                    } else {
                        name
                    };
                    (id, name)
                })
                .collect()
        };
        all.sort_by(|a, b| a.1.cmp(&b.1));
        all
    }

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

#[cfg(not(target_os = "macos"))]
pub use elsewhere::{Listening, allowed, ask, languages, listen, speak, stop_speaking};

/// Where there is no Apple speech recognition, there is no dictation.
#[cfg(not(target_os = "macos"))]
mod elsewhere {
    use super::Sender;

    pub struct Listening;

    impl Listening {
        pub fn finish(&self) {}
        pub fn cancel(&self) {}
    }

    pub fn allowed() -> Option<bool> {
        Some(true)
    }

    pub fn ask(_: Sender) {}

    pub fn listen(_: Option<&str>, _: &[String], _: Sender) -> Result<Listening, String> {
        Err("Dictation is heard by macOS's speech recognition".to_owned())
    }

    pub fn languages() -> Vec<(String, String)> {
        Vec::new()
    }

    pub fn speak(_: &str) {}

    pub fn stop_speaking() {}
}

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
