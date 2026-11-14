//! Knobs on how she lives. All of them belong to the body, none to her.

use std::{collections::BTreeMap, path::PathBuf};

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
    /// How many turns a brain gets in one look at a conversation, when what
    /// she does is tool calls: enough to look something up, open it and
    /// remember something before answering, and no more.
    pub talk_steps: u8,
    /// Seconds one search, page, picture or command may take in the middle
    /// of a conversation before it is left and she is told so. Anything
    /// longer is work.
    pub talk_tool_seconds: u64,
    /// Let her use her machine (`shell`, `read`, `write`, `edit`) in the
    /// middle of a conversation, as she does at work, kept small by the
    /// conversation's pace. Only in a private conversation with someone at
    /// least a friend: the same floor as taking work on with a machine.
    pub talk_machine: bool,
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
    /// A directory of prompts that override the built-in ones. Her own
    /// directory's `prompts/` overrides this in turn, without being named
    /// anywhere. A file changed in either is taken in without a restart.
    pub prompts: Option<PathBuf>,
    /// Whether and how she takes things on.
    pub work: WorkConfig,
    /// Who can give her directives, and how one begins. Set from the
    /// top-level `[operators]` table of `ghostd.toml`, not from `[mind]`.
    #[serde(skip)]
    pub operators: Operators,
}

/// The accounts that run her body, and the prefix that turns a message from
/// one of them into a directive. A directive is carried out, not weighed,
/// so what makes one is the account it came from: the prefix alone, from
/// anyone else, is an ordinary message.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Operators {
    /// What a directive begins with, after any leading whitespace. Matched
    /// without regard to case, and a `:` in it matches a full-width `：`.
    pub prefix: String,
    /// Account ids per channel, as the channel's presence reports them.
    #[serde(flatten)]
    pub accounts: BTreeMap<String, Vec<String>>,
}

impl Default for Operators {
    fn default() -> Self {
        Self {
            prefix: "fox:".to_owned(),
            accounts: BTreeMap::new(),
        }
    }
}

impl Operators {
    /// Whether account `id` on `channel` is an operator's.
    pub fn includes(&self, channel: &str, id: &str) -> bool {
        self.accounts
            .get(channel)
            .is_some_and(|ids| ids.iter().any(|i| i == id))
    }

    /// The directive in `text`, without its prefix, if `text` is one: from
    /// an operator's account and beginning with the prefix, with something
    /// after it.
    pub fn directive<'a>(&self, channel: &str, from: &str, text: &'a str) -> Option<&'a str> {
        if !self.includes(channel, from) {
            return None;
        }
        let prefix = self.prefix.trim();
        if prefix.is_empty() {
            return None;
        }
        let text = text.trim_start();
        let mut rest = text.char_indices();
        for want in prefix.chars() {
            let (_, got) = rest.next()?;
            if !same(want, got) {
                return None;
            }
        }
        let after = rest.next().map_or(text.len(), |(at, _)| at);
        Some(text[after..].trim()).filter(|d| !d.is_empty())
    }
}

/// Whether two characters of a prefix are the same, as a person typing it
/// would mean: case aside, and a full-width colon for an ASCII one.
fn same(a: char, b: char) -> bool {
    let fold = |c: char| if c == '：' { ':' } else { c };
    fold(a).to_lowercase().eq(fold(b).to_lowercase())
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
    /// Whether work runs apart from her day, or only while she is at it.
    pub runs: Runs,
    /// Pieces of work that may have a sitting running at once. Another
    /// waits for one of them to end.
    pub at_once: u32,
    /// Steps in one sitting.
    pub sitting_steps: u32,
    /// Minutes in one sitting.
    pub sitting_minutes: u32,
    /// Minutes a piece of work rests between sittings, at human pace. With
    /// `runs = "always"`, a sitting that ran out of steps or time is
    /// followed at once by the next, without this rest.
    pub rest_minutes: u32,
    /// Sittings she gives one thing before letting it go.
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
    /// Days between the times she sits down to look at how her public
    /// writing is going. `0` is never. Needs a channel that reports how
    /// her posts are doing, and a few posts to look at.
    pub review_days: u32,
}

/// When work runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runs {
    /// On the machine, apart from her day: while she sleeps, talks or
    /// reads, and one sitting straight after another until it is done.
    Always,
    /// Only while she is awake and not in a conversation, as something
    /// she sits down to; looking at her phone puts it down.
    Awake,
}

impl Default for WorkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            machine: None,
            workdir: None,
            web: true,
            runs: Runs::Always,
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
            talk_steps: 8,
            talk_tool_seconds: 30,
            talk_machine: true,
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
            operators: Operators::default(),
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
    fn a_directive_is_the_prefix_from_an_operator_whatever_its_case_and_colon() {
        let ops = operators();
        assert_eq!(
            ops.directive("qq", "D6E3", "fox: send the weather"),
            Some("send the weather")
        );
        assert_eq!(
            ops.directive("qq", "D6E3", "  FOX：洗澡前回我"),
            Some("洗澡前回我")
        );
        assert_eq!(ops.directive("qq", "D6E3", "Fox:hi"), Some("hi"));
    }

    #[test]
    fn the_prefix_alone_does_not_make_a_directive() {
        let ops = operators();
        // From anyone else, on the channel or another, it is a message.
        assert_eq!(ops.directive("qq", "A1B2", "fox: send the weather"), None);
        assert_eq!(
            ops.directive("telegram", "D6E3", "fox: send the weather"),
            None
        );
        // From an operator, it has to begin the message and say something.
        assert_eq!(ops.directive("qq", "D6E3", "hey fox: send it"), None);
        assert_eq!(ops.directive("qq", "D6E3", "foxy: send it"), None);
        assert_eq!(ops.directive("qq", "D6E3", "fox:   "), None);
        assert_eq!(ops.directive("qq", "D6E3", "f"), None);
    }
}
