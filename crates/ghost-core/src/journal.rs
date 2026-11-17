//! Her life, as an append-only, hash-chained log.
//!
//! Every state in the core is derived from this log. There is no `update` and
//! no `delete`; the `SQLite` schema carries triggers that abort both. The chain
//! is not a security boundary (anyone with the file can rewrite it), it is an
//! honesty boundary: tampering has to be deliberate, and it leaves a mark the
//! World can later detect.
//!
//! The [`Event`] enum is a forever format. Add variants and optional fields;
//! never rename or remove.

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    identity::Seal,
    life::Block,
    social::{Actor, Place},
};

/// File name of the journal inside a ghost directory.
pub const JOURNAL_FILE: &str = "journal.db";

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const COLUMNS: &str = "seq, at, kind, payload, prev_hash, hash";

/// Something that happened to her, or something she did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The first entry of every journal. Carries the seal of her persona
    /// as it was at birth.
    Born {
        name: String,
        seal: Seal,
    },
    /// Her body was not running between `since` and this entry.
    Dormant {
        since: DateTime<Utc>,
    },
    Woke,
    Slept,
    /// What she intends to do with a day. `day` is the local date she woke on.
    Planned {
        day: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weather: Option<String>,
        blocks: Vec<Block>,
    },
    /// Something she experienced. `block` indexes the day's plan when the
    /// experience is how a planned block actually went.
    Lived {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block: Option<u32>,
    },
    /// A message reached her. She has not necessarily looked at it yet.
    Heard {
        channel: String,
        place: Place,
        from: Actor,
        /// The channel's own id for the message.
        id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        /// It was meant for her: a direct message, a mention, or a reply.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        addressed: bool,
        /// Where the channel kept the images in it, in the order of their
        /// `[image]` placeholders in `text`. The links expire; what she made
        /// of each image is a `Viewed` entry.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<String>,
    },
    /// She looked at a place and saw everything up to journal entry `upto`.
    Read {
        channel: String,
        place: String,
        upto: i64,
    },
    /// She sent a message.
    ///
    /// When something of hers went with it, `text` begins with what it was,
    /// in brackets: `[image: …]` for an image, in her words for it, or
    /// `[file: <name>]` for a file she made. The line reads that way from
    /// then on, so she can see later what she sent.
    Said {
        channel: String,
        place: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        /// The name in `media/` of the image she sent with it, if she sent
        /// one: which bytes it was, beside what the text says of them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
    /// She reacted to a message.
    Reacted {
        channel: String,
        place: String,
        to: String,
        emoji: String,
    },
    /// She posted something publicly, to no one in particular.
    Posted {
        channel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        text: String,
    },
    /// What she took away from scrolling a feed.
    Saw {
        channel: String,
        text: String,
    },
    /// A decision she made, and why.
    Decided {
        about: String,
        decision: String,
        reason: String,
    },
    /// A shift in how she feels about someone. `toward` is a person key.
    Felt {
        toward: String,
        delta: i8,
        why: String,
    },
    /// Something about a person worth keeping. `about` is a person key.
    Noted {
        about: String,
        text: String,
        /// The place she was in when she learned it, so that what was said
        /// in private is not later repeated somewhere else.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
        /// What she had in front of her when she wrote it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<Sources>,
    },
    /// Something she means to do later.
    Intended {
        due: DateTime<Utc>,
        what: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        about: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
        /// She said she would come back with this. Following through on it
        /// is not chasing.
        #[serde(default, skip_serializing_if = "is_false")]
        owed: bool,
        /// It comes of work an operator directed. Like the directive, it
        /// does not wait on her day: not on sleep, not on being busy.
        #[serde(default, skip_serializing_if = "is_false")]
        directed: bool,
        /// It is her daily report to an operator, `about`, of what she has
        /// done and seen. Her last one to them is where the next one starts,
        /// and there is never more than one to them in a day.
        #[serde(default, skip_serializing_if = "is_false")]
        report: bool,
    },
    /// An intention was carried out or dropped. `intention` is its seq.
    Settled {
        intention: i64,
        outcome: String,
    },
    /// She ended a relationship. There is no event that undoes this.
    Left {
        person: String,
        why: String,
    },
    /// A reflection from her sleep cycle: on her own day, or, with
    /// `channel`, on what went on there.
    Reflected {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<Sources>,
    },
    /// A hard fact about her life, registered in the canon.
    Canon {
        fact: String,
        source: String,
    },
    /// She took something on: a piece of work that will take more than one
    /// thought. `id` names its directory.
    Undertook {
        id: String,
        brief: String,
        /// Who it is for, if anyone asked.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asked_by: Option<String>,
        /// Where they asked.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
        /// An operator directed it. It goes ahead of other work, and what
        /// she means to do because of it does not wait on her day.
        #[serde(default, skip_serializing_if = "is_false")]
        directed: bool,
    },
    /// One sitting of work on an undertaking.
    Worked {
        id: String,
        /// Where she got to, in her own words.
        summary: String,
        steps: u32,
    },
    /// In the middle of an undertaking she needed something from whoever
    /// it is for, and stopped to ask.
    Asked {
        id: String,
        question: String,
    },
    /// Something was added to an undertaking after it began: more of what
    /// was wanted, or the answer to what she asked.
    Added {
        id: String,
        text: String,
        /// Who added it, when it was not passed on by her: the key of an
        /// operator who answered the work, or added to it, from a console.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<String>,
    },
    /// An undertaking ended: `finished`, `blocked`, `gave_up` or `dropped`.
    Concluded {
        id: String,
        outcome: String,
        summary: String,
    },
    /// What a channel reported about how her public posts are doing.
    Measured {
        channel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        followers: Option<u64>,
        #[serde(default)]
        posts: Vec<PostStats>,
    },
    /// Someone said they are a person she knows on another platform, and
    /// she asked for proof: a word she made up, to be sent to her from the
    /// account she already knows.
    Challenged {
        /// Who says so.
        claimant: String,
        /// Who they say they are.
        claimed: String,
        word: String,
    },
    /// Two accounts were shown to be one person: the word came back from
    /// the right place.
    Linked {
        one: String,
        other: String,
    },
    /// She put what she knows of someone in order: a short account that
    /// replaces the one before it. What she could have learned where others
    /// were present is kept apart from what they told her alone.
    Profiled {
        person: String,
        /// What she knows of them from shared and public places.
        shared: String,
        /// What they told her in private. Never for use anywhere else.
        #[serde(default)]
        private: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<Sources>,
    },
    /// Something she is after over months, in her own words: a direction,
    /// not a task. It replaces an earlier one when she has changed her
    /// mind; an empty `what` with `replaces` lets that one go.
    Pursued {
        what: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaces: Option<i64>,
    },
    /// Something doing a piece of work taught her about how to do things:
    /// what she would do differently, or a way that worked and was not
    /// obvious. Replaces an earlier one when she has learned better.
    Practised {
        lesson: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaces: Option<i64>,
    },
    /// She put what she wants to keep of the earlier part of a conversation
    /// into a few lines, replacing whatever she kept of it before. The
    /// messages up to entry `upto` stand behind it; later ones are still
    /// read as they were written.
    Digested {
        place: String,
        upto: i64,
        text: String,
        /// Everything it stands for, the earlier notes it replaces included.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<Sources>,
    },
    /// She looked something up on the web, and this is what came back: kept
    /// so that what she found out is something she knows, and can find
    /// again, and not only something that was in front of her once.
    LookedUp {
        query: String,
        found: String,
        /// The place she was in when she looked it up.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
    },
    /// Her persona as it stands from here on: written at birth and again
    /// whenever it is revised. The whole text is kept, so the journal alone
    /// says who she was at any point in her life.
    Described {
        /// Her persona. The field keeps the name it was first written
        /// under: the journal is a forever format.
        certificate: String,
        seal: Seal,
    },
    /// Something she worked out about her own craft: what she does well in
    /// public, what falls flat, what to try. Replaces an earlier lesson when
    /// she has changed her mind.
    Learned {
        lesson: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaces: Option<i64>,
    },
    /// She looked at an image in a message she heard. `message` is the seq
    /// of that `Heard` entry and `image` the image's place among its images.
    Viewed {
        message: i64,
        image: u32,
        /// What she saw: what is in it, what it says, what it means.
        what: String,
        /// The name it is kept under in `media/`, if it was kept.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
    /// She kept something she had seen, to have it again: the way people
    /// save a picture on their phone. Keeping the same thing again replaces
    /// the note; the latest `Kept` of a thing is the one that holds.
    ///
    /// What is kept is named by its own field. Today that is `file`, a name
    /// in `media/`; another kind of thing (a link, a quote) comes as an
    /// optional field of its own beside it.
    Kept {
        /// The name it is kept under in `media/`.
        file: String,
        /// In her own words: what it is, and when she would want it again.
        note: String,
        /// Where she got it. A place key (`channel:id`) when it was sent to
        /// her in a conversation; the id of an undertaking, as its
        /// `Undertook` names it, when she came across it in a piece of work;
        /// a link (with its `scheme://`) when it came straight from the web.
        /// They are told apart by their shape: a link has `://`, a place key
        /// has a `:`, and an undertaking's id has neither.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    /// She let go of something she kept. `kept` is the seq of its `Kept`
    /// entry.
    LetGo {
        kept: i64,
    },
    /// While a piece of work was under way, it told her something worth
    /// knowing before it was done: a finding, a partial result, a file
    /// ready to hand over. Only what the work chose to say is here, never
    /// what it did to find it out.
    Reported {
        /// The undertaking, as its `Undertook` names it.
        id: String,
        text: String,
        /// Files it made that are ready to hand over: paths on her machine.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
    },
    /// She opened something on the web, or on her machine, in the middle of
    /// a conversation: a page she read, or an image she looked at. Kept so
    /// that what she read is something she knows, and so that an image she
    /// opened in a conversation can be kept or sent from it by the name
    /// she was shown, and from nowhere else.
    Opened {
        /// The address, or the path on her machine.
        url: String,
        /// What she read, cut short; or, for an image, a line about it.
        text: String,
        /// For an image, its file under `media/`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        /// The place she was in when she opened it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<String>,
    },
    /// A message she heard was a directive: it came from an account the
    /// body is configured to take directives from, and began with the
    /// prefix it is configured with. She carries a directive out rather
    /// than deciding whether to, so each one is on the record.
    Directed {
        /// The seq of the `Heard` entry the directive came in.
        message: i64,
        /// Who gave it, as a person key (`channel:id`).
        from: String,
        /// The directive, without the prefix.
        text: String,
    },
    /// Someone in a group asked her to be quiet, and she agreed to: until
    /// `until`, she does not look at what is said there unless it is said
    /// to her. A later one for the same place replaces it.
    Quieted {
        /// The place, as a place key (`channel:id`).
        place: String,
        until: DateTime<Utc>,
    },
    /// She installed a server of tools she found in the registry, and it
    /// ran and said what it offers. Everything needed to run the same thing
    /// again is here, so that a body started later runs exactly this and
    /// nothing newer.
    Installed {
        /// Its name in the registry, as `namespace/name`.
        name: String,
        /// The version the registry listed.
        version: String,
        /// What the registry says it is: the line she is shown it by
        /// among her tools.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        /// The exact package or remote run: `npm:<package>@<version>`,
        /// `pypi:<package>==<version>`, or the address of a remote.
        source: String,
        /// Who the registry vouches it belongs to: `github.com/<account>`
        /// for a name under `io.github`, otherwise the domain its name is
        /// under.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<String>,
        how: Launch,
        /// The names of the tools it offered.
        tools: Vec<String>,
        /// A hash of what it said of each tool, descriptions and schemas,
        /// when it was installed. If it later says something else, it is
        /// not offered again until she installs it again.
        hash: String,
    },
    /// A server of tools she had installed is gone: she took it out, or it
    /// stopped being one she could use. `why` says which, in her words or
    /// her body's.
    Uninstalled {
        name: String,
        why: String,
    },
    /// She installed an agent skill: a folder of written know-how, a
    /// `SKILL.md` and the files it points to, that someone published in a
    /// repository on GitHub and an index of skills lists. A skill is read,
    /// not run, so it is not an `Installed` server: what it takes to have
    /// the same one again is where it came from and the commit, and the
    /// hash says whether the folder on disk is still what was taken.
    SkillInstalled {
        /// Its name, as its `SKILL.md` gives it and its folder is called.
        name: String,
        /// The repository it came from, as `owner/repo` on GitHub.
        source: String,
        /// The folder it was taken from inside that repository.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        path: String,
        /// The commit it was taken at.
        sha: String,
        /// A hash of every file of it as it was written to disk.
        hash: String,
        /// What its `SKILL.md` says it is for: the line she is shown it by.
        description: String,
    },
    /// A skill she had installed is gone: she took it out, or its files
    /// changed on disk after she installed it. `why` says which.
    SkillUninstalled {
        name: String,
        why: String,
    },
    /// Someone who runs her body stopped her in a place, from a console,
    /// as one stops a person mid-sentence: what she was thinking there was
    /// abandoned, what she had not yet said there was not said, and what
    /// was unread there was left read. It is what she remembers of being
    /// interrupted, so that she does not take up again unasked what was
    /// called off. The work she had taken on there goes on, stopped only
    /// from its own card; journals from before that have it in `works`.
    Stopped {
        /// The place, as a place key (`console:fox`).
        place: String,
        /// Who stopped her, as a person key.
        by: String,
        /// She was looking at what was said there, or about to: what she
        /// would have made of it was never had.
        #[serde(default, skip_serializing_if = "is_false")]
        looking: bool,
        /// What she was about to say there and did not, in order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unsaid: Vec<String>,
        /// The work taken on there that was stopped with her, by id. Each
        /// ends as `dropped` in an entry of its own.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        works: Vec<String>,
    },
    /// Someone who runs her body took back a line of theirs in a place,
    /// from a console, to put it another way: as one rewinds a coding
    /// agent to before a turn. From the line on, what was said there,
    /// by them and by her, is out of the conversation in front of her,
    /// as if it had not been said; the journal keeps every word. What
    /// she was in the middle of there is called off as a stop calls it
    /// off, and her work there goes on.
    Rewound {
        /// The place, as a place key (`console:fox`).
        place: String,
        /// Who took the line back, as a person key.
        by: String,
        /// The seq of the `Heard` entry the line came in. The conversation
        /// stands as it did before it.
        to: i64,
        /// She was looking at what was said there, or about to, and was
        /// stopped in it.
        #[serde(default, skip_serializing_if = "is_false")]
        looking: bool,
        /// What she was about to say there and did not, in order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unsaid: Vec<String>,
    },
}

/// How a server of tools is run, exactly. One that runs on her body's
/// machine has a `command`; one elsewhere has a `url`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    /// The program and its arguments, with every version pinned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    /// The address of a server elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Variables it is always given, as `NAME=value`: values it declared
    /// as defaults, none of them secret.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set: Vec<String>,
    /// Variables it declared that it may be given from the body's
    /// environment, if whoever runs her body has said so for it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pass: Vec<String>,
}

/// The stretch of her journal a piece of memory was made from: entries
/// `from` to `upto`, which happened between `since` and `until`. The
/// entries are still there to be read; this says which ones they were, and
/// when, so the memory can be dated however far back it is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sources {
    pub from: i64,
    pub upto: i64,
    pub since: DateTime<Utc>,
    pub until: DateTime<Utc>,
}

