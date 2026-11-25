//! A project's pull requests and issues on GitHub, as `gh` lists, reads and
//! changes them: the lists the PRs and Issues pages show, one read whole
//! (what was said, its checks, its files), and what can be done to it.
//! Each is one [`Call::Forge`](crate::proto::Call), answered as one
//! [`Event::Forge`](crate::proto::Event).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git::{self, FileDiff};

/// Pull requests or issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Pr,
    Issue,
}

impl Kind {
    fn gh(self) -> &'static str {
        match self {
            Self::Pr => "pr",
            Self::Issue => "issue",
        }
    }
}

/// One listed: a pull request or an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub number: u64,
    pub title: String,
    /// open, closed, merged.
    pub state: String,
    pub draft: bool,
    pub author: String,
    pub labels: Vec<String>,
    pub updated: String,
    pub url: String,
    /// A pull request's branch and the one it goes into.
    pub branch: String,
    pub base: String,
    /// approved, changes requested, review required; empty for none.
    pub review: String,
    /// Its checks: passed, failed, still going.
    pub checks: (usize, usize, usize),
    pub comments: usize,
}

/// Something said on it: by whom, when, how (a comment, a review's
/// verdict), and the words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Said {
    pub who: String,
    pub when: String,
    pub how: String,
    pub body: String,
}

/// A check of a pull request: its name, how it stands, where it ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub state: String,
    pub url: String,
}

/// One read whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub item: Item,
    pub body: String,
    pub said: Vec<Said>,
    pub checks: Vec<Check>,
    pub assignees: Vec<String>,
    pub reviewers: Vec<String>,
    /// It conflicts with its base.
    pub conflicts: bool,
    /// The commit its branch is at, to tell when it moved.
    pub head: String,
}

/// What is asked of a project's pull requests or issues.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ask {
    /// Those in `state` (open, closed, merged, all) that `search` finds,
    /// as GitHub's search reads it (`author:@me`, `label:bug`).
    List {
        state: String,
        search: String,
    },
    View(u64),
    /// A pull request's changes.
    Diff(u64),
    /// Do `Act` to the one numbered.
    Act(u64, Act),
    /// A new issue.
    Create {
        title: String,
        body: String,
    },
}

/// What can be done to a pull request or an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Act {
    /// Merge it: squash, merge or rebase; when its checks pass (auto);
    /// its branch deleted after.
    Merge {
        how: String,
        auto: bool,
        delete: bool,
    },
    Close,
    Reopen,
    Ready,
    Draft,
    Comment(String),
    /// approve, request-changes or comment, with words.
    Review {
        verdict: String,
        body: String,
    },
    Edit {
        title: String,
        body: String,
    },
    Label(String),
    Reviewer(String),
    Assignee(String),
    /// Its branch checked out where the project is.
    Checkout,
    /// Run the failed jobs of a run of its checks again, by the run's id.
    Rerun(String),
}

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    Listed(Result<Vec<Item>, String>),
    Viewed(u64, Result<Box<Detail>, String>),
    Diffed(u64, Result<Vec<FileDiff>, String>),
    /// Done to the one numbered (none, a new issue): what gh said, or why not.
    Acted(Option<u64>, Result<String, String>),
}

/// The fields gh is asked for, as each kind has them.
fn fields(kind: Kind, whole: bool) -> &'static str {
    match (kind, whole) {
        (Kind::Pr, false) => {
            "number,title,state,isDraft,author,labels,updatedAt,url,headRefName,baseRefName,reviewDecision,statusCheckRollup,comments"
        }
        (Kind::Pr, true) => {
            "number,title,state,isDraft,author,labels,updatedAt,url,headRefName,baseRefName,reviewDecision,statusCheckRollup,comments,body,reviews,assignees,reviewRequests,mergeable,headRefOid"
        }
        (Kind::Issue, false) => "number,title,state,author,labels,updatedAt,url,comments",
        (Kind::Issue, true) => {
            "number,title,state,author,labels,updatedAt,url,comments,body,assignees"
        }
    }
}

