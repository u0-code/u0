//! Knobs on how she lives. All of them belong to the body, none to her.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MindConfig {
    /// The pace she lives at. `1.0` is human. `0.1` is good for a demo. `0`
    /// removes every wait and lets her answer in her sleep; development only.
    pub tempo: f32,
    /// Longest thing a brain may write back for one deliberation, in tokens.
    pub max_tokens: u32,
    /// How many messages of a conversation she sees when she looks at it.
    pub thread_window: usize,
    /// Ceiling on brain calls per hour. `0` means no ceiling.
    pub calls_per_hour: u32,
    /// Most public posts she will make in a day.
    pub posts_per_day: u32,
    /// Minutes between phone sessions on channels with a feed, as `[min, max]`.
    /// `[0, 0]` turns scrolling off.
    pub browse_minutes: [u32; 2],
    /// Run the life engine: plan days, live through them, reflect at night.
    pub live: bool,
    /// Look up the real weather where she lives when planning a day.
    pub weather: bool,
    /// A directory of prompt templates that override the shipped ones.
    pub prompts: Option<PathBuf>,
}

impl Default for MindConfig {
    fn default() -> Self {
        Self {
            tempo: 1.0,
            max_tokens: 700,
            thread_window: 40,
            calls_per_hour: 120,
            posts_per_day: 3,
            browse_minutes: [45, 150],
            live: true,
            weather: true,
            prompts: None,
        }
    }
}
