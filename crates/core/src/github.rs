//! A project's pull requests and issues on GitHub, as `gh` lists, reads and
//! changes them: the lists the PRs and Issues pages show, one read whole
//! (what was said, its checks, its files), and what can be done to it.
//! Each is one [`Call::Forge`](crate::proto::Call), answered as one
//! [`Event::Forge`](crate::proto::Event).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git;

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

impl Check {
    /// Whether it failed.
    pub fn failed(&self) -> bool {
        matches!(
            self.state.as_str(),
            "failure" | "error" | "timed_out" | "cancelled" | "action_required"
        )
    }

    /// The run of GitHub Actions it is part of, by its id (`Ask::Log`,
    /// `Act::Rerun` take it), read from where it ran: none for a check
    /// run elsewhere.
    pub fn run(&self) -> Option<&str> {
        self.url.split("/runs/").nth(1)?.split('/').next()
    }
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
    /// The conversations on its lines.
    pub threads: Vec<Thread>,
}

/// A conversation on a line of a pull request: where (its line on the new
/// side; none where the line is gone), whether it is resolved or its line
/// has changed since, whether only bots spoke in it, and what was said,
/// first to last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub path: String,
    pub line: Option<u64>,
    pub resolved: bool,
    pub outdated: bool,
    pub bots: bool,
    pub said: Vec<Said>,
}

/// A comment on a line of a pull request's new side, in a review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Line {
    pub path: String,
    pub line: u64,
    pub body: String,
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
    /// The labels there are and the people who can be asked to review or
    /// assigned.
    Candidates,
    /// What a failed run of the checks printed, by the run's id.
    Log(String),
    /// The repository's templates for a new issue.
    Templates,
}

/// What can be done to a pull request or an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Act {
    /// Merge it: squash, merge or rebase; when its checks pass (auto);
    /// its branch deleted after; a squash's message with the agents'
    /// credits taken out (uncredit).
    Merge {
        how: String,
        auto: bool,
        delete: bool,
        uncredit: bool,
    },
    Close,
    Reopen,
    Ready,
    Draft,
    Comment(String),
    /// approve, request-changes or comment, with words, and comments on
    /// its lines.
    Review {
        verdict: String,
        body: String,
        lines: Vec<Line>,
    },
    /// Answer the conversation `thread`.
    Reply {
        thread: String,
        body: String,
    },
    /// Resolve the conversation `thread`, or open it again.
    Resolve {
        thread: String,
        resolved: bool,
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
    /// The pull request's whole patch, as git writes it.
    Diffed(u64, Result<String, String>),
    /// Done to the one numbered (none, a new issue): what gh said, or why not.
    Acted(Option<u64>, Result<String, String>),
    /// The labels there are, and the people.
    Candidates(Result<(Vec<String>, Vec<String>), String>),
    /// The end of what a run printed where it failed, by the run's id.
    Logged(String, Result<String, String>),
    /// Each template for a new issue: its name and what it begins with.
    Templates(Vec<(String, String)>),
}

