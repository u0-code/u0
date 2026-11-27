//! What the server is asked of a project's folder and its git, off the
//! window's thread: where a session works and what its folder holds, its
//! diffs, commits and pull request, its worktree and checkpoints, what you
//! ask of git from the desk, finding things in it, and one of its files
//! read for the column's viewer. Each is one
//! [`Call`](crate::proto::Call), answered as one [`Event`].

use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use agent_client_protocol::v1::schema::ToolCallId;
use serde::{Deserialize, Serialize};

use crate::{
    git,
    proto::Event,
    scripts::{self, Script},
    session::{Begin, Format},
    settings::Submodules,
};

/// A checkpoint, and what changed up to it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub commit: String,
    /// From the turn's checkpoint before to this one.
    pub turn: Option<Vec<git::FileDiff>>,
    /// From the session's first checkpoint to this one.
    pub total: Option<Vec<git::FileDiff>>,
}

/// What you ask of git from the desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitAction {
    Commit(String),
    CommitPush(String),
    /// Commit what is left with `message`, push, and open a pull request
    /// with `title` and `body`.
    PullRequest {
        message: String,
        title: String,
        body: String,
    },
    RemoveWorktree(git::Worktree),
    /// Check out this branch.
    Switch(String),
    /// Rename the worktree's branch, unless it is pushed.
    RenameBranch(String),
    /// Merge the branch's pull request, as gh does.
    /// Merge it as the method says; a squash without the agents' credits
    /// where the second is so.
    Merge(git::Method, bool),
    /// Bring the branch's pull request up to date with its base: by a
    /// merge, or a rebase where it is so.
    UpdateBranch(bool),
    /// Make this branch, from where it is, and check it out.
    NewBranch(String),
    /// Bring in what the upstream has.
    Pull,
    /// Make the folder a repository.
    Init,
    /// Put the repository on GitHub: public, or private.
    Publish(bool),
    /// Save uncommitted changes in a stash, with an optional message.
    StashPush(Option<String>),
    /// Apply (`pop: false`) or pop (`pop: true`) `stash@{index}`.
    StashApply {
        index: usize,
        pop: bool,
    },
    /// Drop `stash@{index}`.
    StashDrop(usize),
}

impl GitAction {
    /// What it does, as "Couldn't …" ends.
    pub fn doing(&self) -> &'static str {
        match self {
            Self::Commit(_) => "commit",
            Self::CommitPush(_) => "commit and push",
            Self::PullRequest { .. } => "open the pull request",
            Self::RemoveWorktree(_) => "remove the worktree",
            Self::Switch(_) => "switch the branch",
            Self::RenameBranch(_) => "rename the branch",
            Self::Merge(..) => "merge the pull request",
            Self::UpdateBranch(_) => "update the branch",
            Self::Pull => "pull",
            Self::NewBranch(_) => "make the branch",
            Self::Init => "make it a repository",
            Self::Publish(_) => "put it on GitHub",
            Self::StashPush(_) => "stash",
            Self::StashApply { .. } => "apply the stash",
            Self::StashDrop(_) => "drop the stash",
        }
    }
}

/// What a project's folder holds: whether it is in a repository, else
/// the repositories in it; and an icon of its own.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Held {
    pub repo: bool,
    pub repos: Vec<PathBuf>,
    pub icon: Option<PathBuf>,
}

/// The branch of `folder`, where the session `key` works, whether it is in
/// a repository (else the repositories it holds), and the scripts its
/// `u0.json` names.
pub fn look(key: u64, folder: PathBuf) -> Event {
    let root = git::root(&folder);
    let repos = if root.is_some() {
        Vec::new()
    } else {
        git::repos(&folder)
    };
    let summaries = git::repo_summaries(&repos);
    Event::Looked {
        key,
        branch: git::branch(&folder),
        repo: root.is_some(),
        repos,
        summaries,
        scripts: root
            .map(|r| scripts::config(&r).scripts)
            .unwrap_or_default(),
        folder,
    }
}

/// Put `script` at `at` in the `u0.json` of the repository at `dir`, or
/// take the one at `at` away; then the session `key`'s folder, read again.
pub fn save_script(dir: PathBuf, at: Option<usize>, script: Option<Script>, key: u64) -> Event {
    match scripts::write_script(&dir, at, script) {
        Ok(()) => look(key, dir),
        Err(why) => Event::Notice(Err(("Couldn't write u0.json".into(), why))),
    }
}

