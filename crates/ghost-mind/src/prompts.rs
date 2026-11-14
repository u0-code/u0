//! The words a brain reads. Data, not code.
//!
//! Every template, every short piece of framing and every tool description
//! is looked for in three places, the first that has it winning: her own
//! directory's `prompts/`, the `[mind] prompts` directory, and what is built
//! into this binary. The built-ins are only the last resort, so that a lone
//! binary still runs; anything a person might want to change is a file.
//!
//! A changed file is taken in at the next use, without a restart: the mind
//! asks for a refresh as it wakes, and at most every couple of seconds that
//! looks at the modification time of each file it could read. Only a file
//! that changed is read again. A file that goes away gives way to the next
//! place; one that cannot be read, or does not parse, is warned about and
//! what was in use stays in use.
//!
//! The templates are whole Markdown files. The short pieces are named
//! entries in `texts.toml`, and the tool words are a table per tool in
//! `tools.toml`; an override of either need only carry what it changes.
//! None of this belongs to her: it is how a brain reads her persona and her
//! journal, and changing it does not change who she is.

use std::{
    collections::{BTreeMap, HashMap},
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::SystemTime,
};

use chrono::{DateTime, Duration, Utc};
use ghost_core::narrate;
use serde::Deserialize;
use tracing::{info, warn};

/// The directory of prompts inside her own.
pub const DIR: &str = "prompts";
const TEXTS: &str = "texts.toml";
const TOOLS: &str = "tools.toml";
/// The least time between two looks at the files.
const CHECK_EVERY: Duration = Duration::seconds(2);

/// Every file a prompts directory can hold, with what is built in.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("self.md", include_str!("../prompts/self.md")),
    ("now.md", include_str!("../prompts/now.md")),
    ("digest.md", include_str!("../prompts/digest.md")),
    ("reply_json.md", include_str!("../prompts/reply_json.md")),
    ("reply_tools.md", include_str!("../prompts/reply_tools.md")),
    ("attend.md", include_str!("../prompts/attend.md")),
    ("plan.md", include_str!("../prompts/plan.md")),
    ("lived.md", include_str!("../prompts/lived.md")),
    ("reflect.md", include_str!("../prompts/reflect.md")),
    ("reflect_on.md", include_str!("../prompts/reflect_on.md")),
    ("browse.md", include_str!("../prompts/browse.md")),
    ("gate.md", include_str!("../prompts/gate.md")),
    ("view.md", include_str!("../prompts/view.md")),
    ("review.md", include_str!("../prompts/review.md")),
    ("working.md", include_str!("../prompts/working.md")),
    (TEXTS, include_str!("../prompts/texts.toml")),
    (TOOLS, include_str!("../prompts/tools.toml")),
];

static SHIPPED_TEXTS: LazyLock<Texts> =
    LazyLock::new(|| texts(built_in(TEXTS)).expect("the built-in texts.toml parses"));
static SHIPPED_TOOLS: LazyLock<Tools> =
    LazyLock::new(|| tools(built_in(TOOLS)).expect("the built-in tools.toml parses"));

type Texts = HashMap<String, String>;
type Tools = BTreeMap<String, Words>;

/// What a tool is called in words: its description, and its arguments'.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Words {
    description: Option<String>,
    #[serde(default)]
    parameters: BTreeMap<String, String>,
}

/// Where a prompt came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Her own directory's `prompts/`.
    Ghost,
    /// The `[mind] prompts` directory in `ghostd.toml`.
    Config,
    /// This binary.
    BuiltIn,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::Ghost => "ghost",
            Self::Config => "config",
            Self::BuiltIn => "built-in",
        })
    }
}

/// One prompt, text or tool, and where what is in use came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The file it is in.
    pub file: &'static str,
    /// The text or tool in that file, for `texts.toml` and `tools.toml`.
    pub key: Option<String>,
    pub origin: Origin,
    /// The file it came from, unless it is built in.
    pub path: Option<PathBuf>,
    /// Whether what is in use is not what is built in.
    pub differs: bool,
}

/// What a file looked like the last time it was looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stamp {
    Missing,
    At(SystemTime, u64),
    /// There, but it could not even be looked at.
    Unreadable,
}

/// A file as last read: the text, if it could ever be read.
#[derive(Debug, Clone)]
struct Held {
    stamp: Stamp,
    text: Option<String>,
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
    /// `review.md`: the brief for sitting down to look at her public writing.
    pub review: String,
    /// `working.md`: what she is told about a piece of work she took on.
    pub working: String,
    texts: Texts,
    tools: Tools,
    /// The directories looked in, the first that has a file winning.
    layers: Vec<(Origin, PathBuf)>,
    /// What each file in them was when last read.
    held: HashMap<PathBuf, Held>,
    /// When the files were last looked at.
    checked: Option<DateTime<Utc>>,
}