/// Answer `ask` of the pull requests or issues of the project in `dir`.
pub fn answer(dir: &Path, kind: Kind, ask: Ask) -> Answer {
    let gh = |args: &[&str]| git::yours(dir, "gh", args);
    match ask {
        Ask::List { state, search } => {
            let mut args = vec![kind.gh(), "list", "--limit", "100", "--state", &state];
            args.extend(["--json", fields(kind, false)]);
            if !search.trim().is_empty() {
                args.extend(["--search", search.trim()]);
            }
            Answer::Listed(gh(&args).and_then(|t| {
                let raw: Vec<Raw> = serde_json::from_str(&t).map_err(|e| e.to_string())?;
                Ok(raw.into_iter().map(Raw::item).collect())
            }))
        }
        Ask::View(n) => {
            let number = n.to_string();
            let read = gh(&[kind.gh(), "view", &number, "--json", fields(kind, true)]);
            Answer::Viewed(
                n,
                read.and_then(|t| serde_json::from_str::<Raw>(&t).map_err(|e| e.to_string()))
                    .map(|raw| Box::new(raw.detail())),
            )
        }
        Ask::Diff(n) => Answer::Diffed(
            n,
            gh(&["pr", "diff", &n.to_string()]).map(|t| git::parse(&t)),
        ),
        Ask::Act(n, act) => {
            let number = n.to_string();
            let args = act_args(kind, &number, &act);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            Answer::Acted(Some(n), gh(&args))
        }
        Ask::Create { title, body } => Answer::Acted(
            None,
            gh(&["issue", "create", "--title", &title, "--body", &body]),
        ),
    }
}

/// What gh is told to do `act` to the one numbered `n`.
fn act_args(kind: Kind, n: &str, act: &Act) -> Vec<String> {
    let k = kind.gh();
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match act {
        Act::Merge { how, auto, delete } => {
            let mut a = words(&["pr", "merge", n, &format!("--{how}")]);
            if *auto {
                a.push("--auto".into());
            }
            if *delete {
                a.push("--delete-branch".into());
            }
            a
        }
        Act::Close => words(&[k, "close", n]),
        Act::Reopen => words(&[k, "reopen", n]),
        Act::Ready => words(&["pr", "ready", n]),
        Act::Draft => words(&["pr", "ready", n, "--undo"]),
        Act::Comment(body) => words(&[k, "comment", n, "--body", body]),
        Act::Review { verdict, body } => {
            words(&["pr", "review", n, &format!("--{verdict}"), "--body", body])
        }
        Act::Edit { title, body } => words(&[k, "edit", n, "--title", title, "--body", body]),
        Act::Label(label) => words(&[k, "edit", n, "--add-label", label]),
        Act::Reviewer(who) => words(&["pr", "edit", n, "--add-reviewer", who]),
        Act::Assignee(who) => words(&[k, "edit", n, "--add-assignee", who]),
        Act::Checkout => words(&["pr", "checkout", n]),
        Act::Rerun(run) => words(&["run", "rerun", run, "--failed"]),
    }
}