/// The folders in `dir` that are not hidden, by name, in order; none
/// where it cannot be read.
pub fn folders(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// What the project `folder` holds.
pub fn read_project(folder: PathBuf) -> Event {
    let repo = git::root(&folder).is_some();
    Event::Held {
        held: Held {
            repo,
            repos: if repo {
                Vec::new()
            } else {
                git::repos(&folder)
            },
            icon: icon_of(&folder),
        },
        folder,
    }
}

/// The icon a project has of its own, if it has one where icons are kept.
fn icon_of(folder: &Path) -> Option<PathBuf> {
    [
        "favicon.svg",
        "favicon.png",
        "public/favicon.svg",
        "public/favicon.png",
        "static/favicon.png",
        "assets/icon.svg",
        "assets/icon.png",
        "icon.svg",
        "icon.png",
        "logo.svg",
        "logo.png",
    ]
    .iter()
    .map(|name| folder.join(name))
    .find(|p| p.is_file())
}

/// What changed in `scope` (the branch against its base, or a commit)
/// where the session `key` works.
pub fn scope_diff(
    dir: &Path,
    key: u64,
    scope: git::Scope,
    base: Option<&str>,
    no_spacing: bool,
) -> Event {
    let result = match &scope {
        git::Scope::Commit(id) => git::commit_diff(dir, id, no_spacing),
        git::Scope::Uncommitted => git::uncommitted(dir, None, no_spacing),
        _ => git::against_base(dir, base, no_spacing),
    };
    Event::Diffed { key, scope, result }
}

/// Make a worktree in `under` for the session `key`, about `about`,
/// on a branch beginning with `prefix`, its submodules checked out as
/// `submodules` says; only the files `u0.json` asks for where it names
/// sparse-checkout patterns.
pub fn make_worktree(
    dir: &Path,
    about: &str,
    under: &Path,
    prefix: &str,
    from: Option<Begin>,
    submodules: Submodules,
    key: u64,
) -> Event {
    let result = match from {
        Some(Begin::Pr(number, _)) => git::pr_worktree(dir, number, under),
        from => {
            let from = match from {
                Some(Begin::Branch(b)) => b,
                _ => git::fresh_base(dir),
            };
            sparse(dir, &from).and_then(|patterns| {
                git::worktree(dir, about, under, prefix, &from, patterns.as_deref())
            })
        }
    };
    let result = result.map(|wt| {
        git::submodules(&wt, submodules);
        let scripts = scripts::config(&wt.path).scripts;
        (wt, scripts)
    });
    Event::Worktree { key, result }
}

/// The sparse-checkout patterns `u0.json` points to, both read as the
/// commit `from` of the repository at `dir` has them: none where it names
/// none, and refused where the file it names is not there, rather than
/// every file checked out.
fn sparse(dir: &Path, from: &str) -> Result<Option<String>, String> {
    let Ok(text) = git::show(dir, from, "u0.json") else {
        return Ok(None);
    };
    let config: scripts::Config =
        serde_json::from_str(&text).map_err(|e| format!("u0.json in {from}: {e}"))?;
    config
        .sparse
        .map(|path| {
            git::show(dir, from, &path).map_err(|_| {
                format!("{path}, which u0.json names for a sparse checkout, is not in {from}")
            })
        })
        .transpose()
}

/// Take a checkpoint; after a turn, read what changed in it and in all.
///
/// `after` says it is taken after the turn `turn`, and `before` and `first`
/// are the turn's checkpoint before it and the session's first; `then` is
/// what is said once it is taken.
pub fn checkpoint(
    dir: &Path,
    name: &str,
    key: u64,
    turn: usize,
    after: bool,
    (before, first): (Option<String>, Option<String>),
    then: Option<String>,
    no_spacing: bool,
) -> Event {
    let result = git::checkpoint(dir, name).map(|commit| {
        let diff = |from: &Option<String>| {
            from.as_deref()
                .filter(|_| after)
                .and_then(|f| git::diff(dir, f, &commit, no_spacing).ok())
        };
        Snapshot {
            turn: diff(&before),
            total: diff(&first),
            commit,
        }
    });
    Event::Checkpoint {
        key,
        turn,
        after,
        result,
        then,
    }
}

/// Put the session's files back as `target` had them.
pub fn restore(
    dir: &Path,
    target: &str,
    name: &str,
    first: Option<String>,
    key: u64,
    turn: usize,
    undo: bool,
    no_spacing: bool,
) -> Event {
    let result = git::restore(dir, target, name).map(|back| {
        let total = first.and_then(|f| {
            let now = git::checkpoint(dir, &format!("{name}-now")).ok()?;
            git::diff(dir, &f, &now, no_spacing).ok()
        });
        (back, total)
    });
    Event::Restored {
        key,
        turn,
        undo,
        result,
    }
}

/// Do `action` in `dir`, saying each step on its way.
pub fn git_action(
    dir: &Path,
    action: GitAction,
    prefix: &str,
    left_out: &[String],
    key: u64,
    say: &dyn Fn(Event),
) -> Event {
    let step = |text: &str| {
        say(Event::GitStep {
            key,
            text: text.to_owned(),
        });
    };
    // Work on the main branch goes on a branch of its own before a pull
    // request is opened for it: named for what the commit says. A push
    // alone goes where you are.
    let own_branch = |m: &str| -> Result<(), String> {
        let main = git::default_branch(dir);
        let now = git::branch(dir);
        if now.is_some()
            && main.as_deref().map(|b| b.trim_start_matches("origin/")) == now.as_deref()
        {
            step("Making a branch");
            git::branch_off(dir, &format!("{prefix}{}", git::slug(m)))?;
        }
        Ok(())
    };
    let result = match &action {
        GitAction::Commit(m) => git::commit(dir, m, left_out).map(|id| format!("Committed {id}")),
        GitAction::CommitPush(m) => {
            step("Committing");
            git::commit(dir, m, left_out).and_then(|id| {
                step("Pushing");
                git::push(dir).map(|_| format!("Committed {id} and pushed"))
            })
        }
        GitAction::PullRequest {
            message,
            title,
            body,
        } => own_branch(message).and_then(|()| {
            step("Committing");
            // Nothing left to commit is no reason to stop.
            let _ = git::commit(dir, message, left_out);
            step("Pushing");
            git::push(dir).and_then(|_| {
                step("Opening the pull request");
                git::pull_request(dir, title, body)
            })
        }),
        GitAction::Switch(b) => git::switch(dir, b).map(|()| format!("On {b}")),
        GitAction::Merge(how, uncredit) => {
            git::merge_pr(dir, *how, *uncredit).map(|_| "The pull request is merged".into())
        }

        // On the host: what it made here comes with a pull.
        GitAction::UpdateBranch(rebase) => git::update_branch(dir, *rebase)
            .map(|_| "The pull request is up to date with its base; pull to have it here".into()),
        GitAction::Pull => git::pull(dir).map(|_| "Pulled".into()),
        GitAction::NewBranch(b) => git::branch_off(dir, b).map(|()| format!("On {b}, new")),
        GitAction::Init => {
            git::init_here(dir).map(|_| "It is a repository now, all of it committed".into())
        }
        GitAction::Publish(public) => git::publish(dir, *public).map(|_| {
            format!(
                "It is on GitHub now, {}",
                if *public { "public" } else { "private" }
            )
        }),
        GitAction::RenameBranch(b) => git::rename_branch(dir, b).map(|()| b.clone()),
        GitAction::RemoveWorktree(wt) => git::remove_worktree(wt)
            .map(|()| format!("Worktree removed; the branch {} is kept", wt.branch)),
        GitAction::StashPush(m) => {
            git::stash_push(dir, m.as_deref()).map(|_| "Changes stashed".into())
        }
        GitAction::StashApply { index, pop } => git::stash_apply(dir, *index, *pop).map(|_| {
            if *pop {
                format!("Popped stash@{{{index}}}")
            } else {
                format!("Applied stash@{{{index}}}")
            }
        }),
        GitAction::StashDrop(index) => {
            git::stash_drop(dir, *index).map(|_| format!("Dropped stash@{{{index}}}"))
        }
    };
    Event::Git {
        key,
        action,
        result,
    }
}

/// Clone `url` into `dir`, saying how it goes, until `stop` is set. Where
/// the host refuses this machine's SSH key, it is cloned over HTTPS with
/// git's credentials for that host instead, as t3code does, and its
/// remote is the HTTPS address.
pub fn clone(url: &str, dir: PathBuf, stop: &AtomicBool, say: &dyn Fn(Event)) -> Event {
    let cloning = |what: &str, share: Option<f32>| {
        say(Event::Cloning {
            what: what.to_owned(),
            share,
        });
    };
    let mut result = git::clone(url, &dir, &cloning, stop);
    if let (Err(why), Some(https)) = (&result, over_https(url))
        && [
            "Permission denied (publickey",
            "Host key verification failed",
        ]
        .iter()
        .any(|s| why.contains(s))
    {
        cloning("Your SSH key was refused: cloning over HTTPS", None);
        result = git::clone(&https, &dir, &cloning, stop);
    }
    Event::Cloned { dir, result }
}

/// The HTTPS address of a repository named by its SSH address
/// (`git@host:owner/repo`, `ssh://git@host:22/owner/repo`).
fn over_https(url: &str) -> Option<String> {
    let (host, path) = if let Some(rest) = url.strip_prefix("ssh://") {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        (host.split(':').next()?, path)
    } else {
        let (who, path) = url.split_once(':')?;
        (who.split_once('@')?.1, path)
    };
    (!host.is_empty() && !path.is_empty() && !path.starts_with('/'))
        .then(|| format!("https://{host}/{path}"))
}

/// One entry inside a `.zip` archive, read from its central directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZipEntry {
    pub path: String,
    pub bytes: u64,
    pub is_dir: bool,
}

