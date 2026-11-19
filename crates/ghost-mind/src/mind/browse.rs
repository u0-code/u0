//! Picking up her phone to scroll, and sometimes posting.
//!
//! A feed is not a conversation. What she scrolls past does not enter her
//! journal item by item; she keeps one line about what she took away, plus
//! whatever she actually engaged with.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use ghost_brain::Completion;
use ghost_core::{
    journal::Event,
    life::{Phase, Reach},
    narrate, social,
};
use tracing::{debug, warn};

use super::{
    Mind, Outcome,
    public::{Verdict, gate},
    speak::Bubble,
};
use crate::{
    Act, Acted, Can, Error, Feed, Incoming, Result,
    actions::{self, Action, Deliberation, clip},
};

/// How many posts she looks at in one scroll.
const SCROLL: u32 = 20;
/// How many of her latest posts are looked through to count today's on
/// one channel.
const POSTS_LOOKED_AT: usize = 200;
/// How many scrolled-past posts she remembers per channel.
const REMEMBERED: usize = 500;

impl Mind {
    /// The channels with a feed she scrolls: each says how often, in its
    /// own settings.
    fn feeds(&self) -> Vec<(String, [u32; 2])> {
        self.channels
            .iter()
            .filter(|(_, c)| c.can.contains(&Can::Feed))
            .map(|(name, _)| (name.clone(), self.cfg.channel(name).browse_minutes))
            .filter(|(_, [_, hi])| *hi > 0)
            .collect()
    }

    /// When she next picks up her phone to scroll, if any channel has a feed
    /// she scrolls: as soon as the most often scrolled of them says.
    pub(super) fn next_phone(&mut self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let [lo, hi] = self
            .feeds()
            .into_iter()
            .map(|(_, minutes)| minutes)
            .min_by_key(|[_, hi]| *hi)?;
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
            .feeds()
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| !self.browsing.contains(name))
            .collect();
        if feeds.is_empty() {
            return Ok(());
        }
        let channel = feeds[self.rng.usize(..feeds.len())].clone();
        self.measure_if_due(&channel, now)?;
        let set = self.cfg.channel(&channel);
        let posted = self.posted_today(&channel, now)?;
        let allowance = if posted >= set.posts_per_day as usize {
            self.prompts.text("posted_enough")
        } else {
            ""
        };
        let template = self.prompts.browse.clone();
        let pairs = vec![
            ("channel", channel.clone()),
            ("posted", posted.to_string()),
            ("allowance", allowance.to_owned()),
            ("actions", self.scrolling_actions(&channel)),
            (
                "standing",
                self.prompts.section(
                    "browse_standing",
                    "standing",
                    &self.standing_lines(&channel, now)?,
                ),
            ),
            (
                "craft",
                self.prompts
                    .section("browse_craft", "craft", &self.craft_lines()?),
            ),
        ];
        let (system, right_now) = self.portrait_on(now, Some(&channel), "")?;
        let (brain, body, max_tokens) =
            (self.think.clone(), self.body.clone(), self.cfg.max_tokens);
        let seen: HashSet<String> = self
            .scrolled
            .get(&channel)
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default();
        let may_post = posted < set.posts_per_day as usize;
        let nothing_new = self.prompts.text("nothing_new").to_owned();
        let handle = self.prompts.text("card_handle").to_owned();
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
            // Only what she has not scrolled past before.
            let items: Vec<Incoming> = match fetched {
                Ok(acted) => acted
                    .items
                    .into_iter()
                    .filter(|item| !seen.contains(&item.id))
                    .collect(),
                Err(e) => {
                    warn!("{channel}: could not load the timeline: {e}");
                    Vec::new()
                }
            };
            if items.is_empty() && !may_post {
                // Nothing new and nothing left to say today: she puts it down.
                return Outcome::Browsed {
                    channel,
                    items,
                    result: Ok(Deliberation::default()),
                };
            }
            let listing = if items.is_empty() {
                nothing_new
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
                                .map(|h| narrate::fill(&handle, &[("handle", h.to_owned())]))
                                .unwrap_or_default(),
                            clip(&item.text.replace('\n', " "), 400)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let mut pairs = pairs;
            pairs.push(("items", listing));
            let prompt = format!("{right_now}\n{}", narrate::fill(&template, &pairs));
            let request = Completion::ask(system, prompt, max_tokens).expecting_json();
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

    /// The actions offered while scrolling, beyond posting and doing
    /// nothing. What a platform forbids an automated account is not offered.
    fn scrolling_actions(&self, channel: &str) -> String {
        let set = self.cfg.channel(channel);
        let mut out = String::new();
        if set.likes {
            out.push_str(self.prompts.text("scroll_like"));
        }
        if set.replies {
            out.push_str(self.prompts.text("scroll_reply"));
        }
        out
    }

    pub(super) fn on_browsed(
        &mut self,
        channel: &str,
        items: &[Incoming],
        result: Result<Deliberation>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.browsing.remove(channel);
        let scrolled = self.scrolled.entry(channel.to_owned()).or_default();
        scrolled.extend(items.iter().map(|item| item.id.clone()));
        while scrolled.len() > REMEMBERED {
            scrolled.pop_front();
        }
        let deliberation = match result {
            Ok(d) => d,
            Err(e) => {
                warn!("{channel}: scrolling came to nothing: {e}");
                return Ok(());
            }
        };
        let item = |n: i64| usize::try_from(n - 1).ok().and_then(|i| items.get(i));
        let set = self.cfg.channel(channel);
        let mut posted = self.posted_today(channel, now)?;
        for action in deliberation.actions {
            match action {
                Action::React { to, emoji } => {
                    let Some(target) = item(to) else { continue };
                    if !set.likes || !self.can(channel, Can::React) {
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
                    if !set.replies {
                        debug!("{channel}: replying to posts not addressed to her is turned off");
                        continue;
                    }
                    if self.agenda.speaking.contains_key(&key)
                        || text.trim().is_empty()
                        || self.ghost.memory().replied_to(&key, &target.id)?
                    {
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
                            images: target.images.clone(),
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
                            file: None,
                        },
                        now,
                    );
                }
                Action::Post { text } => {
                    let text = text.trim().to_owned();
                    if text.is_empty()
                        || posted >= set.posts_per_day as usize
                        || !self.can(channel, Can::Post)
                    {
                        continue;
                    }
                    posted += 1;
                    let body = self.body.clone();
                    // Read it over first, as herself, for what must not go out.
                    let check = if set.gate {
                        Some((self.tick.clone(), self.gate_request(channel, &text, now)?))
                    } else {
                        None
                    };
                    let channel = channel.to_owned();
                    self.jobs.spawn(async move {
                        let text = match check {
                            Some((brain, request)) => {
                                match gate(brain.as_ref(), &request, text.clone()).await {
                                    Verdict::Post(text) => text,
                                    Verdict::Drop(why) => {
                                        return Outcome::Held { channel, text, why };
                                    }
                                }
                            }
                            None => text,
                        };
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

    /// How many times she has posted on `channel` since she woke today.
    fn posted_today(&self, channel: &str, now: DateTime<Utc>) -> Result<usize> {
        let clock = self.life.clock();
        let day_began = clock.instant(clock.day_of(now), clock.wake());
        Ok(self
            .ghost
            .journal()
            .last_n_of(&["posted"], POSTS_LOOKED_AT)?
            .iter()
            .filter(|e| {
                e.at >= day_began
                    && matches!(&e.event, Event::Posted { channel: c, .. } if c == channel)
            })
            .count())
    }
}
