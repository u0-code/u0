//! Her public life: how her posts are doing, what she has learned from
//! that, and reading a post over before it goes out.
//!
//! Wanting to be read is hers, written into who she is or not. Nothing here
//! optimises her. A channel reports numbers; she sees them when she scrolls
//! and when she looks back on a day; if they show her something, she writes
//! a lesson in her own words, and later she may replace it. Her certificate
//! is sealed and stays as it is. What changes is her craft.

use chrono::{DateTime, Duration, Utc};
use ghost_brain::{Brain, Completion};
use ghost_core::{journal::Event, narrate, social::PlaceKind};
use serde_json::Value;
use tracing::{debug, info, warn};

use super::{Mind, Outcome};
use crate::{
    Act, Acted, Can, Result,
    actions::{self, clip},
};

/// How a draft came out of being read over.
pub(super) enum Verdict {
    /// Send this text. It may have been revised.
    Post(String),
    /// Do not send it.
    Drop(String),
}

/// Read a draft over. A draft that could not be read over is held:
/// saying nothing is safer than letting something private out.
pub(super) async fn gate(brain: &dyn Brain, request: &Completion, draft: String) -> Verdict {
    let reply = match brain.complete(request).await {
        Ok(reply) => reply,
        Err(e) => return Verdict::Drop(format!("could not be read over: {e}")),
    };
    let Some(v) = actions::object(&reply.text) else {
        return Verdict::Drop("the read-over came back unreadable".into());
    };
    let why = v["why"].as_str().unwrap_or_default().trim().to_owned();
    match v["verdict"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_lowercase()
        .as_str()
    {
        "post" => Verdict::Post(draft),
        "revise" => match v["text"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
            Some(revised) => Verdict::Post(revised.to_owned()),
            None => Verdict::Drop(if why.is_empty() {
                "nothing left after revising".into()
            } else {
                why
            }),
        },
        _ => Verdict::Drop(if why.is_empty() {
            "not worth posting".into()
        } else {
            why
        }),
    }
}

impl Mind {
    /// What she has learned about her craft, as a short block for a prompt.
    /// Empty when she has learned nothing yet.
    pub(super) fn craft_lines(&self) -> Result<String> {
        Ok(self
            .ghost
            .memory()
            .craft(10)?
            .iter()
            .map(|l| format!("- [{}] {}", l.seq, l.lesson))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// How her posts on `channel` have been doing: followers, and her most
    /// recent measured posts. Empty when nothing has been measured.
    pub(super) fn standing_lines(&self, channel: &str, now: DateTime<Utc>) -> Result<String> {
        let memory = self.ghost.memory();
        let tz = self.life.clock().tz();
        let mut lines = Vec::new();
        let counts = memory.followers(channel, 30)?;
        if let Some((_, latest)) = counts.last() {
            let mut line = format!("- {latest} people follow you on {channel}");
            if let Some((at, earlier)) =
                counts.iter().find(|(at, _)| now - *at <= Duration::days(7))
                && earlier != latest
            {
                line.push_str(&format!(" ({earlier} {})", narrate::ago(*at, now)));
            }
            lines.push(format!("{line}."));
        }
        for post in memory.standings(channel, 8)? {
            lines.push(format!(
                "- {} \"{}\": {} likes, {} replies, {} reposts{}",
                post.at.with_timezone(&tz).format("%m-%d"),
                clip(
                    &post
                        .text
                        .unwrap_or_else(|| "(a post from before you kept track)".into())
                        .replace('\n', " "),
                    110
                ),
                post.likes,
                post.replies,
                post.reposts,
                if post.views > 0 {
                    format!(", seen {} times", post.views)
                } else {
                    String::new()
                },
            ));
        }
        Ok(lines.join("\n"))
    }

    /// Ask `channel` how her posts are doing, if it can say and it has been
    /// long enough since she last asked.
    pub(super) fn measure_if_due(&mut self, channel: &str, now: DateTime<Utc>) -> Result<()> {
        if self.cfg.measure_hours == 0 || !self.can(channel, Can::Metrics) {
            return Ok(());
        }
        let asked_recently = self
            .ghost
            .journal()
            .last_n_of(&["measured"], 8)?
            .iter()
            .any(|e| {
                matches!(&e.event, Event::Measured { channel: c, .. } if c == channel)
                    && now - e.at < Duration::hours(i64::from(self.cfg.measure_hours))
            });
        if asked_recently || !self.measuring.insert(channel.to_owned()) {
            return Ok(());
        }
        let (body, channel) = (self.body.clone(), channel.to_owned());
        self.jobs.spawn(async move {
            let result = body.act(&channel, Act::Measure).await;
            Outcome::Measured { channel, result }
        });
        Ok(())
    }

    pub(super) fn on_measured(
        &mut self,
        channel: &str,
        result: std::result::Result<Acted, String>,
    ) -> Result<()> {
        self.measuring.remove(channel);
        let measure = match result {
            Ok(acted) => acted.measure.unwrap_or_default(),
            Err(e) => {
                warn!("{channel}: could not find out how her posts are doing: {e}");
                return Ok(());
            }
        };
        if measure.followers.is_none() && measure.posts.is_empty() {
            return Ok(());
        }
        info!(
            "{channel}: {} posts measured{}",
            measure.posts.len(),
            measure
                .followers
                .map_or_else(String::new, |f| format!(", {f} followers"))
        );
        self.record(Event::Measured {
            channel: channel.to_owned(),
            followers: measure.followers,
            posts: measure.posts,
        })?;
        Ok(())
    }

    /// The request that reads a draft over before it is posted.
    pub(super) fn gate_request(
        &self,
        channel: &str,
        draft: &str,
        now: DateTime<Utc>,
    ) -> Result<Completion> {
        let memory = self.ghost.memory();
        // What she was told in private lately: the things a post must not
        // carry out into the open.
        let mut private = Vec::new();
        for person in memory.people()?.into_iter().take(40) {
            for note in memory.notes(&person.key, 6)? {
                let told_privately = match note.place.as_deref() {
                    Some(key) => memory
                        .place(key)?
                        .is_some_and(|p| p.kind == PlaceKind::Direct),
                    None => false,
                };
                if told_privately {
                    private.push(format!(
                        "- about {}: {}",
                        person.name,
                        clip(&note.text, 200)
                    ));
                }
            }
        }
        private.truncate(30);
        let canon: Vec<String> = memory
            .canon(30)?
            .iter()
            .map(|f| format!("- {}", f.fact))
            .collect();
        let or_none = |lines: String| {
            if lines.is_empty() {
                "(nothing)".to_owned()
            } else {
                lines
            }
        };
        let prompt = narrate::fill(
            &self.prompts.gate,
            &[
                ("channel", channel.to_owned()),
                ("draft", draft.to_owned()),
                ("private", or_none(private.join("\n"))),
                ("canon", or_none(canon.join("\n"))),
                ("craft", or_none(self.craft_lines()?)),
            ],
        );
        Ok(Completion::ask(self.self_prompt(now)?, prompt, self.cfg.max_tokens).expecting_json())
    }

    pub(super) fn on_held(&mut self, channel: &str, text: &str, why: &str) -> Result<()> {
        debug!("{channel}: held back a post: {why}");
        self.record(Event::Decided {
            about: channel.to_owned(),
            decision: "did not post".into(),
            reason: format!("{why} (the draft: {})", clip(text, 160)),
        })?;
        Ok(())
    }

    /// Lessons from looking back on a day, written down. A lesson may only
    /// replace one she actually holds.
    pub(super) fn learn(&mut self, lessons: Vec<(String, Option<i64>)>) -> Result<()> {
        let held: Vec<i64> = self
            .ghost
            .memory()
            .craft(100)?
            .iter()
            .map(|l| l.seq)
            .collect();
        for (lesson, replaces) in lessons.into_iter().take(2) {
            let lesson = lesson.trim().to_owned();
            if lesson.is_empty() {
                continue;
            }
            self.record(Event::Learned {
                lesson,
                replaces: replaces.filter(|seq| held.contains(seq)),
            })?;
        }
        Ok(())
    }

    /// The part of the nightly look back that is about her public life.
    /// Empty unless something has been measured.
    pub(super) fn public_review(&self, now: DateTime<Utc>) -> Result<String> {
        let mut sections = Vec::new();
        let channels: Vec<&String> = self
            .channels
            .iter()
            .filter(|(_, c)| c.can.contains(&Can::Metrics))
            .map(|(name, _)| name)
            .collect();
        for channel in channels {
            let lines = self.standing_lines(channel, now)?;
            if !lines.is_empty() {
                sections.push(lines);
            }
        }
        if sections.is_empty() {
            return Ok(String::new());
        }
        let craft = self.craft_lines()?;
        Ok(format!(
            "# How what you posted has done\n\n{}\n\n# What you have learned about posting so far\n\n{}\n\n",
            sections.join("\n"),
            if craft.is_empty() {
                "(nothing yet)".to_owned()
            } else {
                craft
            }
        ))
    }
}

/// Lessons out of a reflection: `[{"lesson": "...", "replaces": 12}]`.
pub(super) fn lessons(v: &Value) -> Vec<(String, Option<i64>)> {
    v.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let lesson = match item {
                        Value::String(s) => s.trim().to_owned(),
                        other => other["lesson"]
                            .as_str()
                            .unwrap_or_default()
                            .trim()
                            .to_owned(),
                    };
                    (!lesson.is_empty()).then(|| (lesson, item["replaces"].as_i64()))
                })
                .collect()
        })
        .unwrap_or_default()
}
