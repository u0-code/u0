//! Picking up her phone to scroll, and sometimes posting.
//!
//! A feed is not a conversation. What she scrolls past does not enter her
//! journal item by item; she keeps one line about what she took away, plus
//! whatever she actually engaged with.

use chrono::{DateTime, Duration, Utc};
use ghost_brain::Completion;
use ghost_core::{
    journal::Event,
    life::{Phase, Reach},
    narrate, social,
};
use tracing::{debug, warn};

use super::{Mind, Outcome, speak::Bubble};
use crate::{
    Act, Acted, Can, Error, Feed, Incoming, Result,
    actions::{self, Action, Deliberation, clip},
};

/// How many posts she looks at in one scroll.
const SCROLL: u32 = 20;

impl Mind {
    /// When she next picks up her phone to scroll, if any channel has a feed.
    pub(super) fn next_phone(&mut self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let [lo, hi] = self.cfg.browse_minutes;
        if hi == 0 || !self.channels.values().any(|c| c.can.contains(&Can::Feed)) {
            return None;
        }
        let minutes = self.rng.u32(lo.min(hi)..=hi);
        Some(now + Duration::minutes(i64::from(minutes.max(1))))
    }

    pub(super) fn browse(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.agenda.phone = self.next_phone(now);
        let moment = self.life.moment(self.ghost.journal(), now)?;
        if moment.phase == Phase::Asleep || moment.reach >= Reach::Busy || !self.budget.allow(now) {
            return Ok(());
        }
        let feeds: Vec<String> = self
            .channels
            .iter()
            .filter(|(name, c)| c.can.contains(&Can::Feed) && !self.browsing.contains(*name))
            .map(|(name, _)| name.clone())
            .collect();
        if feeds.is_empty() {
            return Ok(());
        }
        let channel = feeds[self.rng.usize(..feeds.len())].clone();
        let posted = self.posted_today(now)?;
        let allowance = if posted >= self.cfg.posts_per_day as usize {
            "That is enough for one day; do not post again today."
        } else {
            ""
        };
        let template = self.prompts.browse.clone();
        let pairs = vec![
            ("channel", channel.clone()),
            ("posted", posted.to_string()),
            ("allowance", allowance.to_owned()),
        ];
        let system = self.self_prompt(now)?;
        let (brain, body, max_tokens) =
            (self.think.clone(), self.body.clone(), self.cfg.max_tokens);
        self.browsing.insert(channel.clone());
        self.budget.spend(now);
        self.jobs.spawn(async move {
            let fetched = body
                .act(
                    &channel,
                    Act::Fetch {
                        feed: Feed::Timeline,
                        limit: SCROLL,
                    },
                )
                .await;
            let items = match fetched {
                Ok(acted) => acted.items,
                Err(e) => {
                    warn!("{channel}: could not load the timeline: {e}");
                    Vec::new()
                }
            };
            let listing = if items.is_empty() {
                "(nothing new)".to_owned()
            } else {
                items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        format!(
                            "[i{}] {}{}: {}",
                            i + 1,
                            item.from.name,
                            item.from
                                .handle
                                .as_deref()
                                .map(|h| format!(" (@{h})"))
                                .unwrap_or_default(),
                            clip(&item.text.replace('\n', " "), 400)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let mut pairs = pairs;
            pairs.push(("items", listing));
            let request = Completion::ask(system, narrate::fill(&template, &pairs), max_tokens)
                .expecting_json();
            let result = match brain.complete(&request).await {
                Ok(reply) => actions::parse(&reply.text).map_err(Error::Parse),
                Err(e) => Err(e.into()),
            };
            Outcome::Browsed {
                channel,
                items,
                result,
            }
        });
        Ok(())
    }

    pub(super) fn on_browsed(
        &mut self,
        channel: &str,
        items: &[Incoming],
        result: Result<Deliberation>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.browsing.remove(channel);
        let deliberation = match result {
            Ok(d) => d,
            Err(e) => {
                warn!("{channel}: scrolling came to nothing: {e}");
                return Ok(());
            }
        };
        let item = |n: i64| usize::try_from(n - 1).ok().and_then(|i| items.get(i));
        let mut posted = self.posted_today(now)?;
        for action in deliberation.actions {
            match action {
                Action::React { to, emoji } => {
                    let Some(target) = item(to) else { continue };
                    if !self.can(channel, Can::React) {
                        continue;
                    }
                    self.fire(
                        channel,
                        Act::React {
                            place: target.place.id.clone(),
                            to: target.id.clone(),
                            emoji: emoji.clone(),
                        },
                    );
                    self.record(Event::Reacted {
                        channel: channel.to_owned(),
                        place: target.place.id.clone(),
                        to: target.id.clone(),
                        emoji,
                    })?;
                }
                Action::Say {
                    text,
                    reply_to: Some(to),
                } => {
                    let Some(target) = item(to).cloned() else {
                        continue;
                    };
                    let key = social::key(channel, &target.place.id);
                    if self.agenda.speaking.contains_key(&key) || text.trim().is_empty() {
                        continue;
                    }
                    // What she replies to becomes part of her record; the rest
                    // of the timeline does not.
                    let upto = if self.ghost.memory().knows_message(&key, &target.id)? {
                        self.ghost.journal().last_seq()?
                    } else {
                        self.record(Event::Heard {
                            channel: channel.to_owned(),
                            place: target.place.clone(),
                            from: target.from.clone(),
                            id: target.id.clone(),
                            text: target.text.clone(),
                            reply_to: target.reply_to.clone(),
                            addressed: false,
                        })?
                        .seq
                    };
                    self.record(Event::Read {
                        channel: channel.to_owned(),
                        place: target.place.id.clone(),
                        upto,
                    })?;
                    self.begin_speaking_in(
                        &key,
                        channel,
                        &target.place.id,
                        upto,
                        Bubble {
                            text: text.trim().to_owned(),
                            reply_to: Some(target.id),
                        },
                        now,
                    );
                }
                Action::Post { text } => {
                    let text = text.trim().to_owned();
                    if text.is_empty()
                        || posted >= self.cfg.posts_per_day as usize
                        || !self.can(channel, Can::Post)
                    {
                        continue;
                    }
                    posted += 1;
                    let body = self.body.clone();
                    let channel = channel.to_owned();
                    self.jobs.spawn(async move {
                        let result = body.act(&channel, Act::Post { text: text.clone() }).await;
                        Outcome::Posted {
                            channel,
                            text,
                            result,
                        }
                    });
                }
                other => debug!("{channel}: not something she does while scrolling: {other:?}"),
            }
        }
        if !deliberation.thought.is_empty() {
            self.record(Event::Saw {
                channel: channel.to_owned(),
                text: deliberation.thought,
            })?;
        }
        Ok(())
    }

    pub(super) fn on_posted(
        &mut self,
        channel: String,
        text: String,
        result: std::result::Result<Acted, String>,
    ) -> Result<()> {
        match result {
            Ok(acted) => {
                self.record(Event::Posted {
                    channel,
                    id: acted.id,
                    text,
                })?;
            }
            Err(e) => warn!("{channel}: could not post: {e}"),
        }
        Ok(())
    }

    /// How many times she has posted since she woke today.
    fn posted_today(&self, now: DateTime<Utc>) -> Result<usize> {
        let clock = self.life.clock();
        let day_began = clock.instant(clock.day_of(now), clock.wake());
        Ok(self
            .ghost
            .journal()
            .last_n_of(&["posted"], self.cfg.posts_per_day as usize + 1)?
            .iter()
            .filter(|e| e.at >= day_began)
            .count())
    }
}
