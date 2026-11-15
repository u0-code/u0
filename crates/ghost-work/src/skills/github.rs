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

    async fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let answer = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| format!("GitHub could not be asked ({e}). Try again later."))?;
        serde_json::from_slice(&read(answer, ANSWER).await?)
            .map_err(|e| format!("GitHub answered with something unreadable ({e})."))
    }

    /// The folder called `skill` in `owner/repo`, at the commit its
    /// default branch points at now. The folder is the one of that name
    /// with a `SKILL.md` in it; of several, the one nearest the root.
    pub async fn folder(&self, owner: &str, repo: &str, skill: &str) -> Result<Folder, String> {
        let base = format!("{}/repos/{owner}/{repo}", self.api);
        let branch = self.json::<Repo>(&base).await?.default_branch;
        if branch.is_empty()
            || !branch
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        {
            return Err(format!(
                "GitHub named {branch:?} as the default branch, which is not a plain name."
            ));
        }
        let sha = self
            .json::<Commit>(&format!("{base}/commits/{branch}"))
            .await?
            .sha;
        if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "GitHub named {sha:?} as the commit, which is not one."
            ));
        }
        let tree: Tree = self
            .json(&format!("{base}/git/trees/{sha}?recursive=1"))
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
            return Err(if tree.truncated {
                format!(
                    "{owner}/{repo} is too large to look through, and no folder called {skill} with a SKILL.md was found in what could be."
                )
            } else {
                format!("{owner}/{repo} has no folder called {skill} with a SKILL.md in it.")
            });
        };
        let prefix = format!("{path}/");
        let mut files = Vec::new();
        for node in tree.tree.iter().filter(|n| n.path.starts_with(&prefix)) {
            let relative = &node.path[prefix.len()..];
            match (node.kind.as_str(), node.mode.as_str()) {
                ("tree", _) => {}
                (_, "120000") => {
                    return Err(format!(
                        "it has a link, {relative}, and a skill may only have files of its own."
                    ));
                }
                ("commit", _) => {
                    return Err(format!(
                        "it has another repository inside it, {relative}, and a skill may only have files of its own."
                    ));
                }
                ("blob", _) if guard::inside(relative) => files.push(Listed {
                    path: relative.to_owned(),
                    size: node.size,
                }),
                _ => {
                    return Err(format!(
                        "it has a path, {relative}, that leads out of its folder."
                    ));
                }
            }
        }
        Ok(Folder { sha, path, files })
    }

    /// The file at `path` in `owner/repo` at commit `sha`, at most `most`
    /// bytes of it.
    pub async fn file(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
        path: &str,
        most: usize,
    ) -> Result<Vec<u8>, String> {
        let mut url = reqwest::Url::parse(&self.raw)
            .map_err(|e| format!("the address of GitHub's files is wrong ({e})."))?;
        url.path_segments_mut()
            .map_err(|()| "the address of GitHub's files is wrong.".to_owned())?
            .pop_if_empty()
            .extend([owner, repo, sha])
            .extend(path.split('/'));
        let answer = self
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| format!("{path} could not be fetched ({e})."))?;
        read(answer, most).await
    }
}
