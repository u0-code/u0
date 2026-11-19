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
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
pub use ghost_core::prompts::{File, Kind, Layer, Listed, Loop};
use ghost_core::{prompt, prompts::Store};

/// The mind's table of texts: `system/logic/texts/` and
/// `character/texts.toml`, read as one.
pub const TEXTS: &str = "mind";

/// The channels that have a page of their own in `system/channels/`. A
/// channel by any other name reads `chat`'s, unless its settings name
/// another (`[channels.<name>] page`).
pub const PAGES: &[&str] = &["chat", "console", "x"];

/// The chat platforms the chat page says something of in particular:
/// `platform_<name>` in `system/channels/texts.toml`, put into the page as
/// `{{platform}}`. They read the one chat page, so that a change to it
/// reaches every one of them.
pub const PLATFORMS: &[&str] = &["qq", "discord", "telegram"];

/// The files the mind reads, with what is built in. The work loop's are in
/// [`ghost_work::prompts::FILES`], and are read beside them.
pub const FILES: &[File] = &[
    prompt!("character/persona.md", Kind::Whole, Layer::Persona),
    prompt!("character/texts.toml", Kind::Texts(TEXTS), Layer::Persona),
    prompt!("system/being.md", Kind::Whole, Layer::Being),
    prompt!("system/logic/now.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/digest.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/reply_json.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/reply_tools.md", Kind::Whole, Layer::Logic),
    prompt!("system/channels/chat.md", Kind::Whole, Layer::Logic),
    prompt!("system/channels/console.md", Kind::Whole, Layer::Logic),
    prompt!("system/channels/x.md", Kind::Whole, Layer::Logic),
    prompt!(
        "system/channels/texts.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!("system/logic/plan.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/lived.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/reflect.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/reflect_on.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/browse.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/gate.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/view.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/report.md", Kind::Whole, Layer::Logic),
    prompt!("system/logic/review.md", Kind::Whole, Layer::Logic),
    prompt!(
        "system/logic/texts/look.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/moment.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/place.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/addressed.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/right_now.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/elsewhere.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/abilities.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/keep.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/view.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/browse.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/day.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/report.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/working.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/conversation.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/people.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/reread.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/public.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!(
        "system/logic/texts/empty.toml",
        Kind::Texts(TEXTS),
        Layer::Logic
    ),
    prompt!("system/work/working.md", Kind::Whole, Layer::Work),
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

/// [`all_files`], with `over` built in in place of what this binary would
/// otherwise have for the same file: the way a binary that runs a ghost of
/// its own, as the core of a machine does, gives her its own persona and
/// being while still reading a deployment's directory over them.
pub fn files_over(over: &[(&str, &'static str)]) -> Vec<File> {
    let mut files = all_files();
    for file in &mut files {
        if let Some((_, text)) = over.iter().find(|(path, _)| *path == file.path) {
            file.built_in = text;
        }
    }
    files
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
    /// `system/channels/`: the page she reads a place on, one for each
    /// channel that has a page of its own, by name. `chat` is for any
    /// other.
    pages: BTreeMap<String, String>,
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
        Self::load_over(dir, &[])
    }

    /// [`Prompts::load`], with `over` built in: see [`files_over`].
    pub fn load_over(dir: Option<&Path>, over: &[(&str, &'static str)]) -> Self {
        Self::from_store(Store::load(&files_over(over), dir))
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
            pages: PAGES
                .iter()
                .map(|name| {
                    let path = format!("system/channels/{name}.md");
                    ((*name).to_owned(), store.whole(&path).to_owned())
                })
                .collect(),
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

    /// The page called `name`, or the chat page if there is none by that
    /// name.
    pub fn page(&self, name: &str) -> &str {
        self.pages
            .get(name)
            .or_else(|| self.pages.get("chat"))
            .map_or("", String::as_str)
    }

    /// What the page called `name` says of that platform in particular,
    /// for `{{platform}}`; nothing for a platform it says nothing of.
    pub fn platform(&self, name: &str) -> &str {
        if PLATFORMS.contains(&name) {
            self.text(&format!("platform_{name}"))
        } else {
            ""
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

    /// `value`, or text `key` in its place when it is empty: what a slot
    /// under a heading of its own says when there is nothing to put in it.
    /// (Where the heading goes with what fills it, [`Prompts::section`]
    /// leaves out both.)
    pub fn or(&self, value: String, key: &str) -> String {
        if value.is_empty() {
            self.text(key).to_owned()
        } else {
            value
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

    /// The words of the work loop, from the same directory as these. A
    /// store of its own: the work runs apart from the mind and looks at
    /// its files again before each step, as the mind does when it wakes.
    pub fn for_work(&self) -> ghost_work::Prompts {
        ghost_work::Prompts::load(self.store.dir())
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
    use std::{
        collections::BTreeSet,
        fs,
        time::{Duration as StdDuration, UNIX_EPOCH},
    };

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

    /// The code of the crate that is not a test, file by file: each file
    /// up to its module of tests, and nothing under `tests/`.
    fn code() -> Vec<String> {
        fn walk(dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n != "tests") {
                        walk(&path, out);
                    }
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = fs::read_to_string(&path).unwrap();
                    let code = text
                        .split(
                            "#[cfg(test)]
mod tests {",
                        )
                        .next()
                        .unwrap_or_default();
                    out.push(code.to_owned());
                }
            }
        }
        let mut out = Vec::new();
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out
    }

    /// The string literals of `code` that could be a key, with whether
    /// each is a pattern (followed by `=>`) rather than a value. Comments
    /// and character literals are passed over.
    fn literals(code: &str) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        let mut chars = code.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '/' if code[i..].starts_with("//") => {
                    while chars.next_if(|(_, c)| *c != '\n').is_some() {}
                }
                '\'' if code[i..].starts_with("'\"'") || code[i..].starts_with("'\\\"'") => {
                    while chars.next_if(|(_, c)| *c != '\'').is_some() {}
                    chars.next();
                }
                '"' => {
                    let mut text = String::new();
                    let mut escaped = false;
                    for (_, c) in chars.by_ref() {
                        match c {
                            _ if escaped => {
                                escaped = false;
                                text.push('\\');
                            }
                            '\\' => escaped = true,
                            '"' => break,
                            _ => text.push(c),
                        }
                    }
                    let rest = chars.peek().map_or("", |(at, _)| &code[*at..]);
                    if !text.is_empty()
                        && text
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    {
                        out.push((text, rest.trim_start().starts_with("=>")));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The arguments of the call whose `(` is just before `code`, as
    /// written: split at the commas that are not inside brackets or a
    /// string.
    fn arguments(code: &str) -> Vec<&str> {
        let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
        let (mut start, mut out) = (0, Vec::new());
        for (i, c) in code.char_indices() {
            if quoted {
                match c {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => quoted = false,
                    _ => {}
                }
                continue;
            }
            match c {
                '"' => quoted = true,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' if depth == 0 => {
                    out.push(&code[start..i]);
                    return out;
                }
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    out.push(&code[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        out
    }

    /// The keys `code` asks the mind's texts for: whatever is written as
    /// the key of `.text(`, `.fill(` and `.section(`, and as the second
    /// argument of `.or(`, the arms of an `if` or a `match` included.
    fn asked(code: &str) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for (call, nth) in [(".text(", 0), (".fill(", 0), (".section(", 0), (".or(", 1)] {
            for (at, _) in code.match_indices(call) {
                let args = arguments(&code[at + call.len()..]);
                if let Some(arg) = args.get(nth).filter(|_| nth == 0 || args.len() == 2) {
                    keys.extend(
                        literals(arg)
                            .into_iter()
                            .filter(|(_, pattern)| !pattern)
                            .map(|(key, _)| key),
                    );
                }
            }
        }
        keys
    }

    #[test]
    fn every_text_of_the_mind_is_asked_for_and_every_one_asked_for_is_there() {
        let tables: Vec<&str> = FILES
            .iter()
            .filter(|f| f.kind == Kind::Texts(TEXTS))
            .map(|f| f.path)
            .collect();
        let table: BTreeSet<String> = Prompts::shipped()
            .listing()
            .into_iter()
            .filter(|l| tables.contains(&l.file))
            .filter_map(|l| l.key)
            .collect();
        let code = code();
        let asked: BTreeSet<String> = code.iter().flat_map(|c| asked(c)).collect();
        let missing: Vec<&String> = asked.difference(&table).collect();
        assert!(missing.is_empty(), "asked for, and in no file: {missing:?}");
        // A key may be chosen into a variable before it is asked for, so
        // being written anywhere in the code counts; the platforms' are
        // made from their names.
        let written: BTreeSet<String> = code
            .iter()
            .flat_map(|c| literals(c))
            .map(|(key, _)| key)
            .chain(PLATFORMS.iter().map(|p| format!("platform_{p}")))
            .collect();
        let unasked: Vec<&String> = table.difference(&written).collect();
        assert!(
            unasked.is_empty(),
            "in a file, and never asked for: {unasked:?}"
        );
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
        // A channel with no page of its own reads the chat page.
        assert_eq!(shipped.page("term"), shipped.page("chat"));
        assert_eq!(shipped.platform("term"), "");
    }

    #[test]
    fn each_chat_platform_reads_the_chat_page_with_its_own_paragraph_before_how_to_answer() {
        let shipped = Prompts::shipped();
        let chat = shipped.page("chat");
        let without = ghost_core::narrate::fill(chat, &[("platform", String::new())]);
        for (name, begins) in [
            ("qq", "This is QQ: "),
            ("discord", "This is Discord: "),
            ("telegram", "This is Telegram. "),
        ] {
            assert_eq!(shipped.page(name), chat, "{name} has a page of its own");
            let own = shipped.platform(name);
            assert!(own.starts_with(begins) && own.ends_with(".\n\n"), "{own:?}");
            let page = ghost_core::narrate::fill(chat, &[("platform", own.to_owned())]);
            // The page is the chat page, word for word, with the platform's
            // paragraph in front of how she answers and nothing else.
            assert_eq!(
                page,
                without.replacen("{{reply}}", &format!("{own}{{{{reply}}}}"), 1),
                "{name}"
            );
        }
    }

    #[test]
    fn what_is_laid_over_the_built_ins_is_built_in_and_a_deployments_copy_still_wins() {
        let over = [("system/being.md", "a hand on this machine")];
        let theirs = Prompts::load_over(None, &over);
        assert_eq!(theirs.being, "a hand on this machine");
        assert_eq!(
            theirs.persona,
            Prompts::shipped().persona,
            "only what is laid over"
        );
        let dir = tempfile::tempdir().unwrap();
        write_at(&dir.path().join("system/being.md"), "the deployment's", 1);
        assert_eq!(
            Prompts::load_over(Some(dir.path()), &over).being,
            "the deployment's"
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
        assert_eq!(layer("system/channels/chat.md"), Layer::Logic);
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
        assert_eq!(p.page("chat"), Prompts::shipped().page("chat"));
        assert_eq!(p.text("last_turn"), "the deployment's");
        assert_eq!(p.text("not_sent"), Prompts::shipped().text("not_sent"));

        let listed = p.listing();
        // Whether it comes from the deployment's directory, and whether it
        // is not what is built in.
        let origin = |file: &str, key: Option<&str>| {
            listed
                .iter()
                .find(|l| l.file == file && l.key.as_deref() == key)
                .map(|l| (l.path.is_some(), l.differs))
                .unwrap()
        };
        assert_eq!(origin("system/logic/gate.md", None), (true, true));
        assert_eq!(origin("character/persona.md", None), (true, true));
        assert_eq!(origin("system/channels/chat.md", None), (false, false));
        assert_eq!(origin(LOOK, Some("last_turn")), (true, true));
        assert_eq!(origin(LOOK, Some("not_sent")), (false, false));
        assert_eq!(origin(TOOLS, Some("say")), (false, false));
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
    fn the_desks_directory_replaces_her_persona_and_being_and_nothing_else() {
        let desk = tree().join("system/desk");
        let p = Prompts::load(Some(&desk));
        let replaced: Vec<&str> = p
            .listing()
            .into_iter()
            .filter(|l| l.path.is_some())
            .map(|l| l.file)
            .collect();
        assert_eq!(replaced, ["character/persona.md", "system/being.md"]);
        assert_eq!(
            p.persona,
            fs::read_to_string(desk.join("character/persona.md")).unwrap()
        );
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
            "system/work/texts/sitting.toml",
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
            &dir.join("system/work/texts/sitting.toml"),
            "no_call = \"Call one.\"\n",
            1,
        );
        write_at(
            &dir.join(TOOLS),
            "[delegate]\ndescription = \"Hand it on.\"\n",
            1,
        );
        let listed = Prompts::load(Some(dir)).listing();
        // As `origin` above.
        let find = |file: &str, key: Option<&str>| {
            listed
                .iter()
                .find(|l| l.file == file && l.key.as_deref() == key)
                .map(|l| (l.path.is_some(), l.differs))
        };
        assert_eq!(find("system/work/work.md", None), Some((true, false)));
        assert_eq!(
            find("system/work/texts/sitting.toml", Some("no_call")),
            Some((true, true))
        );
        assert_eq!(
            find("system/work/texts/sitting.toml", Some("audit")),
            Some((false, false))
        );
        assert_eq!(find(TOOLS, Some("delegate")), Some((true, true)));
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
                .all(|l| l.path.is_some() && !l.differs)
        );

        write_at(&dir.join(PLAN), "mine", 1);
        let done = Prompts::export(dir, true).unwrap();
        assert!(done.iter().all(|(_, written)| *written));
        assert_eq!(fs::read_to_string(dir.join(PLAN)).unwrap(), built_in(PLAN));
    }
}