/// What a file holds, as the column's viewer shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Body {
    /// Text, cut at `TEXT_MOST` (or `MINIFIED_MOST` when minified and not `full`).
    Text {
        text: String,
        total_bytes: u64,
        truncated: bool,
        minified: bool,
    },
    /// Comma- or tab-separated values parsed into columns and rows, with raw text kept.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        total_rows: usize,
        raw: String,
    },
    /// A `.zip` archive's entries, browsed without extracting.
    Archive { bytes: u64, entries: Vec<ZipEntry> },
    /// A PDF's first pages, each a PNG, and how many it has.
    Pdf { pages: Vec<Vec<u8>>, of: usize },
    /// A picture the window draws from the file itself, with its dimensions and size.
    Image { width: u32, height: u32, bytes: u64 },
    /// Video or audio opened with the system player rather than played inside u0.
    Media { audio: bool, bytes: u64 },
    /// Anything else, by its size.
    Binary(u64),
}

/// The most of a text file the viewer reads, and the preview cap for minified bundles.
const TEXT_MOST: usize = 2 << 20;
const MINIFIED_MOST: usize = 100 << 10;
const ARCHIVE_MOST: u64 = 64 << 20;
const TABLE_ROWS_MOST: usize = 500;
/// The pages of a PDF drawn, and how large (points to pixels).
const PDF_PAGES: usize = 24;
const PDF_SCALE: f32 = 1.5;

