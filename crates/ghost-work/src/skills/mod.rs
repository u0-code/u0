//! Agent skills: written know-how she finds and installs herself, and
//! reads when a job calls for it.
//!
//! A skill is a folder with a `SKILL.md` in it, in the open format that
//! Claude Code and others read: a few lines of frontmatter saying what it
//! is and when to use it, then instructions, and beside them, optionally,
//! `references/`, `scripts/` and `assets/`. People publish them on GitHub
//! for jobs that take more than knowing how in general: filling a PDF
//! form, building a slide deck, testing a web app. Where a server of tools
//! gives her new hands (see [`crate::servers`]), a skill gives her a
//! procedure someone else worked out. She finds them in an index of
//! skills, skills.sh, and installs them herself (`find_skills`,
//! `install_skill`, `uninstall_skill`), with nobody approving, as the
//! owner asked.
//!
//! **Read when wanted.** What a skill is for costs a line; the rest costs
//! thousands of tokens. So she sees each skill she has as one line, its
//! name and what it says it is for, and `use_skill` gives her its body, or
//! one of its files, when a moment calls for it. That is the format's own
//! progressive disclosure, and what Claude Code does.
//!
//! **Hers, on the record.** What she installs is in her journal, as a
//! `SkillInstalled` entry with the repository, the commit and a hash of
//! the folder, and it lives under `skills/` in her directory. This module
//! does not write her journal: it queues what happened, and the mind,
//! which is the one writer, takes it with [`Skills::take_changes`].
//!
//! **Safe by shape, not by asking.** A skill is instructions, and what
//! instructions say is what matters; so everything a skill says reaches
//! her marked as someone else's words (see [`crate::outside`]), naming the
//! skill and the repository it came from, and nothing of it is ever run
//! here. Before that: it is a skill the index lists, in a repository on
//! GitHub, taken at a commit and not a branch, and only its own folder;
//! no auditor the index asks rates it high or critical, and one that
//! nobody audited has no scripts; it is the format's shape, with bounded
//! files, no links and nothing outside its folder, text except small
//! assets (see [`guard`]). A folder that changes on disk after she
//! installed it is put aside when her body next starts, with why.

pub mod github;
pub mod guard;
pub mod index;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use ghost_core::{journal::Event, memory};
use serde::Deserialize;

use self::{
    github::GitHub,
    index::{Hit, Index, Verdict},
};

/// How many skills `find_skills` shows.
const SHOWN: usize = 10;
/// The most characters of a skill's body she is given: about the five
/// thousand tokens the format asks a skill to stay under.
const BODY: usize = 20_000;
/// The most characters of one of a skill's files she is given.
const FILE: usize = 30_000;
/// The most characters of what a skill says it is for that her list of
/// skills shows.
const LINE: usize = 300;
/// Where a skill whose folder changed is moved, under the skills folder.
/// No skill can be called this: a name has no dot.
const ASIDE: &str = ".aside";

/// `[skills]` in `ghostd.toml`: how she finds and keeps skills.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Let her find and install skills herself. Those she has stay hers
    /// either way.
    pub enabled: bool,
    /// Most skills she may have installed at once.
    pub max: usize,
    /// The index's search, asked with `?q=`.
    pub search: String,
    /// The index's audit, asked with `?source=owner/repo&skills=name`.
    pub audit: String,
    /// GitHub's API.
    pub github: String,
    /// Where GitHub serves the files of a repository at a commit.
    pub raw: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            max: 30,
            search: "https://skills.sh/api/search".to_owned(),
            audit: "https://www.skills.sh/tele/audit".to_owned(),
            github: "https://api.github.com".to_owned(),
            raw: "https://raw.githubusercontent.com".to_owned(),
        }
    }
}

/// A skill she has, as her list of skills shows it.
#[derive(Debug, Clone)]
pub struct Summary {
    pub name: String,
    pub description: String,
}

/// One skill she has.
#[derive(Debug, Clone)]
struct Had {
    source: String,
    sha: String,
    description: String,
}

