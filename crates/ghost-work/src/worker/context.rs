//! Keeping the transcript within the model's window.
//!
//! How much room there is comes from what was asked for, what the model
//! says about itself, and any size it has already refused. When the
//! transcript nears that, room is made in the order that costs least:
//! pictures already looked at, then long output trimmed, then old output
//! dropped. If that is not enough the worker writes a handoff and a new
//! sitting begins from the record and the last stretch of the transcript.

use ghost_brain::{Item, Turn, Usage};
use serde_json::Value;

use super::{
    Worker,
    instructions::{Shown, opening_with},
};
use crate::{outside, prompts::Prompts, tool::Toolbox, tools::clip};

/// What one step may write when nothing says and nothing is known about
/// the model.
const UNKNOWN_OUTPUT: u32 = 8192;
/// What one step may write on a model known to manage more: room to think
/// at length and still write a long file.
const KNOWN_OUTPUT: u32 = 32_000;
/// Where a transcript is folded when nothing says and the model's window is
/// not known: small enough for any model worth working with.
pub(super) const UNKNOWN_WINDOW: u64 = 60_000;
/// Room kept free in a known window beyond what one step may write.
const HEADROOM: u64 = 65_536;
/// The model once refused a context it was sent. Stay this far under the
/// size that was refused, as tenths.
const UNDER_REFUSAL: u64 = 7;
/// Folding is done when the transcript is back under this share of the
/// room between where its sitting started and its ceiling, as tenths: far
/// enough down that the next fold is a while away.
const FOLD_TO: u64 = 6;

/// A tool result longer than this many characters is trimmed to its
/// beginning and end when room is needed, wherever in the transcript it is.
pub(super) const PRUNE_OVER: usize = 8192;
const PRUNE_HEAD: usize = 4096;
const PRUNE_TAIL: usize = 1024;
/// How many of the newest items are left whole when old output is dropped:
/// tried in turn, each tighter than the last.
const KEEP_RECENT: [usize; 3] = [12, 6, 3];
/// The share of the ceiling that is carried across a cleared context word
/// for word, as hundredths: the most recent stretch of the transcript.
const TAIL_SHARE: u64 = 16;

/// How many pictures the model has already looked at stay in the
/// transcript. Each one is sent again with every request, at the cost of
/// a long passage, and is seldom looked at twice; the line naming its file
/// stays, so it can be had again.
pub(super) const PICTURES_KEPT: usize = 6;

/// How full the context is, measured where possible and guessed otherwise.
pub(super) struct Gauge {
    /// Tokens the model reported reading and writing at the last step, and
    /// the transcript's size in characters at that moment.
    pub(super) measured: Option<(u64, usize)>,
    /// Characters outside the transcript: the system prompt and tool specs.
    pub(super) fixed: usize,
}

pub(super) fn chars(items: &[Item]) -> usize {
    items.iter().map(Item::weight).sum()
}

impl Gauge {
    pub(super) fn tokens(&self, items: &[Item]) -> u64 {
        let now = chars(items);
        match self.measured {
            Some((tokens, then)) => tokens + (now.saturating_sub(then) / 3) as u64,
            None => ((now + self.fixed) / 3) as u64,
        }
    }

    /// The transcript was made smaller in place. Keep what the last
    /// measurement said about how many characters make a token here, and
    /// apply it to the new size.
    pub(super) fn reweigh(&mut self, items: &[Item]) {
        let now = chars(items);
        if let Some((tokens, then)) = self.measured {
            let was = (then + self.fixed).max(1) as u128;
            let scaled = u128::from(tokens) * (now + self.fixed) as u128 / was;
            self.measured = Some((u64::try_from(scaled).unwrap_or(u64::MAX), now));
        }
    }
}

/// Trim every long tool result but the newest few to its beginning and
/// end. Returns how many characters that freed.
pub(super) fn prune(items: &mut [Item], spare: usize) -> usize {
    let before = chars(items);
    let old = items.len().saturating_sub(spare);
    for item in &mut items[..old] {
        if let Item::Result { output, .. } = item
            && output.len() > PRUNE_OVER
        {
            *output = clip(output, PRUNE_HEAD, PRUNE_TAIL);
        }
    }
    before - chars(items)
}

