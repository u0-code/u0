//! What `ghostd prompts export` copied, and what has become of the copies.
//!
//! A copy of a built-in prompt does not change when the built-in does.
//! After an upgrade it goes on hiding newer wording and new placeholders,
//! and nothing says so: new tools once went without descriptions this way
//! until the files were exported again with `--force`, which wiped the
//! owner's own edits along with the stale text.
//!
//! So an export writes, beside the copies, a record of what each was copied
//! from: `.exported.toml`, holding a hash of each built-in file and, for a
//! file of texts or tools, of each entry in it. At any later start that is
//! enough to tell a copy whose built-in has moved on (warned about, with the
//! placeholders it lacks), and, among those, a copy nobody edited (which
//! [`update`] brings up to date without asking) from one somebody did
//! (which is left alone until its owner says it has been looked at). The
//! record sits beside the files and not in them because a template is sent
//! to a model whole, and a header in it would be read as part of the
//! prompt; beside them, it survives any edit to them.
//!
//! In a file of texts or tools only entries count. One the copy leaves out
//! is taken from the built-in already, so a copy is stale only in the
//! entries it carries whose built-in text has changed. A text is compared
//! with its built-in in whichever file of its table that is.
//!
//! A copy made before there was a record cannot be dated. It is reported
//! only for placeholders it lacks, and [`update`] records it once it is the
//! same as the built-in.
//!
//! The record names each file by its path in the tree. A record written
//! when the copies were flat names them by their old names, and is still
//! read that way, so that copies moved into the tree keep their dates. Only
//! copies at their place in the tree are looked at here: a flat directory
//! is warned about where it is read, and is moved before it is kept up.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{File, Kind};

/// The record, in the directory the copies are in.
pub const RECORD: &str = ".exported.toml";

const HEADER: &str = "\
# What `ghostd prompts export` copied each file here from: a hash of the
# built-in file, and of each entry of a file of texts or tools. ghostd reads
# it to tell a copy whose built-in has changed since, and a copy nobody
# edited from one somebody did. Leave it be.

";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    files: BTreeMap<String, Copied>,
}

impl Record {
    /// What `file` was copied from: under its path, or, in a record made
    /// before the tree, its flat name.
    fn of(&self, file: &File) -> Option<&Copied> {
        self.files
            .get(file.path)
            .or_else(|| self.files.get(file.flat))
    }
}

/// What one file was copied from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Copied {
    built_in: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    entries: BTreeMap<String, String>,
}

impl Copied {
    fn of(file: &File) -> Self {
        Self {
            built_in: hash(file.built_in),
            entries: match file.kind {
                Kind::Whole => BTreeMap::new(),
                Kind::Texts(_) | Kind::Tools => entries(file.built_in)
                    .into_iter()
                    .map(|(key, text)| (key, hash(&text)))
                    .collect(),
            },
        }
    }
}

/// One copy, or one entry of one, that may hide something newer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stale {
    pub path: PathBuf,
    pub file: &'static str,
    /// The text or tool, in a file of texts or tools.
    pub key: Option<String>,
    /// The built-in has changed since this was copied. `false` when that
    /// cannot be known, for a copy with no record: it is then reported only
    /// for what it lacks.
    pub changed: bool,
    /// Placeholders the built-in fills in that the copy does not have.
    pub missing: Vec<String>,
}

/// The copies in `dir` of `files`, and the entries of them, that may hide
/// something newer in the built-in.
pub fn stale(files: &[File], dir: &Path) -> Vec<Stale> {
    let record = read(dir);
    let mut found = Vec::new();
    for file in files {
        let path = dir.join(file.path);
        let Ok(copy) = fs::read_to_string(&path) else {
            continue;
        };
        if copy == file.built_in {
            continue;
        }
        let then = record.of(file);
        // One piece: the whole file, or one entry of it. Reported if its
        // built-in moved on since it was copied, or, with nothing to date
        // it by, if it lacks a placeholder the built-in fills in.
        let mut weigh = |key: Option<&String>, was: Option<&String>, now: &str, mine: &str| {
            let changed = was.is_some_and(|was| *was != hash(now));
            let missing: Vec<String> = placeholders(now)
                .difference(&placeholders(mine))
                .cloned()
                .collect();
            if changed || (was.is_none() && !missing.is_empty()) {
                found.push(Stale {
                    path: path.clone(),
                    file: file.path,
                    key: key.cloned(),
                    changed,
                    missing,
                });
            }
        };
        match file.kind {
            Kind::Whole => weigh(None, then.map(|c| &c.built_in), file.built_in, &copy),
            Kind::Texts(_) | Kind::Tools => {
                let now = built_in_entries(files, file.kind);
                for (key, mine) in entries(&copy) {
                    // A name the built-in does not have is warned about
                    // where the file is read.
                    let Some(now) = now.get(&key) else { continue };
                    if *now != mine {
                        weigh(
                            Some(&key),
                            then.and_then(|c| c.entries.get(&key)),
                            now,
                            &mine,
                        );
                    }
                }
            }
        }
    }
    found
}

