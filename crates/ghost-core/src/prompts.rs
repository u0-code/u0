//! The words a brain reads, kept in files and taken in again when they
//! change. Data, not code.
//!
//! A crate that talks to a brain lists the files it reads, each with what
//! is built into it and the layer it belongs to, and gets a [`Store`] that
//! looks for every one of them in two places, the first that has it
//! winning: the deployment's directory of prompts (`[mind] prompts` in
//! `ghostd.toml`), and the binary. The built-ins are only the last resort,
//! so that a lone binary still runs; anything a person might want to change
//! is a file.
//!
//! Every file belongs to one of four [`Layer`]s: the persona template, which
//! arranges her own fields into prose; her being, what she is and the
//! ground rules she lives by; the logic of her conversations and her day;
//! and her work. All four are shared by every character a deployment runs.
//! What makes one character herself, her name, temperament, voice, story,
//! values and past, is her `ghost.toml`, which is in her journal; nothing
//! in these files is about anyone in particular. So a ghost's own directory
//! holds no prompts: a copy there would make one character work unlike the
//! others, and swapping her persona would no longer be all it takes to
//! swap her. What [`misplaced`] finds there is warned about and not read.
//!
//! A changed file is taken in at the next use, without a restart: whoever
//! holds a store asks it to refresh, and at most every couple of seconds
//! that looks at the modification time of each file it could read. Only a
//! file that changed is read again. A file that goes away gives way to the
//! built-in; one that cannot be read, or does not parse, is warned about
//! and what was in use stays in use.
//!
//! There are three kinds of file. A template is used whole. A file of texts
//! is a TOML table of named pieces, and a file of tools a table per tool of
//! what it is said to be; an override of either need only carry what it
//! changes, and is merged one entry at a time.
//!
//! The mind and the work loop read the same directory: one `tools.toml`
//! says what every tool of both is, and each has its templates and texts
//! beside it under names of its own. Names are unique across layers, so a
//! directory of overrides is flat: a person edits `attend.md`, not
//! `logic/attend.md`, and the built-ins can be arranged differently later
//! without moving anybody's copies. None of it is in her journal, and
//! changing it does not change who she is.
//!
//! A person's copy of a built-in file outlives the build it came from; how
//! a copy that has fallen behind is noticed is [`exported`].

pub mod exported;

use std::{
    collections::{BTreeMap, HashMap},
    fmt, fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use tracing::{info, warn};

use crate::narrate;

/// What a directory of prompts inside a ghost's own would be called. It is
/// not read: see [`misplaced`].
pub const DIR: &str = "prompts";
/// The least time between two looks at the files.
const CHECK_EVERY: Duration = Duration::seconds(2);

/// How a file is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Used as it is.
    Whole,
    /// Named pieces of text, `key = "text"`.
    Texts,
    /// What each tool is said to be: a table per tool.
    Tools,
}

/// Which part of what a brain is told a file is. The built-ins sit in a
/// directory per layer; an override is found by its name alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// The template that arranges her own fields from `ghost.toml` into
    /// prose. It holds no rules, only the arrangement, so that two
    /// characters differ in what fills it and in nothing else.
    Persona,
    /// What she is, a person made of language, and the ground rules: whose
    /// word counts, the safety line, how she keeps things.
    Being,
    /// How a conversation and a day go: looking, replying, planning,
    /// living, reflecting, posting.
    Logic,
    /// How a piece of work goes, and what every tool is.
    Work,
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::Persona => "persona",
            Self::Being => "being",
            Self::Logic => "logic",
            Self::Work => "work",
        })
    }
}

/// One file a store reads, and what is built in for it.
#[derive(Debug, Clone, Copy)]
pub struct File {
    pub name: &'static str,
    pub built_in: &'static str,
    pub kind: Kind,
    pub layer: Layer,
}

/// The two loops a tool can be offered in. A tool may be described
/// differently in each, under `[tool.conversation]` or `[tool.work]`; what
/// one of those leaves out is what the tool's own table says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Loop {
    /// Looking at a conversation, a few turns at a time.
    Conversation,
    /// A piece of work, for as many steps as it takes.
    Work,
}