impl Prompts {
    /// What is built into this binary, and nothing else.
    pub fn shipped() -> Self {
        let text = |name| built_in(name).to_owned();
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
            review: text("review.md"),
            working: text("working.md"),
            texts: SHIPPED_TEXTS.clone(),
            tools: SHIPPED_TOOLS.clone(),
            layers: Vec::new(),
            held: HashMap::new(),
            checked: None,
        }
    }

    /// The prompts of the ghost in `ghost`: from her own `prompts/`, then
    /// from `config` if there is one, then built in. Nothing here fails: a
    /// file that cannot be used is warned about and passed over.
    pub fn load(ghost: &Path, config: Option<&Path>) -> Self {
        let mut prompts = Self::shipped();
        prompts.layers.push((Origin::Ghost, ghost.join(DIR)));
        if let Some(config) = config {
            prompts.layers.push((Origin::Config, config.to_owned()));
        }
        prompts.scan(false);
        prompts
    }

    /// Take in any file that changed, if the files have not been looked at
    /// in the last couple of seconds. Cheap enough to call on every wake.
    pub fn refresh(&mut self, now: DateTime<Utc>) {
        if self.layers.is_empty()
            || self
                .checked
                .is_some_and(|at| now >= at && now - at < CHECK_EVERY)
        {
            return;
        }
        self.checked = Some(now);
        self.scan(true);
    }

    /// A text from `texts.toml`, as it is.
    pub fn text(&self, key: &str) -> &str {
        let text = self.texts.get(key);
        debug_assert!(text.is_some(), "texts.toml has no text called {key}");
        text.map_or("", String::as_str)
    }

    /// A text from `texts.toml`, with its placeholders filled.
    pub fn fill(&self, key: &str, pairs: &[(&str, String)]) -> String {
        narrate::fill(self.text(key), pairs)
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

    /// What tool `name` is described as.
    pub fn tool(&self, name: &str) -> &str {
        let words = self.tools.get(name).and_then(|w| w.description.as_deref());
        debug_assert!(words.is_some(), "tools.toml does not describe {name}");
        words.unwrap_or_default()
    }

    /// What argument `parameter` of tool `name` is described as, if it is.
    pub fn parameter(&self, name: &str, parameter: &str) -> Option<&str> {
        self.tools
            .get(name)?
            .parameters
            .get(parameter)
            .map(String::as_str)
    }

    /// Every template, text and tool, with where what is in use came from.
    pub fn listing(&self) -> Vec<Listed> {
        let mut out = Vec::new();
        let current: HashMap<&str, &str> = self.templates().into_iter().collect();
        for (file, shipped) in BUILT_IN {
            if *file == TEXTS || *file == TOOLS {
                continue;
            }
            let (origin, path) = self.winner(file);
            out.push(Listed {
                file,
                key: None,
                origin,
                path,
                differs: current.get(file).is_some_and(|now| now != shipped),
            });
        }
        let mut keys: Vec<&String> = SHIPPED_TEXTS.keys().collect();
        keys.sort();
        for key in keys {
            let (origin, path) = self.provider(TEXTS, |text| {
                texts(text).is_ok_and(|t| t.contains_key(key.as_str()))
            });
            out.push(Listed {
                file: TEXTS,
                key: Some(key.clone()),
                origin,
                path,
                differs: self.texts.get(key) != SHIPPED_TEXTS.get(key),
            });
        }
        for name in SHIPPED_TOOLS.keys() {
            let (origin, path) = self.provider(TOOLS, |text| {
                tools(text).is_ok_and(|t| t.contains_key(name.as_str()))
            });
            out.push(Listed {
                file: TOOLS,
                key: Some(name.clone()),
                origin,
                path,
                differs: self.tools.get(name) != SHIPPED_TOOLS.get(name),
            });
        }
        out
    }

    /// Write every built-in file into `dir`, for a person to change. A
    /// file already there is left alone unless `force`. Returns each file
    /// and whether it was written.
    pub fn export(dir: &Path, force: bool) -> io::Result<Vec<(PathBuf, bool)>> {
        fs::create_dir_all(dir)?;
        let mut done = Vec::with_capacity(BUILT_IN.len());
        for (name, text) in BUILT_IN {
            let path = dir.join(name);
            let write = force || !path.exists();
            if write {
                fs::write(&path, text)?;
            }
            done.push((path, write));
        }
        Ok(done)
    }

    /// The templates as they are now, by file name.
    fn templates(&self) -> [(&'static str, &str); 15] {
        [
            ("self.md", self.this.as_str()),
            ("now.md", &self.now),
            ("digest.md", &self.digest),
            ("reply_json.md", &self.reply_json),
            ("reply_tools.md", &self.reply_tools),
            ("attend.md", &self.attend),
            ("plan.md", &self.plan),
            ("lived.md", &self.lived),
            ("reflect.md", &self.reflect),
            ("reflect_on.md", &self.reflect_on),
            ("browse.md", &self.browse),
            ("gate.md", &self.gate),
            ("view.md", &self.view),
            ("review.md", &self.review),
            ("working.md", &self.working),
        ]
    }

    fn slot(&mut self, name: &str) -> Option<&mut String> {
        Some(match name {
            "self.md" => &mut self.this,
            "now.md" => &mut self.now,
            "digest.md" => &mut self.digest,
            "reply_json.md" => &mut self.reply_json,
            "reply_tools.md" => &mut self.reply_tools,
            "attend.md" => &mut self.attend,
            "plan.md" => &mut self.plan,
            "lived.md" => &mut self.lived,
            "reflect.md" => &mut self.reflect,
            "reflect_on.md" => &mut self.reflect_on,
            "browse.md" => &mut self.browse,
            "gate.md" => &mut self.gate,
            "view.md" => &mut self.view,
            "review.md" => &mut self.review,
            "working.md" => &mut self.working,
            _ => return None,
        })
    }

    /// The text of `name` in each directory that has a usable one, first
    /// to last.
    fn found(&self, name: &str) -> impl Iterator<Item = (Origin, &Path, &str)> {
        self.layers.iter().filter_map(move |(origin, dir)| {
            let path = dir.join(name);
            let held = self.held.get(&path)?;
            let text = held.text.as_deref()?;
            Some((*origin, dir.as_path(), text))
        })
    }

    /// Where the template `name` in use comes from.
    fn winner(&self, name: &str) -> (Origin, Option<PathBuf>) {
        self.provider(name, |_| true)
    }

    /// The first directory whose `file` says something `has` looks for.
    fn provider(&self, file: &str, has: impl Fn(&str) -> bool) -> (Origin, Option<PathBuf>) {
        self.found(file)
            .find(|(_, _, text)| has(text))
            .map_or((Origin::BuiltIn, None), |(origin, dir, _)| {
                (origin, Some(dir.join(file)))
            })
    }

    /// Look at every file, read again the ones that changed, and if any
    /// did, work out again what is in use. `loud` says each change.
    fn scan(&mut self, loud: bool) -> bool {
        let mut changed = false;
        for (_, dir) in self.layers.clone() {
            // A directory that is not there is one look, not seventeen.
            let there = match fs::metadata(&dir) {
                Ok(_) => Some(true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Some(false),
                Err(_) => None,
            };
            for (name, _) in BUILT_IN {
                let path = dir.join(name);
                let stamp = match there {
                    Some(true) => stamp(&path),
                    Some(false) => Stamp::Missing,
                    None => Stamp::Unreadable,
                };
                changed |= self.take(path, name, stamp, loud);
            }
        }
        if changed {
            self.settle();
        }
        changed
    }

    /// One file, as it is now. Returns whether what it says changed.
    fn take(&mut self, path: PathBuf, name: &str, stamp: Stamp, loud: bool) -> bool {
        let before = self
            .held
            .get(&path)
            .map_or(Stamp::Missing, |h| h.stamp.clone());
        if before == stamp {
            return false;
        }
        match stamp {
            Stamp::Missing => {
                let had = self.held.remove(&path).is_some_and(|h| h.text.is_some());
                if loud && had {
                    info!(
                        "{} is gone: {name} comes from the next place that has it",
                        path.display()
                    );
                }
                had
            }
            Stamp::Unreadable => {
                warn!("cannot look at {}: the {name} in use stays", path.display());
                self.held
                    .entry(path)
                    .or_insert(Held {
                        stamp: Stamp::Missing,
                        text: None,
                    })
                    .stamp = Stamp::Unreadable;
                false
            }
            Stamp::At(..) => match read(&path, name) {
                Ok(text) => {
                    if loud {
                        info!("read {} again", path.display());
                    }
                    let held = Held {
                        stamp,
                        text: Some(text),
                    };
                    let same =
                        self.held.get(&path).and_then(|h| h.text.as_ref()) == held.text.as_ref();
                    self.held.insert(path, held);
                    !same
                }
                Err(why) => {
                    warn!("{}: {why}. The {name} in use stays", path.display());
                    // Remembered as looked at, so that it is not warned
                    // about again until it changes.
                    self.held
                        .entry(path)
                        .or_insert(Held {
                            stamp: Stamp::Missing,
                            text: None,
                        })
                        .stamp = stamp;
                    false
                }
            },
        }
    }

    /// Work out what is in use from what was read.
    fn settle(&mut self) {
        let chosen: Vec<(&str, String)> = BUILT_IN
            .iter()
            .map(|(name, shipped)| {
                let text = self
                    .found(name)
                    .next()
                    .map_or(*shipped, |(_, _, text)| text);
                (*name, text.to_owned())
            })
            .collect();
        for (name, text) in chosen {
            if let Some(slot) = self.slot(name) {
                *slot = text;
            }
        }
        // The texts and tools are put together one entry at a time, the
        // nearest directory last so that it wins.
        let mut merged_texts = SHIPPED_TEXTS.clone();
        let mut merged_tools = SHIPPED_TOOLS.clone();
        let mut from_texts: Vec<&str> = self.found(TEXTS).map(|(_, _, t)| t).collect();
        let mut from_tools: Vec<&str> = self.found(TOOLS).map(|(_, _, t)| t).collect();
        from_texts.reverse();
        from_tools.reverse();
        for text in from_texts {
            for (key, value) in texts(text).unwrap_or_default() {
                if let Some(slot) = merged_texts.get_mut(&key) {
                    *slot = value;
                }
            }
        }
        for text in from_tools {
            for (name, words) in tools(text).unwrap_or_default() {
                if let Some(slot) = merged_tools.get_mut(&name) {
                    if words.description.is_some() {
                        slot.description = words.description;
                    }
                    slot.parameters.extend(words.parameters);
                }
            }
        }
        self.texts = merged_texts;
        self.tools = merged_tools;
    }
}

/// The built-in text of the file `name`.
fn built_in(name: &str) -> &'static str {
    BUILT_IN
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
        .unwrap_or_default()
}

fn stamp(path: &Path) -> Stamp {
    match fs::metadata(path) {
        Ok(meta) => Stamp::At(
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Stamp::Missing,
        Err(_) => Stamp::Unreadable,
    }
}

/// The text of a file, if it is usable as the file `name`. Names in it that
/// mean nothing are warned about; they do not make it unusable.
fn read(path: &Path, name: &str) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot be read: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "is not UTF-8".to_owned())?;
    let unknown: Vec<String> = match name {
        TEXTS => texts(&text)?
            .into_keys()
            .filter(|k| !SHIPPED_TEXTS.contains_key(k))
            .collect(),
        TOOLS => {
            let mut unknown = Vec::new();
            for (tool, words) in tools(&text)? {
                match SHIPPED_TOOLS.get(&tool) {
                    None => unknown.push(tool),
                    Some(shipped) => unknown.extend(
                        words
                            .parameters
                            .into_keys()
                            .filter(|p| !shipped.parameters.contains_key(p))
                            .map(|p| format!("{tool}.{p}")),
                    ),
                }
            }
            unknown
        }
        _ => Vec::new(),
    };
    if !unknown.is_empty() {
        warn!(
            "{}: nothing is called {}; left unused",
            path.display(),
            unknown.join(", ")
        );
    }
    Ok(text)
}

