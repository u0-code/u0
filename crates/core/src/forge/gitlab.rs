//! A project's merge requests and issues on GitLab (gitlab.com or your
//! own), over its REST API: what the PRs and Issues pages show and do.

use std::fmt::Write as _;

use serde_json::{Value, json};

use super::{api, int, items, list, said, text, words};

use crate::github::{Act, Answer, Ask, Check, Detail, Item, Kind, Said, Thread};

/// Answer `ask` of the project at `path` on the GitLab at `host`.
pub fn answer(host: &str, path: &str, kind: Kind, ask: Ask) -> Answer {
    let base = format!("https://{host}/api/v4/projects/{}", super::escape(path));
    let what = match kind {
        Kind::Pr => "merge_requests",
        Kind::Issue => "issues",
    };
    let call = |method: &str, rest: &str, body: Option<Value>| {
        api(
            super::Host::GitLab,
            method,
            &format!("{base}/{what}{rest}"),
            body.as_ref(),
        )
    };
    match ask {
        Ask::List { state, search } => {
            let (mine, words) = words(&search);
            let state = match state.as_str() {
                "open" => "opened",
                other => other,
            };
            let mut query = format!("?per_page=100&state={state}");
            if let Some(scope) = mine {
                query += match scope {
                    "author" => "&scope=created_by_me",
                    _ => "&scope=assigned_to_me",
                };
            }
            if !words.is_empty() {
                let _ = write!(query, "&search={}", super::escape(&words));
            }
            Answer::Listed(items(call("GET", &query, None), item))
        }
        Ask::View(n) => Answer::Viewed(
            n,
            call("GET", &format!("/{n}"), None).map(|one| {
                // GitLab shows notes only to someone signed in: without a
                // token, it reads with none.
                let notes = call("GET", &format!("/{n}/discussions?per_page=100"), None)
                    .unwrap_or_default();
                Box::new(detail(&one, &notes))
            }),
        ),
        Ask::Diff(n) => Answer::Diffed(
            n,
            call("GET", &format!("/{n}/diffs?per_page=100"), None).map(|files| patches(&files)),
        ),
        Ask::Act(n, act) => {
            let at = format!("/{n}");
            let done = match act {
                Act::Merge {
                    how, auto, delete, ..
                } => call(
                    "PUT",
                    &format!("{at}/merge"),
                    Some(json!({
                        "squash": how == "squash",
                        "should_remove_source_branch": delete,
                        "merge_when_pipeline_succeeds": auto,
                    })),
                ),
                Act::Close => call("PUT", &at, Some(json!({"state_event": "close"}))),
                Act::Reopen => call("PUT", &at, Some(json!({"state_event": "reopen"}))),
                Act::Ready | Act::Draft => {
                    let draft = matches!(act, Act::Draft);
                    call("GET", &at, None).and_then(|one| {
                        let title = text(&one, "/title");
                        let rest = title.trim_start_matches("Draft:").trim_start().to_owned();
                        let title = if draft {
                            format!("Draft: {rest}")
                        } else {
                            rest
                        };
                        call("PUT", &at, Some(json!({ "title": title })))
                    })
                }
                Act::Comment(body) => call(
                    "POST",
                    &format!("{at}/notes"),
                    Some(json!({ "body": body })),
                ),
                Act::Review {
                    verdict,
                    body,
                    lines,
                } => {
                    // Each comment on a line a discussion of its own, placed
                    // by the commits the merge request's diff is between.
                    let placed = if lines.is_empty() {
                        Ok(Value::Null)
                    } else {
                        call("GET", &at, None).and_then(|one| {
                            let refs = &one["diff_refs"];
                            for l in &lines {
                                call(
                                    "POST",
                                    &format!("{at}/discussions"),
                                    Some(json!({"body": l.body, "position": {
                                        "position_type": "text",
                                        "base_sha": refs["base_sha"],
                                        "start_sha": refs["start_sha"],
                                        "head_sha": refs["head_sha"],
                                        "old_path": l.path,
                                        "new_path": l.path,
                                        "new_line": l.line,
                                    }})),
                                )?;
                            }
                            Ok(Value::Null)
                        })
                    };
                    let said = if let Err(e) = placed {
                        Err(e)
                    } else if body.is_empty() {
                        Ok(Value::Null)
                    } else {
                        call(
                            "POST",
                            &format!("{at}/notes"),
                            Some(json!({ "body": body })),
                        )
                    };
                    said.and_then(|_| match verdict.as_str() {
                        "approve" => call("POST", &format!("{at}/approve"), None),
                        _ => Ok(Value::Null),
                    })
                }
                Act::Edit { title, body } => call(
                    "PUT",
                    &at,
                    Some(json!({"title": title, "description": body})),
                ),
                Act::Reply { thread, body } => call(
                    "POST",
                    &format!("{at}/discussions/{thread}/notes"),
                    Some(json!({ "body": body })),
                ),
                Act::Resolve { thread, resolved } => call(
                    "PUT",
                    &format!("{at}/discussions/{thread}"),
                    Some(json!({ "resolved": resolved })),
                ),
                Act::Label(label) => call("PUT", &at, Some(json!({ "add_labels": label }))),
                Act::Reviewer(_) | Act::Assignee(_) | Act::Checkout | Act::Rerun(_) => {
                    Err("GitLab takes this on its own page".into())
                }
            };
            Answer::Acted(Some(n), done.map(|_| "Done".into()))
        }
        Ask::Create { title, body } => Answer::Acted(
            None,
            call(
                "POST",
                "",
                Some(json!({"title": title, "description": body})),
            )
            .map(|v| text(&v, "/web_url")),
        ),
        other => super::not_here("GitLab", other),
    }
}

