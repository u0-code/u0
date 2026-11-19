//! Taking one skill's folder out of a repository on GitHub, at a commit.
//!
//! A branch moves; a commit does not. So the repository's default branch
//! is turned into the commit it points at first, and everything after
//! that is read at that commit: the tree, to find the skill's folder and
//! see every file in it before any is fetched, and then each file. Only
//! that folder is fetched, never the rest of the repository. Every limit
//! that can be checked from the tree (how many files, how large, links,
//! paths) is checked before a byte of a file is read.

use std::time::Duration;

use serde::Deserialize;

use super::{guard, read};
use crate::prompts::{Store, TEXTS};

/// How long one request to GitHub may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most bytes of an answer about a repository that are read. A tree
/// of a large repository runs to megabytes.
const ANSWER: usize = 16 * 1024 * 1024;

/// One file of the folder, as the tree lists it.
#[derive(Debug, Clone)]
pub struct Listed {
    /// Its path from the skill's folder.
    pub path: String,
    pub size: u64,
}

/// A skill's folder at a commit, with every file it has, not yet fetched.
#[derive(Debug, Clone)]
pub struct Folder {
    pub sha: String,
    /// Its path inside the repository; empty for the repository's root.
    pub path: String,
    pub files: Vec<Listed>,
}

/// Where GitHub's API and its raw files are, and how they are asked.
pub struct GitHub {
    api: String,
    raw: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct Repo {
    default_branch: String,
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
}

#[derive(Deserialize)]
struct Tree {
    #[serde(default)]
    tree: Vec<Node>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct Node {
    path: String,
    #[serde(default)]
    mode: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    size: u64,
}

impl GitHub {
    pub fn new(api: &str, raw: &str) -> Self {
        Self {
            api: api.trim_end_matches('/').to_owned(),
            raw: raw.trim_end_matches('/').to_owned(),
            http: ghost_net::builder()
                .timeout(TIMEOUT)
                .user_agent(concat!("ghost/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
        }
    }

    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        words: &Store,
        url: &str,
    ) -> Result<T, String> {
        let answer = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| words.fill(TEXTS, "github_unreached", &[("why", e.to_string())]))?;
        serde_json::from_slice(&read(words, answer, ANSWER).await?)
            .map_err(|e| words.fill(TEXTS, "github_unreadable", &[("why", e.to_string())]))
    }

    /// The folder called `skill` in `owner/repo`, at the commit its
    /// default branch points at now. The folder is the one of that name
    /// with a `SKILL.md` in it; of several, the one nearest the root. Why
    /// it cannot be had, in `words`.
    pub async fn folder(
        &self,
        words: &Store,
        owner: &str,
        repo: &str,
        skill: &str,
    ) -> Result<Folder, String> {
        let base = format!("{}/repos/{owner}/{repo}", self.api);
        let branch = self.json::<Repo>(words, &base).await?.default_branch;
        if branch.is_empty()
            || !branch
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        {
            return Err(words.fill(
                TEXTS,
                "branch_unplain",
                &[("branch", format!("{branch:?}"))],
            ));
        }
        let sha = self
            .json::<Commit>(words, &format!("{base}/commits/{branch}"))
            .await?
            .sha;
        if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(words.fill(TEXTS, "commit_unplain", &[("sha", format!("{sha:?}"))]));
        }
        let tree: Tree = self
            .json(words, &format!("{base}/git/trees/{sha}?recursive=1"))
            .await?;
        let mut found: Vec<&str> = tree
            .tree
            .iter()
            .filter(|n| n.kind == "blob")
            .filter_map(|n| n.path.strip_suffix("/SKILL.md"))
            .filter(|dir| dir.rsplit('/').next() == Some(skill))
            .collect();
        found.sort_by_key(|dir| (dir.matches('/').count(), dir.len()));
        let Some(path) = found.first().map(|p| (*p).to_owned()) else {
            let key = if tree.truncated {
                "repository_too_large"
            } else {
                "no_skill_folder"
            };
            return Err(words.fill(
                TEXTS,
                key,
                &[
                    ("repository", format!("{owner}/{repo}")),
                    ("skill", skill.to_owned()),
                ],
            ));
        };
        let prefix = format!("{path}/");
        let mut files = Vec::new();
        for node in tree.tree.iter().filter(|n| n.path.starts_with(&prefix)) {
            let relative = &node.path[prefix.len()..];
            let refused = |key: &str| words.fill(TEXTS, key, &[("path", relative.to_owned())]);
            match (node.kind.as_str(), node.mode.as_str()) {
                ("tree", _) => {}
                (_, "120000") => return Err(refused("has_a_link")),
                ("commit", _) => return Err(refused("has_a_repository")),
                ("blob", _) if guard::inside(relative) => files.push(Listed {
                    path: relative.to_owned(),
                    size: node.size,
                }),
                _ => return Err(refused("path_leads_out")),
            }
        }
        Ok(Folder { sha, path, files })
    }

    /// The file at `path` in `owner/repo` at commit `sha`, at most `most`
    /// bytes of it, or why not, in `words`.
    pub async fn file(
        &self,
        words: &Store,
        owner: &str,
        repo: &str,
        sha: &str,
        path: &str,
        most: usize,
    ) -> Result<Vec<u8>, String> {
        let wrong = |why: String| words.fill(TEXTS, "raw_address_wrong", &[("why", why)]);
        let mut url = reqwest::Url::parse(&self.raw).map_err(|e| wrong(e.to_string()))?;
        url.path_segments_mut()
            .map_err(|()| words.text(TEXTS, "raw_address_no_path").to_owned())?
            .pop_if_empty()
            .extend([owner, repo, sha])
            .extend(path.split('/'));
        let answer = self
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| {
                words.fill(
                    TEXTS,
                    "unfetched",
                    &[("path", path.to_owned()), ("why", e.to_string())],
                )
            })?;
        read(words, answer, most).await
    }
}