fn texts(text: &str) -> Result<Texts, String> {
    toml::from_str(text).map_err(|e| format!("does not parse: {e}"))
}

fn tools(text: &str) -> Result<Tools, String> {
    toml::from_str(text).map_err(|e| format!("does not parse: {e}"))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration as StdDuration, UNIX_EPOCH};

    use chrono::TimeZone;

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
        assert!(!SHIPPED_TEXTS.is_empty());
        for (name, words) in SHIPPED_TOOLS.iter() {
            assert!(words.description.is_some(), "{name} has no description");
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
            "look = \"the config's\"\nno_look_up = \"the config's\"\n",
            1,
        );
        let p = Prompts::load(ghost.path(), Some(config.path()));
        assert_eq!(p.plan, "hers");
        assert_eq!(p.gate, "the config's gate");
        assert_eq!(p.attend, Prompts::shipped().attend);
        assert_eq!(p.text("look"), "hers");
        assert_eq!(p.text("no_look_up"), "the config's");
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
        assert_eq!(origin(TEXTS, Some("no_look_up")), (Origin::Config, true));
        assert_eq!(origin(TOOLS, Some("say")), (Origin::BuiltIn, false));
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
    fn export_writes_every_built_in_and_overwrites_only_when_forced() {
        let ghost = tempfile::tempdir().unwrap();
        let dir = ghost.path().join(DIR);
        write_at(&dir.join("plan.md"), "mine", 1);

        let done = Prompts::export(&dir, false).unwrap();
        assert_eq!(done.len(), BUILT_IN.len());
        for (name, text) in BUILT_IN {
            let written = done.iter().find(|(p, _)| p.ends_with(name)).unwrap().1;
            assert_eq!(written, *name != "plan.md", "{name}");
            if *name != "plan.md" {
                assert_eq!(fs::read_to_string(dir.join(name)).unwrap(), *text);
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