/// One listed, as GitLab says it.
fn item(v: &Value) -> Item {
    let mut checks = (0, 0, 0);
    let pipeline = text(v, "/head_pipeline/status");
    if !pipeline.is_empty() {
        match crate::github::standing(&pipeline) {
            0 => checks.0 = 1,
            1 => checks.1 = 1,
            _ => checks.2 = 1,
        }
    }
    Item {
        number: int(v, "/iid"),
        title: text(v, "/title"),
        state: match text(v, "/state").as_str() {
            "opened" => "open".into(),
            other => other.into(),
        },
        draft: v["draft"].as_bool().unwrap_or(false),
        author: text(v, "/author/username"),
        labels: list(v, "/labels", ""),
        updated: text(v, "/updated_at"),
        url: text(v, "/web_url"),
        branch: text(v, "/source_branch"),
        base: text(v, "/target_branch"),
        review: match text(v, "/detailed_merge_status").as_str() {
            "not_approved" => "review required".into(),
            _ => String::new(),
        },
        checks,
        comments: int(v, "/user_notes_count") as usize,
    }
}

/// One read whole, with what was said on it in its `discussions`: those on
/// a line its threads, the rest said on it (GitLab's own notes of what
/// changed left out).
fn detail(v: &Value, discussions: &Value) -> Detail {
    let pipeline = text(v, "/head_pipeline/status");
    let (on_lines, rest): (Vec<&Value>, Vec<&Value>) = (discussions.as_array().into_iter())
        .flatten()
        .partition(|d| !d.pointer("/notes/0/position").is_none_or(Value::is_null));
    let notes = |d: &Value| -> Vec<Said> {
        (d["notes"].as_array().into_iter().flatten())
            .filter(|n| !n["system"].as_bool().unwrap_or(false))
            .map(|n| said(n, "/author/username", "/created_at", "commented", "/body"))
            .collect()
    };
    Detail {
        item: item(v),
        body: text(v, "/description"),
        said: rest.iter().flat_map(|d| notes(d)).collect(),
        threads: on_lines
            .iter()
            .map(|d| Thread {
                id: text(d, "/id"),
                path: text(d, "/notes/0/position/new_path"),
                line: d
                    .pointer("/notes/0/position/new_line")
                    .and_then(Value::as_u64),
                resolved: d.pointer("/notes/0/resolved").and_then(Value::as_bool) == Some(true),
                outdated: false,
                said: notes(d),
            })
            .collect(),
        checks: (!pipeline.is_empty())
            .then(|| Check {
                name: "pipeline".into(),
                state: pipeline,
                url: text(v, "/head_pipeline/web_url"),
            })
            .into_iter()
            .collect(),
        assignees: list(v, "/assignees", "/username"),
        reviewers: list(v, "/reviewers", "/username"),
        conflicts: v["has_conflicts"].as_bool().unwrap_or(false),
        head: text(v, "/sha"),
    }
}