impl Had {
    /// Where it came from, as the mark on what she reads of it says.
    fn origin(&self) -> String {
        format!(
            "{} at {}",
            self.source,
            self.sha.chars().take(12).collect::<String>()
        )
    }
}

/// Every skill she has, and the index she finds more in.
pub struct Skills {
    cfg: Config,
    /// Where each skill's folder is.
    dir: PathBuf,
    index: Arc<Index>,
    github: GitHub,
    had: Mutex<BTreeMap<String, Had>>,
    /// What happened that her journal should say, oldest first.
    changes: Mutex<Vec<Event>>,
}

fn hold<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Skills {
    /// The skills kept under `dir`.
    pub fn new(cfg: Config, dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            index: Arc::new(Index::new(&cfg.search, &cfg.audit)),
            github: GitHub::new(&cfg.github, &cfg.raw),
            cfg,
            dir,
            had: Mutex::new(BTreeMap::new()),
            changes: Mutex::new(Vec::new()),
        })
    }

    /// Whether she may find and install skills herself.
    pub fn installs(&self) -> bool {
        self.cfg.enabled
    }

    /// Take in what her memory says she has installed. A body does this
    /// once, as it starts. A skill whose folder is not what she installed,
    /// to the byte, is moved aside under `.aside/` for whoever runs her
    /// body to look at, and is hers no longer until she installs it again:
    /// something other than the install wrote to it, and what it says now
    /// is nobody's she chose.
    pub fn know(&self, skills: Vec<memory::Skill>) {
        for skill in skills {
            if !guard::name_ok(&skill.name) {
                continue;
            }
            let why = match guard::hash(&self.dir.join(&skill.name)) {
                Ok(hash) if hash == skill.hash => None,
                Ok(_) => Some(
                    "its files changed on disk after you installed it, so it is put aside until you install it again and see what it says now.",
                ),
                Err(_) => Some(
                    "its folder is gone, or holds something other than files, so it is put aside until you install it again.",
                ),
            };
            match why {
                None => {
                    hold(&self.had).insert(
                        skill.name,
                        Had {
                            source: skill.source,
                            sha: skill.sha,
                            description: skill.description,
                        },
                    );
                }
                Some(why) => {
                    self.put_aside(&skill.name);
                    self.record(Event::SkillUninstalled {
                        name: skill.name,
                        why: why.to_owned(),
                    });
                }
            }
        }
    }

    /// Move the folder of skill `name` under `.aside/`, over whatever was
    /// put aside under that name before.
    fn put_aside(&self, name: &str) {
        let from = self.dir.join(name);
        if !from.exists() {
            return;
        }
        let to = self.dir.join(ASIDE).join(name);
        let _ = std::fs::remove_dir_all(&to);
        if std::fs::create_dir_all(self.dir.join(ASIDE)).is_err()
            || std::fs::rename(&from, &to).is_err()
        {
            let _ = std::fs::remove_dir_all(&from);
        }
    }

    /// What her journal should say that it does not yet. Taken once.
    pub fn take_changes(&self) -> Vec<Event> {
        std::mem::take(&mut *hold(&self.changes))
    }

    fn record(&self, event: Event) {
        hold(&self.changes).push(event);
    }

    /// Every skill she has, by name.
    pub fn listing(&self) -> Vec<Summary> {
        hold(&self.had)
            .iter()
            .map(|(name, had)| Summary {
                name: name.clone(),
                description: crate::servers::short(had.description.trim(), LINE),
            })
            .collect()
    }

    /// Whether she has any skill at all.
    pub fn any(&self) -> bool {
        !hold(&self.had).is_empty()
    }

    /// What `find_skills` shows for `query`: what the index lists, most
    /// installed first, each with what its auditors say and whether it can
    /// be installed here.
    pub async fn find(&self, query: &str) -> Result<String, String> {
        let query = query.trim();
        if query.chars().count() < 2 {
            return Err("say at least two letters of what to look for.".to_owned());
        }
        let mut hits = self.index.search(query).await?;
        hits.sort_by(|a, b| b.installs.cmp(&a.installs));
        let mut seen = Vec::new();
        hits.retain(|h| {
            let new = !seen.contains(&h.id);
            seen.push(h.id.clone());
            new
        });
        if hits.is_empty() {
            return Ok(format!(
                "The index of skills has nothing for \"{query}\". Try another word for the job, in English."
            ));
        }
        let shown: Vec<Hit> = hits.iter().take(SHOWN).cloned().collect();
        let audits = self.audits(&shown).await;
        let mut out = Vec::new();
        for (n, (hit, audit)) in shown.iter().zip(audits).enumerate() {
            let mut lines = vec![format!("{}. {}: {} installs", n + 1, hit.id, hit.installs)];
            if !hit.description.is_empty() {
                lines.push(format!(
                    "   {}",
                    crate::servers::short(&hit.description, 200)
                ));
            }
            lines.push(format!(
                "   audit: {}",
                audit.as_ref().map_or_else(
                    || "none found".to_owned(),
                    |verdicts| verdicts.iter().map(said).collect::<Vec<_>>().join("; ")
                )
            ));
            lines.push(format!(
                "   installs here: {}",
                self.installable(hit, audit.as_deref())
            ));
            out.push(lines.join("\n"));
        }
        if hits.len() > SHOWN {
            out.push(format!(
                "({} more, installed less; a narrower word finds fewer.)",
                hits.len() - SHOWN
            ));
        }
        Ok(out.join("\n"))
    }

    /// What the auditors say of each of `hits`, in their order, asked all
    /// at once.
    async fn audits(&self, hits: &[Hit]) -> Vec<Option<Vec<Verdict>>> {
        let mut asked = tokio::task::JoinSet::new();
        for (at, hit) in hits.iter().enumerate() {
            if hit.source.is_empty() {
                continue;
            }
            let (index, source, skill) =
                (self.index.clone(), hit.source.clone(), hit.skill.clone());
            asked.spawn(async move { (at, index.audit(&source, &skill).await) });
        }
        let mut audits = vec![None; hits.len()];
        while let Some(Ok((at, audit))) = asked.join_next().await {
            audits[at] = audit;
        }
        audits
    }

    /// Whether `hit` can be installed here, as `find_skills` says it.
    fn installable(&self, hit: &Hit, audit: Option<&[Verdict]>) -> String {
        if index::split(&hit.id).is_none() {
            return "no, its id is not a repository on GitHub and a skill in it.".to_owned();
        }
        if hit.source.is_empty() {
            return "no, the index names another repository as its source than its id does."
                .to_owned();
        }
        if let Some(v) = audit.into_iter().flatten().find(|v| v.refuses()) {
            return format!("no, {} rates it {} risk.", v.auditor, v.risk);
        }
        let had = hold(&self.had);
        if !had.contains_key(&hit.skill) && had.len() >= self.cfg.max {
            return format!(
                "no, you have {} skills, the most you can have; uninstall one first.",
                had.len()
            );
        }
        let mut yes = if audit.is_some() {
            "yes".to_owned()
        } else {
            "only if it has no scripts, since nobody has audited it".to_owned()
        };
        if had.contains_key(&hit.skill) {
            yes.push_str(" (you have a skill of this name; installing takes its latest commit)");
        }
        yes
    }

    /// Install the skill the index lists as `id`: check it, take its
    /// folder at the commit its repository is at now, check every file of
    /// it, write it under her skills, and keep it. Returns what she has
    /// now, in a sentence or two.
    pub async fn install(&self, id: &str) -> Result<String, String> {
        let id = id.trim();
        let Some((owner, repo, skill)) = index::split(id) else {
            return Err(format!(
                "{id} is not a skill's id as the index gives one, owner/repo/skill. Use the id `find_skills` showed."
            ));
        };
        let source = format!("{owner}/{repo}");
        {
            let had = hold(&self.had);
            if !had.contains_key(skill) && had.len() >= self.cfg.max {
                return Err(format!(
                    "you already have {} skills, the most you can have. Uninstall one you use least first.",
                    had.len()
                ));
            }
        }
        // Only what the index lists, from the repository its id names.
        let listed = self
            .index
            .search(skill)
            .await?
            .into_iter()
            .any(|h| h.id == id && !h.source.is_empty());
        if !listed {
            return Err(format!(
                "the index of skills does not list {id}. Use the exact id `find_skills` showed."
            ));
        }
        let audit = self.index.audit(&source, skill).await;
        if let Some(v) = audit.iter().flatten().find(|v| v.refuses()) {
            return Err(format!(
                "{} rates it {} risk, and a skill rated high or critical is not installed.",
                v.auditor, v.risk
            ));
        }
        let folder = self.github.folder(owner, repo, skill).await?;
        if folder.files.len() > guard::MAX_FILES {
            return Err(format!(
                "it has {} files, more than the {} a skill may have.",
                folder.files.len(),
                guard::MAX_FILES
            ));
        }
        let total: u64 = folder.files.iter().map(|f| f.size).sum();
        if total > guard::MAX_BYTES {
            return Err(format!(
                "its files come to {total} bytes, more than the {} a skill may have.",
                guard::MAX_BYTES
            ));
        }
        if let Some(big) = folder
            .files
            .iter()
            .find(|f| guard::asset(&f.path) && f.size > guard::MAX_ASSET)
        {
            return Err(format!(
                "{} is {} bytes, more than the {} one asset may be.",
                big.path,
                big.size,
                guard::MAX_ASSET
            ));
        }
        let scripts = folder.files.iter().any(|f| f.path.starts_with("scripts/"));
        if audit.is_none() && scripts {
            return Err(
                "nobody has audited it, and it has scripts; a skill with scripts is only installed once an auditor has looked at it."
                    .to_owned(),
            );
        }
        let mut fetched = Vec::new();
        for file in &folder.files {
            let full = if folder.path.is_empty() {
                file.path.clone()
            } else {
                format!("{}/{}", folder.path, file.path)
            };
            let most = usize::try_from(file.size).unwrap_or(usize::MAX);
            let bytes = self
                .github
                .file(owner, repo, &folder.sha, &full, most)
                .await?;
            if !guard::asset(&file.path) && !guard::text(&bytes) {
                return Err(format!(
                    "{} is not text, and outside assets/ a skill may only have text.",
                    file.path
                ));
            }
            fetched.push((file.path.clone(), bytes));
        }
        let text = fetched
            .iter()
            .find(|(path, _)| path == "SKILL.md")
            .and_then(|(_, bytes)| std::str::from_utf8(bytes).ok())
            .unwrap_or_default();
        let front = guard::front(text, skill)?;
        let hash = self
            .write(&front.name, &fetched)
            .map_err(|e| format!("it could not be written to disk ({e})."))?;
        let had = Had {
            source: source.clone(),
            sha: folder.sha.clone(),
            description: front.description.clone(),
        };
        let origin = had.origin();
        hold(&self.had).insert(front.name.clone(), had);
        self.record(Event::SkillInstalled {
            name: front.name.clone(),
            source,
            path: folder.path,
            sha: folder.sha,
            hash,
            description: front.description.clone(),
        });
        let mut said = format!(
            "Installed the skill {} from {origin}. It says it is for: {} Read it with `use_skill` when a piece of work or a conversation calls for it.",
            front.name, front.description
        );
        if scripts {
            said.push_str(
                " It has scripts: they are someone else's code, and nothing runs them for you.",
            );
        }
        Ok(said)
    }

    /// Write the files of skill `name` into its folder, replacing what was
    /// there, and return the hash of what was written. Written beside it
    /// first and moved into place, so that a folder is never half one
    /// skill and half another. No file is made executable.
    fn write(&self, name: &str, files: &[(String, Vec<u8>)]) -> std::io::Result<String> {
        std::fs::create_dir_all(&self.dir)?;
        let incoming = self.dir.join(format!(".incoming-{name}"));
        let _ = std::fs::remove_dir_all(&incoming);
        for (path, bytes) in files {
            let to = incoming.join(path);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&to, bytes)?;
        }
        let hash = guard::hash(&incoming)?;
        let target = self.dir.join(name);
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(&incoming, &target)?;
        Ok(hash)
    }

    /// Take out the skill she installed called `name`, for `why`.
    pub fn uninstall(&self, name: &str, why: &str) -> Result<String, String> {
        let name = name.trim();
        if hold(&self.had).remove(name).is_none() {
            return Err(self.none_called(name));
        }
        let _ = std::fs::remove_dir_all(self.dir.join(name));
        self.record(Event::SkillUninstalled {
            name: name.to_owned(),
            why: why.to_owned(),
        });
        Ok(format!("Uninstalled the skill {name}."))
    }

    fn none_called(&self, name: &str) -> String {
        let have: Vec<String> = hold(&self.had).keys().cloned().collect();
        if have.is_empty() {
            format!("you have no skill called {name}, and no skills at all yet.")
        } else {
            format!(
                "you have no skill called {name}. You have: {}.",
                have.join(", ")
            )
        }
    }

    /// What `use_skill` gives her: the body of skill `name`, or with `file`
    /// that one of its files, and where the skill came from.
    pub fn read(&self, name: &str, file: Option<&str>) -> Result<(String, String), String> {
        let name = name.trim();
        let Some(had) = hold(&self.had).get(name).cloned() else {
            return Err(self.none_called(name));
        };
        let dir = self.dir.join(name);
        let files = files(&dir);
        let Some(file) = file.map(str::trim).filter(|f| !f.is_empty()) else {
            let text = std::fs::read_to_string(dir.join("SKILL.md"))
                .map_err(|e| format!("the skill {name} could not be read ({e})."))?;
            let front = guard::front(&text, name)?;
            let mut out = head(&front.body, BODY);
            let others: Vec<&String> = files.iter().filter(|f| *f != "SKILL.md").collect();
            if !others.is_empty() {
                out.push_str(&format!(
                    "\n\n[files of this skill: {}]",
                    others
                        .iter()
                        .map(|f| f.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            return Ok((out, had.origin()));
        };
        let missing = || {
            format!(
                "the skill {name} has no file {file}. Its files are: {}.",
                files.join(", ")
            )
        };
        if !guard::inside(file) || !files.iter().any(|f| f == file) {
            return Err(missing());
        }
        let path = dir.join(file);
        let within = path
            .canonicalize()
            .ok()
            .zip(dir.canonicalize().ok())
            .is_some_and(|(p, d)| p.starts_with(d));
        if !within {
            return Err(missing());
        }
        let bytes = std::fs::read(&path).map_err(|_| missing())?;
        let out = if guard::text(&bytes) {
            head(&String::from_utf8_lossy(&bytes), FILE)
        } else {
            format!(
                "[{file} is not text: {} bytes. It is a file for the skill to use, not to read.]",
                bytes.len()
            )
        };
        Ok((out, had.origin()))
    }
}

/// Every file under `dir`, by its path from there, in order. Only files:
/// a link is not followed and not listed.
fn files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                walk(root, &path, out);
            } else if kind.is_file()
                && let Ok(relative) = path.strip_prefix(root)
            {
                out.push(
                    relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// The first `most` characters of `text`, and if there was more, a note
/// that says so: a skill is meant to be short, and the rest is better
/// asked for than read whole.
fn head(text: &str, most: usize) -> String {
    let total = text.chars().count();
    if total <= most {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(most).collect();
    out.push_str(&format!(
        "\n[cut here: {} more characters. What it leaves out may be in its other files.]",
        total - most
    ));
    out
}

/// One auditor's verdict, as `find_skills` shows it.
fn said(verdict: &Verdict) -> String {
    match verdict.score {
        Some(score) => format!("{} {}, score {score}", verdict.auditor, verdict.risk),
        None => format!("{} {}", verdict.auditor, verdict.risk),
    }
}

/// The body of a web answer, refused once it runs past `most` bytes.
pub(crate) async fn read(mut answer: reqwest::Response, most: usize) -> Result<Vec<u8>, String> {
    let url = answer.url().clone();
    let too_large = || format!("{url} sent more than the {most} bytes that are read from it.");
    if answer.content_length().is_some_and(|n| n > most as u64) {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = answer
        .chunk()
        .await
        .map_err(|e| format!("{url} could not be read to the end ({e})."))?
    {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > most {
            return Err(too_large());
        }
    }
    Ok(bytes)
}
