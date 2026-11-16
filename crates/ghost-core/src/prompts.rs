//! The words a brain reads, kept in files and taken in again when they
//! change. Data, not code.
//!
//! Every prompt is in one tree, `prompts/` at the top of the repository,
//! split in two. `system/` is Ghost's own and the same for every character:
//! what she is and the ground rules, how a conversation and a day go, how a
//! piece of work goes and what every tool is. `character/` is how a
//! character's own `ghost.toml` is turned into prose: the persona template
//! and the few texts it puts around her fields. What makes one character
//! herself, her name, temperament, voice, story, values and past, is her
//! `ghost.toml`, which is in her journal; nothing in the tree is about
//! anyone in particular.
//!
//! A crate that talks to a brain lists the files of the tree it reads, each
//! with its path, what is built into it (see [`prompt!`](crate::prompt))
//! and its [`Layer`], and gets a [`Store`] that looks for every one of them
//! in two places, the first that has it winning: the deployment's
//! directory of prompts (`[mind] prompts` in `ghostd.toml`), and the
//! binary. The built-ins are only the last resort, so that a lone binary
//! still runs; anything a person might want to change is a file.
//!
//! A deployment's directory mirrors the tree: `system/logic/attend.md`
//! there stands for `system/logic/attend.md` here, so that what a person
//! edits sits where they would look for it, and `ghostd prompts export`
//! writes the same tree. Before the tree, such a directory was flat, every
//! file at its top under a name of its own; a file still found there by its
//! old name is read, with a warning, once, that says where it belongs now
//! (see [`flat`]). A ghost's own directory holds no prompts: a copy there
//! would make one character work unlike the others, and swapping her
//! persona would no longer be all it takes to swap her. What [`misplaced`]
//! finds there is warned about and not read.
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
//! changes, and is merged one entry at a time. A reader's texts are one
//! table spread over several files, a file per feature, so that a person
//! looking for what she is told about a group finds a short file about
//! groups and not a grab bag; the files are only where the keys are kept,
//! and the code asks the table. So a key is in one file of its table and no
//! other: two built-in files that both have it are a mistake a test
//! catches, and two files of a deployment's that both have it are warned
//! about, the first of them winning.
//!
//! None of it is in her journal, and changing it does not change who she
//! is. A person's copy of a built-in file outlives the build it came from;
//! how a copy that has fallen behind is noticed is [`exported`].

pub mod exported;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
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

/// A file of the tree, built in: its path under `prompts/`, the text of
/// that file compiled into the crate that names it, and the rest of
/// [`File`]. Every crate's list of files is made with this, so that a path
/// is written once and cannot point somewhere the built-in did not come
/// from.
///
/// ```ignore
/// prompt!("system/logic/attend.md", Kind::Whole, Layer::Logic, "attend.md")
/// ```
#[macro_export]
macro_rules! prompt {
    ($path:literal, $kind:expr, $layer:expr, $flat:expr) => {
        $crate::prompts::File {
            path: $path,
            built_in: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../prompts/",
                $path
            )),
            kind: $kind,
            layer: $layer,
            flat: $flat,
        }
    };
}

/// How a file is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Used as it is.
    Whole,
    /// Named pieces of text, `key = "text"`, in the table named: one a
    /// reader asks for texts by, which may be spread over several files.
    Texts(&'static str),
    /// What each tool is said to be: a table per tool.
    Tools,
}

/// Which part of what a brain is told a file is. `character/` holds the
/// persona layer and nothing else; the other three are `system/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// The template that arranges her own fields from `ghost.toml` into
    /// prose, and the texts it puts around them. It holds no rules, only
    /// the arrangement, so that two characters differ in what fills it and
    /// in nothing else.
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
    /// Where it is under `prompts/`, and so in a deployment's directory:
    /// `system/logic/attend.md`.
    pub path: &'static str,
    pub built_in: &'static str,
    pub kind: Kind,
    pub layer: Layer,
    /// What it was called in a flat directory of overrides, before there
    /// was a tree. Every file of a table of texts was in the one file this
    /// names.
    pub flat: &'static str,
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
    /// The file it is in, by its path in the tree.
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

/// One name a file may be found under in a deployment's directory, and the
/// file it stands for. A file of texts found under its flat name stands for
/// its whole table.
#[derive(Debug, Clone, Copy)]
struct Source {
    name: &'static str,
    file: File,
    flat: bool,
}

