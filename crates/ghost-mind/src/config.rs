//! Knobs on how she lives. All of them belong to the body, none to her.

use std::path::PathBuf;

use serde::Deserialize;

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
    /// Ceiling on brain calls per hour. `0` means no ceiling.
    pub calls_per_hour: u32,
    /// Most public posts she will make in a day.
    pub posts_per_day: u32,
    /// Minutes between phone sessions on channels with a feed, as `[min, max]`.
    /// `[0, 0]` turns scrolling off.
    pub browse_minutes: [u32; 2],
    /// Like posts while scrolling. Off by default: X forbids automated
    /// likes, and a like from her is one.
    pub public_likes: bool,
    /// Reply in public to posts that were not addressed to her. Off by
    /// default: X allows automated replies only to people who wrote to the
    /// account first. Replies to mentions are not affected.
    pub public_replies: bool,
    /// Read a post over before it goes out: for anything learned in
    /// private, anything that contradicts what is true of her, and whether
    /// it is worth posting at all.
    pub gate: bool,
    /// Hours between asking a channel how her posts are doing. `0` is never.
    pub measure_hours: u32,
    /// Run the life engine: plan days, live through them, reflect at night.
    pub live: bool,
    /// Look up the real weather where she lives when planning a day.
    pub weather: bool,
    /// A directory of prompt templates that override the shipped ones.
    pub prompts: Option<PathBuf>,
    /// Whether and how she takes things on.
    pub work: WorkConfig,
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
    /// Steps in one sitting.
    pub sitting_steps: u32,
    /// Minutes in one sitting.
    pub sitting_minutes: u32,
    /// Minutes she rests between sittings, at human pace.
    pub rest_minutes: u32,
    /// Sittings she gives one thing before letting it go.
    pub max_sittings: u32,
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
    /// Days between the times she sits down to look at how her public
    /// writing is going. `0` is never. Needs a channel that reports how
    /// her posts are doing, and a few posts to look at.
    pub review_days: u32,
}

impl Default for WorkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            machine: None,
            workdir: None,
            web: true,
            sitting_steps: 40,
            sitting_minutes: 15,
            rest_minutes: 10,
            max_sittings: 12,
            per_day: 4,
            pursue_days: 2,
            ask_hours: 24,
            context_tokens: None,
            review_days: 7,
        }
    }
}

impl Default for MindConfig {
    fn default() -> Self {
        Self {
            tempo: 1.0,
            max_tokens: 2000,
            thread_window: 40,
            talk_tools: true,
            calls_per_hour: 120,
            posts_per_day: 3,
            browse_minutes: [45, 150],
            public_likes: false,
            public_replies: false,
            gate: true,
            measure_hours: 12,
            live: true,
            weather: true,
            prompts: None,
            work: WorkConfig::default(),
        }
    }
}