/// Where a prompt came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The deployment's directory of prompts, named in configuration.
    Deployment,
    /// This binary.
    BuiltIn,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::Deployment => "deployment",
            Self::BuiltIn => "built-in",
        })
    }
}

/// One template, text or tool, and where what is in use came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The file it is in.
    pub file: &'static str,
    /// The layer that file belongs to.
    pub layer: Layer,
    /// The text or tool in that file, for a file of texts or tools.
    pub key: Option<String>,
    pub origin: Origin,
    /// The file it came from, unless it is built in.
    pub path: Option<PathBuf>,
    /// Whether what is in use is not what is built in.
    pub differs: bool,
}

type Texts = HashMap<String, String>;
type Tools = BTreeMap<String, Words>;

/// What a tool is said to be: its description, and its arguments'. An
/// argument inside a list of objects is named by its path, `steps.text`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Said {
    description: Option<String>,
    #[serde(default)]
    parameters: BTreeMap<String, String>,
}

impl Said {
    /// Lay `over` on top: what it says wins, what it leaves out stays.
    fn take(&mut self, over: Self) {
        if over.description.is_some() {
            self.description = over.description;
        }
        self.parameters.extend(over.parameters);
    }
}

/// A tool's words, and how it is said differently in either loop.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Words {
    description: Option<String>,
    /// What having it lets her do, in a few words, for a tool that is
    /// worth telling her she has.
    ability: Option<String>,
    #[serde(default)]
    parameters: BTreeMap<String, String>,
    conversation: Option<Said>,
    work: Option<Said>,
}

impl Words {
    fn take(&mut self, over: Self) {
        if over.description.is_some() {
            self.description = over.description;
        }
        if over.ability.is_some() {
            self.ability = over.ability;
        }
        self.parameters.extend(over.parameters);
        for (mine, theirs) in [
            (&mut self.conversation, over.conversation),
            (&mut self.work, over.work),
        ] {
            if let Some(theirs) = theirs {
                mine.get_or_insert_with(Said::default).take(theirs);
            }
        }
    }

    fn in_loop(&self, lp: Loop) -> Option<&Said> {
        match lp {
            Loop::Conversation => self.conversation.as_ref(),
            Loop::Work => self.work.as_ref(),
        }
    }

    /// Every argument named anywhere in it, for telling a name that means
    /// nothing from one that does.
    fn names(&self) -> impl Iterator<Item = &String> {
        self.parameters.keys().chain(
            [&self.conversation, &self.work]
                .into_iter()
                .flatten()
                .flat_map(|s| s.parameters.keys()),
        )
    }
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

/// The files one reader of prompts reads, as they are now.
#[derive(Debug, Clone)]
pub struct Store {
    files: Vec<File>,
    shipped_texts: HashMap<&'static str, Texts>,
    shipped_tools: Tools,
    /// The templates in use, by file.
    whole: HashMap<&'static str, String>,
    /// The texts in use, by file.
    texts: HashMap<&'static str, Texts>,
    tools: Tools,
    /// The directories looked in, the first that has a file winning.
    layers: Vec<(Origin, PathBuf)>,
    /// What each file in them was when last read.
    held: HashMap<PathBuf, Held>,
    /// When the files were last looked at.
    checked: Option<DateTime<Utc>>,
}

impl Store {
    /// What is built in for `files`, and nothing else.
    ///
    /// # Panics
    ///
    /// If a built-in file of texts or tools does not parse: that is a
    /// mistake in the binary, and every test would show it.
    pub fn shipped(files: &[File]) -> Self {
        let mut shipped_texts = HashMap::new();
        let mut shipped_tools = Tools::new();
        let mut whole = HashMap::new();
        for file in files {
            match file.kind {
                Kind::Whole => {
                    whole.insert(file.name, file.built_in.to_owned());
                }
                Kind::Texts => {
                    let parsed = texts(file.built_in)
                        .unwrap_or_else(|e| panic!("the built-in {} {e}", file.name));
                    shipped_texts.insert(file.name, parsed);
                }
                Kind::Tools => {
                    let parsed = tools(file.built_in)
                        .unwrap_or_else(|e| panic!("the built-in {} {e}", file.name));
                    shipped_tools.extend(parsed);
                }
            }
        }
        Self {
            files: files.to_vec(),
            texts: shipped_texts.clone(),
            shipped_texts,
            tools: shipped_tools.clone(),
            shipped_tools,
            whole,
            layers: Vec::new(),
            held: HashMap::new(),
            checked: None,
        }
    }