/// The fields gh is asked for, as each kind has them.
fn fields(kind: Kind, whole: bool) -> &'static str {
    match (kind, whole) {
        (Kind::Pr, false) => {
            "number,title,state,isDraft,author,labels,updatedAt,url,headRefName,baseRefName,reviewDecision,statusCheckRollup,comments,isCrossRepository,headRepositoryOwner"
        }
        (Kind::Pr, true) => {
            "number,title,state,isDraft,author,labels,updatedAt,url,headRefName,baseRefName,reviewDecision,statusCheckRollup,comments,isCrossRepository,headRepositoryOwner,body,reviews,assignees,reviewRequests,mergeable,headRefOid"
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
                    .and_then(|raw| {
                        let mut d = raw.detail();
                        if kind == Kind::Pr {
                            let n = format!("n={n}");
                            let query = format!("query={THREADS}");
                            let said = gh(&[
                                "api",
                                "graphql",
                                "-F",
                                "owner={owner}",
                                "-F",
                                "name={repo}",
                                "-F",
                                &n,
                                "-f",
                                &query,
                            ])?;
                            d.threads = threads(&said)?;
                        }
                        Ok(Box::new(d))
                    }),
            )
        }
        Ask::Diff(n) => Answer::Diffed(n, gh(&["pr", "diff", &n.to_string()])),
        Ask::Act(n, act) => {
            let number = n.to_string();
            let mut args = act_args(kind, &number, &act);
            if let Act::Merge {
                how,
                uncredit: true,
                ..
            } = &act
                && how == "squash"
            {
                match squash_message(dir, Some(&number)) {
                    Ok(body) => args.extend(["--body".into(), body]),
                    Err(e) => return Answer::Acted(Some(n), Err(e)),
                }
            }
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            Answer::Acted(Some(n), gh(&args))
        }
        Ask::Create { title, body } => Answer::Acted(
            None,
            gh(&["issue", "create", "--title", &title, "--body", &body]),
        ),
        Ask::Candidates => Answer::Candidates(
            gh(&[
                "label", "list", "--limit", "200", "--json", "name", "-q", ".[].name",
            ])
            .and_then(|labels| {
                let people = gh(&[
                    "api",
                    "repos/{owner}/{repo}/assignees",
                    "--paginate",
                    "-q",
                    ".[].login",
                ])?;
                let lines = |t: &str| t.lines().map(str::to_owned).collect();
                Ok((lines(&labels), lines(&people)))
            }),
        ),
        Ask::Log(run) => {
            let log = gh(&["run", "view", &run, "--log-failed"]);
            // The end of it, where a failure is said.
            let tail = |t: String| {
                let lines: Vec<&str> = t.lines().collect();
                lines[lines.len().saturating_sub(LOG_LINES)..].join("\n")
            };
            Answer::Logged(run, log.map(tail))
        }
        Ask::Templates => Answer::Templates(templates(dir)),
    }
}

/// GitHub's query for a pull request's conversations on its lines.
const THREADS: &str = "query($owner:String!,$name:String!,$n:Int!){repository(owner:$owner,name:$name){pullRequest(number:$n){reviewThreads(first:100){nodes{id isResolved isOutdated path line comments(first:100){nodes{author{login __typename} body createdAt}}}}}}}";

/// The conversations in what GitHub answered `THREADS`.
fn threads(said: &str) -> Result<Vec<Thread>, String> {
    use crate::forge::text;
    let v: serde_json::Value = serde_json::from_str(said).map_err(|e| e.to_string())?;
    let all = |v: &serde_json::Value, p: &str| {
        v.pointer(p)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    Ok(all(&v, "/data/repository/pullRequest/reviewThreads/nodes")
        .iter()
        .map(|t| Thread {
            id: text(t, "/id"),
            path: text(t, "/path"),
            line: t["line"].as_u64(),
            resolved: t["isResolved"].as_bool().unwrap_or(false),
            outdated: t["isOutdated"].as_bool().unwrap_or(false),
            bots: (all(t, "/comments/nodes").iter())
                .all(|c| c.pointer("/author/__typename").and_then(|v| v.as_str()) == Some("Bot")),
            said: all(t, "/comments/nodes")
                .iter()
                .map(|c| crate::forge::said(c, "/author/login", "/createdAt", "commented", "/body"))
                .collect(),
        })
        .collect())
}

/// How many of a failed run's last lines are shown.
const LOG_LINES: usize = 120;

/// The issue templates in `dir`'s `.github/ISSUE_TEMPLATE/`, Markdown
/// ones: each by the name its front matter gives, else its file's, and
/// what follows the front matter.
pub fn templates(dir: &Path) -> Vec<(String, String)> {
    let mut files: Vec<_> = std::fs::read_dir(dir.join(".github/ISSUE_TEMPLATE"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            Some(template(&stem, &text))
        })
        .collect()
}

/// A template's name and body, from its text: front matter between `---`
/// lines (its `name:`), then the body.
fn template(stem: &str, text: &str) -> (String, String) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (stem.to_owned(), text.to_owned());
    };
    let (front, body) = rest.split_once("\n---\n").unwrap_or((rest, ""));
    let name = front
        .lines()
        .find_map(|l| l.strip_prefix("name:"))
        .map_or_else(
            || stem.to_owned(),
            |n| n.trim().trim_matches(['"', '\'']).to_owned(),
        );
    (name, body.trim_start().to_owned())
}

