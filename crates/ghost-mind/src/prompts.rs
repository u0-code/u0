//! The words a brain reads. Data, not code.
//!
//! Every template, every short piece of framing and every tool description
//! is looked for in three places, the first that has it winning: her own
//! directory's `prompts/`, the `[mind] prompts` directory, and what is built
//! into this binary. The built-ins are only the last resort, so that a lone
//! binary still runs; anything a person might want to change is a file.
//! How that is done is [`ghost_core::prompts`]; the work loop reads the same
//! directory the same way.
//!
//! A changed file is taken in at the next use, without a restart: the mind
//! asks for a refresh as it wakes, and at most every couple of seconds that
//! looks at the modification time of each file it could read.
//!
//! The templates are whole Markdown files. The short pieces are named
//! entries in `texts.toml`, and the tool words are a table per tool in
//! `tools.toml`, which says what every tool is in a conversation and at
//! work alike. Beside them are the work loop's own files, `work.md` and
//! `work_texts.toml`, so that one directory holds everything a brain is
//! told. None of this belongs to her: it is how a brain reads her persona
//! and her journal, and changing it does not change who she is.

use std::{
    io,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use ghost_core::prompts::Store;
pub use ghost_core::prompts::{DIR, File, Kind, Listed, Loop, Origin};

const TEXTS: &str = "texts.toml";

/// The files the mind reads, with what is built in. The work loop's are in
/// [`ghost_work::prompts::FILES`], and are read beside them.
pub const FILES: &[File] = &[
    whole("self.md", include_str!("../prompts/self.md")),
    whole("now.md", include_str!("../prompts/now.md")),
    whole("digest.md", include_str!("../prompts/digest.md")),
    whole("reply_json.md", include_str!("../prompts/reply_json.md")),
    whole("reply_tools.md", include_str!("../prompts/reply_tools.md")),
    whole("attend.md", include_str!("../prompts/attend.md")),
    whole("plan.md", include_str!("../prompts/plan.md")),
    whole("lived.md", include_str!("../prompts/lived.md")),
    whole("reflect.md", include_str!("../prompts/reflect.md")),
    whole("reflect_on.md", include_str!("../prompts/reflect_on.md")),
    whole("browse.md", include_str!("../prompts/browse.md")),
    whole("gate.md", include_str!("../prompts/gate.md")),
    whole("view.md", include_str!("../prompts/view.md")),
    whole("report.md", include_str!("../prompts/report.md")),
    whole("review.md", include_str!("../prompts/review.md")),
    whole("working.md", include_str!("../prompts/working.md")),
    File {
        name: TEXTS,
        built_in: include_str!("../prompts/texts.toml"),
        kind: Kind::Texts,
    },
];

const fn whole(name: &'static str, built_in: &'static str) -> File {
    File {
        name,
        built_in,
        kind: Kind::Whole,
    }
}

/// Every file a prompts directory can hold: the mind's, then the work
/// loop's.
pub fn all_files() -> Vec<File> {
    FILES
        .iter()
        .chain(ghost_work::prompts::FILES)
        .copied()
        .collect()
}

#[derive(Debug, Clone)]
pub struct Prompts {
    /// `self.md`: who she is. The system prompt, and the part of every
    /// request that does not change from one to the next.
    pub this: String,
    /// `now.md`: what moment this is and what has been going on. It changes
    /// by the hour, so it goes late in a request, after what stays put.
    pub now: String,
    /// `digest.md`: keeping the earlier part of a long conversation.
    pub digest: String,
    /// `reply_json.md`: how to answer when what she does is written as JSON.
    pub reply_json: String,
    /// `reply_tools.md`: how to answer when what she does is tool calls.
    pub reply_tools: String,
    /// `attend.md`: looking at a conversation.
    pub attend: String,
    /// `plan.md`: sketching a day.
    pub plan: String,
    /// `lived.md`: how a stretch of the day actually went.
    pub lived: String,
    /// `reflect.md`: looking back on her own day before sleep.
    pub reflect: String,
    /// `reflect_on.md`: looking back on one platform before sleep.
    pub reflect_on: String,
    /// `browse.md`: scrolling a feed.
    pub browse: String,
    /// `gate.md`: reading a post over before it goes out.
    pub gate: String,
    /// `view.md`: glancing at an image someone sent.
    pub view: String,
    /// `report.md`: telling an operator what she has been doing.
    pub report: String,
    /// `review.md`: the brief for sitting down to look at her public writing.
    pub review: String,
    /// `working.md`: what she is told about a piece of work she took on.
    pub working: String,
    store: Store,
}

impl Prompts {
    /// What is built into this binary, and nothing else.
    pub fn shipped() -> Self {
        Self::from_store(Store::shipped(&all_files()))
    }

    /// The prompts of the ghost in `ghost`: from her own `prompts/`, then
    /// from `config` if there is one, then built in. Nothing here fails: a
    /// file that cannot be used is warned about and passed over.
    pub fn load(ghost: &Path, config: Option<&Path>) -> Self {
        Self::from_store(Store::load(&all_files(), ghost, config))
    }

    fn from_store(store: Store) -> Self {
        let text = |name| store.whole(name).to_owned();
        Self {
            this: text("self.md"),
            now: text("now.md"),
            digest: text("digest.md"),
            reply_json: text("reply_json.md"),
            reply_tools: text("reply_tools.md"),
            attend: text("attend.md"),
            plan: text("plan.md"),
            lived: text("lived.md"),
            reflect: text("reflect.md"),
            reflect_on: text("reflect_on.md"),
            browse: text("browse.md"),
            gate: text("gate.md"),
            view: text("view.md"),
            report: text("report.md"),
            review: text("review.md"),
            working: text("working.md"),
            store,
        }
    }

    /// Take in any file that changed, if the files have not been looked at
    /// in the last couple of seconds. Cheap enough to call on every wake.
    pub fn refresh(&mut self, now: DateTime<Utc>) {
        if self.store.refresh(now) {
            *self = Self::from_store(std::mem::replace(&mut self.store, Store::shipped(&[])));
        }
    }

    /// A text from `texts.toml`, as it is.
    pub fn text(&self, key: &str) -> &str {
        self.store.text(TEXTS, key)
    }

    /// A text from `texts.toml`, with its placeholders filled.
    pub fn fill(&self, key: &str, pairs: &[(&str, String)]) -> String {
        self.store.fill(TEXTS, key, pairs)
    }

    /// A text from `texts.toml` around `value`, put in as `{{name}}`; or
    /// nothing at all when `value` is empty.
    pub fn section(&self, key: &str, name: &str, value: &str) -> String {
        if value.trim().is_empty() {
            String::new()
        } else {
            self.fill(key, &[(name, value.to_owned())])
        }
    }

    /// What tool `name` is described as in a conversation.
    pub fn tool(&self, name: &str) -> &str {
        self.store.tool(name, Loop::Conversation)
    }

    /// What argument `parameter` of tool `name` is described as in a
    /// conversation, if it is.
    pub fn parameter(&self, name: &str, parameter: &str) -> Option<&str> {
        self.store.parameter(name, parameter, Loop::Conversation)
    }

    /// Everything read, for what reads tools.toml itself.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The words of the work loop, from the same places as these.
    pub fn for_work(&self) -> ghost_work::Prompts {
        ghost_work::Prompts::layered(self.store.layers())
    }

    /// Every template, text and tool, with where what is in use came from.
    pub fn listing(&self) -> Vec<Listed> {
        self.store.listing()
    }

    /// Write every built-in file into `dir`, for a person to change. A
    /// file already there is left alone unless `force`. Returns each file
    /// and whether it was written.
    pub fn export(dir: &Path, force: bool) -> io::Result<Vec<(PathBuf, bool)>> {
        Store::export(&all_files(), dir, force)
    }
}

/// The built-in text of the file `name`.
#[cfg(test)]
fn built_in(name: &str) -> &'static str {
    all_files()
        .iter()
        .find(|f| f.name == name)
        .map(|f| f.built_in)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration as StdDuration, UNIX_EPOCH};

    use std::fs;

    use chrono::{Duration, TimeZone};

    use super::*;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 14, 9, 0, 0).unwrap()
    }

    /// Write `text` to `path` and say it was written at `secs` past the
    /// epoch, so that a change is seen without waiting for a clock.
    fn write_at(path: &Path, text: impl AsRef<[u8]>, secs: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + StdDuration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn the_built_in_files_parse_and_describe_every_tool() {
        let shipped = Prompts::shipped();
        for listed in shipped.listing() {
            if listed.file == "tools.toml" {
                let name = listed.key.unwrap();
                assert!(!shipped.tool(&name).is_empty(), "{name} has no description");
            }
        }
        assert_eq!(shipped.attend, include_str!("../prompts/attend.md"));
    }

    #[test]
    fn her_own_prompts_win_over_the_config_directory_which_wins_over_the_built_in() {
        let (ghost, config) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write_at(&ghost.path().join("prompts/plan.md"), "hers", 1);
        write_at(&config.path().join("plan.md"), "the config's", 1);
        write_at(&config.path().join("gate.md"), "the config's gate", 1);
        write_at(
            &ghost.path().join("prompts/texts.toml"),
            "look = \"hers\"\n",
            1,
        );
        write_at(
            &config.path().join("texts.toml"),
            "look = \"the config's\"\ncannot_here = \"the config's\"\n",
            1,
        );
        let p = Prompts::load(ghost.path(), Some(config.path()));
        assert_eq!(p.plan, "hers");
        assert_eq!(p.gate, "the config's gate");
        assert_eq!(p.attend, Prompts::shipped().attend);
        assert_eq!(p.text("look"), "hers");
        assert_eq!(p.text("cannot_here"), "the config's");
        assert_eq!(p.text("not_sent"), Prompts::shipped().text("not_sent"));

        let listed = p.listing();
        let origin = |file: &str, key: Option<&str>| {
            listed
                .iter()
                .find(|l| l.file == file && l.key.as_deref() == key)
                .map(|l| (l.origin, l.differs))
                .unwrap()
        };
        assert_eq!(origin("plan.md", None), (Origin::Ghost, true));
        assert_eq!(origin("gate.md", None), (Origin::Config, true));
        assert_eq!(origin("attend.md", None), (Origin::BuiltIn, false));
        assert_eq!(origin(TEXTS, Some("look")), (Origin::Ghost, true));
        assert_eq!(origin(TEXTS, Some("cannot_here")), (Origin::Config, true));
        assert_eq!(origin("tools.toml", Some("say")), (Origin::BuiltIn, false));
    }

    #[test]
    fn an_edited_file_is_taken_in_at_the_next_use_without_a_restart() {
        let ghost = tempfile::tempdir().unwrap();
        let plan = ghost.path().join("prompts/plan.md");
        write_at(&plan, "first", 1);
        let mut p = Prompts::load(ghost.path(), None);
        assert_eq!(p.plan, "first");
        p.refresh(t0());

        write_at(&plan, "second", 2);
        // Within a couple of seconds of the last look, nothing is looked at.
        p.refresh(t0() + Duration::seconds(1));
        assert_eq!(p.plan, "first");
        p.refresh(t0() + Duration::seconds(2));
        assert_eq!(p.plan, "second");

        // A file that was not there before is found as well.
        write_at(
            &ghost.path().join("prompts/tools.toml"),
            "[say]\ndescription = \"Talk.\"\n",
            3,
        );
        p.refresh(t0() + Duration::seconds(4));
        assert_eq!(p.tool("say"), "Talk.");
        assert_eq!(
            p.parameter("say", "text"),
            Prompts::shipped().parameter("say", "text")
        );
    }

    #[test]
    fn a_deleted_file_gives_way_to_the_next_place_that_has_it() {
        let (ghost, config) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let hers = ghost.path().join("prompts/plan.md");
        write_at(&hers, "hers", 1);
        write_at(&config.path().join("plan.md"), "the config's", 1);
        let mut p = Prompts::load(ghost.path(), Some(config.path()));
        assert_eq!(p.plan, "hers");

        fs::remove_file(&hers).unwrap();
        p.refresh(t0());
        assert_eq!(p.plan, "the config's");

        fs::remove_file(config.path().join("plan.md")).unwrap();
        p.refresh(t0() + Duration::seconds(5));
        assert_eq!(p.plan, Prompts::shipped().plan);
    }

    #[test]
    fn a_file_that_cannot_be_used_leaves_the_text_in_use() {
        let ghost = tempfile::tempdir().unwrap();
        let plan = ghost.path().join("prompts/plan.md");
        let texts = ghost.path().join("prompts/texts.toml");
        write_at(&plan, "good", 1);
        write_at(&texts, "look = \"good\"\n", 1);
        let mut p = Prompts::load(ghost.path(), None);

        write_at(&plan, [0xff, 0xfe, 0x00], 2);
        write_at(&texts, "look = \"half a string\n", 2);
        p.refresh(t0());
        assert_eq!(p.plan, "good");
        assert_eq!(p.text("look"), "good");

        // Put right, it is taken in.
        write_at(&plan, "better", 3);
        write_at(&texts, "look = \"better\"\n", 3);
        p.refresh(t0() + Duration::seconds(5));
        assert_eq!(p.plan, "better");
        assert_eq!(p.text("look"), "better");
    }

    #[test]
    fn a_section_is_left_out_when_what_fills_it_is_empty() {
        let p = Prompts::shipped();
        assert_eq!(p.section("examples", "examples", "  \n"), "");
        assert!(
            p.section("examples", "examples", "them: hi")
                .ends_with("in this voice:\n\nthem: hi")
        );
    }

    #[test]
    fn export_and_list_cover_the_work_loops_files_and_every_tool_of_both_loops() {
        let ghost = tempfile::tempdir().unwrap();
        let dir = ghost.path().join(DIR);
        let written: Vec<String> = Prompts::export(&dir, false)
            .unwrap()
            .into_iter()
            .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        for name in [
            "self.md",
            "texts.toml",
            "work.md",
            "work_texts.toml",
            "tools.toml",
        ] {
            assert!(written.iter().any(|w| w == name), "{name} was not exported");
            assert!(dir.join(name).exists());
        }
        assert_eq!(
            fs::read_to_string(dir.join("work.md")).unwrap(),
            ghost_work::prompts::FILES[0].built_in
        );

        // Changed where it was exported to, it is listed as hers.
        write_at(&dir.join("work_texts.toml"), "no_call = \"Call one.\"\n", 1);
        write_at(
            &dir.join("tools.toml"),
            "[delegate]\ndescription = \"Hand it on.\"\n",
            1,
        );
        let listed = Prompts::load(ghost.path(), None).listing();
        let find = |file: &str, key: Option<&str>| {
            listed
                .iter()
                .find(|l| l.file == file && l.key.as_deref() == key)
                .map(|l| (l.origin, l.differs))
        };
        assert_eq!(find("work.md", None), Some((Origin::Ghost, false)));
        assert_eq!(
            find("work_texts.toml", Some("no_call")),
            Some((Origin::Ghost, true))
        );
        assert_eq!(
            find("work_texts.toml", Some("audit")),
            Some((Origin::BuiltIn, false))
        );
        assert_eq!(
            find("tools.toml", Some("delegate")),
            Some((Origin::Ghost, true))
        );
        // A tool of the conversation and one of work, in the one file.
        assert!(find("tools.toml", Some("say")).is_some());
        assert!(find("tools.toml", Some("shell")).is_some());
    }

    #[test]
    fn export_writes_every_built_in_and_overwrites_only_when_forced() {
        let ghost = tempfile::tempdir().unwrap();
        let dir = ghost.path().join(DIR);
        write_at(&dir.join("plan.md"), "mine", 1);

        let done = Prompts::export(&dir, false).unwrap();
        assert_eq!(done.len(), all_files().len());
        for File {
            name,
            built_in: text,
            ..
        } in all_files()
        {
            let written = done.iter().find(|(p, _)| p.ends_with(name)).unwrap().1;
            assert_eq!(written, name != "plan.md", "{name}");
            if name != "plan.md" {
                assert_eq!(fs::read_to_string(dir.join(name)).unwrap(), text);
            }
        }
        assert_eq!(fs::read_to_string(dir.join("plan.md")).unwrap(), "mine");

        // What was exported is what is built in, so nothing differs.
        fs::remove_file(dir.join("plan.md")).unwrap();
        Prompts::export(&dir, false).unwrap();
        assert!(
            Prompts::load(ghost.path(), None)
                .listing()
                .iter()
                .all(|l| l.origin == Origin::Ghost && !l.differs)
        );

        write_at(&dir.join("plan.md"), "mine", 1);
        let done = Prompts::export(&dir, true).unwrap();
        assert!(done.iter().all(|(_, written)| *written));
        assert_eq!(
            fs::read_to_string(dir.join("plan.md")).unwrap(),
            built_in("plan.md")
        );
    }
}