/// Take pictures out of the transcript, all but the newest `keep` of those
/// the model has already been shown. Pictures that came back since its
/// last step have not been looked at yet, and always stay. Returns how many
/// characters that freed.
pub(super) fn unsee(items: &mut [Item], keep: usize, words: &Prompts) -> usize {
    let before = chars(items);
    let shown = items
        .iter()
        .rposition(|item| matches!(item, Item::Assistant { .. }))
        .unwrap_or(0);
    let mut left = keep;
    for item in items[..shown].iter_mut().rev() {
        let Item::Result {
            output, pictures, ..
        } = item
        else {
            continue;
        };
        if pictures.len() <= left {
            left -= pictures.len();
            continue;
        }
        // Older ones would fit where this did not, but are older still.
        left = 0;
        output.push('\n');
        output.push_str(words.text(if pictures.len() == 1 {
            "no_longer_shown_one"
        } else {
            "no_longer_shown_many"
        }));
        pictures.clear();
    }
    before - chars(items)
}

/// The newest stretch of a transcript that fits in `budget` characters and
/// can stand on its own: it never opens on a result whose call is gone, and
/// never includes the first item, which is the opening it will follow.
fn tail_of(items: &[Item], budget: usize) -> Vec<Item> {
    let mut used = 0;
    let mut from = items.len();
    for (i, item) in items.iter().enumerate().skip(1).rev() {
        used += item.weight();
        if used > budget {
            break;
        }
        from = i;
    }
    while items
        .get(from)
        .is_some_and(|item| matches!(item, Item::Result { .. }))
    {
        from += 1;
    }
    items.get(from..).unwrap_or_default().to_vec()
}

/// Shorten long strings inside a call's arguments (file contents, mostly).
fn thin(args: &mut Value) {
    if let Value::Object(map) = args {
        for value in map.values_mut() {
            if let Value::String(s) = value
                && s.len() > 400
            {
                let kept: String = s.chars().take(160).collect();
                *s = format!("{kept}\n[... the rest was dropped to save context ...]");
            }
        }
    }
}

/// Drop the bulk of everything but the most recent items. Returns how many
/// characters that freed.
fn shed(items: &mut [Item], keep: usize) -> usize {
    let before = chars(items);
    let old = items.len().saturating_sub(keep);
    for item in &mut items[..old] {
        match item {
            Item::Result { output, .. } if output.len() > 400 => {
                let kept: String = output.chars().take(200).collect();
                // What came from outside stays marked as such: the tag
                // that ends it is kept, after the note.
                let close = outside::closing();
                let ended = if output.lines().any(|line| line == close) {
                    format!("\n{close}")
                } else {
                    String::new()
                };
                *output = format!(
                    "{kept}\n[... the rest of this output was dropped to save context; run it again if you need it ...]{ended}"
                );
            }
            // What the provider asked to have handed back (`carry`) stays:
            // some reject a transcript whose earlier thinking is missing.
            Item::Assistant { calls, .. } => calls.iter_mut().for_each(|c| thin(&mut c.args)),
            _ => {}
        }
    }
    before - chars(items)
}

impl Worker {
    /// How many tokens the transcript may reach before it is folded: what
    /// was asked for, or most of the model's window with room left to
    /// answer in, and in either case well under any size the model has
    /// already refused.
    /// How much one step may write.
    pub(super) fn max_output(&self) -> u32 {
        self.limits.max_output.unwrap_or_else(|| {
            self.brain
                .max_output()
                .map_or(UNKNOWN_OUTPUT, |most| most.min(KNOWN_OUTPUT))
        })
    }

    pub(super) fn budget(&self) -> u64 {
        let wanted = self.limits.context_tokens.unwrap_or_else(|| {
            self.brain.window().map_or(UNKNOWN_WINDOW, |window| {
                let reserved = u64::from(self.max_output()) + HEADROOM.min(window / 8);
                (window * 4 / 5).min(window.saturating_sub(reserved))
            })
        });
        match self.undertaking.state().refused_at {
            Some(refused) => wanted.min(refused * UNDER_REFUSAL / 10),
            None => wanted,
        }
        .max(4000)
    }