    /// `files` as they are: from the deployment's directory `dir` if there
    /// is one, then built in. Nothing here fails: a file that cannot be
    /// used is warned about and passed over.
    pub fn load(files: &[File], dir: Option<&Path>) -> Self {
        let layers: Vec<(Origin, PathBuf)> = dir
            .map(|dir| (Origin::Deployment, dir.to_owned()))
            .into_iter()
            .collect();
        Self::layered(files, &layers)
    }

    /// `files` as they are in `layers`, the first that has a file winning,
    /// then built in.
    pub fn layered(files: &[File], layers: &[(Origin, PathBuf)]) -> Self {
        let mut store = Self::shipped(files);
        store.layers = layers.to_vec();
        store.scan(false);
        store
    }

    /// The directories looked in, first to last.
    pub fn layers(&self) -> &[(Origin, PathBuf)] {
        &self.layers
    }

    /// Take in any file that changed, if the files have not been looked at
    /// in the last couple of seconds. Cheap enough to call on every wake.
    /// Returns whether anything in use changed.
    pub fn refresh(&mut self, now: DateTime<Utc>) -> bool {
        if self.layers.is_empty()
            || self
                .checked
                .is_some_and(|at| now >= at && now - at < CHECK_EVERY)
        {
            return false;
        }
        self.checked = Some(now);
        self.scan(true)
    }

    /// Take in any file that changed, however recently the files were
    /// looked at. Returns whether anything in use changed.
    pub fn refresh_now(&mut self) -> bool {
        !self.layers.is_empty() && self.scan(true)
    }

    /// The template in the file `name`, as it is.
    pub fn whole(&self, name: &str) -> &str {
        let text = self.whole.get(name);
        debug_assert!(text.is_some(), "no template is called {name}");
        text.map_or("", String::as_str)
    }

    /// The text `key` from the file of texts `file`, as it is.
    pub fn text(&self, file: &str, key: &str) -> &str {
        let text = self.texts.get(file).and_then(|t| t.get(key));
        debug_assert!(text.is_some(), "{file} has no text called {key}");
        text.map_or("", String::as_str)
    }

    /// The text `key` from `file`, with its placeholders filled.
    pub fn fill(&self, file: &str, key: &str, pairs: &[(&str, String)]) -> String {
        narrate::fill(self.text(file, key), pairs)
    }

    /// What tool `name` is described as in `lp`.
    pub fn tool(&self, name: &str, lp: Loop) -> &str {
        let words = self.tools.get(name).and_then(|w| {
            w.in_loop(lp)
                .and_then(|s| s.description.as_deref())
                .or(w.description.as_deref())
        });
        debug_assert!(words.is_some(), "tools.toml does not describe {name}");
        words.unwrap_or_default()
    }

    /// What having tool `name` lets her do, in a few words, if it says.
    pub fn ability(&self, name: &str) -> Option<&str> {
        self.tools.get(name)?.ability.as_deref()
    }

    /// What argument `parameter` of tool `name` is described as in `lp`,
    /// if it is.
    pub fn parameter(&self, name: &str, parameter: &str, lp: Loop) -> Option<&str> {
        let words = self.tools.get(name)?;
        words
            .in_loop(lp)
            .and_then(|s| s.parameters.get(parameter))
            .or_else(|| words.parameters.get(parameter))
            .map(String::as_str)
    }