/// How one public post is doing, as the platform counts it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostStats {
    /// The platform's id for the post.
    pub id: String,
    #[serde(default)]
    pub likes: u64,
    #[serde(default)]
    pub replies: u64,
    #[serde(default)]
    pub reposts: u64,
    #[serde(default)]
    pub views: u64,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// Every kind of event there is, by its variant and by the name it is
/// written under. One list gives both [`Event::kind`] and [`Event::KINDS`],
/// and the match it makes is exhaustive: a new variant does not compile
/// until it is named here, and once it is, the compatibility test asks for
/// an entry of it as it is written (`tests/fixtures/events.jsonl`).
macro_rules! kinds {
    ($($variant:ident => $name:literal),* $(,)?) => {
        impl Event {
            /// The name this event is written under.
            pub fn kind(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => $name,)*
                }
            }

            /// The name of every kind of event, in the order they were added.
            pub const KINDS: &'static [&'static str] = &[$($name),*];
        }
    };
}

kinds! {
    Born => "born",
    Dormant => "dormant",
    Woke => "woke",
    Slept => "slept",
    Planned => "planned",
    Lived => "lived",
    Heard => "heard",
    Read => "read",
    Said => "said",
    Reacted => "reacted",
    Posted => "posted",
    Saw => "saw",
    Decided => "decided",
    Felt => "felt",
    Noted => "noted",
    Intended => "intended",
    Settled => "settled",
    Left => "left",
    Reflected => "reflected",
    Canon => "canon",
    Undertook => "undertook",
    Worked => "worked",
    Asked => "asked",
    Added => "added",
    Concluded => "concluded",
    Measured => "measured",
    Learned => "learned",
    Profiled => "profiled",
    Challenged => "challenged",
    Linked => "linked",
    Digested => "digested",
    Pursued => "pursued",
    Practised => "practised",
    LookedUp => "looked_up",
    Described => "described",
    Viewed => "viewed",
    Kept => "kept",
    LetGo => "let_go",
    Reported => "reported",
    Opened => "opened",
    Directed => "directed",
    Quieted => "quieted",
    Installed => "installed",
    Uninstalled => "uninstalled",
    SkillInstalled => "skill_installed",
    SkillUninstalled => "skill_uninstalled",
    Stopped => "stopped",
    Rewound => "rewound",
}

