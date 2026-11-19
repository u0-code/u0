//! What the work leaves behind as it goes: the transcript it can be picked
//! up from, the history nothing is taken out of, the step log, and the
//! pictures they name, kept under `media/` beside the database.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use ghost_brain::{Item, Picture};
use ghost_core::media;
use rusqlite::{TransactionBehavior, params};
use serde_json::{Value, json};

use crate::{
    prompts::{Store, TEXTS},
    steps::Line,
};

use super::{
    State, Undertaking,
    db::{append, load, other, store},
};

/// The line that stands for a picture wherever its bytes are not kept.
pub fn placeholder(file: &str) -> String {
    format!("[image: {file}]")
}

/// An item as the history keeps it: a result's pictures become the lines
/// that name their files, where those are not already in its output.
fn without_pictures(item: &Item) -> std::borrow::Cow<'_, Item> {
    let Item::Result {
        id,
        name,
        output,
        failed,
        pictures,
    } = item
    else {
        return std::borrow::Cow::Borrowed(item);
    };
    if pictures.is_empty() {
        return std::borrow::Cow::Borrowed(item);
    }
    let mut output = output.clone();
    for p in pictures {
        let line = placeholder(&media::name(&p.bytes, &p.mime));
        if !output.contains(&line) {
            output.push('\n');
            output.push_str(&line);
        }
    }
    std::borrow::Cow::Owned(Item::result(id, name, output, *failed))
}

/// An item from a saved transcript. A picture is a reference to a file
/// under `root/media/`, which is read back. A reference whose file is gone
/// is dropped, and the result says so in `words`.
fn from_disk(words: &Store, mut saved: Value, root: &Path) -> Option<Item> {
    let written = saved
        .as_object_mut()
        .and_then(|item| item.remove("pictures"));
    let mut item: Item = serde_json::from_value(saved).ok()?;
    let (
        Some(Value::Array(written)),
        Item::Result {
            output, pictures, ..
        },
    ) = (written, &mut item)
    else {
        return Some(item);
    };
    for picture in written {
        let file = picture["file"].as_str()?;
        // Only a bare name is looked for, and only under `media/`.
        let bytes = Path::new(file)
            .file_name()
            .filter(|bare| *bare == file)
            .and_then(|_| fs::read(root.join(media::MEDIA_DIR).join(file)).ok());
        match bytes {
            Some(bytes) => pictures.push(Picture {
                mime: picture["mime"].as_str().unwrap_or_default().to_owned(),
                bytes,
            }),
            None => output.push_str(&words.fill(TEXTS, "image_gone", &[("file", file.to_owned())])),
        }
    }
    Some(item)
}

impl Undertaking {
    /// Where pictures are kept: beside the database.
    fn media_root(&self) -> &Path {
        self.db.parent().unwrap_or_else(|| Path::new("."))
    }

    /// Where the file `name` is kept under `media/`, if it is there. Only
    /// a bare name is looked for: a path is never followed.
    pub fn media(&self, name: &str) -> Option<PathBuf> {
        let bare = !name.starts_with('.')
            && Path::new(name)
                .file_name()
                .is_some_and(|n| n.to_str() == Some(name));
        let path = self.media_root().join(media::MEDIA_DIR).join(name);
        (bare && path.is_file()).then_some(path)
    }

    /// The sitting in progress, if one was left unfinished. Pictures saved
    /// as references are read back from `media/`; one that is gone is said
    /// to be, in `words`.
    pub fn transcript(&self, words: &Store) -> Option<Vec<Item>> {
        let read = self.read_transcript(words);
        let items = match read {
            Ok(Some(items)) => {
                // The rows this handle saves on from: what it just read.
                *self.saved() = Some(items.clone());
                items
            }
            Ok(None) => {
                *self.saved() = Some(Vec::new());
                return None;
            }
            Err(e) => {
                tracing::warn!("could not read the transcript of {}: {e}", self.id);
                return None;
            }
        };
        Some(items)
    }

