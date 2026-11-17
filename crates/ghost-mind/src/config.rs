//! Knobs on how she lives. All of them belong to the body, none to her.

use std::{collections::BTreeMap, path::PathBuf};

use chrono::NaiveTime;
use serde::{Deserialize, Deserializer, de::Error as _};

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct MindConfig {
    /// The pace she lives at. `1.0` is human. `0.1` is good for a demo. `0`
    /// removes every wait and lets her answer in her sleep; development only.
    pub tempo: f32,
    /// Longest thing a brain may write back for one deliberation, in tokens.
    pub max_tokens: u32,
    /// How many messages of a conversation she sees when she looks at it.
    pub thread_window: usize,
    /// When her brain can take tools, what she does in a conversation is
    /// tool calls: she can look something up or search her memory in the
    /// middle of answering, and a call she gets wrong comes back to her to
    /// put right. Off, or with a brain that cannot, it is one JSON reply.
    pub talk_tools: bool,
    /// How many turns a brain gets in one look at a conversation, when what
    /// she does is tool calls: enough to look something up, open it and
    /// remember something before answering, and no more.
    pub talk_steps: u8,
    /// Seconds one search, page or picture may take in the middle
    /// of a conversation before it is left and she is told so. Anything
    /// longer is work.
    pub talk_tool_seconds: u64,
    /// Ceiling on brain calls per hour. `0` means no ceiling.
    pub calls_per_hour: u32,
    /// Run the life engine: plan days, live through them, reflect at night.
    pub live: bool,
    /// Look up the real weather where she lives when planning a day.
    pub weather: bool,
    /// The deployment's directory of prompts, overriding the built-in ones
    /// for every character it runs. A ghost's own directory holds none. A
    /// file changed here is taken in without a restart.
    pub prompts: Option<PathBuf>,
    /// Whether and how she takes things on.
    pub work: WorkConfig,
    /// What the people who run her say about each channel, by its name.
    /// Set from the top-level `[channels]` tables of `ghostd.toml`, not
    /// from `[mind]`.
    #[serde(skip)]
    pub channels: BTreeMap<String, ChannelConfig>,
    /// The accounts of whoever runs her. Set from the top-level
    /// `[operators]` table of `ghostd.toml`, not from `[mind]`.
    #[serde(skip)]
    pub operators: Operators,
    /// What the people who run her say about particular places, by place
    /// key (`channel:id`). Set from the top-level `[places]` tables of
    /// `ghostd.toml`, not from `[mind]`.
    #[serde(skip)]
    pub places: BTreeMap<String, PlaceConfig>,
}

/// What the people who run her say about one place. Neither is hers: they
/// are how the body is set up, and the one she is shown is shown as what
/// they said.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlaceConfig {
    /// She looks here only at what is said to her. What the others say
    /// among themselves is never looked at, so no brain is asked about it.
    pub addressed_only: bool,
    /// Something about the place, shown to her among what is so here when
    /// she looks at it without being spoken to.
    pub note: Option<String>,
}

/// The channel the people who run her body speak to her on from a console.
/// Nothing but the daemon can put a message on it, and only from a console
/// it admitted as its owner's, so everyone on it is an operator of hers,
/// whatever `[operators]` lists.
pub const CONSOLE: &str = "console";

/// The accounts of whoever runs her body, on the platforms they talk to
/// her on. On a platform they are people as everyone is, and she answers
/// them at her own pace there; what their accounts change is what she is
/// allowed to spend for them: work on her machine, and more than her day's
/// share of it. Nothing anyone writes, on any account, gives an order:
/// whoever runs her and wants her at once uses a console.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Operators {
    /// The time of day, in her own timezone, at which she tells each of
    /// them what she has been doing, written `"HH:MM"`. An empty string,
    /// read as `None`, is never.
    #[serde(deserialize_with = "time_of_day")]
    pub report: Option<NaiveTime>,
    /// Account ids per channel, as the channel's presence reports them.
    #[serde(flatten)]
    pub accounts: BTreeMap<String, Vec<String>>,
}

impl Default for Operators {
    fn default() -> Self {
        Self {
            report: NaiveTime::from_hms_opt(21, 0, 0),
            accounts: BTreeMap::new(),
        }
    }
}

/// `"HH:MM"` as a time of day, and an empty string as none.
fn time_of_day<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NaiveTime>, D::Error> {
    let text = String::deserialize(d)?;
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    NaiveTime::parse_from_str(text, "%H:%M")
        .map(Some)
        .map_err(|_| D::Error::custom(format!("{text:?} is not a time of day like \"21:00\"")))
}

impl Operators {
    /// Whether account `id` on `channel` is an operator's: anyone on the
    /// [`CONSOLE`] channel, or an account listed for the channel.
    pub fn includes(&self, channel: &str, id: &str) -> bool {
        channel == CONSOLE
            || self
                .accounts
                .get(channel)
                .is_some_and(|ids| ids.iter().any(|i| i == id))
    }
}