    /// Every template, text and tool, with where what is in use came from.
    pub fn listing(&self) -> Vec<Listed> {
        let mut out = Vec::new();
        for file in &self.files {
            match file.kind {
                Kind::Whole => {
                    let (origin, path) = self.provider(file.name, |_| true);
                    out.push(Listed {
                        file: file.name,
                        layer: file.layer,
                        key: None,
                        origin,
                        path,
                        differs: self.whole(file.name) != file.built_in,
                    });
                }
                Kind::Texts => {
                    let shipped = &self.shipped_texts[file.name];
                    let mut keys: Vec<&String> = shipped.keys().collect();
                    keys.sort();
                    for key in keys {
                        let (origin, path) = self.provider(file.name, |text| {
                            texts(text).is_ok_and(|t| t.contains_key(key.as_str()))
                        });
                        out.push(Listed {
                            file: file.name,
                            layer: file.layer,
                            key: Some(key.clone()),
                            origin,
                            path,
                            differs: self.texts[file.name].get(key) != shipped.get(key),
                        });
                    }
                }
                Kind::Tools => {
                    for name in self.shipped_tools.keys() {
                        let (origin, path) = self.provider(file.name, |text| {
                            tools(text).is_ok_and(|t| t.contains_key(name.as_str()))
                        });
                        out.push(Listed {
                            file: file.name,
                            layer: file.layer,
                            key: Some(name.clone()),
                            origin,
                            path,
                            differs: self.tools.get(name) != self.shipped_tools.get(name),
                        });
                    }
                }
            }
        }
        out
    }

    /// Write what is built in for `files` into `dir`, for a person to
    /// change, and note what each was copied from (see [`exported`]). A
    /// file already there is left alone unless `force`. Returns each file
    /// and whether it was written.
    pub fn export(files: &[File], dir: &Path, force: bool) -> io::Result<Vec<(PathBuf, bool)>> {
        fs::create_dir_all(dir)?;
        let mut done = Vec::with_capacity(files.len());
        let mut written = Vec::new();
        for file in files {
            let path = dir.join(file.name);
            let write = force || !path.exists();
            if write {
                fs::write(&path, file.built_in)?;
                written.push(*file);
            }
            done.push((path, write));
        }
        exported::note(dir, &written)?;
        Ok(done)
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
            // A directory that is not there is one look, not twenty.
            let there = match fs::metadata(&dir) {
                Ok(_) => Some(true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Some(false),
                Err(_) => None,
            };
            for file in self.files.clone() {
                let path = dir.join(file.name);
                let stamp = match there {
                    Some(true) => stamp(&path),
                    Some(false) => Stamp::Missing,
                    None => Stamp::Unreadable,
                };
                changed |= self.take(path, file, stamp, loud);
            }
        }
        if changed {
            self.settle();
        }
        changed
    }