    /// The transcript on disk; an item that cannot be read leaves none.
    fn read_transcript(&self, words: &Store) -> rusqlite::Result<Option<Vec<Item>>> {
        let conn = self.conn();
        let rows: Vec<(Option<String>, Option<String>)> = conn
            .prepare(
                "SELECT t.item, h.item FROM transcript_items t
                 LEFT JOIN history h ON h.id = t.id AND h.n = t.history
                 WHERE t.id = ?1 ORDER BY t.n",
            )?
            .query_map([&self.id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let read = |text: &str| {
            serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|item| from_disk(words, item, self.media_root()))
        };
        if rows.is_empty() {
            return Ok(None);
        }
        Ok(rows
            .iter()
            .map(|(item, kept)| read(item.as_deref().or(kept.as_deref())?))
            .collect::<Option<Vec<Item>>>())
    }

    fn saved(&self) -> std::sync::MutexGuard<'_, Option<Vec<Item>>> {
        self.saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Save the transcript `items`, of which the first `remembered` are
    /// in the history already: the rest join it, and the transcript is
    /// written where it differs from what is on disk. Done whenever a
    /// sitting stops, so that it can be picked up from there. An image
    /// that cannot be kept is said to be gone, in `words`.
    pub fn save(&self, words: &Store, items: &[Item], remembered: usize) -> io::Result<()> {
        self.write_step(words, None::<(fn(&mut State), &Line)>, items, remembered)
            .map(|_| ())
    }

    /// One step, written in one transaction: `change` made to the state,
    /// `line` added to the step log, and the transcript saved as
    /// [`Undertaking::save`] saves it. A process stopped anywhere can be
    /// picked up from the step before or the step after, never from half
    /// of one.
    pub fn step<T>(
        &self,
        words: &Store,
        change: impl FnOnce(&mut State) -> T,
        line: &Line,
        items: &[Item],
        remembered: usize,
    ) -> io::Result<T> {
        self.write_step(words, Some((change, line)), items, remembered)?
            .ok_or_else(|| io::Error::other("the step's change was not made"))
    }

    /// What [`Undertaking::save`] and [`Undertaking::step`] write.
    ///
    /// The transcript is kept an item a row. Only the rows that differ
    /// from what this handle knows to be on disk are written: after a
    /// step, the items it added. An item that is new to the history and
    /// holds no picture is the history's row word for word, and its row
    /// here only says which one. Pictures are kept under `media/` before
    /// the transaction, and named in the row.
    fn write_step<T, F: FnOnce(&mut State) -> T>(
        &self,
        words: &Store,
        step: Option<(F, &Line)>,
        items: &[Item],
        remembered: usize,
    ) -> io::Result<Option<T>> {
        let remembered = remembered.min(items.len());
        let history: Vec<String> = items[remembered..]
            .iter()
            .map(|item| serde_json::to_string(&without_pictures(item)))
            .collect::<serde_json::Result<_>>()
            .map_err(other)?;
        let known = self.saved().clone();
        // Each row to write: its place, and its item unless it is the
        // history's.
        let mut rows: Vec<(usize, Option<String>)> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if known.as_ref().and_then(|k| k.get(i)) == Some(item) {
                continue;
            }
            let own = matches!(item, Item::Result { pictures, .. } if !pictures.is_empty())
                || i < remembered;
            let text = if own {
                Some(
                    serde_json::to_string(&self.to_disk(words, item).map_err(other)?)
                        .map_err(other)?,
                )
            } else {
                None
            };
            rows.push((i, text));
        }
        let mut conn = self.conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(other)?;
        let out = match step {
            Some((change, line)) => {
                let mut state = load(&tx, &self.id)
                    .map_err(other)?
                    .unwrap_or_else(|| self.cached().clone());
                let out = change(&mut state);
                store(&tx, &self.id, &state).map_err(other)?;
                append(
                    &tx,
                    "log",
                    "line",
                    &self.id,
                    [serde_json::to_string(line).map_err(other)?],
                )
                .map_err(other)?;
                Some((out, state))
            }
            None => None,
        };
        let last: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(n), 0) FROM history WHERE id = ?1",
                [&self.id],
                |row| row.get(0),
            )
            .map_err(other)?;
        append(&tx, "history", "item", &self.id, history).map_err(other)?;
        // Work that has ended keeps no transcript (see `db::store`): what
        // it held is in the history, and nothing picks it up again.
        let ended = match &out {
            Some((_, state)) => state.finish.is_some(),
            None => load(&tx, &self.id)
                .map_err(other)?
                .is_some_and(|state| state.finish.is_some()),
        };
        // Rows past its end go; all of them where what is on disk is not
        // known, or the work has ended.
        let keep = if known.is_some() && !ended {
            items.len()
        } else {
            0
        };
        tx.execute(
            "DELETE FROM transcript_items WHERE id = ?1 AND n > ?2",
            params![self.id, i64::try_from(keep).map_err(other)?],
        )
        .map_err(other)?;
        if !ended {
            let mut put = tx
                .prepare(
                    "INSERT OR REPLACE INTO transcript_items (id, n, item, history)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(other)?;
            for (i, text) in rows {
                let kept = match text {
                    Some(_) => None,
                    None => Some(last + 1 + i64::try_from(i - remembered).map_err(other)?),
                };
                let n = i64::try_from(i + 1).map_err(other)?;
                put.execute(params![self.id, n, text, kept])
                    .map_err(other)?;
            }
        }
        tx.commit().map_err(other)?;
        *self.saved() = Some(if ended { Vec::new() } else { items.to_vec() });
        Ok(out.map(|(out, state)| {
            *self.cached() = state;
            out
        }))
    }

    /// Keep images a tool returned beside this database, under `media/`,
    /// the way a ghost keeps what she is shown. Returns the name each one
    /// is kept under, which names the same bytes whether or not keeping
    /// them worked.
    pub fn keep(&self, pictures: &[Picture]) -> Vec<String> {
        pictures
            .iter()
            .map(
                |p| match media::store(self.media_root(), &p.bytes, &p.mime) {
                    Ok(name) => name,
                    Err(e) => {
                        tracing::warn!("could not keep an image: {e}");
                        media::name(&p.bytes, &p.mime)
                    }
                },
            )
            .collect()
    }

    /// An item as the saved transcript holds it. A picture is kept before
    /// it is named, so a name on disk always has its file; one that cannot
    /// be kept is left out, and its result says so in `words`.
    fn to_disk(&self, words: &Store, item: &Item) -> serde_json::Result<Value> {
        let Item::Result {
            id,
            name,
            output,
            failed,
            pictures,
        } = item
        else {
            return serde_json::to_value(item);
        };
        if pictures.is_empty() {
            return serde_json::to_value(item);
        }
        let mut output = output.clone();
        let mut refs = Vec::new();
        for p in pictures {
            match media::store(self.media_root(), &p.bytes, &p.mime) {
                Ok(file) => refs.push(json!({"mime": p.mime, "file": file})),
                Err(e) => {
                    tracing::warn!("could not keep an image: {e}");
                    output.push_str(words.text(TEXTS, "image_unkept"));
                }
            }
        }
        let mut v = serde_json::to_value(Item::result(id, name, output, *failed))?;
        if !refs.is_empty() {
            v["pictures"] = Value::Array(refs);
        }
        Ok(v)
    }

    /// Add items to the history, in the order they entered the transcript.
    /// The transcript is trimmed and cleared as it fills; this is the copy
    /// nothing is ever taken out of.
    ///
    /// Pictures are not written here: the bytes are under `media/`, and the
    /// history says which file each one is with a line `[image: <file>]`.
    pub fn remember(&self, items: &[Item]) {
        if items.is_empty() {
            return;
        }
        let rows: Result<Vec<String>, _> = items
            .iter()
            .map(|item| serde_json::to_string(&without_pictures(item)))
            .collect();
        let written = rows
            .map_err(other)
            .and_then(|rows| self.appending("history", "item", rows).map_err(other));
        if let Err(e) = written {
            tracing::warn!("could not add to the history: {e}");
        }
    }

    fn appending(
        &self,
        table: &str,
        column: &str,
        rows: impl IntoIterator<Item = String>,
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        append(&tx, table, column, &self.id, rows)?;
        tx.commit()
    }

    /// Everything the transcript ever held, in order.
    pub fn history(&self) -> Vec<Item> {
        self.column("SELECT item FROM history WHERE id = ?1 ORDER BY n")
            .iter()
            .filter_map(|text| serde_json::from_str(text).ok())
            .collect()
    }

    fn column(&self, sql: &str) -> Vec<String> {
        let conn = self.conn();
        let read = conn.prepare(sql).and_then(|mut query| {
            query
                .query_map([&self.id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()
        });
        read.unwrap_or_else(|e| {
            tracing::warn!("could not read the undertaking {}: {e}", self.id);
            Vec::new()
        })
    }

    /// Append one line to the step log.
    pub fn log(&self, line: &Line) {
        let written = serde_json::to_string(line)
            .map_err(other)
            .and_then(|line| self.appending("log", "line", [line]).map_err(other));
        if let Err(e) = written {
            tracing::warn!("could not write the step log: {e}");
        }
    }

    /// The step log, oldest line first.
    pub fn logged(&self) -> Vec<Line> {
        self.column("SELECT line FROM log WHERE id = ?1 ORDER BY n")
            .iter()
            .filter_map(|text| serde_json::from_str(text).ok())
            .collect()
    }
}