/// What [`update`] did with one copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Updated {
    /// It was an unedited copy of an older built-in, and is now the
    /// current one.
    Replaced,
    /// It already was the current built-in.
    Current,
    /// Its owner said it has been brought up to date by hand: it is left
    /// as it is, and recorded as a copy of the current built-in.
    Marked,
    /// It was edited, or there is no record of what it was copied from:
    /// left as it is.
    LeftAlone,
}

/// Bring each copy in `dir` that nobody edited up to its current built-in,
/// leave the rest alone, and record copies of the current built-ins as
/// such. A file whose path is in `looked_at` is one its owner has brought
/// up to date by hand; it is recorded and left as it is. A file with no
/// copy in `dir` is passed over.
pub fn update(
    files: &[File],
    dir: &Path,
    looked_at: &[&str],
) -> io::Result<Vec<(PathBuf, Updated)>> {
    let mut record = read(dir);
    let mut done = Vec::new();
    for file in files {
        let path = dir.join(file.path);
        let copy = match fs::read_to_string(&path) {
            Ok(copy) => copy,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let unedited = record.of(file).is_some_and(|c| c.built_in == hash(&copy));
        let what = if copy == file.built_in {
            Updated::Current
        } else if looked_at.contains(&file.path) {
            Updated::Marked
        } else if unedited {
            fs::write(&path, file.built_in)?;
            Updated::Replaced
        } else {
            Updated::LeftAlone
        };
        if what != Updated::LeftAlone {
            record.files.insert(file.path.to_owned(), Copied::of(file));
        }
        done.push((path, what));
    }
    if done.iter().any(|(_, what)| *what != Updated::LeftAlone) {
        write(dir, &record)?;
    }
    Ok(done)
}

/// Note that each of `files` was just copied into `dir` as it is built in.
pub(super) fn note(dir: &Path, files: &[File]) -> io::Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut record = read(dir);
    for file in files {
        record.files.insert(file.path.to_owned(), Copied::of(file));
    }
    write(dir, &record)
}

fn read(dir: &Path) -> Record {
    fs::read_to_string(dir.join(RECORD))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn write(dir: &Path, record: &Record) -> io::Result<()> {
    let text = toml::to_string(record).map_err(io::Error::other)?;
    fs::write(dir.join(RECORD), format!("{HEADER}{text}"))
}

/// Short, because it only has to tell one version of a file from another.
fn hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..16].to_owned()
}