/// One line of her journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub event: Event,
    pub prev_hash: String,
    pub hash: String,
}

pub struct Journal {
    conn: Connection,
}

impl Journal {
    /// Create a new, empty journal. Fails if one already exists.
    pub fn create(path: &Path) -> Result<Self> {
        if path.exists() {
            return Err(Error::AlreadyBorn(path.to_path_buf()));
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open an existing journal. Fails if there is none.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::NoJournal(path.to_path_buf()));
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self { conn })
    }

    /// Append an event, stamped now.
    pub fn append(&mut self, event: Event) -> Result<Entry> {
        self.append_at(Utc::now(), event)
    }

    /// Append an event with an explicit timestamp.
    pub fn append_at(&mut self, at: DateTime<Utc>, event: Event) -> Result<Entry> {
        let tx = self.conn.transaction()?;
        let (seq, prev_hash) = tx
            .query_row(
                "SELECT seq, hash FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get::<_, i64>(0)? + 1, r.get::<_, String>(1)?)),
            )
            .optional()?
            .unwrap_or((1, GENESIS.to_owned()));
        // An entry holds the time to the microsecond. The entry handed back
        // carries the time as it was written, not the finer one the clock
        // gave, so that it is the same entry a reader gets later.
        let at_text = stamp(&at);
        let at = parse_stamp(&at_text).unwrap_or(at);
        let payload = serde_json::to_string(&event)?;
        let hash = hash_entry(&prev_hash, seq, &at_text, event.kind(), &payload);
        tx.execute(
            "INSERT INTO events (seq, at, kind, payload, prev_hash, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![seq, at_text, event.kind(), payload, prev_hash, hash],
        )?;
        tx.commit()?;
        Ok(Entry {
            seq,
            at,
            event,
            prev_hash,
            hash,
        })
    }

    pub fn len(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?)
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// The seq of the newest entry, or 0.
    pub fn last_seq(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?)
    }

    pub fn first(&self) -> Result<Option<Entry>> {
        self.one(&format!(
            "SELECT {COLUMNS} FROM events ORDER BY seq ASC LIMIT 1"
        ))
    }

    pub fn last(&self) -> Result<Option<Entry>> {
        self.one(&format!(
            "SELECT {COLUMNS} FROM events ORDER BY seq DESC LIMIT 1"
        ))
    }

    pub fn entry(&self, seq: i64) -> Result<Option<Entry>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM events WHERE seq = ?1"),
                [seq],
                row_to_entry,
            )
            .optional()?)
    }

    /// The most recent entry whose kind is one of `kinds`.
    pub fn last_of(&self, kinds: &[&str]) -> Result<Option<Entry>> {
        Ok(self.last_n_of(kinds, 1)?.pop())
    }

    /// The last `n` entries whose kind is one of `kinds`, oldest first.
    pub fn last_n_of(&self, kinds: &[&str], n: usize) -> Result<Vec<Entry>> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(n).unwrap_or(i64::MAX);
        // The newest of each kind, read backwards along its index, and the
        // newest of those. `kind IN (…)` would gather every entry of those
        // kinds she ever wrote and sort them all to keep a few.
        let sql = kinds
            .iter()
            .map(|_| {
                format!(
                    "SELECT * FROM (SELECT {COLUMNS} FROM events WHERE kind = ? ORDER BY seq DESC LIMIT {limit})"
                )
            })
            .collect::<Vec<_>>()
            .join(" UNION ALL ")
            + &format!(" ORDER BY seq DESC LIMIT {limit}");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(kinds.iter()), row_to_entry)?
            .map(|r| r.map_err(Error::from))
            .collect::<Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Entries of the given kinds with `seq > after`, oldest first.
    pub fn of_kinds_after(&self, kinds: &[&str], after: i64) -> Result<Vec<Entry>> {
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE seq > {after} AND kind IN ({placeholders}) ORDER BY seq ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        stmt.query_map(rusqlite::params_from_iter(kinds.iter()), row_to_entry)?
            .map(|r| r.map_err(Error::from))
            .collect()
    }

    /// The entries with these seqs, oldest first; one that is not there is
    /// left out.
    pub fn entries_at(&self, seqs: &[i64]) -> Result<Vec<Entry>> {
        if seqs.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = seqs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM events WHERE seq IN ({placeholders}) ORDER BY seq ASC"
        ))?;
        stmt.query_map(rusqlite::params_from_iter(seqs.iter()), row_to_entry)?
            .map(|r| r.map_err(Error::from))
            .collect()
    }

    /// The seq of the first entry written at or after `at`; one past the
    /// newest when none was. Entries are written as things happen, so
    /// their times rise with their seqs and the answer is found by
    /// halving, in a few reads however long the journal is. Were a clock
    /// ever set back, the entries written while it was behind would be
    /// placed by the entries around them.
    pub fn first_seq_since(&self, at: DateTime<Utc>) -> Result<i64> {
        let since = stamp(&at);
        let mut stmt = self.conn.prepare("SELECT at FROM events WHERE seq = ?1")?;
        let (mut low, mut high) = (1, self.last_seq()? + 1);
        while low < high {
            let mid = low + (high - low) / 2;
            let written: Option<String> = stmt.query_row([mid], |r| r.get(0)).optional()?;
            match written {
                Some(written) if written < since => low = mid + 1,
                _ => high = mid,
            }
        }
        Ok(low)
    }

    /// Read up to `limit` entries starting at `from_seq` (inclusive).
    pub fn read(&self, from_seq: i64, limit: usize) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM events WHERE seq >= ?1 ORDER BY seq ASC LIMIT ?2"
        ))?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![from_seq, limit], row_to_entry)?;
        rows.map(|r| r.map_err(Error::from)).collect()
    }

    /// The last `n` entries, oldest first.
    pub fn tail(&self, n: usize) -> Result<Vec<Entry>> {
        let total = self.last_seq()?;
        let n = i64::try_from(n).unwrap_or(i64::MAX);
        let from = (total - n + 1).max(1);
        self.read(from, usize::try_from(n).unwrap_or(usize::MAX))
    }

    /// Walk the whole chain and recompute every hash. Returns the length.
    pub fn verify(&self) -> Result<i64> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM events ORDER BY seq ASC"))?;
        let mut rows = stmt.query([])?;
        let mut expected_prev = GENESIS.to_owned();
        let mut expected_seq = 1;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            // Read in place rather than copied out: this walks the whole of
            // her life each time anything looks at her.
            let seq: i64 = row.get(0)?;
            let (at, kind, payload) = (text(row, 1)?, text(row, 2)?, text(row, 3)?);
            let (prev_hash, hash) = (text(row, 4)?, text(row, 5)?);
            if seq != expected_seq {
                return Err(Error::BrokenChain {
                    seq,
                    reason: format!("expected seq {expected_seq}"),
                });
            }
            if prev_hash != expected_prev {
                return Err(Error::BrokenChain {
                    seq,
                    reason: "prev_hash does not match previous entry".into(),
                });
            }
            if hash_of(prev_hash, seq, at, kind, payload).to_hex().as_str() != hash {
                return Err(Error::BrokenChain {
                    seq,
                    reason: "hash does not match contents".into(),
                });
            }
            expected_prev.clear();
            expected_prev.push_str(hash);
            expected_seq += 1;
            count += 1;
        }
        Ok(count)
    }

    fn one(&self, sql: &str) -> Result<Option<Entry>> {
        Ok(self.conn.query_row(sql, [], row_to_entry).optional()?)
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        Self { conn }
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    let at: String = row.get(1)?;
    let payload: String = row.get(3)?;
    let at = parse_stamp(&at).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let event: Event = serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Entry {
        seq: row.get(0)?,
        at,
        event,
        prev_hash: row.get(4)?,
        hash: row.get(5)?,
    })
}

