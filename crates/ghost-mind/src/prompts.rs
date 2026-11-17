//! The words a brain reads. Data, not code.
//!
//! Every template, every short piece of framing and every tool description
//! is looked for in two places, the first that has it winning: the
//! deployment's directory (`[mind] prompts` in `ghostd.toml`), and what is
//! built into this binary from the tree `prompts/` at the top of the
//! repository. The built-ins are only the last resort, so that a lone
//! binary still runs; anything a person might want to change is a file.
//! How that is done is [`ghost_core::prompts`]; the work loop reads the
//! same directory the same way.
//!
//! [`FILES`] is every file of the tree the mind reads. `character/` is how
//! her own `ghost.toml` is turned into prose: `persona.md`, which arranges
//! her fields and holds nothing else, and the texts it puts around them.
//! `system/being.md` is what she is and the ground rules, ending with what
//! is true of her life; the system prompt is the persona and her being, one
//! after the other, filled in. `system/logic/` is how a conversation and a
//! day go, with its short framings in `system/logic/texts/`, a file per
//! feature, read with the persona's texts as one table. `system/work/working.md`
//! is what she is told about a piece of work, beside the work loop's own
//! files.
//!
//! None of these belong to her or to any one character: they are how a
//! brain reads whoever's persona and journal it is given. Swapping one
//! character for another is a matter of her `ghost.toml` alone. A changed
//! file is taken in at the next use, without a restart: the mind asks for
//! a refresh as it wakes, and at most every couple of seconds that looks at
//! the modification time of each file it could read.

