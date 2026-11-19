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
use crate::prompts::{Store, TEXTS};

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
    /// Where it came from, as the mark on what she reads of it says, in
    /// `words`.
    fn origin(&self, words: &Store) -> String {
        words.fill(
            TEXTS,
            "skill_origin",
            &[
                ("repository", self.source.clone()),
                ("commit", self.sha.chars().take(12).collect()),
            ],
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
    /// is nobody's she chose. Her journal is told why in `words`.
    pub fn know(&self, words: &Store, skills: Vec<memory::Skill>) {
        for skill in skills {
            if !guard::name_ok(&skill.name) {
                continue;
            }
            let why = match guard::hash(&self.dir.join(&skill.name)) {
                Ok(hash) if hash == skill.hash => None,
                Ok(_) => Some(words.text(TEXTS, "skill_changed")),
                Err(_) => Some(words.text(TEXTS, "skill_folder_gone")),
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

    /// What `find_skills` shows for `query`, in `words`: what the index
    /// lists, most installed first, each with what its auditors say and
    /// whether it can be installed here.
    pub async fn find(&self, words: &Store, query: &str) -> Result<String, String> {
        let query = query.trim();
        if query.chars().count() < 2 {
            return Err(words.text(TEXTS, "find_skills_short").to_owned());
        }
        let mut hits = self.index.search(words, query).await?;
        hits.sort_by(|a, b| b.installs.cmp(&a.installs));
        let mut seen = Vec::new();
        hits.retain(|h| {
            let new = !seen.contains(&h.id);
            seen.push(h.id.clone());
            new
        });
        if hits.is_empty() {
            return Ok(words.fill(TEXTS, "no_skills_found", &[("query", query.to_owned())]));
        }
        let shown: Vec<Hit> = hits.iter().take(SHOWN).cloned().collect();
        let audits = self.audits(words, &shown).await;
        let mut out = Vec::new();
        for (n, (hit, audit)) in shown.iter().zip(audits).enumerate() {
            let about = if hit.description.is_empty() {
                String::new()
            } else {
                words.fill(
                    TEXTS,
                    "skill_about",
                    &[("description", crate::servers::short(&hit.description, 200))],
                )
            };
            let audit_said = audit.as_ref().map_or_else(
                || words.text(TEXTS, "audit_none").to_owned(),
                |verdicts| {
                    verdicts
                        .iter()
                        .map(|v| said(words, v))
                        .collect::<Vec<_>>()
                        .join("; ")
                },
            );
            out.push(words.fill(
                TEXTS,
                "skill_found",
                &[
                    ("n", (n + 1).to_string()),
                    ("id", hit.id.clone()),
                    ("installs", hit.installs.to_string()),
                    ("about", about),
                    ("audit", audit_said),
                    ("here", self.installable(words, hit, audit.as_deref())),
                ],
            ));
        }
        if hits.len() > SHOWN {
            out.push(words.fill(
                TEXTS,
                "more_skills_found",
                &[("count", (hits.len() - SHOWN).to_string())],
            ));
        }
        Ok(out.join("\n"))
    }

    /// What the auditors say of each of `hits`, in their order, asked all
    /// at once.
    async fn audits(&self, words: &Store, hits: &[Hit]) -> Vec<Option<Vec<Verdict>>> {
        let words = Arc::new(words.clone());
        let mut asked = tokio::task::JoinSet::new();
        for (at, hit) in hits.iter().enumerate() {
            if hit.source.is_empty() {
                continue;
            }
            let (index, words, source, skill) = (
                self.index.clone(),
                words.clone(),
                hit.source.clone(),
                hit.skill.clone(),
            );
            asked.spawn(tracing::Instrument::in_current_span(async move {
                (at, index.audit(&words, &source, &skill).await)
            }));
        }
        let mut audits = vec![None; hits.len()];
        while let Some(Ok((at, audit))) = asked.join_next().await {
            audits[at] = audit;
        }
        audits
    }

    /// Whether `hit` can be installed here, as `find_skills` says it in
    /// `words`.
    fn installable(&self, words: &Store, hit: &Hit, audit: Option<&[Verdict]>) -> String {
        let no = |why: String| words.fill(TEXTS, "installs_no", &[("why", why)]);
        if index::split(&hit.id).is_none() {
            return no(words.text(TEXTS, "skill_id_not_github").to_owned());
        }
        if hit.source.is_empty() {
            return no(words.text(TEXTS, "skill_source_differs").to_owned());
        }
        if let Some(v) = audit.into_iter().flatten().find(|v| v.refuses()) {
            return no(words.fill(
                TEXTS,
                "skill_rated",
                &[("auditor", v.auditor.clone()), ("risk", v.risk.clone())],
            ));
        }
        let had = hold(&self.had);
        if !had.contains_key(&hit.skill) && had.len() >= self.cfg.max {
            return no(words.fill(
                TEXTS,
                "skills_full_here",
                &[("count", had.len().to_string())],
            ));
        }
        let mut yes = if audit.is_some() {
            words.text(TEXTS, "installs_yes").to_owned()
        } else {
            words.text(TEXTS, "skill_unaudited").to_owned()
        };
        if had.contains_key(&hit.skill) {
            yes.push_str(words.text(TEXTS, "skill_had"));
        }
        yes
    }

    /// Install the skill the index lists as `id`: check it, take its
    /// folder at the commit its repository is at now, check every file of
    /// it, write it under her skills, and keep it. Returns what she has
    /// now, in a sentence or two in `words`.
    pub async fn install(&self, words: &Store, id: &str) -> Result<String, String> {
        let id = id.trim();
        let Some((owner, repo, skill)) = index::split(id) else {
            return Err(words.fill(TEXTS, "skill_bad_id", &[("id", id.to_owned())]));
        };
        let source = format!("{owner}/{repo}");
        {
            let had = hold(&self.had);
            if !had.contains_key(skill) && had.len() >= self.cfg.max {
                return Err(words.fill(TEXTS, "skills_full", &[("count", had.len().to_string())]));
            }
        }
        // Only what the index lists, from the repository its id names.
        let listed = self
            .index
            .search(words, skill)
            .await?
            .into_iter()
            .any(|h| h.id == id && !h.source.is_empty());
        if !listed {
            return Err(words.fill(TEXTS, "skill_unlisted", &[("id", id.to_owned())]));
        }
        let audit = self.index.audit(words, &source, skill).await;
        if let Some(v) = audit.iter().flatten().find(|v| v.refuses()) {
            return Err(words.fill(
                TEXTS,
                "skill_refused",
                &[("auditor", v.auditor.clone()), ("risk", v.risk.clone())],
            ));
        }
        let folder = self.github.folder(words, owner, repo, skill).await?;
        if folder.files.len() > guard::MAX_FILES {
            return Err(words.fill(
                TEXTS,
                "too_many_files",
                &[
                    ("count", folder.files.len().to_string()),
                    ("most", guard::MAX_FILES.to_string()),
                ],
            ));
        }
        let total: u64 = folder.files.iter().map(|f| f.size).sum();
        if total > guard::MAX_BYTES {
            return Err(words.fill(
                TEXTS,
                "too_many_bytes",
                &[
                    ("bytes", total.to_string()),
                    ("most", guard::MAX_BYTES.to_string()),
                ],
            ));
        }
        if let Some(big) = folder
            .files
            .iter()
            .find(|f| guard::asset(&f.path) && f.size > guard::MAX_ASSET)
        {
            return Err(words.fill(
                TEXTS,
                "asset_too_large",
                &[
                    ("path", big.path.clone()),
                    ("bytes", big.size.to_string()),
                    ("most", guard::MAX_ASSET.to_string()),
                ],
            ));
        }
        let scripts = folder.files.iter().any(|f| f.path.starts_with("scripts/"));
        if audit.is_none() && scripts {
            return Err(words.text(TEXTS, "unaudited_scripts").to_owned());
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
                .file(words, owner, repo, &folder.sha, &full, most)
                .await?;
            if !guard::asset(&file.path) && !guard::text(&bytes) {
                return Err(words.fill(TEXTS, "not_text", &[("path", file.path.clone())]));
            }
            fetched.push((file.path.clone(), bytes));
        }
        let text = fetched
            .iter()
            .find(|(path, _)| path == "SKILL.md")
            .and_then(|(_, bytes)| std::str::from_utf8(bytes).ok())
            .unwrap_or_default();
        let front = guard::front(words, text, skill)?;
        let hash = self
            .write(&front.name, &fetched)
            .map_err(|e| words.fill(TEXTS, "skill_unwritten", &[("why", e.to_string())]))?;
        let had = Had {
            source: source.clone(),
            sha: folder.sha.clone(),
            description: front.description.clone(),
        };
        let origin = had.origin(words);
        hold(&self.had).insert(front.name.clone(), had);
        self.record(Event::SkillInstalled {
            name: front.name.clone(),
            source,
            path: folder.path,
            sha: folder.sha,
            hash,
            description: front.description.clone(),
        });
        let mut said = words.fill(
            TEXTS,
            "skill_installed",
            &[
                ("name", front.name),
                ("origin", origin),
                ("description", front.description),
            ],
        );
        if scripts {
            said.push_str(words.text(TEXTS, "skill_has_scripts"));
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

    /// Take out the skill she installed called `name`, for `why`. What is
    /// said of it is said in `words`.
    pub fn uninstall(&self, words: &Store, name: &str, why: &str) -> Result<String, String> {
        let name = name.trim();
        if hold(&self.had).remove(name).is_none() {
            return Err(self.none_called(words, name));
        }
        let _ = std::fs::remove_dir_all(self.dir.join(name));
        self.record(Event::SkillUninstalled {
            name: name.to_owned(),
            why: why.to_owned(),
        });
        Ok(words.fill(TEXTS, "skill_uninstalled", &[("name", name.to_owned())]))
    }

    fn none_called(&self, words: &Store, name: &str) -> String {
        let have: Vec<String> = hold(&self.had).keys().cloned().collect();
        let named = ("name", name.to_owned());
        if have.is_empty() {
            words.fill(TEXTS, "no_skills_yet", &[named])
        } else {
            words.fill(TEXTS, "no_skill_but", &[named, ("skills", have.join(", "))])
        }
    }

    /// What `use_skill` gives her: the body of skill `name`, or with `file`
    /// that one of its files, and where the skill came from. What is said
    /// around it is said in `words`.
    pub fn read(
        &self,
        words: &Store,
        name: &str,
        file: Option<&str>,
    ) -> Result<(String, String), String> {
        let name = name.trim();
        let Some(had) = hold(&self.had).get(name).cloned() else {
            return Err(self.none_called(words, name));
        };
        let dir = self.dir.join(name);
        let files = files(&dir);
        let Some(file) = file.map(str::trim).filter(|f| !f.is_empty()) else {
            let text = std::fs::read_to_string(dir.join("SKILL.md")).map_err(|e| {
                words.fill(
                    TEXTS,
                    "skill_unreadable",
                    &[("name", name.to_owned()), ("why", e.to_string())],
                )
            })?;
            let front = guard::front(words, &text, name)?;
            let mut out = head(words, &front.body, BODY);
            let others: Vec<&String> = files.iter().filter(|f| *f != "SKILL.md").collect();
            if !others.is_empty() {
                out.push_str(
                    &words.fill(
                        TEXTS,
                        "skill_files",
                        &[(
                            "files",
                            others
                                .iter()
                                .map(|f| f.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        )],
                    ),
                );
            }
            return Ok((out, had.origin(words)));
        };
        let missing = || {
            words.fill(
                TEXTS,
                "no_skill_file",
                &[
                    ("name", name.to_owned()),
                    ("file", file.to_owned()),
                    ("files", files.join(", ")),
                ],
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
            head(words, &String::from_utf8_lossy(&bytes), FILE)
        } else {
            words.fill(
                TEXTS,
                "skill_file_not_text",
                &[
                    ("file", file.to_owned()),
                    ("bytes", bytes.len().to_string()),
                ],
            )
        };
        Ok((out, had.origin(words)))
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
/// in `words` that says so: a skill is meant to be short, and the rest is
/// better asked for than read whole.
fn head(words: &Store, text: &str, most: usize) -> String {
    let total = text.chars().count();
    if total <= most {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(most).collect();
    out.push_str(&words.fill(TEXTS, "skill_cut", &[("count", (total - most).to_string())]));
    out
}

/// One auditor's verdict, as `find_skills` shows it in `words`.
fn said(words: &Store, verdict: &Verdict) -> String {
    let mut pairs = vec![
        ("auditor", verdict.auditor.clone()),
        ("risk", verdict.risk.clone()),
    ];
    match verdict.score {
        Some(score) => {
            pairs.push(("score", score.to_string()));
            words.fill(TEXTS, "verdict_scored", &pairs)
        }
        None => words.fill(TEXTS, "verdict", &pairs),
    }
}

/// The body of a web answer, refused once it runs past `most` bytes, and
/// why in `words`.
pub(crate) async fn read(
    words: &Store,
    mut answer: reqwest::Response,
    most: usize,
) -> Result<Vec<u8>, String> {
    let url = answer.url().clone();
    let too_large = || {
        words.fill(
            TEXTS,
            "answer_too_long",
            &[("url", url.to_string()), ("most", most.to_string())],
        )
    };
    if answer.content_length().is_some_and(|n| n > most as u64) {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = answer.chunk().await.map_err(|e| {
        words.fill(
            TEXTS,
            "answer_broken_off",
            &[("url", url.to_string()), ("why", e.to_string())],
        )
    })? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > most {
            return Err(too_large());
        }
    }
    Ok(bytes)
}