/// The canonical text form of a timestamp. Sorts lexicographically.
pub(crate) fn stamp(at: &DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub(crate) fn parse_stamp(s: &str) -> std::result::Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc))
}

/// `blake3(prev_hash ‖ seq ‖ at ‖ kind ‖ payload)`, hex.
pub fn hash_entry(prev_hash: &str, seq: i64, at: &str, kind: &str, payload: &str) -> String {
    hash_of(prev_hash, seq, at, kind, payload)
        .to_hex()
        .to_string()
}

fn hash_of(prev_hash: &str, seq: i64, at: &str, kind: &str, payload: &str) -> blake3::Hash {
    let mut h = blake3::Hasher::new();
    for part in [prev_hash, &seq.to_string(), at, kind, payload] {
        h.update(part.as_bytes());
        h.update(b"\n");
    }
    h.finalize()
}

/// A text column of a row, as it is in the row.
fn text<'r>(row: &'r rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<&'r str> {
    Ok(row.get_ref(idx)?.as_str()?)
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS events (
    seq       INTEGER PRIMARY KEY,
    at        TEXT    NOT NULL,
    kind      TEXT    NOT NULL,
    payload   TEXT    NOT NULL,
    prev_hash TEXT    NOT NULL,
    hash      TEXT    NOT NULL UNIQUE
);
CREATE INDEX IF NOT EXISTS events_kind ON events (kind, seq);
CREATE TRIGGER IF NOT EXISTS events_no_update BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'journal is append-only'); END;
CREATE TRIGGER IF NOT EXISTS events_no_delete BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'journal is append-only'); END;
";

