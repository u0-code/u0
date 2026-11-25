//! A repository's pull requests on Bitbucket Cloud, over its REST API.
//! Bitbucket keeps no issues Ghost reads: a project's issues there come
//! from Linear, where it is bound to a team.

use std::fmt::Write as _;

use serde_json::{Value, json};

use super::{Host, api, int, items, list, said, text, words};
use crate::github::{Act, Answer, Ask, Detail, Item, Kind};

/// Answer `ask` of the repository at `path` (workspace/repository).
pub fn answer(path: &str, kind: Kind, ask: Ask) -> Answer {
    if kind == Kind::Issue {
        let none = "Bitbucket's issues are not read here; bind the project to a Linear team";
        return match ask {
            Ask::List { .. } => Answer::Listed(Err(none.into())),
            _ => Answer::Acted(None, Err(none.into())),
        };
    }
    let base = format!("https://api.bitbucket.org/2.0/repositories/{path}/pullrequests");
    let call = |method: &str, rest: &str, body: Option<Value>| {
        api(
            Host::Bitbucket,
            method,
            &format!("{base}{rest}"),
            body.as_ref(),
        )
    };
    match ask {
        Ask::List { state, search } => {
            let states: &[&str] = match state.as_str() {
                "open" => &["OPEN"],
                "merged" => &["MERGED"],
                "closed" => &["DECLINED", "SUPERSEDED"],
                _ => &["OPEN", "MERGED", "DECLINED", "SUPERSEDED"],
            };
            let mut query = String::from("?pagelen=50");
            for s in states {
                let _ = write!(query, "&state={s}");
            }
            let (_, words) = words(&search);
            if !words.is_empty() {
                let _ = write!(
                    query,
                    "&q={}",
                    super::escape(&format!("title ~ \"{}\"", words.replace('"', "")))
                );
            }
            Answer::Listed(items(call("GET", &query, None), item))
        }
        Ask::View(n) => Answer::Viewed(
            n,
            call("GET", &format!("/{n}"), None).and_then(|one| {
                let comments = call("GET", &format!("/{n}/comments?pagelen=100"), None)?;
                Ok(Box::new(detail(&one, &comments)))
            }),
        ),
        Ask::Diff(n) => Answer::Diffed(
            n,
            super::raw(Host::Bitbucket, &format!("{base}/{n}/diff")).map(|t| crate::git::parse(&t)),
        ),
        Ask::Act(n, act) => {
            let at = format!("/{n}");
            let done = match act {
                Act::Merge { how, delete, .. } => call(
                    "POST",
                    &format!("{at}/merge"),
                    Some(json!({
                        "merge_strategy": match how.as_str() {
                            "squash" => "squash",
                            "rebase" => "fast_forward",
                            _ => "merge_commit",
                        },
                        "close_source_branch": delete,
                    })),
                ),
                Act::Close => call("POST", &format!("{at}/decline"), None),
                Act::Comment(body) => call(
                    "POST",
                    &format!("{at}/comments"),
                    Some(json!({"content": {"raw": body}})),
                ),
                Act::Review { verdict, body } => {
                    let said = if body.is_empty() {
                        Ok(Value::Null)
                    } else {
                        call(
                            "POST",
                            &format!("{at}/comments"),
                            Some(json!({"content": {"raw": body}})),
                        )
                    };
                    said.and_then(|_| match verdict.as_str() {
                        "approve" => call("POST", &format!("{at}/approve"), None),
                        "request-changes" => call("POST", &format!("{at}/request-changes"), None),
                        _ => Ok(Value::Null),
                    })
                }
                Act::Edit { title, body } => call(
                    "PUT",
                    &at,
                    Some(json!({"title": title, "description": body})),
                ),
                _ => Err("Bitbucket takes this on its own page".into()),
            };
            Answer::Acted(Some(n), done.map(|_| "Done".into()))
        }
        Ask::Create { .. } => Answer::Acted(None, Err("Bitbucket keeps no issues here".into())),
        other => super::not_here("Bitbucket", other),
    }
}

/// One listed, as Bitbucket says it.
fn item(v: &Value) -> Item {
    let approved = v
        .pointer("/participants")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| p["approved"].as_bool().unwrap_or(false))
        .count();
    Item {
        number: int(v, "/id"),
        title: text(v, "/title"),
        state: match text(v, "/state").as_str() {
            "OPEN" => "open".into(),
            "MERGED" => "merged".into(),
            _ => "closed".into(),
        },
        draft: v["draft"].as_bool().unwrap_or(false),
        author: text(v, "/author/display_name"),
        labels: Vec::new(),
        updated: text(v, "/updated_on"),
        url: text(v, "/links/html/href"),
        branch: text(v, "/source/branch/name"),
        base: text(v, "/destination/branch/name"),
        review: if approved > 0 {
            "approved".into()
        } else {
            String::new()
        },
        checks: (0, 0, 0),
        comments: int(v, "/comment_count") as usize,
    }
}

/// One read whole, with its `comments`.
fn detail(v: &Value, comments: &Value) -> Detail {
    Detail {
        item: item(v),
        body: text(v, "/description"),
        said: comments
            .pointer("/values")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|c| !c["deleted"].as_bool().unwrap_or(false))
            .map(|c| {
                said(
                    c,
                    "/user/display_name",
                    "/created_on",
                    "commented",
                    "/content/raw",
                )
            })
            .collect(),
        checks: Vec::new(),
        assignees: Vec::new(),
        reviewers: list(v, "/reviewers", "/display_name"),
        conflicts: false,
        head: text(v, "/source/commit/hash"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declined_pull_request_reads_as_closed_with_its_approvals() {
        let v = json!({"id": 3, "title": "Quote", "state": "DECLINED", "draft": false,
            "author": {"display_name": "Ann"}, "updated_on": "t",
            "links": {"html": {"href": "u"}}, "source": {"branch": {"name": "fix"}, "commit": {"hash": "abc"}},
            "destination": {"branch": {"name": "main"}}, "comment_count": 1,
            "participants": [{"approved": true}], "reviewers": [{"display_name": "Bo"}],
            "description": "why"});
        let comments = json!({"values": [{"user": {"display_name": "Cy"}, "created_on": "t",
            "content": {"raw": "ok"}, "deleted": false}]});
        let d = detail(&v, &comments);
        assert_eq!(
            (d.item.state.as_str(), d.item.review.as_str()),
            ("closed", "approved")
        );
        assert_eq!((d.said[0].who.as_str(), d.head.as_str()), ("Cy", "abc"));
        assert_eq!(d.reviewers, ["Bo"]);
    }
}
