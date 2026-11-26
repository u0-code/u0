//! A repository's pull requests on Azure DevOps, over its REST API. Its
//! API gives no patch, so a pull request's changes are read from git:
//! the two commits it was last compared at fetched, and compared as GitHub
//! does.

use std::path::Path;

use serde_json::{Value, json};

use super::{Host, api, int, items, list, said, text};
use crate::github::{Act, Answer, Ask, Detail, Item, Kind};

/// A repository on Azure DevOps: its organisation, project and name.
pub struct Repo {
    org: String,
    project: String,
    name: String,
}

impl Repo {
    /// The repository a remote at `host`/`path` names, where it is on Azure
    /// DevOps: `dev.azure.com/org/project/_git/repo`,
    /// `ssh.dev.azure.com:v3/org/project/repo` or
    /// `org.visualstudio.com/project/_git/repo`.
    pub fn of(host: &str, path: &str) -> Option<Self> {
        let parts: Vec<&str> = path.split('/').filter(|p| *p != "_git").collect();
        let (org, rest) = match host {
            "dev.azure.com" => (parts.first()?.to_string(), &parts[1..]),
            "ssh.dev.azure.com" => (parts.get(1)?.to_string(), &parts[2..]),
            _ => (
                host.strip_suffix(".visualstudio.com")?.to_owned(),
                &parts[..],
            ),
        };
        match rest {
            [project, name] => Some(Self {
                org,
                project: (*project).to_owned(),
                name: (*name).to_owned(),
            }),
            _ => None,
        }
    }

    fn api(&self) -> String {
        format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullrequests",
            self.org, self.project, self.name
        )
    }

    fn page(&self, n: u64) -> String {
        format!(
            "https://dev.azure.com/{}/{}/_git/{}/pullrequest/{n}",
            self.org, self.project, self.name
        )
    }
}

/// Answer `ask` of `repo`, cloned in `dir`.
pub fn answer(dir: &Path, repo: &Repo, kind: Kind, ask: Ask) -> Answer {
    if kind == Kind::Issue {
        let none = "Azure Boards are not read here; bind the project to a Linear team";
        return match ask {
            Ask::List { .. } => Answer::Listed(Err(none.into())),
            _ => Answer::Acted(None, Err(none.into())),
        };
    }
    let base = repo.api();
    let call = |method: &str, rest: &str, body: Option<Value>| {
        let join = if rest.contains('?') { '&' } else { '?' };
        let url = format!("{base}{rest}{join}api-version=7.1");
        api(Host::Azure, method, &url, body.as_ref())
    };
    let one = |v: &Value| {
        let mut item = item(v);
        item.url = repo.page(item.number);
        item
    };
    match ask {
        Ask::List { state, .. } => {
            let status = match state.as_str() {
                "open" => "active",
                "merged" => "completed",
                "closed" => "abandoned",
                _ => "all",
            };
            let listed = items(
                call(
                    "GET",
                    &format!("?searchCriteria.status={status}&$top=100"),
                    None,
                ),
                item,
            );
            Answer::Listed(listed.map(|all| {
                all.into_iter()
                    .map(|mut i| {
                        i.url = repo.page(i.number);
                        i
                    })
                    .collect()
            }))
        }
        Ask::View(n) => Answer::Viewed(
            n,
            call("GET", &format!("/{n}"), None).and_then(|v| {
                let threads = call("GET", &format!("/{n}/threads"), None)?;
                let mut d = detail(&v, &threads);
                d.item = one(&v);
                Ok(Box::new(d))
            }),
        ),
        Ask::Diff(n) => Answer::Diffed(
            n,
            call("GET", &format!("/{n}"), None).and_then(|v| {
                // By the commits it was last compared at: a merged one's
                // branch may be gone.
                let from = text(&v, "/lastMergeSourceCommit/commitId");
                let into = text(&v, "/lastMergeTargetCommit/commitId");
                crate::git::yours(dir, "git", &["fetch", "-q", "origin", &from, &into])?;
                let range = format!("{into}...{from}");

                crate::git::yours(dir, "git", &["diff", &range])
            }),
        ),
        Ask::Act(n, act) => {
            let at = format!("/{n}");
            let status = |s: &str| call("PATCH", &at, Some(json!({ "status": s })));
            let done = match act {
                Act::Close => status("abandoned"),
                Act::Reopen => status("active"),
                Act::Ready | Act::Draft => call(
                    "PATCH",
                    &at,
                    Some(json!({"isDraft": matches!(act, Act::Draft)})),
                ),
                Act::Merge { how, delete, .. } => call("GET", &at, None).and_then(|v| {
                    call(
                        "PATCH",
                        &at,
                        Some(json!({
                            "status": "completed",
                            "lastMergeSourceCommit": v["lastMergeSourceCommit"],
                            "completionOptions": {
                                "mergeStrategy": match how.as_str() {
                                    "squash" => "squash",
                                    "rebase" => "rebase",
                                    _ => "noFastForward",
                                },
                                "deleteSourceBranch": delete,
                            },
                        })),
                    )
                }),
                Act::Comment(body) | Act::Review { body, .. } => call(
                    "POST",
                    &format!("{at}/threads"),
                    Some(json!({"comments": [{"content": body, "commentType": 1}], "status": 1})),
                ),
                Act::Edit { title, body } => call(
                    "PATCH",
                    &at,
                    Some(json!({"title": title, "description": body})),
                ),
                _ => Err("Azure DevOps takes this on its own page".into()),
            };
            Answer::Acted(Some(n), done.map(|_| "Done".into()))
        }
        Ask::Create { .. } => Answer::Acted(None, Err("Azure Boards are not written here".into())),
        other => super::not_here("Azure DevOps", other),
    }
}

