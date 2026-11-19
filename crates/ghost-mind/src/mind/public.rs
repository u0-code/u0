//! Her public life: how her posts are doing, and what she has learned from
//! that.
//!
//! Wanting to be read is hers, written into who she is or not. Nothing here
//! optimises her. A channel reports numbers; she sees them when she scrolls
//! and when she looks back on a day; if they show her something, she writes
//! a lesson in her own words, and later she may replace it. None of it
//! touches her persona. What changes is her craft.

use chrono::{DateTime, Duration, Utc};
use ghost_core::{journal::Event, narrate, when::when};
use serde_json::Value;
use tracing::{info, warn};

use super::{Mind, Outcome, Purpose};
use crate::{Act, Acted, Can, Prompts, Result, actions::clip};

/// The start of the id of an undertaking that is a review of her own
/// public writing.
pub(super) const REVIEW: &str = "review-";
/// How many measured posts make a review worth sitting down to.
const REVIEW_NEEDS: usize = 5;

/// The lessons and intentions in a review's summary: its `LESSON:` and
/// `TRY:` lines.
pub(super) fn conclusions(words: &Prompts, summary: &str) -> Vec<(String, Option<i64>)> {
    summary
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['-', '*', ' ']);
            if let Some(lesson) = line.strip_prefix("LESSON:") {
                Some(lesson.trim().to_owned())
            } else {
                line.strip_prefix("TRY:")
                    .map(|what| words.fill("try_this_week", &[("what", what.trim().to_owned())]))
            }
        })
        .filter(|text| text.len() > 12)
        .map(|text| (text, None))
        .collect()
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
        let mut lines = Vec::new();
        let counts = memory.followers(channel, 30)?;
        if let Some((_, latest)) = counts.last() {
            let then = match counts.iter().find(|(at, _)| now - *at <= Duration::days(7)) {
                Some((at, earlier)) if earlier != latest => self.prompts.fill(
                    "followers_then",
                    &[
                        ("count", earlier.to_string()),
                        ("when", when(*at, now, self.life.clock())),
                    ],
                ),
                _ => String::new(),
            };
            lines.push(self.prompts.fill(
                "followers",
                &[
                    ("count", latest.to_string()),
                    ("channel", channel.to_owned()),
                    ("then", then),
                ],
            ));
        }
        for post in memory.standings(channel, 8)? {
            let text = post
                .text
                .unwrap_or_else(|| self.prompts.text("post_untracked").to_owned());
            let views = if post.views > 0 {
                self.prompts
                    .fill("post_views", &[("views", post.views.to_string())])
            } else {
                String::new()
            };
            lines.push(self.prompts.fill(
                "post_standing",
                &[
                    (
                        "when",
                        narrate::capital(&when(post.at, now, self.life.clock())),
                    ),
                    ("text", clip(&text.replace('\n', " "), 110)),
                    ("likes", post.likes.to_string()),
                    ("replies", post.replies.to_string()),
                    ("reposts", post.reposts.to_string()),
                    ("views", views),
                ],
            ));
        }
        Ok(lines.join("\n"))
    }

    /// Ask `channel` how her posts are doing, if it can say and it has been
    /// long enough since she last asked.
    pub(super) fn measure_if_due(&mut self, channel: &str, now: DateTime<Utc>) -> Result<()> {
        let hours = self.cfg.channel(channel).measure_hours;
        if hours == 0 || !self.can(channel, Can::Metrics) {
            return Ok(());
        }
        let asked_recently = self
            .ghost
            .journal()
            .last_n_of(&["measured"], 8)?
            .iter()
            .any(|e| {
                matches!(&e.event, Event::Measured { channel: c, .. } if c == channel)
                    && now - e.at < Duration::hours(i64::from(hours))
            });
        if asked_recently
            || self
                .jobs
                .any(|p| matches!(p, Purpose::Measure(at) if at == channel))
        {
            return Ok(());
        }
        let (body, channel) = (self.body.clone(), channel.to_owned());
        self.jobs
            .spawn_for(Purpose::Measure(channel.clone()), async move {
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

    /// On waking: is it time to sit down and look at how her public writing
    /// is going? If so she takes it on herself, like anything else.
    pub(super) fn consider_review(&mut self, now: DateTime<Utc>) -> Result<()> {
        // As often as the most often of the channels she posts on says.
        let Some(days) = self
            .channels
            .iter()
            .filter(|(_, c)| c.can.contains(&Can::Post))
            .map(|(name, _)| self.cfg.channel(name).review_days)
            .filter(|days| *days > 0)
            .min()
        else {
            return Ok(());
        };
        if !self.can_work() {
            return Ok(());
        }
        let memory = self.ghost.memory();
        let since = now - Duration::days(i64::from(days));
        let reviewed_lately = memory
            .undertakings(false, 60)?
            .iter()
            .any(|u| u.id.starts_with(REVIEW) && u.at > since);
        if reviewed_lately {
            return Ok(());
        }
        let mut standing = Vec::new();
        let mut measured = 0;
        let channels: Vec<String> = self
            .channels
            .iter()
            .filter(|(_, c)| c.can.contains(&Can::Metrics))
            .map(|(name, _)| name.clone())
            .collect();
        for channel in &channels {
            measured += memory.standings(channel, 30)?.len();
            let lines = self.standing_lines(channel, now)?;
            if !lines.is_empty() {
                standing.push(lines);
            }
        }
        if measured < REVIEW_NEEDS {
            return Ok(());
        }
        let craft = self.craft_lines()?;
        let cares = self.ghost.identity().seed().cares_about.join(", ");
        let brief = narrate::fill(
            self.prompts.review.trim_end(),
            &[
                ("standing", standing.join("\n")),
                ("craft", self.prompts.or(craft, "nothing_yet")),
                ("cares", self.prompts.or(cares, "not_said")),
            ],
        );
        info!("time to look at how her public writing is going");
        self.record(Event::Undertook {
            id: format!("{REVIEW}{}", now.format("%Y%m%d")),
            brief,
            asked_by: None,
            place: None,
            directed: false,
            dir: None,
        })?;
        self.agenda.work.get_or_insert(now + Duration::minutes(30));
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
        Ok(self.prompts.fill(
            "public_review",
            &[
                ("standing", sections.join("\n")),
                ("craft", self.prompts.or(craft, "nothing_yet")),
            ],
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