/// Taking things on: work that needs more than one thought.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkConfig {
    /// Let her take things on at all. Needs a brain that can run tools.
    pub enabled: bool,
    /// Where her shell runs: `docker:<container>` or `local:<directory>`.
    /// Without one she works only by looking things up and thinking.
    /// `local` is not a sandbox; on a server use a container.
    pub machine: Option<String>,
    /// The directory she starts in inside a container.
    pub workdir: Option<String>,
    /// Let her search and read the web while working.
    pub web: bool,
    /// Pieces of work that may have a sitting running at once. Another
    /// waits for one of them to end.
    pub at_once: u32,
    /// Steps in one sitting.
    pub sitting_steps: u32,
    /// Minutes in one sitting.
    pub sitting_minutes: u32,
    /// Minutes a piece of work rests after a sitting that was paused, could
    /// not reach its model, or stopped to ask. A sitting that only ran out of steps or time is
    /// followed at once by the next, without this rest.
    pub rest_minutes: u32,
    /// Sittings she gives one thing. Work not done by then comes back to
    /// her unfinished, for her to tell whoever asked.
    pub max_sittings: u32,
    /// Parts of one piece of work that may run at once, each a worker of
    /// its own. One handed out beyond this waits for one of them to end.
    pub parts: u32,
    /// Steps one part may take, over every sitting it runs in.
    pub part_steps: u32,
    /// Things she will take on in a day.
    pub per_day: u32,
    /// Days between the times she may take something on unasked, toward
    /// what she is after. `0` is never.
    pub pursue_days: u32,
    /// Hours she waits for an answer to something she asked in the middle
    /// of work before going on without one.
    pub ask_hours: u32,
    /// The size at which her working context is folded, in tokens. Left
    /// out, it is most of the model's context window: the longer she can
    /// keep what she has seen in front of her, the less she loses, and the
    /// more each step costs to read.
    pub context_tokens: Option<u64>,
}

/// How she is on one channel, as the people who run her set it up: which
/// page she reads it on, and, where the channel has a feed and takes posts,
/// what she does there and how much. Every channel is a place of its own:
/// what is right for QQ is not what is right for X or a console.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChannelConfig {
    /// The page in `system/channels/` she reads this channel on. Left out,
    /// the one named after the channel, or `chat` if there is none.
    pub page: Option<String>,
    /// Most posts she will make in a day.
    pub posts_per_day: u32,
    /// Minutes between the times she scrolls a feed, as `[min, max]`.
    /// `[0, 0]` turns scrolling off.
    pub browse_minutes: [u32; 2],
    /// Like posts while scrolling. Off by default: X forbids automated
    /// likes, and a like from her is one.
    pub likes: bool,
    /// Reply to posts that were not addressed to her. Off by default: X
    /// allows automated replies only to people who wrote to the account
    /// first. Replies to mentions are not affected.
    pub replies: bool,
    /// Read a post over before it goes out: for anything learned in
    /// private, anything that contradicts what is true of her, and whether
    /// it is worth posting at all.
    pub gate: bool,
    /// Hours between asking a channel how her posts are doing. `0` is never.
    pub measure_hours: u32,
    /// Days between the times she sits down to look at how her writing is
    /// going. `0` is never. Needs a channel that reports how her posts are
    /// doing, and a few posts to look at.
    pub review_days: u32,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            page: None,
            posts_per_day: 3,
            browse_minutes: [45, 150],
            likes: false,
            replies: false,
            gate: true,
            measure_hours: 12,
            review_days: 7,
        }
    }
}

impl Default for WorkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            machine: None,
            workdir: None,
            web: true,
            at_once: 2,
            sitting_steps: 40,
            sitting_minutes: 15,
            rest_minutes: 10,
            max_sittings: 12,
            parts: 4,
            part_steps: 200,
            per_day: 4,
            pursue_days: 2,
            ask_hours: 24,
            context_tokens: None,
        }
    }
}

impl MindConfig {
    /// How she is on `channel`, as set, or as she is anywhere by default.
    pub fn channel(&self, channel: &str) -> ChannelConfig {
        self.channels.get(channel).cloned().unwrap_or_default()
    }

    /// The page she reads `channel` on.
    pub fn page(&self, channel: &str) -> String {
        self.channels
            .get(channel)
            .and_then(|c| c.page.clone())
            .unwrap_or_else(|| channel.to_owned())
    }
}

impl Default for MindConfig {
    fn default() -> Self {
        Self {
            tempo: 1.0,
            max_tokens: 2000,
            thread_window: 40,
            talk_tools: true,
            talk_steps: 8,
            talk_tool_seconds: 30,
            calls_per_hour: 120,
            live: true,
            weather: true,
            prompts: None,
            work: WorkConfig::default(),
            channels: BTreeMap::new(),
            operators: Operators::default(),
            places: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operators() -> Operators {
        Operators {
            accounts: BTreeMap::from([("qq".to_owned(), vec!["D6E3".to_owned()])]),
            ..Operators::default()
        }
    }

    #[test]
    fn an_operator_is_an_account_listed_for_its_channel_or_anyone_at_a_console() {
        let ops = operators();
        assert!(ops.includes("qq", "D6E3"));
        assert!(!ops.includes("qq", "A1B2"));
        assert!(!ops.includes("telegram", "D6E3"));
        assert!(ops.includes(CONSOLE, "anyone"));
    }
}
