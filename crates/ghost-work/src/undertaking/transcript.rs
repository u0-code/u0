//! What the work leaves behind as it goes: the transcript it can be picked
//! up from, the history nothing is taken out of, the step log, and the
//! pictures they name, kept under `media/` beside the database.

use std::{
    fs,
    path::{Path, PathBuf},
};

use ghost_brain::{Item, Picture};
use ghost_core::media;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use super::{
    Undertaking,
    db::{append, other},
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

/// An item from a saved transcript. A picture is either a reference to a
/// file under `root/media/`, which is read back, or, as this was first
/// written, its bytes in base64. A reference whose file is gone is dropped,
/// and the result says so.
fn from_disk(mut saved: Value, root: &Path) -> Option<Item> {
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
        let Some(file) = picture["file"].as_str() else {
            pictures.push(serde_json::from_value(picture).ok()?);
            continue;
        };
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
            None => output.push_str(&format!(
                "\n[The image {file} is no longer on disk, so it cannot be shown.]"
            )),
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
    /// as references are read back from `media/`.
    pub fn transcript(&self) -> Option<Vec<Item>> {
        let text: String = self
            .conn()
            .query_row(
                "SELECT items FROM transcript WHERE id = ?1",
                [&self.id],
                |row| row.get(0),
            )
            .optional()
            .ok()??;
        let saved: Vec<Value> = serde_json::from_str(&text).ok()?;
        saved
            .into_iter()
            .map(|item| from_disk(item, self.media_root()))
            .collect()
    }

    /// Replace the transcript with `items`. Done after every step, so that
    /// a process stopped anywhere can be picked up from the last one.
    ///
    /// It is rewritten whole every time, so it holds no image bytes: each
    /// picture is kept under `media/` first, and saved as the name of its
    /// file.
    pub fn save_transcript(&self, items: &[Item]) {
        let saved = items
            .iter()
            .map(|item| self.to_disk(item))
            .collect::<Result<Vec<Value>, _>>()
            .and_then(|items| serde_json::to_string(&items))
            .map_err(other)
            .and_then(|text| {
                self.conn()
                    .execute(
                        "INSERT INTO transcript (id, items) VALUES (?1, ?2)
                     ON CONFLICT (id) DO UPDATE SET items = excluded.items",
                        params![self.id, text],
                    )
                    .map_err(other)
            });
        if let Err(e) = saved {
            tracing::warn!("could not save the transcript: {e}");
        }
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
    /// be kept is left out, and its result says so.
    fn to_disk(&self, item: &Item) -> serde_json::Result<Value> {
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
                    output.push_str("\n[An image could not be kept on disk, and was dropped.]");
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
    pub fn log(&self, line: &serde_json::Value) {
        if let Err(e) = self.appending("log", "line", [line.to_string()]) {
            tracing::warn!("could not write the step log: {e}");
        }
    }

    /// The step log, oldest line first.
    pub fn logged(&self) -> Vec<serde_json::Value> {
        self.column("SELECT line FROM log WHERE id = ?1 ORDER BY n")
            .iter()
            .filter_map(|text| serde_json::from_str(text).ok())
            .collect()
    }
}