/// Read `path` for the viewer: text, CSV/TSV table, `.zip` archive, PDF pages,
/// image, video/audio metadata, or binary size.
pub fn read_file(path: PathBuf, full: bool) -> Event {
    let body = read_body(&path, full).map(Box::new);
    Event::FileRead { path, body }
}

fn read_body(path: &Path, full: bool) -> Result<Body, String> {
    use std::io::Read as _;
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let total_bytes = meta.len();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let kind = mime_guess::from_path(path).first();

    if matches!(
        ext.as_str(),
        "mp4"
            | "mov"
            | "m4v"
            | "webm"
            | "mkv"
            | "avi"
            | "mp3"
            | "wav"
            | "m4a"
            | "aac"
            | "flac"
            | "ogg"
    ) || kind.as_ref().is_some_and(|m| {
        m.type_() == mime_guess::mime::VIDEO || m.type_() == mime_guess::mime::AUDIO
    }) {
        let audio = matches!(ext.as_str(), "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg")
            || kind
                .as_ref()
                .is_some_and(|m| m.type_() == mime_guess::mime::AUDIO);
        return Ok(Body::Media {
            audio,
            bytes: total_bytes,
        });
    }

    if kind
        .as_ref()
        .is_some_and(|m| Format::of(m.essence_str()).is_some())
    {
        let (width, height) =
            imagesize::size(path).map_or((0, 0), |s| (s.width as u32, s.height as u32));
        return Ok(Body::Image {
            width,
            height,
            bytes: total_bytes,
        });
    }

    if ext == "zip" && total_bytes <= ARCHIVE_MOST {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if let Some(entries) = zip_entries(&bytes) {
            return Ok(Body::Archive {
                bytes: total_bytes,
                entries,
            });
        }
        return Ok(Body::Binary(total_bytes));
    }

    if kind.is_some_and(|m| m == mime_guess::mime::APPLICATION_PDF) {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        return pdf_pages(bytes);
    }

    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    std::io::Read::take(file, (TEXT_MOST + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    body_of(path, bytes, total_bytes, full)
}

fn body_of(path: &Path, mut bytes: Vec<u8>, total_bytes: u64, full: bool) -> Result<Body, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let kind = mime_guess::from_path(path).first();
    if kind
        .as_ref()
        .is_some_and(|m| Format::of(m.essence_str()).is_some())
    {
        let (width, height) =
            imagesize::blob_size(&bytes).map_or((0, 0), |s| (s.width as u32, s.height as u32));
        return Ok(Body::Image {
            width,
            height,
            bytes: total_bytes,
        });
    }
    if kind.is_some_and(|m| m == mime_guess::mime::APPLICATION_PDF) || bytes.starts_with(b"%PDF-") {
        return pdf_pages(bytes);
    }
    if ext == "zip"
        && let Some(entries) = zip_entries(&bytes)
    {
        return Ok(Body::Archive {
            bytes: total_bytes,
            entries,
        });
    }
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Ok(Body::Binary(total_bytes));
    }
    if matches!(ext.as_str(), "csv" | "tsv") && total_bytes <= TEXT_MOST as u64 {
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        let sep = if ext == "tsv" { '\t' } else { ',' };
        if let Some((headers, rows, total_rows)) = parse_delimited(&raw, sep) {
            return Ok(Body::Table {
                headers,
                rows,
                total_rows,
                raw,
            });
        }
    }
    let lines = bytes.split(|&b| b == b'\n').count();
    let minified = matches!(ext.as_str(), "js" | "mjs" | "cjs" | "css" | "json")
        && bytes.len() > MINIFIED_MOST
        && bytes.len() / lines > 400;
    let cap = if minified && !full {
        MINIFIED_MOST
    } else {
        TEXT_MOST
    };
    let truncated = total_bytes > cap as u64 || bytes.len() > cap;
    if bytes.len() > cap {
        bytes.truncate(cap);
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated && text.ends_with('\u{FFFD}') {
        text.pop();
    }
    Ok(Body::Text {
        text,
        total_bytes,
        truncated,
        minified,
    })
}

/// Format minified JSON or JS/CSS source for inspection in the viewer.
pub fn pretty_print(ext: &str, text: &str) -> String {
    if ext.eq_ignore_ascii_case("json")
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(text)
        && let Ok(pretty) = serde_json::to_string_pretty(&val)
    {
        return pretty;
    }
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut indent = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in text.chars() {
        if let Some(q) = quote {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => {
                quote = Some(ch);
                out.push(ch);
            }
            '{' => {
                indent += 1;
                out.push_str("{\n");
                for _ in 0..indent {
                    out.push_str("  ");
                }
            }
            '}' => {
                indent = indent.saturating_sub(1);
                out.push('\n');
                for _ in 0..indent {
                    out.push_str("  ");
                }
                out.push('}');
            }
            ';' => {
                out.push_str(";\n");
                for _ in 0..indent {
                    out.push_str("  ");
                }
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Parse CSV or TSV text into headers, up to `TABLE_ROWS_MOST` rows, and total row count.
pub fn parse_delimited(text: &str, sep: char) -> Option<(Vec<String>, Vec<Vec<String>>, usize)> {
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' && field.is_empty() {
            in_quotes = true;
        } else if ch == sep {
            row.push(std::mem::take(&mut field));
        } else if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            if row.iter().any(|c| !c.is_empty()) || row.len() > 1 {
                records.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else if ch == '\n' {
            row.push(std::mem::take(&mut field));
            if row.iter().any(|c| !c.is_empty()) || row.len() > 1 {
                records.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else {
            field.push(ch);
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|c| !c.is_empty()) || row.len() > 1 {
            records.push(row);
        }
    }
    let headers = records.first()?.clone();
    if headers.is_empty() {
        return None;
    }
    let total_rows = records.len().saturating_sub(1);
    let cols = headers.len();
    let rows = records
        .into_iter()
        .skip(1)
        .take(TABLE_ROWS_MOST)
        .map(|mut r| {
            r.resize(cols, String::new());
            r
        })
        .collect();
    Some((headers, rows, total_rows))
}

/// Read the central directory of a `.zip` archive without extracting it.
pub fn zip_entries(bytes: &[u8]) -> Option<Vec<ZipEntry>> {
    if bytes.len() < 22 {
        return None;
    }
    let min = bytes.len().saturating_sub(65_557);
    let eocd = (min..=bytes.len() - 22)
        .rev()
        .find(|&i| bytes.get(i..i + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))?;
    let total = u16::from_le_bytes(bytes.get(eocd + 10..eocd + 12)?.try_into().ok()?) as usize;
    let mut pos = u32::from_le_bytes(bytes.get(eocd + 16..eocd + 20)?.try_into().ok()?) as usize;
    let mut entries = Vec::with_capacity(total.min(5000));
    for _ in 0..total.min(5000) {
        if bytes.get(pos..pos + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            break;
        }
        let size = u64::from(u32::from_le_bytes(
            bytes.get(pos + 24..pos + 28)?.try_into().ok()?,
        ));
        let name_len = u16::from_le_bytes(bytes.get(pos + 28..pos + 30)?.try_into().ok()?) as usize;
        let extra_len =
            u16::from_le_bytes(bytes.get(pos + 30..pos + 32)?.try_into().ok()?) as usize;
        let comment_len =
            u16::from_le_bytes(bytes.get(pos + 32..pos + 34)?.try_into().ok()?) as usize;
        let name_bytes = bytes.get(pos + 46..pos + 46 + name_len)?;
        let path = String::from_utf8_lossy(name_bytes).into_owned();
        let is_dir = path.ends_with('/');
        entries.push(ZipEntry {
            path,
            bytes: size,
            is_dir,
        });
        pos = pos.checked_add(46 + name_len + extra_len + comment_len)?;
    }
    Some(entries)
}

/// Extract the `.zip` archive `archive` into `dest`.
pub fn extract_archive(archive: &Path, dest: &Path) -> Event {
    let result = std::fs::create_dir_all(dest)
        .map_err(|e| e.to_string())
        .and_then(|()| {
            crate::registry::run(
                std::process::Command::new("unzip")
                    .args(["-q", "-o"])
                    .arg(archive)
                    .arg("-d")
                    .arg(dest),
            )
            .map(|_| format!("Extracted to {}", dest.display()))
        })
        .map_err(|why| ("Couldn't extract the archive".into(), why));
    Event::Notice(result)
}

/// Copy `from` into `to`, creating parent directories when needed.
pub fn copy_file(from: &Path, to: &Path) -> Event {
    let result = (|| {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(from, to).map_err(|e| e.to_string())?;
        Ok(format!("Copied to {}", to.display()))
    })()
    .map_err(|why| ("Couldn't copy the file".into(), why));
    Event::Notice(result)
}

/// A PDF's first pages drawn, as hayro draws them: in Rust, with no library
/// of the system's or a web view.
fn pdf_pages(bytes: Vec<u8>) -> Result<Body, String> {
    use hayro::{
        PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings,
        hayro_syntax::Pdf, render, vello_cpu::color::palette::css::WHITE,
    };
    let pdf = Pdf::new(bytes).map_err(|e| format!("not a PDF that can be read: {e:?}"))?;
    let (cache, interpret, settings) = (
        RenderCache::new(),
        InterpreterSettings::default(),
        RenderSettings::default(),
    );
    let pixmap = PixmapSettings {
        x_scale: PDF_SCALE,
        y_scale: PDF_SCALE,
        bg_color: WHITE,
    };
    let all = pdf.pages();
    let pages = all
        .iter()
        .take(PDF_PAGES)
        .filter_map(|page| {
            render(page, &cache, &interpret, &settings, &pixmap)
                .into_png()
                .ok()
        })
        .collect();
    Ok(Body::Pdf {
        pages,
        of: all.len(),
    })
}

/// Find the line each diff of the calls `calls` of the session `key` starts
/// on in its file.
pub fn place(key: u64, calls: Vec<(ToolCallId, Vec<(PathBuf, String)>)>) -> Event {
    Event::Placed {
        key,
        starts: calls
            .into_iter()
            .map(|(id, diffs)| (id, diffs.iter().map(|(f, t)| line_of(f, t)).collect()))
            .collect(),
    }
}

/// The line of `file` that `text` starts on, counting from one.
fn line_of(file: &Path, text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let whole = std::fs::read_to_string(file).ok()?;
    let at = whole.find(text)?;
    Some(whole[..at].matches('\n').count() + 1)
}

/// The paths among `all` that hold the letters `typed`, in order (each
/// word typed, in any order), best first: a name above a folder, letters
/// together above letters apart, then the shorter. Each with where its
/// letters stand, by character: nucleo's matcher, as Helix's file
/// picker finds them.
pub fn paths<'a>(
    all: impl IntoIterator<Item = &'a String>,
    typed: &str,
) -> Vec<(&'a String, Vec<usize>)> {
    use nucleo_matcher::{
        Config, Matcher, Utf32Str,
        pattern::{CaseMatching, Normalization, Pattern},
    };
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(typed, CaseMatching::Ignore, Normalization::Smart);
    let mut chars = Vec::new();
    let mut found: Vec<(u32, &String, Vec<usize>)> = all
        .into_iter()
        .filter_map(|path| {
            let mut at = Vec::new();
            let score = pattern.indices(Utf32Str::new(path, &mut chars), &mut matcher, &mut at)?;
            at.sort_unstable();
            at.dedup();
            Some((score, path, at.into_iter().map(|n| n as usize).collect()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
    found.into_iter().map(|(_, path, at)| (path, at)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn a_path_is_found_by_its_letters_in_order_best_first_then_the_shorter() {
        let files = [
            "a/ma/b.rs",
            "app/domain/maps.rs",
            "docs/Main.md",
            "src/main.rs",
            "x.rs",
        ]
        .map(String::from)
        .to_vec();
        let found = paths(&files, "main");
        let names: Vec<&str> = found.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(names[..2], ["src/main.rs", "docs/Main.md"]);
        assert_eq!(found[0].1, [4, 5, 6, 7]);
        assert!(
            paths(&files, "ma")
                .iter()
                .all(|(f, _)| f.as_str() != "x.rs")
        );
        assert_eq!(
            paths(&files, "smr").first().map(|(f, _)| f.as_str()),
            Some("src/main.rs")
        );
    }

    #[test]
    fn an_ssh_address_has_an_https_one_and_others_have_none() {
        assert_eq!(
            over_https("git@github.com:o/r.git").as_deref(),
            Some("https://github.com/o/r.git")
        );
        assert_eq!(
            over_https("ssh://git@gitlab.com:22/g/sub/r").as_deref(),
            Some("https://gitlab.com/g/sub/r")
        );
        assert_eq!(over_https("https://github.com/o/r"), None);
        assert_eq!(over_https("/Users/me/repo"), None);
    }

    #[test]
    fn a_file_reads_as_text_a_picture_or_bytes_by_what_it_holds() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("a.rs");
        std::fs::write(&text, "fn a() {}\n").unwrap();
        let bin = dir.path().join("a.bin");
        std::fs::write(&bin, [1u8, 0, 2]).unwrap();
        let read = |p: &Path| match read_file(p.to_path_buf(), false) {
            Event::FileRead { body, .. } => body.map(|b| *b),
            _ => unreachable!(),
        };
        assert_eq!(
            read(&text),
            Ok(Body::Text {
                text: "fn a() {}\n".into(),
                total_bytes: 10,
                truncated: false,
                minified: false,
            })
        );
        assert_eq!(read(&bin), Ok(Body::Binary(3)));
        assert!(read(&dir.path().join("a.png")).is_err());
        std::fs::write(dir.path().join("b.png"), [0u8]).unwrap();
        assert_eq!(
            read(&dir.path().join("b.png")),
            Ok(Body::Image {
                width: 0,
                height: 0,
                bytes: 1,
            })
        );
        std::fs::write(dir.path().join("clip.mp4"), [0u8, 1, 2, 3]).unwrap();
        assert_eq!(
            read(&dir.path().join("clip.mp4")),
            Ok(Body::Media {
                audio: false,
                bytes: 4,
            })
        );
        let csv = dir.path().join("tokens.csv");
        std::fs::write(&csv, "role,hex\nbg,\"#161616\"\n").unwrap();
        assert_eq!(
            read(&csv),
            Ok(Body::Table {
                headers: vec!["role".into(), "hex".into()],
                rows: vec![vec!["bg".into(), "#161616".into()]],
                total_rows: 1,
                raw: "role,hex\nbg,\"#161616\"\n".into(),
            })
        );
        // Minimal valid ZIP archive with one entry `hello.txt` (size 5).
        let mut zip = Vec::new();
        // Local file header
        zip.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        zip.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&5u32.to_le_bytes());
        zip.extend_from_slice(&5u32.to_le_bytes());
        zip.extend_from_slice(&9u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(b"hello.txt");
        zip.extend_from_slice(b"hello");
        let cd_offset = zip.len() as u32;
        // Central directory header
        zip.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        zip.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&5u32.to_le_bytes());
        zip.extend_from_slice(&5u32.to_le_bytes());
        zip.extend_from_slice(&9u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(b"hello.txt");
        let cd_size = (zip.len() as u32) - cd_offset;
        // End of central directory
        zip.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        zip.extend_from_slice(&1u16.to_le_bytes());
        zip.extend_from_slice(&1u16.to_le_bytes());
        zip.extend_from_slice(&cd_size.to_le_bytes());
        zip.extend_from_slice(&cd_offset.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        let zip_path = dir.path().join("release.zip");
        std::fs::write(&zip_path, &zip).unwrap();
        assert_eq!(
            read(&zip_path),
            Ok(Body::Archive {
                bytes: zip.len() as u64,
                entries: vec![ZipEntry {
                    path: "hello.txt".into(),
                    bytes: 5,
                    is_dir: false,
                }],
            })
        );
        assert!(read(&dir.path().join("broken.pdf")).is_err());
        std::fs::write(dir.path().join("broken.pdf"), b"%PDF-1.4 no more").unwrap();
        assert!(read(&dir.path().join("broken.pdf")).is_err());
    }

    #[test]
    fn a_pdf_reads_as_its_pages_drawn() {
        let pdf = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 50]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n";
        let Ok(Body::Pdf { pages, of }) = body_of(
            Path::new("a.pdf"),
            pdf.as_bytes().to_vec(),
            pdf.len() as u64,
            false,
        ) else {
            panic!("the PDF was not read");
        };
        assert_eq!((pages.len(), of), (1, 1));
        assert!(pages[0].starts_with(b"\x89PNG"));
    }

    #[test]
    fn a_diffs_place_is_found_in_its_file_once_the_call_is_done() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.rs");
        std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
        let mut s = Session::new(1, "x", dir.path().to_path_buf());
        let json = format!(
            r#"{{"sessionUpdate":"tool_call","toolCallId":"e","title":"Edit","kind":"edit","status":"completed","content":[{{"type":"diff","path":"{}","oldText":"3\n","newText":"three\nfour\n"}}]}}"#,
            file.display()
        );
        s.apply(serde_json::from_str(&json).unwrap());
        assert!(s.tools().next().unwrap().starts.is_empty());
        let Event::Placed { starts, .. } = place(1, std::mem::take(&mut s.placing)) else {
            panic!("the diffs were not placed");
        };
        for (id, starts) in starts {
            s.placed(&id, starts);
        }
        assert_eq!(s.tools().next().unwrap().starts, vec![Some(3)]);
    }
}
