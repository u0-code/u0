//! Every tool a conversation offers her, or tells her of, defined once.
//!
//! Each entry says what it is, what a place must have before it is
//! offered there, and whether what it does comes back to her and so asks
//! for another turn. What a place is decides what is offered there,
//! through these, and nothing else does; the order here is the order she
//! is offered them in. What each tool is said to be is in `tools.toml`.
//!
//! Three kinds. Saying something, reacting, keeping quiet, leaving, taking
//! work on and steering it are things she does with people in front of
//! her, read straight into an [`Action`](crate::actions::Action): their
//! arguments are here, and a test holds them to what an action reads. The
//! web, her servers of tools and her skills reach beyond the conversation
//! and run as they do at work, off to the side, a few seconds each: they
//! are offered when she has them, as ghost-work describes them. A few
//! tools only work has are told of where she could take work on, since
//! someone may want what she can only get by working at it.

/// What a tool needs of a place, or of her, before it is offered there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A body that can take work on.
    Work,
    /// Work of hers under way, to steer or stop.
    UnderWay,
    /// Her memory to search.
    Memory,
    /// A place that takes files.
    Files,
    /// A place where several people talk, not all of it to her.
    Group,
    /// A place among people, where she keeps her own pace: not a terminal,
    /// where she is waited on. Putting off an answer, reacting instead of
    /// answering, being moved by someone and leaving them are things she
    /// does with people, not with whoever runs her at a console.
    Social,
    /// A channel that can put a reaction on a message.
    React,
    /// A channel that lets her write to someone who has not just written
    /// to her: coming back to a conversation later, or saying something she
    /// meant to, needs one.
    WritesFirst,
    /// Work that could find servers of tools in a registry and install them.
    Registry,
    /// Work that could find skills in an index and install them.
    SkillIndex,
}

/// One argument of something she does: its name, its JSON type, and
/// whether a call must give it.
pub type Arg = (&'static str, &'static str, bool);

/// What a tool is.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// Something she does with people in front of her, with its arguments.
    Hers(&'static [Arg]),
    /// A tool of work that reaches beyond the conversation, offered when
    /// she has it here.
    Reaching,
    /// A tool only work has, never offered in a conversation.
    Work,
}

/// One tool.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub name: &'static str,
    pub kind: Kind,
    pub needs: &'static [Need],
    /// Whether what it does comes back to her, so that it is worth
    /// nothing on the last turn of a look.
    pub brings_back: bool,
}

const fn hers(name: &'static str, args: &'static [Arg], needs: &'static [Need]) -> Entry {
    Entry {
        name,
        kind: Kind::Hers(args),
        needs,
        brings_back: false,
    }
}

const fn reaching(name: &'static str) -> Entry {
    Entry {
        name,
        kind: Kind::Reaching,
        needs: &[],
        brings_back: true,
    }
}

const fn work(name: &'static str, needs: &'static [Need]) -> Entry {
    Entry {
        name,
        kind: Kind::Work,
        needs,
        brings_back: false,
    }
}