/// Each entry of a file of texts or tools, written out the same way
/// whatever the layout of the file it came from.
fn entries(text: &str) -> BTreeMap<String, String> {
    toml::from_str::<toml::Table>(text)
        .map(|table| {
            table
                .into_iter()
                .map(|(key, value)| (key, value.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Each entry built into the files of `kind`: a table of texts, which may
/// be spread over several files, or the tools.
fn built_in_entries(files: &[File], kind: Kind) -> BTreeMap<String, String> {
    files
        .iter()
        .filter(|f| f.kind == kind)
        .flat_map(|f| entries(f.built_in))
        .collect()
}

/// The names in `{{name}}`, which is how a prompt is handed what it is
/// about.
fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let name = after[..close].trim();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            found.insert(name.to_owned());
        }
        rest = &after[close + 2..];
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::{Layer, Store};

    const PLAN: &str = "system/logic/plan.md";
    const TEXTS: &str = "system/logic/texts/look.toml";

    const THEN: &[File] = &[
        File {
            path: PLAN,
            built_in: "Plan {{day}}.",
            kind: Kind::Whole,
            layer: Layer::Logic,
            flat: "plan.md",
        },
        File {
            path: TEXTS,
            built_in: "look = \"Look at {{place}}.\"\nother = \"Other.\"\n",
            kind: Kind::Texts("mind"),
            layer: Layer::Logic,
            flat: "texts.toml",
        },
    ];

    /// The same files, as a later build has them.
    const NOW: &[File] = &[
        File {
            path: PLAN,
            built_in: "Plan {{day}} around {{weather}}.",
            kind: Kind::Whole,
            layer: Layer::Logic,
            flat: "plan.md",
        },
        File {
            path: TEXTS,
            built_in: "look = \"Look at {{place}} with {{who}}.\"\nother = \"Other.\"\nnew = \"New.\"\n",
            kind: Kind::Texts("mind"),
            layer: Layer::Logic,
            flat: "texts.toml",
        },
    ];

    #[test]
    fn a_copy_whose_built_in_changed_since_it_was_exported_is_stale_with_what_it_lacks() {
        let dir = tempfile::tempdir().unwrap();
        Store::export(THEN, dir.path(), false).unwrap();
        assert!(stale(THEN, dir.path()).is_empty());

        let found = stale(NOW, dir.path());
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].file, PLAN);
        assert!(found[0].changed);
        assert_eq!(found[0].missing, ["weather"]);
        // Only the entry whose built-in changed: `other` did not, and
        // `new` is not in the copy, so it comes from the built-in already.
        assert_eq!(found[1].key.as_deref(), Some("look"));
        assert!(found[1].changed);
        assert_eq!(found[1].missing, ["who"]);

        // A copy cut down to the entries its owner changed is stale only in
        // those.
        fs::write(dir.path().join(TEXTS), "other = \"Mine.\"\n").unwrap();
        assert!(stale(NOW, dir.path()).iter().all(|s| s.file == PLAN));
    }

    #[test]
    fn update_follows_an_unedited_copy_and_leaves_an_edited_one_alone() {
        let dir = tempfile::tempdir().unwrap();
        Store::export(THEN, dir.path(), false).unwrap();
        let edited = "look = \"Look hard at {{place}}.\"\n";
        fs::write(dir.path().join(TEXTS), edited).unwrap();

        let done = update(NOW, dir.path(), &[]).unwrap();
        assert_eq!(
            done.iter().map(|(_, what)| *what).collect::<Vec<_>>(),
            [Updated::Replaced, Updated::LeftAlone]
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(PLAN)).unwrap(),
            NOW[0].built_in
        );
        assert_eq!(fs::read_to_string(dir.path().join(TEXTS)).unwrap(), edited);
        // The edited copy is still said to be behind, until its owner says
        // it has been looked at.
        let left = stale(NOW, dir.path());
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].key.as_deref(), Some("look"));

        let done = update(NOW, dir.path(), &[TEXTS]).unwrap();
        assert_eq!(done[1].1, Updated::Marked);
        assert_eq!(fs::read_to_string(dir.path().join(TEXTS)).unwrap(), edited);
        assert!(stale(NOW, dir.path()).is_empty());
    }

    #[test]
    fn a_copy_with_no_record_is_reported_only_for_what_it_lacks() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("system/logic/texts")).unwrap();
        fs::write(dir.path().join(PLAN), "Plan the day.").unwrap();
        fs::write(
            dir.path().join(TEXTS),
            "look = \"Look at {{place}}, {{who}}.\"\n",
        )
        .unwrap();
        let found = stale(NOW, dir.path());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(!found[0].changed);
        assert_eq!(found[0].missing, ["day", "weather"]);
        // Nothing is replaced that cannot be dated.
        let done = update(NOW, dir.path(), &[]).unwrap();
        assert!(done.iter().all(|(_, what)| *what == Updated::LeftAlone));
    }

    #[test]
    fn a_record_made_when_the_copies_were_flat_still_dates_them_once_moved_into_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        // As an export before the tree wrote it: the copies at the top, and
        // the record under their flat names.
        fs::write(dir.path().join("plan.md"), THEN[0].built_in).unwrap();
        fs::write(
            dir.path().join(RECORD),
            format!(
                "[files.\"plan.md\"]\nbuilt_in = \"{}\"\n\n[files.\"texts.toml\"]\nbuilt_in = \"0\"\n\n\
                 [files.\"texts.toml\".entries]\nlook = \"{}\"\n",
                hash(THEN[0].built_in),
                hash("\"Look at {{place}}.\""),
            ),
        )
        .unwrap();
        // Moved into the tree by hand.
        fs::create_dir_all(dir.path().join("system/logic/texts")).unwrap();
        fs::rename(dir.path().join("plan.md"), dir.path().join(PLAN)).unwrap();
        fs::write(dir.path().join(TEXTS), "look = \"Look at {{place}}.\"\n").unwrap();

        let found = stale(NOW, dir.path());
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|s| s.changed), "{found:?}");
        // The template nobody edited is brought up to date, as it would have
        // been where it was.
        let done = update(NOW, dir.path(), &[]).unwrap();
        assert_eq!(done[0].1, Updated::Replaced);
        assert_eq!(
            fs::read_to_string(dir.path().join(PLAN)).unwrap(),
            NOW[0].built_in
        );
    }
}