/// A merge request's patch: each file's hunks, as GitLab gives them, under
/// the header git would write.
fn patches(files: &Value) -> String {
    let mut whole = String::new();
    for f in files.as_array().into_iter().flatten() {
        let (old, new) = (text(f, "/old_path"), text(f, "/new_path"));
        let from = if f["new_file"].as_bool().unwrap_or(false) {
            "/dev/null".to_owned()
        } else {
            format!("a/{old}")
        };
        let to = if f["deleted_file"].as_bool().unwrap_or(false) {
            "/dev/null".to_owned()
        } else {
            format!("b/{new}")
        };
        let _ = write!(
            whole,
            "diff --git a/{old} b/{new}\n--- {from}\n+++ {to}\n{}",
            text(f, "/diff")
        );
        if !whole.ends_with('\n') {
            whole.push('\n');
        }
    }
    whole
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_merge_request_reads_as_a_pull_request_does() {
        let v = json!({"iid": 7, "title": "Fix", "state": "opened", "draft": true,
            "author": {"username": "ann"}, "labels": ["bug"], "updated_at": "2026-11-25T10:00:00Z",
            "web_url": "u", "source_branch": "fix", "target_branch": "main",
            "detailed_merge_status": "not_approved", "user_notes_count": 2,
            "head_pipeline": {"status": "failed", "web_url": "p"}, "has_conflicts": true,
            "description": "why", "sha": "abc", "assignees": [{"username": "bo"}], "reviewers": []});
        let notes = json!([
            {"id": "d1", "notes": [{"author": {"username": "cy"}, "body": "ok", "created_at": "t", "system": false}]},
            {"id": "d2", "notes": [{"author": {"username": "cy"}, "body": "added 1 commit", "created_at": "t", "system": true}]},
            {"id": "d3", "notes": [{"author": {"username": "bo"}, "body": "why?", "created_at": "t", "system": false,
                "resolved": true, "position": {"new_path": "a.rs", "new_line": 4}}]}]);
        let d = detail(&v, &notes);
        assert_eq!(
            (d.threads.len(), d.threads[0].id.as_str(), d.threads[0].line),
            (1, "d3", Some(4))
        );
        assert!(d.threads[0].resolved);
        assert_eq!(
            (d.item.number, d.item.state.as_str(), d.item.draft),
            (7, "open", true)
        );
        assert_eq!(d.item.checks, (0, 1, 0));
        assert_eq!(d.item.review, "review required");
        assert_eq!(d.said.len(), 1);
        assert_eq!(d.assignees, ["bo"]);
        assert!(d.conflicts);
    }

    #[test]
    fn a_merge_requests_files_read_as_git_writes_them() {
        let files = json!([{"old_path": "a.rs", "new_path": "a.rs", "new_file": false, "deleted_file": false,
            "diff": "@@ -1 +1 @@\n-x\n+y\n"}, {"old_path": "b.rs", "new_path": "b.rs", "new_file": true,
            "deleted_file": false, "diff": "@@ -0,0 +1 @@\n+z\n"}]);
        assert_eq!(
            patches(&files),
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-x\n+y\n\
             diff --git a/b.rs b/b.rs\n--- /dev/null\n+++ b/b.rs\n@@ -0,0 +1 @@\n+z\n"
        );
    }
}