/// Every tool, in the order she is offered them.
pub const TOOLS: &[Entry] = &[
    hers(
        "say",
        &[("text", "string", true), ("reply_to", "integer", false)],
        &[],
    ),
    hers(
        "react",
        &[("to", "integer", true), ("emoji", "string", true)],
        &[Need::Social, Need::React],
    ),
    hers(
        "later",
        &[("minutes", "number", true), ("why", "string", false)],
        &[Need::Social, Need::WritesFirst],
    ),
    hers(
        "note",
        &[("about", "string", false), ("text", "string", true)],
        &[],
    ),
    hers(
        "feel",
        &[
            ("toward", "string", false),
            ("delta", "number", true),
            ("why", "string", false),
        ],
        &[Need::Social],
    ),
    hers(
        "intend",
        &[
            ("hours", "number", true),
            ("what", "string", true),
            ("about", "string", false),
        ],
        &[Need::WritesFirst],
    ),
    hers("canon", &[("fact", "string", true)], &[]),
    Entry {
        brings_back: true,
        ..hers("recall", &[("query", "string", true)], &[Need::Memory])
    },
    Entry {
        brings_back: true,
        ..hers("reread", &[("memory", "integer", true)], &[Need::Memory])
    },
    hers(
        "keep",
        &[
            ("file", "string", false),
            ("message", "integer", false),
            ("image", "integer", false),
            ("note", "string", true),
        ],
        &[],
    ),
    hers("let_go", &[("thing", "integer", true)], &[]),
    reaching("search"),
    reaching("fetch"),
    reaching("shell"),
    reaching("read"),
    reaching("write"),
    reaching("edit"),
    reaching("load_tools"),
    work("find_tools", &[Need::Registry]),
    reaching("use_skill"),
    work("find_skills", &[Need::SkillIndex]),
    hers(
        "undertake",
        &[
            ("brief", "string", true),
            ("for", "string", false),
            ("dir", "string", false),
        ],
        &[Need::Work],
    ),
    hers(
        "steer",
        &[("work", "integer", true), ("text", "string", true)],
        &[Need::UnderWay],
    ),
    hers("stop", &[("work", "integer", true)], &[Need::UnderWay]),
    hers(
        "send_file",
        &[
            ("file", "string", false),
            ("message", "integer", false),
            ("image", "integer", false),
            ("text", "string", false),
            ("what", "string", false),
        ],
        &[Need::Files],
    ),
    hers("quiet", &[("hours", "number", true)], &[Need::Group]),
    hers("leave", &[("why", "string", false)], &[Need::Social]),
];

/// The tool called `name`, if there is one.
pub fn find(name: &str) -> Option<&'static Entry> {
    TOOLS.iter().find(|e| e.name == name)
}

/// Whether a call to `name` comes back to her. A tool that is in no
/// catalogue, such as one of a server she loaded, does.
pub fn brings_back(name: &str) -> bool {
    find(name).is_none_or(|e| e.brings_back)
}

#[cfg(test)]
mod tests {
    use ghost_brain::Call;
    use serde_json::{Map, Value, json};

    use super::*;
    use crate::{Prompts, actions};

    /// A value of JSON type `kind`.
    fn sample(kind: &str) -> Value {
        match kind {
            "integer" => json!(3),
            "number" => json!(2.5),
            _ => json!("x"),
        }
    }

    fn call(name: &str, args: &[&Arg]) -> Call {
        let args: Map<String, Value> = args
            .iter()
            .map(|(arg, kind, _)| ((*arg).to_owned(), sample(kind)))
            .collect();
        Call {
            id: "1".into(),
            name: name.into(),
            args: Value::Object(args),
        }
    }

    #[test]
    fn every_tool_is_described() {
        let words = Prompts::shipped();
        for e in TOOLS {
            assert!(
                !words.tool(e.name).is_empty(),
                "{} is not described",
                e.name
            );
        }
    }

    #[test]
    fn every_argument_offered_for_what_she_does_is_one_her_action_reads() {
        let words = Prompts::shipped();
        for e in TOOLS {
            let Kind::Hers(args) = e.kind else { continue };
            let required: Vec<&Arg> = args.iter().filter(|a| a.2).collect();
            let least = actions::from_call(&words, &call(e.name, &required))
                .unwrap_or_else(|why| panic!("{} with what it requires: {why}", e.name));
            for arg in args {
                if arg.2 {
                    // Left out, a call is refused: it is required.
                    let without: Vec<&Arg> =
                        required.iter().copied().filter(|a| a.0 != arg.0).collect();
                    assert!(
                        actions::from_call(&words, &call(e.name, &without)).is_err(),
                        "{} reads without {}",
                        e.name,
                        arg.0
                    );
                } else {
                    // Given, it changes what she does: it is read.
                    let mut with = required.clone();
                    with.push(arg);
                    let more = actions::from_call(&words, &call(e.name, &with))
                        .unwrap_or_else(|why| panic!("{} with {}: {why}", e.name, arg.0));
                    assert_ne!(more, least, "{} does not read {}", e.name, arg.0);
                }
            }
        }
    }
}