    /// The size at which a sitting is folded: the budget, or enough room
    /// above where the sitting started to get something done, whichever is
    /// larger. Without the second part a long task statement or handoff
    /// would have every step folding the one before it.
    pub(super) fn ceiling(&self, base: u64) -> u64 {
        let budget = self.budget();
        budget.max(base + budget / 2)
    }

    /// Make room. First on a copy: long output is trimmed, then old output
    /// is dropped in tightening rounds. If that brings the transcript far
    /// enough down, the copy becomes the transcript. If it does not, the
    /// transcript is left as it was for one more request, in which the
    /// worker writes a handoff (`ask`), and then starts again from the
    /// record and the last stretch of what it was doing, untrimmed.
    ///
    /// Returns what the handoff cost, and the size the new sitting starts
    /// at if one began.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn fold(
        &self,
        items: &mut Vec<Item>,
        recorded: &mut usize,
        shown: &Shown,
        tools: &Toolbox,
        gauge: &mut Gauge,
        base: u64,
        ceiling: u64,
        ask: bool,
    ) -> (Usage, Option<u64>) {
        let words = &shown.words;
        // What the sitting started at cannot be folded away, so the aim is
        // set in the room above it.
        let base = base.min(ceiling);
        let target = base + (ceiling - base) * FOLD_TO / 10;
        let mut trial = items.clone();
        let mut trial_gauge = Gauge {
            measured: gauge.measured,
            fixed: gauge.fixed,
        };
        // Pictures go before any text: they cost the most, and are worth
        // the least once they have been looked at.
        let mut freed = unsee(&mut trial, 0, words);
        trial_gauge.reweigh(&trial);
        if trial_gauge.tokens(&trial) > target {
            freed += prune(&mut trial, KEEP_RECENT[2]);
            trial_gauge.reweigh(&trial);
        }
        for keep in KEEP_RECENT {
            if trial_gauge.tokens(&trial) <= target {
                break;
            }
            freed += shed(&mut trial, keep);
            trial_gauge.reweigh(&trial);
        }
        let after = trial_gauge.tokens(&trial);
        if after <= target {
            tracing::info!(
                "folded: dropped {freed} characters of old output, about {after} tokens remain"
            );
            *items = trial;
            *gauge = trial_gauge;
            return (Usage::default(), None);
        }

        // Not enough. The untouched transcript goes to the model once more,
        // with the same tools as every other step so that the request reads
        // from the provider's cache up to its last message.
        self.save(items, recorded);
        let mut spent = Usage::default();
        if ask {
            let asking = Item::user(words.text("handoff"));
            let mut asking_items = items.clone();
            asking_items.push(asking.clone());
            let turn = Turn {
                system: shown.system.clone(),
                items: asking_items,
                tools: shown.specs.clone(),
                max_tokens: self.max_output(),
                temperature: self.limits.temperature,
                effort: self.limits.effort,
            };
            match self.brain.step(&turn).await {
                Ok(step) => {
                    spent = step.usage;
                    for call in &step.calls {
                        if matches!(call.name.as_str(), "handoff" | "plan") {
                            let _ = tools.run(call).await;
                        }
                    }
                    self.undertaking.remember(&[asking, step.into_item()]);
                }
                Err(e) => tracing::warn!("no handoff could be written: {e}"),
            }
        }
        let share = if ask { TAIL_SHARE } else { TAIL_SHARE / 2 };
        let room = usize::try_from(ceiling * share / 100 * 3).unwrap_or(usize::MAX);
        let tail = tail_of(items, room);
        let state = self.undertaking.update(|state| {
            state.sittings += 1;
            state.clone()
        });
        let opening = Item::user(opening_with(words, &state, !tail.is_empty()));
        self.undertaking.remember(std::slice::from_ref(&opening));
        tracing::info!(
            "a new sitting begins from the plan, the handoff and the last {} items",
            tail.len()
        );
        *items = vec![opening];
        items.extend(tail);
        *recorded = items.len();
        gauge.measured = None;
        (spent, Some(gauge.tokens(items)))
    }
}