/// What gh says of one, as it says it.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Raw {
    number: u64,
    title: String,
    state: String,
    is_draft: bool,
    author: Who,
    labels: Vec<Named>,
    updated_at: String,
    url: String,
    head_ref_name: String,
    base_ref_name: String,
    review_decision: String,
    status_check_rollup: Vec<RawCheck>,
    comments: Vec<RawSaid>,
    body: String,
    reviews: Vec<RawSaid>,
    assignees: Vec<Who>,
    review_requests: Vec<Who>,
    mergeable: String,
    head_ref_oid: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Who {
    login: String,
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Named {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawCheck {
    name: String,
    context: String,
    conclusion: String,
    state: String,
    status: String,
    details_url: String,
    target_url: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawSaid {
    author: Who,
    body: String,
    created_at: String,
    submitted_at: String,
    state: String,
}

impl RawCheck {
    /// How it stands, in one word: its conclusion, else its state.
    fn word(&self) -> String {
        [&self.conclusion, &self.state, &self.status]
            .into_iter()
            .find(|w| !w.is_empty())
            .cloned()
            .unwrap_or_default()
            .to_lowercase()
    }
}

/// Where a check's word puts it: passed, failed, or still going.
fn standing(word: &str) -> usize {
    match word {
        "success" | "neutral" | "skipped" | "completed" => 0,
        "failure" | "error" | "cancelled" | "timed_out" | "action_required" | "startup_failure" => {
            1
        }
        _ => 2,
    }
}

impl Raw {
    fn item(self) -> Item {
        let mut checks = (0, 0, 0);
        for c in &self.status_check_rollup {
            match standing(&c.word()) {
                0 => checks.0 += 1,
                1 => checks.1 += 1,
                _ => checks.2 += 1,
            }
        }
        Item {
            number: self.number,
            title: self.title,
            state: self.state.to_lowercase(),
            draft: self.is_draft,
            author: self.author.login,
            labels: self.labels.into_iter().map(|l| l.name).collect(),
            updated: self.updated_at,
            url: self.url,
            branch: self.head_ref_name,
            base: self.base_ref_name,
            review: self.review_decision.to_lowercase().replace('_', " "),
            checks,
            comments: self.comments.len(),
        }
    }

    fn detail(mut self) -> Detail {
        let said = |s: RawSaid, how: &str| Said {
            who: if s.author.login.is_empty() {
                s.author.name
            } else {
                s.author.login
            },
            when: if s.created_at.is_empty() {
                s.submitted_at
            } else {
                s.created_at
            },
            how: if s.state.is_empty() {
                how.to_owned()
            } else {
                s.state.to_lowercase().replace('_', " ")
            },
            body: s.body,
        };
        let mut all: Vec<Said> = std::mem::take(&mut self.comments)
            .into_iter()
            .map(|s| said(s, "commented"))
            .chain(
                std::mem::take(&mut self.reviews)
                    .into_iter()
                    .map(|s| said(s, "reviewed")),
            )
            .collect();
        all.sort_by(|a, b| a.when.cmp(&b.when));
        let checks = self
            .status_check_rollup
            .iter()
            .map(|c| Check {
                name: if c.name.is_empty() {
                    c.context.clone()
                } else {
                    c.name.clone()
                },
                state: c.word(),
                url: if c.details_url.is_empty() {
                    c.target_url.clone()
                } else {
                    c.details_url.clone()
                },
            })
            .collect();
        let body = std::mem::take(&mut self.body);
        let assignees = std::mem::take(&mut self.assignees)
            .into_iter()
            .map(|w| w.login)
            .collect();
        let reviewers = std::mem::take(&mut self.review_requests)
            .into_iter()
            .map(|w| w.login)
            .collect();
        let conflicts = self.mergeable == "CONFLICTING";
        let head = std::mem::take(&mut self.head_ref_oid);
        Detail {
            item: self.item(),
            body,
            said: all,
            checks,
            assignees,
            reviewers,
            conflicts,
            head,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_request_reads_with_its_checks_counted_and_what_was_said_in_order() {
        let raw: Raw = serde_json::from_str(
            r#"{"number":7,"title":"Fix","state":"OPEN","isDraft":true,"author":{"login":"ann"},
            "labels":[{"name":"bug"}],"updatedAt":"2026-11-25T10:00:00Z","url":"u","headRefName":"fix",
            "baseRefName":"main","reviewDecision":"CHANGES_REQUESTED","mergeable":"CONFLICTING",
            "statusCheckRollup":[{"name":"build","conclusion":"SUCCESS"},{"name":"test","conclusion":"FAILURE"},
            {"context":"ci/x","state":"PENDING"}],
            "comments":[{"author":{"login":"bo"},"body":"later","createdAt":"2026-11-25T12:00:00Z"}],
            "reviews":[{"author":{"login":"cy"},"body":"no","submittedAt":"2026-11-25T11:00:00Z","state":"CHANGES_REQUESTED"}],
            "body":"why","headRefOid":"abc"}"#,
        )
        .unwrap();
        let d = raw.detail();
        assert_eq!(d.item.checks, (1, 1, 1));
        assert_eq!(d.item.review, "changes requested");
        assert_eq!(
            (d.item.state.as_str(), d.item.draft, d.conflicts),
            ("open", true, true)
        );
        assert_eq!(
            d.said.iter().map(|s| s.who.as_str()).collect::<Vec<_>>(),
            ["cy", "bo"]
        );
        assert_eq!(d.said[0].how, "changes requested");
        assert_eq!(d.checks[2].name, "ci/x");
    }

    #[test]
    fn what_is_done_to_one_is_said_to_gh_as_gh_takes_it() {
        let merge = Act::Merge {
            how: "squash".into(),
            auto: true,
            delete: true,
        };
        assert_eq!(
            act_args(Kind::Pr, "7", &merge),
            ["pr", "merge", "7", "--squash", "--auto", "--delete-branch"]
        );
        assert_eq!(
            act_args(Kind::Issue, "3", &Act::Close),
            ["issue", "close", "3"]
        );
        assert_eq!(
            act_args(Kind::Pr, "7", &Act::Draft),
            ["pr", "ready", "7", "--undo"]
        );
    }
}