/// One listed, as Azure DevOps says it (its page is the repository's).
fn item(v: &Value) -> Item {
    let votes: Vec<i64> = v
        .pointer("/reviewers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r["vote"].as_i64())
        .collect();
    Item {
        number: int(v, "/pullRequestId"),
        title: text(v, "/title"),
        state: match text(v, "/status").as_str() {
            "active" => "open".into(),
            "completed" => "merged".into(),
            _ => "closed".into(),
        },
        draft: v["isDraft"].as_bool().unwrap_or(false),
        author: text(v, "/createdBy/displayName"),
        labels: list(v, "/labels", "/name"),
        updated: text(v, "/creationDate"),
        url: String::new(),
        branch: text(v, "/sourceRefName")
            .trim_start_matches("refs/heads/")
            .to_owned(),
        base: text(v, "/targetRefName")
            .trim_start_matches("refs/heads/")
            .to_owned(),
        review: if votes.iter().any(|&v| v < 0) {
            "changes requested".into()
        } else if votes.iter().any(|&v| v > 0) {
            "approved".into()
        } else {
            String::new()
        },
        checks: (0, 0, 0),
        comments: 0,
    }
}

/// One read whole, with what people said in its `threads` (the system's
/// own notes left out).
fn detail(v: &Value, threads: &Value) -> Detail {
    let said: Vec<_> = threads
        .pointer("/value")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|t| t["comments"].as_array().cloned().unwrap_or_default())
        .filter(|c| c["commentType"] == "text")
        .map(|c| {
            said(
                &c,
                "/author/displayName",
                "/publishedDate",
                "commented",
                "/content",
            )
        })
        .collect();
    let mut item = item(v);
    item.comments = said.len();
    Detail {
        item,
        body: text(v, "/description"),
        said,
        checks: Vec::new(),
        assignees: Vec::new(),
        reviewers: list(v, "/reviewers", "/displayName"),
        conflicts: text(v, "/mergeStatus") == "conflicts",
        head: text(v, "/lastMergeSourceCommit/commitId"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_way_of_naming_a_repository_on_azure_devops_is_read() {
        for (host, path) in [
            ("dev.azure.com", "org/proj/_git/repo"),
            ("ssh.dev.azure.com", "v3/org/proj/repo"),
            ("org.visualstudio.com", "proj/_git/repo"),
        ] {
            let r = Repo::of(host, path).unwrap();
            assert_eq!(
                (r.org.as_str(), r.project.as_str(), r.name.as_str()),
                ("org", "proj", "repo")
            );
        }
        assert!(Repo::of("github.com", "a/b").is_none());
    }

    #[test]
    fn a_pull_request_with_a_vote_against_reads_as_changes_requested() {
        let v = json!({"pullRequestId": 5, "title": "T", "status": "active", "isDraft": false,
            "createdBy": {"displayName": "Ann"}, "sourceRefName": "refs/heads/fix",
            "targetRefName": "refs/heads/main", "reviewers": [{"vote": 10}, {"vote": -5}],
            "mergeStatus": "conflicts"});
        let threads = json!({"value": [{"comments": [
            {"author": {"displayName": "Bo"}, "content": "ok", "publishedDate": "t", "commentType": "text"},
            {"author": {"displayName": "Bot"}, "content": "voted", "publishedDate": "t", "commentType": "system"}]}]});
        let d = detail(&v, &threads);
        assert_eq!(
            (d.item.branch.as_str(), d.item.review.as_str()),
            ("fix", "changes requested")
        );
        assert_eq!((d.item.comments, d.conflicts), (1, true));
    }
}
