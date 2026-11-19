//! The index of skills: what it lists, and what its auditors say of each.
//!
//! skills.sh lists skills that people publish in repositories on GitHub,
//! with how many times each has been installed, and has each audited by a
//! few services that rate how risky it looks. Neither is documented as an
//! API; they are what `npx skills find` asks. So what comes back is read
//! leniently, field by field, and anything that is not the shape of a
//! GitHub repository and a skill in it is shown as not installable rather
//! than guessed at. The index vouches for nothing: it is where a skill is
//! found, and its auditors are a second opinion the guards act on.

use std::time::Duration;

use serde_json::Value;

use super::read;
use crate::prompts::{Store, TEXTS};

/// The most bytes of an answer from the index that are read.
const ANSWER: usize = 1024 * 1024;
/// How long one question to the index may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// One skill the index lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// `owner/repo/skill`.
    pub id: String,
    /// The GitHub repository it is in, `owner/repo`.
    pub source: String,
    /// What the index calls the skill: the folder it is looked for in.
    pub skill: String,
    pub installs: u64,
    /// What the index says it is for, if it says.
    pub description: String,
}

/// What one auditor said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub auditor: String,
    /// `safe`, `low`, `medium`, `high`, `critical`, or whatever else the
    /// auditor said, in lowercase.
    pub risk: String,
    pub score: Option<u64>,
}

impl Verdict {
    /// Whether this verdict alone keeps a skill out.
    pub fn refuses(&self) -> bool {
        matches!(self.risk.as_str(), "high" | "critical")
    }
}

/// Where the index is, and how it is asked.
pub struct Index {
    search: String,
    audit: String,
    http: reqwest::Client,
}

impl Index {
    pub fn new(search: &str, audit: &str) -> Self {
        Self {
            search: search.to_owned(),
            audit: audit.to_owned(),
            http: ghost_net::builder()
                .timeout(TIMEOUT)
                .user_agent(concat!("ghost/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Every skill the index lists for `query`, as it lists them. A hit
    /// whose id is not `owner/repo/skill` is kept, with no source, so that
    /// she is told it cannot be installed rather than not shown it.
    /// Why there are none is said in `words`.
    pub async fn search(&self, words: &Store, query: &str) -> Result<Vec<Hit>, String> {
        let answer = self
            .http
            .get(&self.search)
            .query(&[("q", query)])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| words.fill(TEXTS, "index_unreached", &[("why", e.to_string())]))?;
        let bytes = read(words, answer, ANSWER).await?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| words.fill(TEXTS, "index_unreadable", &[("why", e.to_string())]))?;
        Ok(value["skills"]
            .as_array()
            .map(|hits| hits.iter().filter_map(hit).collect())
            .unwrap_or_default())
    }

    /// What the auditors say of `skill` in `source`. `None` when nobody
    /// has said anything: the audit could not be had, or held no verdict.
    pub async fn audit(&self, words: &Store, source: &str, skill: &str) -> Option<Vec<Verdict>> {
        let answer = self
            .http
            .get(&self.audit)
            .query(&[("source", source), ("skills", skill)])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .ok()?;
        let value: Value = serde_json::from_slice(&read(words, answer, ANSWER).await.ok()?).ok()?;
        let verdicts: Vec<Verdict> = value
            .as_object()?
            .iter()
            .filter_map(|(auditor, said)| {
                Some(Verdict {
                    auditor: auditor.clone(),
                    risk: said["risk"].as_str()?.trim().to_ascii_lowercase(),
                    score: said["score"].as_u64(),
                })
            })
            .collect();
        (!verdicts.is_empty()).then_some(verdicts)
    }
}

/// One hit as the index wrote it, if it has an id at all.
fn hit(value: &Value) -> Option<Hit> {
    let id = value["id"].as_str()?.trim().to_owned();
    let installs = value["installs"]
        .as_u64()
        .or_else(|| value["installs"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0);
    let description = value["description"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let (source, skill) = match split(&id) {
        Some((owner, repo, skill)) => {
            let source = format!("{owner}/{repo}");
            // What the index says the source is has to be the repository
            // the id names: a skill is installed from where its id says.
            let said = value["source"].as_str().unwrap_or(&source);
            if said.eq_ignore_ascii_case(&source) {
                (source, skill.to_owned())
            } else {
                (String::new(), skill.to_owned())
            }
        }
        None => (String::new(), String::new()),
    };
    Some(Hit {
        id,
        source,
        skill,
        installs,
        description,
    })
}

/// `owner/repo/skill`, if `id` is that and each part is a plain name: an
/// account and a repository GitHub could have, and a folder name.
pub fn split(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.split('/');
    let (owner, repo, skill) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let plain = |part: &str, extra: &[char]| {
        !part.is_empty()
            && part.len() <= 100
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || extra.contains(&c))
    };
    (plain(owner, &[]) && plain(repo, &['.', '_']) && plain(skill, &['.', '_']))
        .then_some((owner, repo, skill))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_hit_whose_source_is_not_the_repository_its_id_names_cannot_be_installed() {
        let honest = hit(
            &json!({"id": "anthropics/skills/pdf", "source": "anthropics/skills", "installs": 9}),
        )
        .unwrap();
        assert_eq!(honest.source, "anthropics/skills");
        assert_eq!(honest.skill, "pdf");
        assert_eq!(honest.installs, 9);
        let other =
            hit(&json!({"id": "anthropics/skills/pdf", "source": "mallory/skills"})).unwrap();
        assert!(other.source.is_empty());
        for bad in [
            "a/b",
            "a/b/c/d",
            "a/../c",
            "a b/c/d",
            "a/b/.git",
            "https://x/y/z",
        ] {
            assert!(split(bad).is_none(), "{bad}");
        }
    }
}
