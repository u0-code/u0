//! What she decided at the end of a look, carried out: written in her
//! journal, and set in motion.

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};
use ghost_core::{
    journal::Event,
    memory::{Person, PlaceState},
    social::PlaceKind,
    will,
};
use tracing::debug;

use super::{
    Mind,
    attend::{Thinking, Why},
    give::Gift,
    speak::Bubble,
};
use crate::{
    Act, Can, Result,
    actions::{Action, Deliberation},
};

impl Mind {
    /// Turn what she decided into journal entries and things in motion.
    pub(super) fn apply(
        &mut self,
        place: &PlaceState,
        job: &Thinking,
        deliberation: Deliberation,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if job.upto > place.read_upto {
            self.record(Event::Read {
                channel: place.channel.clone(),
                place: place.id.clone(),
                upto: job.upto,
            })?;
        }
        let mut bubbles = VecDeque::new();
        let mut did: Vec<String> = Vec::new();
        for action in deliberation.actions {
            match action {
                Action::Say { text, reply_to } => {
                    let text = text.trim().to_owned();
                    if !text.is_empty() {
                        let reply_to = match reply_to {
                            Some(seq) => self.platform_id(place, seq)?,
                            // A public reply has to hang off something: the
                            // newest message she read, unless she chose one.
                            None if place.kind == PlaceKind::Public && bubbles.is_empty() => {
                                self.platform_id(place, job.upto)?
                            }
                            None => None,
                        };
                        bubbles.push_back(Bubble {
                            text,
                            reply_to,
                            file: None,
                        });
                    }
                }
                Action::React { to, emoji } => {
                    if self.can(&place.channel, Can::React)
                        && let Some(id) = self.platform_id(place, to)?
                    {
                        self.fire(
                            &place.channel,
                            Act::React {
                                place: place.id.clone(),
                                to: id.clone(),
                                emoji: emoji.clone(),
                            },
                        );
                        self.record(Event::Reacted {
                            channel: place.channel.clone(),
                            place: place.id.clone(),
                            to: id,
                            emoji: emoji.clone(),
                        })?;
                        did.push(format!("reacted {emoji}"));
                    }
                }
                Action::Silence { why } => did.push(if why.is_empty() {
                    "stayed quiet".to_owned()
                } else {
                    format!("stayed quiet: {why}")
                }),
                Action::Later { minutes, why } => {
                    let about = self.resolve(place, None)?.map(|p| p.key);
                    if self.intend(
                        now + minutes_from(minutes.clamp(1.0, 7.0 * 24.0 * 60.0)),
                        format!("get back to this conversation: {why}"),
                        about,
                        Some(place.key.clone()),
                        true,
                    )? {
                        did.push("put it off".to_owned());
                    }
                }
                Action::SendFile {
                    file,
                    message,
                    image,
                    text,
                    what,
                } => match self.giveable(place, file.as_deref(), message, image)? {
                    Ok(mut gift) => {
                        // Her own words for it, when she gave them, are
                        // what her line says she sent.
                        if let (Gift::Image { what: shown, .. }, Some(words)) =
                            (&mut gift, what.filter(|w| !w.trim().is_empty()))
                        {
                            *shown = words;
                        }
                        bubbles.push_back(Bubble {
                            text: text.unwrap_or_default().trim().to_owned(),
                            reply_to: None,
                            file: Some(gift),
                        });
                        did.push("sent a file".to_owned());
                    }
                    // Where what she does is tool calls she was told at
                    // once; answering in JSON, it is only written down.
                    Err(why) => {
                        self.record(Event::Decided {
                            about: place.key.clone(),
                            decision: "did not send a file".into(),
                            reason: why,
                        })?;
                    }
                },
                Action::Undertake { brief, for_whom } => {
                    let asker = self.resolve(place, for_whom.as_deref())?;
                    let directed = match &job.why {
                        Why::Directive(directives) => directives.as_slice(),
                        _ => &[],
                    };
                    match self.undertake(&brief, asker.as_ref(), Some(place), directed, now)? {
                        None => did.push("took something on".to_owned()),
                        Some(why) => {
                            self.record(Event::Decided {
                                about: place.key.clone(),
                                decision: "did not take it on".into(),
                                reason: why,
                            })?;
                        }
                    }
                }
                Action::Steer { work, text } => match self.steer(work, &text, now)? {
                    None => did.push("passed something on to her work".to_owned()),
                    Some(why) => {
                        self.record(Event::Decided {
                            about: place.key.clone(),
                            decision: "did not pass it on to her work".into(),
                            reason: why,
                        })?;
                    }
                },
                Action::Stop { work } => match self.stop_work(work, now)? {
                    None => did.push("stopped a piece of work".to_owned()),
                    Some(why) => {
                        self.record(Event::Decided {
                            about: place.key.clone(),
                            decision: "did not stop the work".into(),
                            reason: why,
                        })?;
                    }
                },
                Action::Note { about, text } => {
                    if let Some(person) = self.resolve(place, about.as_deref())? {
                        let sources = self.shown(place)?;
                        self.record(Event::Noted {
                            about: person.key,
                            text,
                            place: Some(place.key.clone()),
                            sources,
                        })?;
                        did.push("noted something".to_owned());
                    }
                }
                Action::Feel { toward, delta, why } => {
                    #[allow(clippy::cast_possible_truncation)]
                    let delta = delta.round().clamp(-3.0, 3.0) as i8;
                    if delta != 0
                        && let Some(person) = self.resolve(place, toward.as_deref())?
                    {
                        self.record(Event::Felt {
                            toward: person.key,
                            delta,
                            why,
                        })?;
                        did.push(format!("felt {delta:+}"));
                    }
                }
                Action::Intend { hours, what, about } => {
                    let person = self.resolve(place, about.as_deref())?;
                    let target = person
                        .as_ref()
                        .and_then(|p| p.direct.clone())
                        .unwrap_or_else(|| place.key.clone());
                    if self.intend(
                        now + minutes_from(hours.clamp(0.25, 24.0 * 30.0) * 60.0),
                        what,
                        person.map(|p| p.key),
                        Some(target),
                        false,
                    )? {
                        did.push("made a note to self".to_owned());
                    }
                }
                Action::Keep {
                    message,
                    image,
                    file,
                    note,
                } => match self.keep(place, message, image, file.as_deref(), &note)? {
                    None => did.push("kept something".to_owned()),
                    // Nothing to keep is nothing done: there is no entry.
                    Some(why) => debug!("{}: kept nothing: {why}", place.key),
                },
                Action::LetGo { thing } => {
                    if self.let_go(thing)? {
                        did.push("let something go".to_owned());
                    }
                }
                Action::Canon { fact } => {
                    self.record(Event::Canon {
                        fact,
                        source: format!("said in {}", place.key),
                    })?;
                }
                Action::Quiet { hours } => {
                    if self.go_quiet(place, hours, now)? {
                        did.push("went quiet".to_owned());
                    }
                }
                Action::Leave { why } => {
                    if let Some(done) = self.leave(place, why, now)? {
                        did.push(done.to_owned());
                    }
                }
                // Recall has had its rounds; posting only happens while scrolling.
                Action::Recall { .. } | Action::Reread { .. } | Action::Post { .. } => {}
            }
        }
        if !bubbles.is_empty() {
            did.insert(
                0,
                match bubbles.len() {
                    1 => "said something".to_owned(),
                    n => format!("said {n} things"),
                },
            );
        }
        let decision = if did.is_empty() {
            "nothing".to_owned()
        } else {
            did.join("; ")
        };
        let decided = self.record(Event::Decided {
            about: place.key.clone(),
            decision: decision.clone(),
            reason: deliberation.thought,
        })?;
        // Whoever watched her rounds of this look can find where it ended.
        self.end_look(job, decided.seq);
        if let Why::Intention(intention) = &job.why {
            self.record(Event::Settled {
                intention: intention.seq,
                outcome: decision,
            })?;
            self.pursuing.remove(&intention.seq);
        }
        self.refresh_intentions()?;
        if bubbles.is_empty() {
            // Pick up anything that arrived while she was thinking.
            self.schedule_attend(&place.key, now)
        } else {
            self.begin_speaking(place, job, bubbles, now);
            Ok(())
        }
    }