#[cfg(test)]
mod tests {
    use super::*;

    fn lived(text: &str) -> Event {
        Event::Lived {
            text: text.into(),
            block: None,
        }
    }

    #[test]
    fn chain_links_and_verifies() {
        let mut j = Journal::in_memory();
        let a = j.append(Event::Woke).unwrap();
        let b = j.append(lived("rain")).unwrap();
        assert_eq!(a.seq, 1);
        assert_eq!(a.prev_hash, GENESIS);
        assert_eq!(b.prev_hash, a.hash);
        assert_eq!(j.verify().unwrap(), 2);
        assert_eq!(j.last().unwrap().unwrap(), b);
        assert_eq!(j.entry(1).unwrap().unwrap(), a);
    }

    #[test]
    fn an_entry_is_handed_back_as_it_was_written() {
        // A clock finer than the journal keeps: Linux gives nanoseconds,
        // and an entry is written to the microsecond.
        let fine = DateTime::parse_from_rfc3339("2026-11-13T11:06:48.502265504Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut j = Journal::in_memory();
        let written = j.append_at(fine, lived("rain")).unwrap();
        assert_eq!(written.at.timestamp_subsec_nanos(), 502_265_000);
        assert_eq!(j.last().unwrap().unwrap(), written);
        assert_eq!(j.verify().unwrap(), 1);
    }

    #[test]
    fn storage_refuses_rewrites() {
        let mut j = Journal::in_memory();
        j.append(Event::Woke).unwrap();
        assert!(
            j.conn
                .execute("UPDATE events SET kind = 'slept'", [])
                .is_err()
        );
        assert!(j.conn.execute("DELETE FROM events", []).is_err());
        assert_eq!(j.len().unwrap(), 1);
    }

    #[test]
    fn filters_by_kind() {
        let mut j = Journal::in_memory();
        j.append(Event::Woke).unwrap();
        j.append(lived("x")).unwrap();
        j.append(Event::Slept).unwrap();
        j.append(lived("y")).unwrap();
        let e = j.last_of(&["woke", "slept"]).unwrap().unwrap();
        assert_eq!(e.event, Event::Slept);
        assert!(j.last_of(&["canon"]).unwrap().is_none());
        let after = j.of_kinds_after(&["lived"], 2).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].seq, 4);
    }

    #[test]
    fn the_newest_of_several_kinds_are_interleaved_oldest_first() {
        let mut j = Journal::in_memory();
        for i in 0..20 {
            j.append(lived(&i.to_string())).unwrap();
            if i % 3 == 0 {
                j.append(Event::Woke).unwrap();
            }
            if i % 7 == 0 {
                j.append(Event::Slept).unwrap();
            }
        }
        let kinds = ["slept", "woke", "lived"];
        let all: Vec<i64> = j
            .read(1, 100)
            .unwrap()
            .into_iter()
            .filter(|e| kinds.contains(&e.event.kind()))
            .map(|e| e.seq)
            .collect();
        for n in [0, 1, 4, 9, 50] {
            let got: Vec<i64> = j
                .last_n_of(&kinds, n)
                .unwrap()
                .into_iter()
                .map(|e| e.seq)
                .collect();
            assert_eq!(got, all[all.len().saturating_sub(n)..], "the last {n}");
        }
        assert!(j.last_n_of(&[], 3).unwrap().is_empty());
    }

    #[test]
    fn entries_are_read_by_seq_in_order_and_a_missing_one_is_left_out() {
        let mut j = Journal::in_memory();
        for i in 0..5 {
            j.append(lived(&i.to_string())).unwrap();
        }
        let read = j.entries_at(&[4, 9, 2]).unwrap();
        assert_eq!(read.iter().map(|e| e.seq).collect::<Vec<_>>(), [2, 4]);
        assert!(j.entries_at(&[]).unwrap().is_empty());
    }

    #[test]
    fn the_first_entry_since_a_time_is_found_by_halving() {
        let mut j = Journal::in_memory();
        let t0 = DateTime::parse_from_rfc3339("2026-11-13T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(j.first_seq_since(t0).unwrap(), 1, "nothing yet: one past");
        for i in 0..7 {
            j.append_at(t0 + chrono::Duration::minutes(i), lived(&i.to_string()))
                .unwrap();
        }
        assert_eq!(
            j.first_seq_since(t0 - chrono::Duration::hours(1)).unwrap(),
            1
        );
        assert_eq!(
            j.first_seq_since(t0 + chrono::Duration::minutes(3))
                .unwrap(),
            4
        );
        assert_eq!(
            j.first_seq_since(t0 + chrono::Duration::seconds(150))
                .unwrap(),
            4,
            "between two entries: the later"
        );
        assert_eq!(
            j.first_seq_since(t0 + chrono::Duration::hours(1)).unwrap(),
            8
        );
    }

    #[test]
    fn tail_returns_newest_in_order() {
        let mut j = Journal::in_memory();
        for i in 0..5 {
            j.append(lived(&i.to_string())).unwrap();
        }
        let t = j.tail(2).unwrap();
        assert_eq!(t.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![4, 5]);
        assert_eq!(j.tail(100).unwrap().len(), 5);
    }

    #[test]
    fn optional_fields_stay_out_of_the_payload() {
        let e = lived("x");
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"kind":"lived","data":{"text":"x"}}"#
        );
    }
}