    /// One file, as it is now. Returns whether what it says changed.
    fn take(&mut self, path: PathBuf, file: File, stamp: Stamp, loud: bool) -> bool {
        let name = file.name;
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
            Stamp::At(..) => match self.read(&path, file) {
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
        let mut tools_now = self.shipped_tools.clone();
        for file in self.files.clone() {
            // The texts and tools are put together one entry at a time,
            // the nearest directory last so that it wins.
            let mut layers: Vec<String> = self
                .found(file.name)
                .map(|(_, _, t)| t.to_owned())
                .collect();
            match file.kind {
                Kind::Whole => {
                    let text = layers.first().map_or(file.built_in, String::as_str);
                    self.whole.insert(file.name, text.to_owned());
                }
                Kind::Texts => {
                    let mut merged = self.shipped_texts[file.name].clone();
                    layers.reverse();
                    for text in layers {
                        for (key, value) in texts(&text).unwrap_or_default() {
                            if let Some(slot) = merged.get_mut(&key) {
                                *slot = value;
                            }
                        }
                    }
                    self.texts.insert(file.name, merged);
                }
                Kind::Tools => {
                    layers.reverse();
                    for text in layers {
                        for (name, words) in tools(&text).unwrap_or_default() {
                            if let Some(slot) = tools_now.get_mut(&name) {
                                slot.take(words);
                            }
                        }
                    }
                }
            }
        }
        self.tools = tools_now;
    }

    /// The text of a file, if it is usable as `file`. Names in it that mean
    /// nothing are warned about; they do not make it unusable.
    fn read(&self, path: &Path, file: File) -> Result<String, String> {
        let bytes = fs::read(path).map_err(|e| format!("cannot be read: {e}"))?;
        let text = String::from_utf8(bytes).map_err(|_| "is not UTF-8".to_owned())?;
        let unknown: Vec<String> = match file.kind {
            Kind::Whole => Vec::new(),
            Kind::Texts => {
                let shipped = &self.shipped_texts[file.name];
                texts(&text)?
                    .into_keys()
                    .filter(|k| !shipped.contains_key(k))
                    .collect()
            }
            Kind::Tools => {
                let mut unknown = Vec::new();
                for (tool, words) in tools(&text)? {
                    match self.shipped_tools.get(&tool) {
                        None => unknown.push(tool),
                        Some(shipped) => {
                            let known: Vec<&String> = shipped.names().collect();
                            unknown.extend(
                                words
                                    .names()
                                    .filter(|p| !known.contains(p))
                                    .map(|p| format!("{tool}.{p}")),
                            );
                        }
                    }
                }
                unknown
            }
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
}

/// The files of `files` in the `prompts/` of the ghost in `ghost`, which
/// are not read. Everything a brain is told is shared by every character a
/// deployment runs, the persona template included; what is hers alone is
/// her `ghost.toml`. Whoever starts her says where these belong instead.
pub fn misplaced(files: &[File], ghost: &Path) -> Vec<PathBuf> {
    let dir = ghost.join(DIR);
    files
        .iter()
        .map(|file| dir.join(file.name))
        .filter(|path| path.exists())
        .collect()
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

    const FILES: &[File] = &[
        File {
            name: "plan.md",
            built_in: "built in",
            kind: Kind::Whole,
            layer: Layer::Logic,
        },
        File {
            name: "texts.toml",
            built_in: "look = \"built in\"\nother = \"built in\"\n",
            kind: Kind::Texts,
            layer: Layer::Logic,
        },
        File {
            name: "tools.toml",
            built_in: "[keep]\ndescription = \"Keep it.\"\n\n[keep.parameters]\nnote = \"Why.\"\n\n\
                       [keep.conversation]\ndescription = \"Keep it here.\"\n",
            kind: Kind::Tools,
            layer: Layer::Work,
        },
    ];

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
    fn a_tool_is_said_in_each_loop_as_that_loop_says_it_or_as_the_tool_does() {
        let dir = tempfile::tempdir().unwrap();
        write_at(
            &dir.path().join("tools.toml"),
            "[keep.work.parameters]\nnote = \"For the work.\"\n",
            1,
        );
        let store = Store::load(FILES, Some(dir.path()));
        assert_eq!(store.tool("keep", Loop::Conversation), "Keep it here.");
        assert_eq!(store.tool("keep", Loop::Work), "Keep it.");
        assert_eq!(
            store.parameter("keep", "note", Loop::Conversation),
            Some("Why.")
        );
        assert_eq!(
            store.parameter("keep", "note", Loop::Work),
            Some("For the work.")
        );
    }

    #[test]
    fn the_deployments_file_wins_and_a_change_to_it_is_taken_in_without_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let texts = dir.path().join("texts.toml");
        write_at(&texts, "look = \"the deployment's\"\n", 1);
        let mut store = Store::load(FILES, Some(dir.path()));
        assert_eq!(store.text("texts.toml", "look"), "the deployment's");
        assert_eq!(store.text("texts.toml", "other"), "built in");
        assert!(!store.refresh(t0()));

        write_at(&texts, "look = \"the deployment's, again\"\n", 2);
        assert!(!store.refresh(t0() + Duration::seconds(1)));
        assert!(store.refresh(t0() + Duration::seconds(2)));
        assert_eq!(store.text("texts.toml", "look"), "the deployment's, again");
        assert_eq!(store.whole("plan.md"), "built in");
    }

    #[test]
    fn a_prompt_in_a_ghosts_own_directory_is_found_as_misplaced_and_never_read() {
        let (ghost, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write_at(&ghost.path().join(DIR).join("plan.md"), "hers", 1);
        write_at(&ghost.path().join(DIR).join("notes.txt"), "not a prompt", 1);
        assert_eq!(
            misplaced(FILES, ghost.path()),
            [ghost.path().join(DIR).join("plan.md")]
        );
        assert_eq!(Store::load(FILES, None).whole("plan.md"), "built in");
        write_at(&dir.path().join("plan.md"), "the deployment's", 1);
        assert_eq!(
            Store::load(FILES, Some(dir.path())).whole("plan.md"),
            "the deployment's"
        );
        assert!(misplaced(FILES, dir.path()).is_empty());
    }
}