    /// Leaving is the end of a process. Before things have cooled it only
    /// registers as how badly she wanted to.
    fn leave(
        &mut self,
        place: &PlaceState,
        why: String,
        now: DateTime<Utc>,
    ) -> Result<Option<&'static str>> {
        if place.kind != PlaceKind::Direct {
            return Ok(None);
        }
        let Some(person) = self.ghost.memory().person_of_direct(&place.key)? else {
            return Ok(None);
        };
        if will::may_leave(&will::standing(&person, now)) {
            self.record(Event::Left {
                person: person.key,
                why,
            })?;
            Ok(Some("left"))
        } else {
            self.record(Event::Felt {
                toward: person.key,
                delta: -3,
                why: format!("wanted to leave: {why}"),
            })?;
            Ok(Some("pulled away"))
        }
    }

    /// The channel's id for a message she pointed at, if it is in this place.
    fn platform_id(&self, place: &PlaceState, seq: i64) -> Result<Option<String>> {
        Ok(self
            .ghost
            .memory()
            .line(seq)?
            .filter(|(key, _)| *key == place.key)
            .and_then(|(_, line)| line.platform_id))
    }

    /// Who a brain meant by a name. No name means whoever this private
    /// conversation is with.
    fn resolve(&self, place: &PlaceState, name: Option<&str>) -> Result<Option<Person>> {
        let memory = self.ghost.memory();
        let direct = || -> Result<Option<Person>> {
            if place.kind == PlaceKind::Direct {
                Ok(memory.person_of_direct(&place.key)?)
            } else {
                Ok(None)
            }
        };
        let Some(name) = name.map(|n| n.trim().trim_start_matches('@').to_lowercase()) else {
            return direct();
        };
        if name.is_empty() {
            return direct();
        }
        let found = memory.people_in(&place.key, 30)?.into_iter().find(|p| {
            p.name.to_lowercase() == name
                || p.handle
                    .as_deref()
                    .is_some_and(|h| h.to_lowercase() == name)
        });
        match found {
            Some(person) => Ok(Some(person)),
            None => direct(),
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
pub(super) fn minutes_from(minutes: f64) -> Duration {
    Duration::seconds((minutes * 60.0).round() as i64)
}
