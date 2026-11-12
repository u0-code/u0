//! Turning who she is and what has happened into words a brain can read.
//!
//! The template is data, not code: [`DEFAULT_SELF_TEMPLATE`] is
//! `prompts/self.md`, and the daemon may substitute another file. It belongs to
//! the body, not the person: it is how a brain reads her certificate, and the
//! certificate itself stays sealed.

use chrono::{DateTime, Utc};

use crate::{
    identity::SealedIdentity,
    journal::{Entry, Event},
    life::{Clock, Phase},
};

/// The template shipped with the body.
pub const DEFAULT_SELF_TEMPLATE: &str = include_str!("../prompts/self.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Them,
    Her,
}

/// One turn of a conversation, as the brain will see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub speaker: Speaker,
    pub text: String,
}

/// Everything the brain needs to know about this moment.
pub struct Scene<'a> {
    pub identity: &'a SealedIdentity,
    pub clock: &'a Clock,
    pub now: DateTime<Utc>,
    pub person: &'a str,
    pub channel: &'a str,
    /// Recent `lived` / `planned` / `reflected` entries, oldest first.
    pub recent: &'a [Entry],
}

impl Scene<'_> {
    /// Fill `template` with this scene. Placeholders are `{{name}}` and friends.
    pub fn system_prompt(&self, template: &str) -> String {
        let seed = self.identity.seed();
        let tz = self.clock.tz();
        let now = self
            .now
            .with_timezone(&tz)
            .format("%A %Y-%m-%d %H:%M %Z")
            .to_string();
        let phase = match self.clock.phase_at(self.now) {
            Phase::Awake => "awake",
            Phase::Asleep => "asleep; it is past your bedtime",
        };
        let recent = if self.recent.is_empty() {
            "(nothing worth noting yet)".to_owned()
        } else {
            self.recent
                .iter()
                .map(|e| {
                    format!(
                        "- {} {}",
                        e.at.with_timezone(&tz).format("%m-%d %H:%M"),
                        summary(&e.event)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let values = list(&seed.values);
        let cares = list(&seed.cares_about);
        let cannot = list(&seed.cannot_stand);
        let pairs: [(&str, &str); 14] = [
            ("name", &seed.name),
            ("pronouns", &seed.pronouns),
            ("city", &seed.home.city),
            ("country", &seed.home.country),
            ("temperament", seed.temperament.trim()),
            ("voice", seed.voice.trim()),
            ("origin", seed.origin.trim()),
            ("values", &values),
            ("cares_about", &cares),
            ("cannot_stand", &cannot),
            ("now", &now),
            ("phase", phase),
            ("person", self.person),
            ("channel", self.channel),
        ];
        let mut out = template.to_owned();
        for (key, value) in pairs {
            out = out.replace(&format!("{{{{{key}}}}}"), value);
        }
        out.replace("{{recent}}", &recent)
    }
}

/// Flatten `heard` / `said` entries involving `person` into alternating turns.
///
/// Consecutive turns by the same speaker are merged, and leading turns by her
/// are dropped so the thread always opens with them.
pub fn thread(entries: &[Entry], person: &str) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for e in entries {
        let (speaker, text) = match &e.event {
            Event::Heard { from, text, .. } if from == person => (Speaker::Them, text),
            Event::Said { to, text, .. } if to == person => (Speaker::Her, text),
            _ => continue,
        };
        match turns.last_mut() {
            Some(last) if last.speaker == speaker => {
                last.text.push('\n');
                last.text.push_str(text);
            }
            _ => turns.push(Turn {
                speaker,
                text: text.clone(),
            }),
        }
    }
    let skip = turns
        .iter()
        .take_while(|t| t.speaker == Speaker::Her)
        .count();
    turns.drain(..skip);
    turns
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "(nothing listed)".to_owned()
    } else {
        items.join(", ")
    }
}

fn summary(event: &Event) -> String {
    match event {
        Event::Lived { text } => text.clone(),
        Event::Planned { plan } => format!("you planned: {plan}"),
        Event::Reflected { text } => format!("you thought: {text}"),
        Event::Woke => "you woke up".into(),
        Event::Slept => "you went to sleep".into(),
        Event::Canon { fact, .. } => fact.clone(),
        other => other.kind().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(event: Event) -> Entry {
        Entry {
            seq: 0,
            at: Utc::now(),
            event,
            prev_hash: String::new(),
            hash: String::new(),
        }
    }

    fn heard(from: &str, text: &str) -> Entry {
        entry(Event::Heard {
            channel: "cli".into(),
            from: from.into(),
            text: text.into(),
        })
    }

    fn said(to: &str, text: &str) -> Entry {
        entry(Event::Said {
            channel: "cli".into(),
            to: to.into(),
            text: text.into(),
        })
    }

    #[test]
    fn thread_merges_and_filters() {
        let entries = vec![
            said("a", "dropped"),
            heard("a", "hi"),
            heard("a", "you there?"),
            said("a", "yes"),
            heard("b", "not this one"),
            heard("a", "ok"),
        ];
        let t = thread(&entries, "a");
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].speaker, Speaker::Them);
        assert_eq!(t[0].text, "hi\nyou there?");
        assert_eq!(t[1].speaker, Speaker::Her);
        assert_eq!(t[2].text, "ok");
    }

    #[test]
    fn template_has_no_unfilled_placeholders() {
        let seed = crate::identity::Seed::parse(SAMPLE).unwrap();
        let bytes = SAMPLE.as_bytes();
        let identity = crate::identity::SealedIdentity::new(seed, crate::identity::Seal::of(bytes));
        let clock = Clock::of(&identity);
        let scene = Scene {
            identity: &identity,
            clock: &clock,
            now: Utc::now(),
            person: "someone",
            channel: "cli",
            recent: &[],
        };
        let out = scene.system_prompt(DEFAULT_SELF_TEMPLATE);
        assert!(!out.contains("{{"), "unfilled placeholder in:\n{out}");
        assert!(out.contains("Lin"));
        assert!(out.contains("Chengdu"));
    }

    const SAMPLE: &str = r#"
name = "Lin"
pronouns = "she/her"
born = "2026-11-12"
temperament = "quiet"
voice = "short"
origin = "made"

[home]
city = "Chengdu"
country = "CN"
timezone = "Asia/Shanghai"

[rhythm]
wake = "08:30"
sleep = "00:30"
"#;
}