use std::{
    io,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
pub use ghost_core::prompts::{DIR, File, Kind, Layer, Listed, Loop, Origin};
use ghost_core::{prompt, prompts::Store};

/// The mind's table of texts: `system/logic/texts/` and
/// `character/texts.toml`, read as one.
pub const TEXTS: &str = "mind";
/// Where each of the mind's texts was before there was a tree.
const FLAT_TEXTS: &str = "texts.toml";

/// The files the mind reads, with what is built in. The work loop's are in
/// [`ghost_work::prompts::FILES`], and are read beside them.
pub const FILES: &[File] = &[
    prompt!(
        "character/persona.md",
        Kind::Whole,
        Layer::Persona,
        "persona.md"
    ),
    prompt!(
        "character/texts.toml",
        Kind::Texts(TEXTS),
        Layer::Persona,
        FLAT_TEXTS
    ),
    prompt!("system/being.md", Kind::Whole, Layer::Being, "being.md"),
    prompt!("system/logic/now.md", Kind::Whole, Layer::Logic, "now.md"),
    prompt!(
        "system/logic/digest.md",
        Kind::Whole,
        Layer::Logic,
        "digest.md"
    ),
    prompt!(
        "system/logic/reply_json.md",
        Kind::Whole,
        Layer::Logic,
        "reply_json.md"
    ),
    prompt!(
        "system/logic/reply_tools.md",
        Kind::Whole,
        Layer::Logic,
        "reply_tools.md"
    ),
    prompt!(
        "system/logic/attend.md",
        Kind::Whole,
        Layer::Logic,
        "attend.md"
    ),
    prompt!(
        "system/logic/terminal.md",
        Kind::Whole,
        Layer::Logic,
        "terminal.md"
    ),
    prompt!("system/logic/plan.md", Kind::Whole, Layer::Logic, "plan.md"),
    prompt!(
        "system/logic/lived.md",
        Kind::Whole,
        Layer::Logic,
        "lived.md"
    ),
    prompt!(
        "system/logic/reflect.md",
        Kind::Whole,
        Layer::Logic,
        "reflect.md"
    ),
    prompt!(
        "system/logic/reflect_on.md",
        Kind::Whole,
        Layer::Logic,
        "reflect_on.md"
    ),
    prompt!(
        "system/logic/browse.md",
        Kind::Whole,
        Layer::Logic,
        "browse.md"
    ),
    prompt!("system/logic/gate.md", Kind::Whole, Layer::Logic, "gate.md"),
    prompt!("system/logic/view.md", Kind::Whole, Layer::Logic, "view.md"),
    prompt!(
        "system/logic/report.md",
        Kind::Whole,
        Layer::Logic,
        "report.md"
    ),
    prompt!(
        "system/logic/review.md",
        Kind::Whole,
        Layer::Logic,
        "review.md"
    ),
    prompt!(
        "system/logic/texts/look.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/moment.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/place.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/addressed.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/right_now.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/elsewhere.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/abilities.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/keep.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/view.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/browse.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/day.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/report.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/logic/texts/working.toml",
        Kind::Texts(TEXTS),
        Layer::Logic,
        FLAT_TEXTS
    ),
    prompt!(
        "system/work/working.md",
        Kind::Whole,
        Layer::Work,
        "working.md"
    ),
];

/// Every file a prompts directory can hold: the mind's, then the work
/// loop's.
pub fn all_files() -> Vec<File> {
    FILES
        .iter()
        .chain(ghost_work::prompts::FILES)
        .copied()
        .collect()
}

/// The mind's templates as they are, each by its file (those named by
/// their name alone are in `system/logic/`), and the rest of what it reads.
#[derive(Debug, Clone)]
pub struct Prompts {
    /// `character/persona.md`: her own fields from `ghost.toml`, arranged
    /// as prose. The first part of the system prompt, and the only part
    /// that differs from one character to another.
    pub persona: String,
    /// `system/being.md`: what she is and the ground rules, then what is
    /// true of her life. The rest of the system prompt.
    pub being: String,
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
    /// `terminal.md`: looking at what was said at a console, where she is
    /// waited on.
    pub terminal: String,
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
    /// `system/work/working.md`: what the work loop is told about a piece
    /// of work she took on and handed to it.
    pub working: String,
    store: Store,
}

impl Prompts {
    /// What is built into this binary, and nothing else.
    pub fn shipped() -> Self {
        Self::from_store(Store::shipped(&all_files()))
    }

    /// The prompts in the deployment's directory `dir`, if there is one,
    /// then built in. Nothing here fails: a file that cannot be used is
    /// warned about and passed over.
    pub fn load(dir: Option<&Path>) -> Self {
        Self::from_store(Store::load(&all_files(), dir))
    }

    fn from_store(store: Store) -> Self {
        let text = |name| store.whole(name).to_owned();
        Self {
            persona: text("character/persona.md"),
            being: text("system/being.md"),
            now: text("system/logic/now.md"),
            digest: text("system/logic/digest.md"),
            reply_json: text("system/logic/reply_json.md"),
            reply_tools: text("system/logic/reply_tools.md"),
            attend: text("system/logic/attend.md"),
            terminal: text("system/logic/terminal.md"),
            plan: text("system/logic/plan.md"),
            lived: text("system/logic/lived.md"),
            reflect: text("system/logic/reflect.md"),
            reflect_on: text("system/logic/reflect_on.md"),
            browse: text("system/logic/browse.md"),
            gate: text("system/logic/gate.md"),
            view: text("system/logic/view.md"),
            report: text("system/logic/report.md"),
            review: text("system/logic/review.md"),
            working: text("system/work/working.md"),
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

    /// A text of the mind's, as it is.
    pub fn text(&self, key: &str) -> &str {
        self.store.text(TEXTS, key)
    }

    /// A text of the mind's, with its placeholders filled.
    pub fn fill(&self, key: &str, pairs: &[(&str, String)]) -> String {
        self.store.fill(TEXTS, key, pairs)
    }

    /// A text of the mind's around `value`, put in as `{{name}}`; or
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

    /// Everything read, for what reads `system/tools.toml` itself.
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

    /// The system prompt: her persona, then her being, each filled in from
    /// `pairs`. The persona comes first because it is who the rest is said
    /// to; being ends with what is true of her life, which grows.
    pub fn system(&self, pairs: &[(&str, String)]) -> String {
        format!(
            "{}\n\n{}",
            ghost_core::narrate::fill(&self.persona, pairs).trim_end(),
            ghost_core::narrate::fill(&self.being, pairs)
        )
    }

    /// Write every built-in file into `dir`, each at its path in the tree,
    /// for a person to change. A file already there is left alone unless
    /// `force`. Returns each file and whether it was written.
    pub fn export(dir: &Path, force: bool) -> io::Result<Vec<(PathBuf, bool)>> {
        Store::export(&all_files(), dir, force)
    }
}

/// The built-in text of the file at `path`.
#[cfg(test)]
fn built_in(path: &str) -> &'static str {
    all_files()
        .iter()
        .find(|f| f.path == path)
        .map(|f| f.built_in)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration as StdDuration, UNIX_EPOCH};

    use std::fs;

    use chrono::{Duration, TimeZone};

    use super::*;

    const PLAN: &str = "system/logic/plan.md";
    const LOOK: &str = "system/logic/texts/look.toml";
    const TOOLS: &str = "system/tools.toml";

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

    /// The tree at the top of the repository.
    fn tree() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prompts")
    }

    #[test]
    fn the_built_in_files_parse_and_describe_every_tool() {
        let shipped = Prompts::shipped();
        for listed in shipped.listing() {
            if listed.file == TOOLS {
                let name = listed.key.unwrap();
                assert!(!shipped.tool(&name).is_empty(), "{name} has no description");
            }
        }
        assert_eq!(
            shipped.attend,
            include_str!("../../../prompts/system/logic/attend.md")
        );
    }

    #[test]
    fn the_built_in_texts_have_each_key_in_one_file_of_its_table() {
        assert_eq!(
            ghost_core::prompts::duplicates(&all_files()),
            [],
            "a text is in two files"
        );
        let mut paths: Vec<&str> = all_files().iter().map(|f| f.path).collect();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), all_files().len(), "a file is listed twice");
    }

    #[test]
    fn every_file_in_the_tree_is_built_in_except_the_desks_and_the_readme() {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    let rel = path.strip_prefix(root).unwrap();
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let root = tree();
        let mut found = Vec::new();
        walk(&root, &root, &mut found);
        found.retain(|p| p != "README.md" && !p.starts_with("system/desk/"));
        found.sort();
        let mut built_in: Vec<String> = all_files().iter().map(|f| f.path.to_owned()).collect();
        built_in.sort();
        assert_eq!(found, built_in);
    }

    #[test]
    fn character_holds_the_persona_layer_and_system_holds_the_rest() {
        for file in all_files() {
            assert_eq!(
                file.path.starts_with("character/"),
                file.layer == Layer::Persona,
                "{}",
                file.path
            );
            assert!(
                file.path.starts_with("character/") || file.path.starts_with("system/"),
                "{}",
                file.path
            );
        }
        let layer = |path: &str| all_files().iter().find(|f| f.path == path).unwrap().layer;
        assert_eq!(layer("system/being.md"), Layer::Being);
        assert_eq!(layer("system/logic/attend.md"), Layer::Logic);
        assert_eq!(layer(LOOK), Layer::Logic);
        assert_eq!(layer("system/work/working.md"), Layer::Work);
        assert_eq!(layer(TOOLS), Layer::Work);
    }

    #[test]
    fn only_the_persona_template_holds_her_fields() {
        // The fields that are hers alone are put in through the persona
        // template and no other. (The words `character/texts.toml`
        // introduces her examples with are said around them, in the place
        // the persona template gives them.)
        for field in [
            "pronouns",
            "temperament",
            "voice",
            "origin",
            "examples",
            "facets",
            "values",
            "cares_about",
            "cannot_stand",
        ] {
            let placeholder = format!("{{{{{field}}}}}");
            for file in all_files().iter().filter(|f| f.kind == Kind::Whole) {
                assert_eq!(
                    file.built_in.contains(&placeholder),
                    file.path == "character/persona.md",
                    "{placeholder} in {}",
                    file.path
                );
            }
        }
    }

    #[test]
    fn the_deployments_prompts_win_over_the_built_in_file_by_file_and_entry_by_entry() {
        let dir = tempfile::tempdir().unwrap();
        write_at(
            &dir.path().join("system/logic/gate.md"),
            "the deployment's gate",
            1,
        );
        write_at(
            &dir.path().join("character/persona.md"),
            "Call me {{name}}.",
            1,
        );
        write_at(
            &dir.path().join(LOOK),
            "last_turn = \"the deployment's\"\n",
            1,
        );
        let p = Prompts::load(Some(dir.path()));
        assert_eq!(p.gate, "the deployment's gate");
        assert_eq!(p.persona, "Call me {{name}}.");
        assert_eq!(p.attend, Prompts::shipped().attend);
        assert_eq!(p.text("last_turn"), "the deployment's");
        assert_eq!(p.text("not_sent"), Prompts::shipped().text("not_sent"));

        let listed = p.listing();
        let origin = |file: &str, key: Option<&str>| {
            listed
                .iter()
                .find(|l| l.file == file && l.key.as_deref() == key)
                .map(|l| (l.origin, l.differs))
                .unwrap()
        };
        assert_eq!(
            origin("system/logic/gate.md", None),
            (Origin::Deployment, true)
        );
        assert_eq!(
            origin("character/persona.md", None),
            (Origin::Deployment, true)
        );
        assert_eq!(
            origin("system/logic/attend.md", None),
            (Origin::BuiltIn, false)
        );
        assert_eq!(origin(LOOK, Some("last_turn")), (Origin::Deployment, true));
        assert_eq!(origin(LOOK, Some("not_sent")), (Origin::BuiltIn, false));
        assert_eq!(origin(TOOLS, Some("say")), (Origin::BuiltIn, false));
    }

    #[test]
    fn the_persona_texts_are_read_with_the_logic_texts_as_one_table() {
        let dir = tempfile::tempdir().unwrap();
        write_at(
            &dir.path().join("character/texts.toml"),
            "examples = \"Like this:\\n\\n{{examples}}\"\n",
            1,
        );
        let p = Prompts::load(Some(dir.path()));
        assert_eq!(
            p.section("examples", "examples", "them: hi"),
            "Like this:\n\nthem: hi"
        );
        assert_eq!(p.text("weather"), Prompts::shipped().text("weather"));
    }

    #[test]
    fn an_old_flat_directory_still_loads_and_is_told_where_each_file_goes() {
        let dir = tempfile::tempdir().unwrap();
        write_at(&dir.path().join("plan.md"), "a flat plan", 1);
        write_at(
            &dir.path().join("texts.toml"),
            "weather = \"Flat weather: {{weather}}.\"\nexamples = \"Flat.\\n\\n{{examples}}\"\n",
            1,
        );
        write_at(
            &dir.path().join("work_texts.toml"),
            "no_call = \"Call one.\"\n",
            1,
        );
        write_at(
            &dir.path().join("tools.toml"),
            "[say]\ndescription = \"Talk.\"\n",
            1,
        );
        let p = Prompts::load(Some(dir.path()));
        assert_eq!(p.plan, "a flat plan");
        assert_eq!(p.text("weather"), "Flat weather: {{weather}}.");
        assert_eq!(p.section("examples", "examples", "x"), "Flat.\n\nx");
        assert_eq!(p.tool("say"), "Talk.");
        assert_eq!(p.for_work().text("no_call"), "Call one.");

        let said = ghost_core::prompts::flat(&all_files(), dir.path()).unwrap();
        for moved in [
            "plan.md goes to system/logic/plan.md",
            "texts.toml goes to a file per feature: examples to character/texts.toml; \
             weather to system/logic/texts/day.toml",
            "work_texts.toml goes to system/work/texts.toml",
            "tools.toml goes to system/tools.toml",
        ] {
            assert!(said.contains(moved), "{moved:?} is not in {said:?}");
        }
    }

    #[test]
    fn the_desks_directory_replaces_her_persona_and_being_and_nothing_else() {
        let desk = tree().join("system/desk");
        let p = Prompts::load(Some(&desk));
        let replaced: Vec<&str> = p
            .listing()
            .into_iter()
            .filter(|l| l.origin == Origin::Deployment)
            .map(|l| l.file)
            .collect();
        assert_eq!(replaced, ["character/persona.md", "system/being.md"]);
        assert_eq!(
            p.persona,
            fs::read_to_string(desk.join("character/persona.md")).unwrap()
        );
        assert!(ghost_core::prompts::flat(&all_files(), &desk).is_none());
    }

    #[test]
    fn an_edited_file_is_taken_in_at_the_next_use_without_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let plan = dir.path().join(PLAN);
        write_at(&plan, "first", 1);
        let mut p = Prompts::load(Some(dir.path()));
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
            &dir.path().join(TOOLS),
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
    fn a_deleted_file_gives_way_to_the_built_in() {
        let dir = tempfile::tempdir().unwrap();
        let plan = dir.path().join(PLAN);
        write_at(&plan, "the deployment's", 1);
        let mut p = Prompts::load(Some(dir.path()));
        assert_eq!(p.plan, "the deployment's");

        fs::remove_file(&plan).unwrap();
        p.refresh(t0());
        assert_eq!(p.plan, Prompts::shipped().plan);
    }

    #[test]
    fn a_file_that_cannot_be_used_leaves_the_text_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let plan = dir.path().join(PLAN);
        let texts = dir.path().join(LOOK);
        write_at(&plan, "good", 1);
        write_at(&texts, "last_turn = \"good\"\n", 1);
        let mut p = Prompts::load(Some(dir.path()));

        write_at(&plan, [0xff, 0xfe, 0x00], 2);
        write_at(&texts, "last_turn = \"half a string\n", 2);
        p.refresh(t0());
        assert_eq!(p.plan, "good");
        assert_eq!(p.text("last_turn"), "good");

        // Put right, it is taken in.
        write_at(&plan, "better", 3);
        write_at(&texts, "last_turn = \"better\"\n", 3);
        p.refresh(t0() + Duration::seconds(5));
        assert_eq!(p.plan, "better");
        assert_eq!(p.text("last_turn"), "better");
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
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let written: Vec<PathBuf> = Prompts::export(dir, false)
            .unwrap()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        for path in [
            "character/persona.md",
            "character/texts.toml",
            "system/being.md",
            LOOK,
            "system/work/work.md",
            "system/work/texts.toml",
            TOOLS,
        ] {
            assert!(written.contains(&dir.join(path)), "{path} was not exported");
            assert!(dir.join(path).is_file());
        }
        assert_eq!(
            fs::read_to_string(dir.join("system/work/work.md")).unwrap(),
            ghost_work::prompts::FILES[0].built_in
        );

        // Changed where it was exported to, it is listed as the deployment's.
        write_at(
            &dir.join("system/work/texts.toml"),
            "no_call = \"Call one.\"\n",
            1,
        );
        write_at(
            &dir.join(TOOLS),
            "[delegate]\ndescription = \"Hand it on.\"\n",
            1,
        );
        let listed = Prompts::load(Some(dir)).listing();
        let find = |file: &str, key: Option<&str>| {
            listed
                .iter()
                .find(|l| l.file == file && l.key.as_deref() == key)
                .map(|l| (l.origin, l.differs))
        };
        assert_eq!(
            find("system/work/work.md", None),
            Some((Origin::Deployment, false))
        );
        assert_eq!(
            find("system/work/texts.toml", Some("no_call")),
            Some((Origin::Deployment, true))
        );
        assert_eq!(
            find("system/work/texts.toml", Some("audit")),
            Some((Origin::BuiltIn, false))
        );
        assert_eq!(
            find(TOOLS, Some("delegate")),
            Some((Origin::Deployment, true))
        );
        // A tool of the conversation and one of work, in the one file.
        assert!(find(TOOLS, Some("say")).is_some());
        assert!(find(TOOLS, Some("shell")).is_some());
    }

    #[test]
    fn export_writes_every_built_in_and_overwrites_only_when_forced() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        write_at(&dir.join(PLAN), "mine", 1);

        let done = Prompts::export(dir, false).unwrap();
        assert_eq!(done.len(), all_files().len());
        for File {
            path,
            built_in: text,
            ..
        } in all_files()
        {
            let written = done.iter().find(|(p, _)| *p == dir.join(path)).unwrap().1;
            assert_eq!(written, path != PLAN, "{path}");
            if path != PLAN {
                assert_eq!(fs::read_to_string(dir.join(path)).unwrap(), text);
            }
        }
        assert_eq!(fs::read_to_string(dir.join(PLAN)).unwrap(), "mine");

        // What was exported is what is built in, so nothing differs.
        fs::remove_file(dir.join(PLAN)).unwrap();
        Prompts::export(dir, false).unwrap();
        assert!(
            Prompts::load(Some(dir))
                .listing()
                .iter()
                .all(|l| l.origin == Origin::Deployment && !l.differs)
        );

        write_at(&dir.join(PLAN), "mine", 1);
        let done = Prompts::export(dir, true).unwrap();
        assert!(done.iter().all(|(_, written)| *written));
        assert_eq!(fs::read_to_string(dir.join(PLAN)).unwrap(), built_in(PLAN));
    }
}