/// The names to look for in a directory, each once: every file's path,
/// then every flat name, so that in one directory the tree wins.
fn sources(files: &[File]) -> Vec<Source> {
    let mut out: Vec<Source> = files
        .iter()
        .map(|&file| Source {
            name: file.path,
            file,
            flat: false,
        })
        .collect();
    for &file in files {
        if !out.iter().any(|s| s.name == file.flat) {
            out.push(Source {
                name: file.flat,
                file,
                flat: true,
            });
        }
    }
    out
}

/// The files of `files` that both have a key of the same table, as
/// `(key, first, second)` by path. There should be none.
pub fn duplicates(files: &[File]) -> Vec<(String, &'static str, &'static str)> {
    let mut homes: HashMap<(&'static str, String), &'static str> = HashMap::new();
    let mut found = Vec::new();
    for file in files {
        let Kind::Texts(table) = file.kind else {
            continue;
        };
        for key in texts(file.built_in).unwrap_or_default().into_keys() {
            match homes.get(&(table, key.clone())) {
                Some(first) => found.push((key, *first, file.path)),
                None => {
                    homes.insert((table, key), file.path);
                }
            }
        }
    }
    found.sort();
    found
}

/// The files of one reader's prompts, as they are now.
#[derive(Debug, Clone)]
pub struct Store {
    files: Vec<File>,
    sources: Vec<Source>,
    /// The texts built in, by table.
    shipped_texts: HashMap<&'static str, Texts>,
    shipped_tools: Tools,
    /// The templates in use, by path.
    whole: HashMap<&'static str, String>,
    /// The texts in use, by table.
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
    /// If a built-in file of texts or tools does not parse, or two of them
    /// have the same key of a table: that is a mistake in the binary, and
    /// every test would show it.
    pub fn shipped(files: &[File]) -> Self {
        if let Some((key, first, second)) = duplicates(files).into_iter().next() {
            panic!("the built-in {first} and {second} both have {key}");
        }
        let mut shipped_texts: HashMap<&'static str, Texts> = HashMap::new();
        let mut shipped_tools = Tools::new();
        let mut whole = HashMap::new();
        for file in files {
            match file.kind {
                Kind::Whole => {
                    whole.insert(file.path, file.built_in.to_owned());
                }
                Kind::Texts(table) => {
                    let parsed = texts(file.built_in)
                        .unwrap_or_else(|e| panic!("the built-in {} {e}", file.path));
                    shipped_texts.entry(table).or_default().extend(parsed);
                }
                Kind::Tools => {
                    let parsed = tools(file.built_in)
                        .unwrap_or_else(|e| panic!("the built-in {} {e}", file.path));
                    shipped_tools.extend(parsed);
                }
            }
        }
        Self {
            files: files.to_vec(),
            sources: sources(files),
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
        for (_, dir) in layers {
            say_flat(files, dir);
        }
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

    /// The template at `path` in the tree, as it is.
    pub fn whole(&self, path: &str) -> &str {
        let text = self.whole.get(path);
        debug_assert!(text.is_some(), "no template is at {path}");
        text.map_or("", String::as_str)
    }

    /// The text `key` from the table of texts `table`, as it is.
    pub fn text(&self, table: &str, key: &str) -> &str {
        let text = self.texts.get(table).and_then(|t| t.get(key));
        debug_assert!(text.is_some(), "the {table} texts have none called {key}");
        text.map_or("", String::as_str)
    }

    /// The text `key` from `table`, with its placeholders filled.
    pub fn fill(&self, table: &str, key: &str, pairs: &[(&str, String)]) -> String {
        narrate::fill(self.text(table, key), pairs)
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

    /// Every template, text and tool, under the file the built-in has it
    /// in, with where what is in use came from.
    pub fn listing(&self) -> Vec<Listed> {
        let mut out = Vec::new();
        for file in &self.files {
            let listed = |key: Option<String>, origin, path, differs| Listed {
                file: file.path,
                layer: file.layer,
                key,
                origin,
                path,
                differs,
            };
            match file.kind {
                Kind::Whole => {
                    let (origin, path) = self.provider(*file, |_| true);
                    let differs = self.whole(file.path) != file.built_in;
                    out.push(listed(None, origin, path, differs));
                }
                Kind::Texts(table) => {
                    let mut keys: Vec<String> = texts(file.built_in)
                        .unwrap_or_default()
                        .into_keys()
                        .collect();
                    keys.sort();
                    for key in keys {
                        let (origin, path) = self.provider(*file, |text| {
                            texts(text).is_ok_and(|t| t.contains_key(&key))
                        });
                        let differs =
                            self.texts[table].get(&key) != self.shipped_texts[table].get(&key);
                        out.push(listed(Some(key), origin, path, differs));
                    }
                }
                Kind::Tools => {
                    for name in tools(file.built_in).unwrap_or_default().into_keys() {
                        let (origin, path) = self.provider(*file, |text| {
                            tools(text).is_ok_and(|t| t.contains_key(&name))
                        });
                        let differs = self.tools.get(&name) != self.shipped_tools.get(&name);
                        out.push(listed(Some(name), origin, path, differs));
                    }
                }
            }
        }
        out
    }

    /// Write what is built in for `files` into `dir`, each at its path in
    /// the tree, for a person to change, and note what each was copied
    /// from (see [`exported`]). A file already there is left alone unless
    /// `force`. Returns each file and whether it was written.
    pub fn export(files: &[File], dir: &Path, force: bool) -> io::Result<Vec<(PathBuf, bool)>> {
        fs::create_dir_all(dir)?;
        let mut done = Vec::with_capacity(files.len());
        let mut written = Vec::new();
        for file in files {
            let path = dir.join(file.path);
            let write = force || !path.exists();
            if write {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&path, file.built_in)?;
                written.push(*file);
            }
            done.push((path, write));
        }
        exported::note(dir, &written)?;
        Ok(done)
    }

    /// The text held under `name` in `dir`, if there is a usable one.
    fn held_text(&self, dir: &Path, name: &str) -> Option<&str> {
        self.held.get(&dir.join(name))?.text.as_deref()
    }

    /// The sources in a directory that stand for what `file` holds, in the
    /// order they win in: for a template, its path then its flat name; for
    /// texts, every file of its table and then the flat names; for tools,
    /// every file of tools and then theirs.
    fn standing_for(&self, file: File) -> impl Iterator<Item = &Source> {
        self.sources.iter().filter(move |s| match file.kind {
            Kind::Whole => s.file.path == file.path,
            kind => s.file.kind == kind,
        })
    }

    /// The first directory, and the file in it, that says something `has`
    /// looks for among what stands for `file`.
    fn provider(&self, file: File, has: impl Fn(&str) -> bool) -> (Origin, Option<PathBuf>) {
        for (origin, dir) in &self.layers {
            for source in self.standing_for(file) {
                if self.held_text(dir, source.name).is_some_and(&has) {
                    return (*origin, Some(dir.join(source.name)));
                }
            }
        }
        (Origin::BuiltIn, None)
    }

    /// Look at every file, read again the ones that changed, and if any
    /// did, work out again what is in use. `loud` says each change.
    fn scan(&mut self, loud: bool) -> bool {
        let mut changed = false;
        for (_, dir) in self.layers.clone() {
            // A directory that is not there is one look, not forty.
            let there = match fs::metadata(&dir) {
                Ok(_) => Some(true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Some(false),
                Err(_) => None,
            };
            for source in self.sources.clone() {
                let path = dir.join(source.name);
                let stamp = match there {
                    Some(true) => stamp(&path),
                    Some(false) => Stamp::Missing,
                    None => Stamp::Unreadable,
                };
                changed |= self.take(path, source, stamp, loud);
            }
        }
        if changed {
            self.settle();
        }
        changed
    }

    /// One file, as it is now. Returns whether what it says changed.
    fn take(&mut self, path: PathBuf, source: Source, stamp: Stamp, loud: bool) -> bool {
        let name = source.name;
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
            Stamp::At(..) => match self.read(&path, source) {
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
        let mut texts_now = self.shipped_texts.clone();
        let mut settled: Vec<Kind> = Vec::new();
        for file in self.files.clone() {
            // A table of texts, and the tools, are settled once for all
            // their files.
            if file.kind != Kind::Whole {
                if settled.contains(&file.kind) {
                    continue;
                }
                settled.push(file.kind);
            }
            // What each directory holds for it, the farthest directory
            // first, and in each what wins first.
            let found: Vec<(PathBuf, Vec<(Source, String)>)> = self
                .layers
                .iter()
                .rev()
                .map(|(_, dir)| {
                    let here = self
                        .standing_for(file)
                        .filter_map(|s| Some((*s, self.held_text(dir, s.name)?.to_owned())))
                        .collect();
                    (dir.clone(), here)
                })
                .collect();
            match file.kind {
                Kind::Whole => {
                    let text = found
                        .iter()
                        .rev()
                        .find_map(|(_, here)| here.first())
                        .map_or(file.built_in, |(_, text)| text.as_str());
                    self.whole.insert(file.path, text.to_owned());
                }
                Kind::Texts(table) => {
                    let merged = texts_now.entry(table).or_default();
                    for (dir, here) in found {
                        // A key is taken from the first file of this
                        // directory that has it. The tree comes before the
                        // flat names, so a flat file quietly gives way; two
                        // files of the tree with the same key are a mistake
                        // a person should hear about.
                        let mut from: HashMap<String, &str> = HashMap::new();
                        for (source, text) in here {
                            for (key, value) in texts(&text).unwrap_or_default() {
                                let Some(slot) = merged.get_mut(&key) else {
                                    continue;
                                };
                                if let Some(first) = from.get(&key) {
                                    if !source.flat {
                                        warn!(
                                            "{} and {} both have {key}, which belongs in one file of the {table} texts: the one in {first} is used",
                                            dir.join(first).display(),
                                            dir.join(source.name).display(),
                                        );
                                    }
                                    continue;
                                }
                                from.insert(key, source.name);
                                *slot = value;
                            }
                        }
                    }
                }
                Kind::Tools => {
                    // Laid on one after another, so what wins goes last.
                    for (_, here) in found {
                        for (_, text) in here.into_iter().rev() {
                            for (name, words) in tools(&text).unwrap_or_default() {
                                if let Some(slot) = tools_now.get_mut(&name) {
                                    slot.take(words);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.tools = tools_now;
        self.texts = texts_now;
    }

    /// The text of a file, if it is usable as `source`. Names in it that
    /// mean nothing are warned about; they do not make it unusable.
    fn read(&self, path: &Path, source: Source) -> Result<String, String> {
        let bytes = fs::read(path).map_err(|e| format!("cannot be read: {e}"))?;
        let text = String::from_utf8(bytes).map_err(|_| "is not UTF-8".to_owned())?;
        let unknown: Vec<String> = match source.file.kind {
            Kind::Whole => Vec::new(),
            Kind::Texts(table) => {
                let shipped = &self.shipped_texts[table];
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

/// What to say about the files in `dir` that are laid out the old, flat
/// way, if there are any: each, and where in the tree it belongs now. A
/// flat file of texts is said key by key, since its keys now live in a
/// file per feature.
pub fn flat(files: &[File], dir: &Path) -> Option<String> {
    let mut moves = Vec::new();
    for source in sources(files).into_iter().filter(|s| s.flat) {
        let path = dir.join(source.name);
        if !path.is_file() {
            continue;
        }
        let to = match source.file.kind {
            Kind::Texts(table) => {
                let of_table: Vec<&File> = files
                    .iter()
                    .filter(|f| f.kind == Kind::Texts(table) && f.flat == source.name)
                    .collect();
                if let [only] = of_table.as_slice() {
                    only.path.to_owned()
                } else {
                    // Its keys, by the file each now belongs in.
                    let copy = fs::read_to_string(&path)
                        .ok()
                        .and_then(|t| texts(&t).ok())
                        .unwrap_or_default();
                    let said: Vec<String> = of_table
                        .iter()
                        .filter_map(|home| {
                            let built_in = texts(home.built_in).unwrap_or_default();
                            let mut keys: Vec<&str> = copy
                                .keys()
                                .filter(|k| built_in.contains_key(*k))
                                .map(String::as_str)
                                .collect();
                            keys.sort_unstable();
                            (!keys.is_empty())
                                .then(|| format!("{} to {}", keys.join(", "), home.path))
                        })
                        .collect();
                    if said.is_empty() {
                        "a file per feature".to_owned()
                    } else {
                        format!("a file per feature: {}", said.join("; "))
                    }
                }
            }
            _ => source.file.path.to_owned(),
        };
        moves.push(format!("{} goes to {to}", source.name));
    }
    if moves.is_empty() {
        return None;
    }
    Some(format!(
        "{} is laid out flat, the way a directory of prompts was before it mirrored prompts/. \
         Its files are still read, but each now belongs at its place in the tree, which is looked at first: {}",
        dir.display(),
        moves.join("; ")
    ))
}

/// Warn about `dir` being laid out flat, once for the life of the program
/// however many stores read it.
pub fn say_flat(files: &[File], dir: &Path) {
    static SAID: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
    let Some(said) = flat(files, dir) else {
        return;
    };
    if SAID
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(dir.to_owned())
    {
        warn!("{said}");
    }
}

/// The files of `files` in the `prompts/` of the ghost in `ghost`, by
/// their path in the tree or their old flat name, which are not read.
/// Everything a brain is told is shared by every character a deployment
/// runs, the persona template included; what is hers alone is her
/// `ghost.toml`. Whoever starts her says where these belong instead.
pub fn misplaced(files: &[File], ghost: &Path) -> Vec<PathBuf> {
    let dir = ghost.join(DIR);
    sources(files)
        .into_iter()
        .map(|s| dir.join(s.name))
        .filter(|path| path.is_file())
        .collect()
}

/// The files of `files` that `names` pick: each name is a path in the
/// tree, or a directory of it such as `system/logic`. Every name has to
/// pick something; the one that does not is returned as the error.
pub fn pick(files: &[File], names: &[String]) -> Result<Vec<File>, String> {
    let picks = |name: &str, file: &File| {
        let name = name.trim_end_matches('/');
        file.path == name
            || file
                .path
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    if let Some(unknown) = names
        .iter()
        .find(|name| !files.iter().any(|f| picks(name, f)))
    {
        return Err(unknown.clone());
    }
    Ok(files
        .iter()
        .filter(|f| names.is_empty() || names.iter().any(|n| picks(n, f)))
        .copied()
        .collect())
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
            path: "system/logic/plan.md",
            built_in: "built in",
            kind: Kind::Whole,
            layer: Layer::Logic,
            flat: "plan.md",
        },
        File {
            path: "system/logic/texts/look.toml",
            built_in: "look = \"built in\"\n",
            kind: Kind::Texts("mind"),
            layer: Layer::Logic,
            flat: "texts.toml",
        },
        File {
            path: "system/logic/texts/day.toml",
            built_in: "other = \"built in\"\n",
            kind: Kind::Texts("mind"),
            layer: Layer::Logic,
            flat: "texts.toml",
        },
        File {
            path: "system/tools.toml",
            built_in: "[keep]\ndescription = \"Keep it.\"\n\n[keep.parameters]\nnote = \"Why.\"\n\n\
                       [keep.conversation]\ndescription = \"Keep it here.\"\n",
            kind: Kind::Tools,
            layer: Layer::Work,
            flat: "tools.toml",
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
            &dir.path().join("system/tools.toml"),
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
        let texts = dir.path().join("system/logic/texts/look.toml");
        write_at(&texts, "look = \"the deployment's\"\n", 1);
        let mut store = Store::load(FILES, Some(dir.path()));
        assert_eq!(store.text("mind", "look"), "the deployment's");
        assert_eq!(store.text("mind", "other"), "built in");
        assert!(!store.refresh(t0()));

        write_at(&texts, "look = \"the deployment's, again\"\n", 2);
        assert!(!store.refresh(t0() + Duration::seconds(1)));
        assert!(store.refresh(t0() + Duration::seconds(2)));
        assert_eq!(store.text("mind", "look"), "the deployment's, again");
        assert_eq!(store.whole("system/logic/plan.md"), "built in");
    }

    #[test]
    fn the_files_of_a_table_of_texts_are_read_as_one_table_wherever_a_key_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        // `other` is built into day.toml; a deployment may keep its own in
        // any file of the table.
        write_at(
            &dir.path().join("system/logic/texts/look.toml"),
            "other = \"kept with the look\"\n",
            1,
        );
        let store = Store::load(FILES, Some(dir.path()));
        assert_eq!(store.text("mind", "look"), "built in");
        assert_eq!(store.text("mind", "other"), "kept with the look");
        let listed = store.listing();
        let other = listed
            .iter()
            .find(|l| l.key.as_deref() == Some("other"))
            .unwrap();
        assert_eq!(other.file, "system/logic/texts/day.toml");
        assert_eq!(
            other.path.as_deref(),
            Some(dir.path().join("system/logic/texts/look.toml").as_path())
        );
        assert!(other.differs);
    }

    #[test]
    fn a_key_in_two_files_of_a_table_is_found_and_the_first_of_a_deployments_wins() {
        let twice = [
            FILES[1],
            File {
                built_in: "look = \"again\"\nother = \"built in\"\n",
                ..FILES[2]
            },
        ];
        assert_eq!(
            duplicates(&twice),
            [(
                "look".to_owned(),
                "system/logic/texts/look.toml",
                "system/logic/texts/day.toml"
            )]
        );
        assert!(duplicates(FILES).is_empty());

        let dir = tempfile::tempdir().unwrap();
        write_at(
            &dir.path().join("system/logic/texts/look.toml"),
            "look = \"first\"\n",
            1,
        );
        write_at(
            &dir.path().join("system/logic/texts/day.toml"),
            "look = \"second\"\n",
            1,
        );
        assert_eq!(
            Store::load(FILES, Some(dir.path())).text("mind", "look"),
            "first"
        );
    }

    #[test]
    fn a_flat_directory_is_still_read_and_said_to_belong_in_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        write_at(&dir.path().join("plan.md"), "flat plan", 1);
        write_at(
            &dir.path().join("texts.toml"),
            "look = \"flat look\"\nother = \"flat other\"\n",
            1,
        );
        write_at(
            &dir.path().join("tools.toml"),
            "[keep]\ndescription = \"Flat.\"\n",
            1,
        );
        // The tree wins over a flat file in the same directory.
        write_at(
            &dir.path().join("system/logic/texts/day.toml"),
            "other = \"tree other\"\n",
            1,
        );
        let store = Store::load(FILES, Some(dir.path()));
        assert_eq!(store.whole("system/logic/plan.md"), "flat plan");
        assert_eq!(store.text("mind", "look"), "flat look");
        assert_eq!(store.text("mind", "other"), "tree other");
        assert_eq!(store.tool("keep", Loop::Work), "Flat.");

        let said = flat(FILES, dir.path()).unwrap();
        assert!(
            said.contains("plan.md goes to system/logic/plan.md"),
            "{said}"
        );
        assert!(
            said.contains("tools.toml goes to system/tools.toml"),
            "{said}"
        );
        assert!(
            said.contains(
                "texts.toml goes to a file per feature: look to system/logic/texts/look.toml; \
                 other to system/logic/texts/day.toml"
            ),
            "{said}"
        );
        // A directory laid out as the tree has nothing to say.
        fs::remove_file(dir.path().join("plan.md")).unwrap();
        fs::remove_file(dir.path().join("texts.toml")).unwrap();
        fs::remove_file(dir.path().join("tools.toml")).unwrap();
        assert_eq!(flat(FILES, dir.path()), None);
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
        assert_eq!(
            Store::load(FILES, None).whole("system/logic/plan.md"),
            "built in"
        );
        write_at(
            &dir.path().join("system/logic/plan.md"),
            "the deployment's",
            1,
        );
        assert_eq!(
            Store::load(FILES, Some(dir.path())).whole("system/logic/plan.md"),
            "the deployment's"
        );
        assert!(misplaced(FILES, dir.path()).is_empty());
    }

    #[test]
    fn files_are_picked_by_their_path_or_a_directory_of_the_tree() {
        let paths = |names: &[&str]| {
            let names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
            pick(FILES, &names).map(|files| files.iter().map(|f| f.path).collect::<Vec<_>>())
        };
        assert_eq!(paths(&[]).unwrap().len(), FILES.len());
        assert_eq!(
            paths(&["system/logic/texts/"]).unwrap(),
            [
                "system/logic/texts/look.toml",
                "system/logic/texts/day.toml"
            ]
        );
        assert_eq!(
            paths(&["system/tools.toml"]).unwrap(),
            ["system/tools.toml"]
        );
        // Part of a name is not a directory.
        assert_eq!(paths(&["system/lo"]), Err("system/lo".to_owned()));
        assert_eq!(paths(&["plan.md"]), Err("plan.md".to_owned()));
    }
}