/// What gh is told to do `act` to the one numbered `n`.
fn act_args(kind: Kind, n: &str, act: &Act) -> Vec<String> {
    let k = kind.gh();
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let mut args = match act {
        Act::Merge {
            how, auto, delete, ..
        } => {
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
        Act::Review {
            verdict,
            body,
            lines,
        } => {
            let event = match verdict.as_str() {
                "approve" => "APPROVE",
                "request-changes" => "REQUEST_CHANGES",
                _ => "COMMENT",
            };
            let mut a = words(&[
                "api",
                &format!("repos/{{owner}}/{{repo}}/pulls/{n}/reviews"),
                "--method",
                "POST",
                "-f",
                &format!("event={event}"),
                "-f",
                &format!("body={body}"),
            ]);
            // gh starts another object in the array at each field it has.
            for l in lines {
                a.extend([
                    "-f".into(),
                    format!("comments[][path]={}", l.path),
                    "-F".into(),
                    format!("comments[][line]={}", l.line),
                    "-f".into(),
                    "comments[][side]=RIGHT".into(),
                    "-f".into(),
                    format!("comments[][body]={}", l.body),
                ]);
            }
            a
        }
        Act::Reply { thread, body } => words(&[
            "api",
            "graphql",
            "-f",
            &format!("id={thread}"),
            "-f",
            &format!("body={body}"),
            "-f",
            "query=mutation($id:ID!,$body:String!){addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$id,body:$body}){comment{id}}}",
        ]),
        Act::Resolve { thread, resolved } => {
            let verb = if *resolved {
                "resolveReviewThread"
            } else {
                "unresolveReviewThread"
            };
            words(&[
                "api",
                "graphql",
                "-f",
                &format!("id={thread}"),
                "-f",
                &format!(
                    "query=mutation($id:ID!){{{verb}(input:{{threadId:$id}}){{thread{{id}}}}}}"
                ),
            ])
        }
        Act::Edit { title, body } => words(&[k, "edit", n, "--title", title, "--body", body]),
        Act::Label(label) => words(&[k, "edit", n, "--add-label", label]),
        Act::Reviewer(who) => words(&["pr", "edit", n, "--add-reviewer", who]),
        Act::Assignee(who) => words(&[k, "edit", n, "--add-assignee", who]),
        Act::Checkout => words(&["pr", "checkout", n]),
        Act::Rerun(run) => words(&["run", "rerun", run, "--failed"]),
    };
    // What GitHub's API answers is JSON, not for you: what is said
    // of it done is "done".
    if args[0] == "api" {
        args.insert(1, "--silent".into());
    }
    args
}

/// The message GitHub squashes the pull request numbered `n` (or the
/// branch's in `dir`) with, its agents' credits taken out: one commit's
/// body, or each commit's message listed.
pub fn squash_message(dir: &Path, n: Option<&str>) -> Result<String, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Commit {
        message_headline: String,
        message_body: String,
    }
    #[derive(Deserialize)]
    struct Commits {
        commits: Vec<Commit>,
    }
    let mut args = vec!["pr", "view"];
    args.extend(n);
    args.extend(["--json", "commits"]);
    let read: Commits =
        serde_json::from_str(&git::yours(dir, "gh", &args)?).map_err(|e| e.to_string())?;
    let message = match &read.commits[..] {
        [one] => one.message_body.clone(),
        all => all
            .iter()
            .map(|c| {
                format!("* {}\n\n{}", c.message_headline, c.message_body)
                    .trim()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    Ok(crate::forge::uncredited(&message))
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
    is_cross_repository: bool,
    head_repository_owner: Who,
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
pub(crate) fn standing(word: &str) -> usize {
    match word {
        "success" | "neutral" | "skipped" | "completed" => 0,
        "failure" | "failed" | "error" | "cancelled" | "canceled" | "timed_out"
        | "action_required" | "startup_failure" => 1,

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
            // A fork's branch, by its owner, as GitHub writes it.
            branch: if self.is_cross_repository {
                format!(
                    "{}:{}",
                    self.head_repository_owner.login, self.head_ref_name
                )
            } else {
                self.head_ref_name
            },
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
            threads: Vec::new(),
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
            "body":"why","headRefOid":"abc","isCrossRepository":true,"headRepositoryOwner":{"login":"bo"}}"#,
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
        assert_eq!(d.item.branch, "bo:fix");
    }

    #[test]
    fn an_issue_template_reads_by_the_name_its_front_matter_gives() {
        let text = "---\nname: \"Bug report\"\nabout: Something broke\n---\n\n**What happened**\n";
        assert_eq!(
            template("bug", text),
            ("Bug report".to_owned(), "**What happened**\n".to_owned())
        );
        assert_eq!(
            template("plain", "Say it."),
            ("plain".to_owned(), "Say it.".to_owned())
        );
    }

    #[test]
    fn what_is_done_to_one_is_said_to_gh_as_gh_takes_it() {
        let merge = Act::Merge {
            how: "squash".into(),
            auto: true,
            delete: true,
            uncredit: true,
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

    #[test]
    fn a_review_with_comments_on_lines_is_one_call_each_comment_an_object() {
        let line = |path: &str, line, body: &str| Line {
            path: path.into(),
            line,
            body: body.into(),
        };
        let review = Act::Review {
            verdict: "request-changes".into(),
            body: "Two things".into(),
            lines: vec![line("src/a.rs", 3, "why?"), line("src/b.rs", 9, "typo")],
        };
        let args = act_args(Kind::Pr, "7", &review);
        // What the API answers is not shown as what was done.
        assert_eq!(
            args[1..3],
            ["--silent", "repos/{owner}/{repo}/pulls/7/reviews"]
        );
        assert!(args.contains(&"event=REQUEST_CHANGES".to_owned()));
        let fields: Vec<&str> = (args.iter())
            .filter(|a| a.starts_with("comments"))
            .map(String::as_str)
            .collect();
        assert_eq!(
            fields,
            [
                "comments[][path]=src/a.rs",
                "comments[][line]=3",
                "comments[][side]=RIGHT",
                "comments[][body]=why?",
                "comments[][path]=src/b.rs",
                "comments[][line]=9",
                "comments[][side]=RIGHT",
                "comments[][body]=typo",
            ]
        );
        let reopen = Act::Resolve {
            thread: "T1".into(),
            resolved: false,
        };
        let args = act_args(Kind::Pr, "7", &reopen);
        assert!(args.last().unwrap().contains("unresolveReviewThread"));
        let reply = Act::Reply {
            thread: "T1".into(),
            body: "Fixed".into(),
        };
        assert_eq!(act_args(Kind::Pr, "7", &reply)[..2], ["api", "--silent"]);
        assert_eq!(act_args(Kind::Pr, "7", &Act::Close), ["pr", "close", "7"]);
    }

    #[test]
    fn a_pull_requests_conversations_read_with_where_they_are_and_who_said_what() {
        let said = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[
            {"id":"T1","isResolved":false,"isOutdated":false,"path":"src/a.rs","line":3,
             "comments":{"nodes":[{"author":{"login":"ann","__typename":"User"},"body":"why?","createdAt":"t1"},
                                  {"author":{"login":"bo","__typename":"User"},"body":"because","createdAt":"t2"}]}},
            {"id":"T3","isResolved":false,"isOutdated":false,"path":"src/c.rs","line":1,
             "comments":{"nodes":[{"author":{"login":"lint","__typename":"Bot"},"body":"x","createdAt":"t3"}]}},
            {"id":"T2","isResolved":true,"isOutdated":true,"path":"src/b.rs","line":null,
             "comments":{"nodes":[]}}]}}}}}"#;
        let read = threads(said).unwrap();
        assert_eq!(
            (read[0].line, read[0].said[1].who.as_str()),
            (Some(3), "bo")
        );
        assert_eq!(
            (read[2].line, read[2].resolved, read[2].outdated),
            (None, true, true)
        );
        assert_eq!((read[0].bots, read[1].bots), (false, true));
    }
}
