//! A Linear team's issues, as the Issues page shows and changes them, over
//! Linear's GraphQL API with your API key. A project is bound to a team
//! by its key (`ENG`); an issue is numbered within it (`ENG-12` is 12).

use serde_json::{Value, json};

use super::{Host, api, int, items, list, said, text, words};
use crate::github::{Act, Answer, Ask, Detail, Item};

/// The fields of an issue the list shows.
const FIELDS: &str = "number title url updatedAt state { type } creator { name } labels { nodes { name } } comments { nodes { id } }";

/// Ask Linear `query` with `variables`; its `data`.
fn ask(query: &str, variables: &Value) -> Result<Value, String> {
    let body = json!({ "query": query, "variables": variables });
    let answer = api(
        Host::Linear,
        "POST",
        "https://api.linear.app/graphql",
        Some(&body),
    )?;
    match answer.pointer("/errors/0/message").and_then(Value::as_str) {
        Some(why) => Err(format!("Linear: {why}")),
        None => Ok(answer["data"].clone()),
    }
}

/// Answer `ask` of the issues of the team keyed `team`.
pub fn answer(team: &str, ask_: Ask) -> Answer {
    let id = |n: u64| format!("{team}-{n}");
    match ask_ {
        Ask::List { state, search } => {
            let mut filter = json!({ "team": { "key": { "eq": team } } });
            let done = json!(["completed", "canceled"]);
            match state.as_str() {
                "open" => filter["state"] = json!({ "type": { "nin": done } }),
                "closed" => filter["state"] = json!({ "type": { "in": done } }),
                _ => {}
            }
            let (mine, words) = words(&search);
            match mine {
                Some("author") => filter["creator"] = json!({ "isMe": { "eq": true } }),
                Some(_) => filter["assignee"] = json!({ "isMe": { "eq": true } }),
                None => {}
            }
            if !words.is_empty() {
                filter["title"] = json!({ "containsIgnoreCase": words });
            }
            let query = format!(
                "query($filter: IssueFilter) {{ issues(first: 100, filter: $filter, orderBy: updatedAt) {{ nodes {{ {FIELDS} }} }} }}"
            );
            Answer::Listed(items(
                ask(&query, &json!({ "filter": filter })).map(|d| d["issues"]["nodes"].clone()),
                item,
            ))
        }
        Ask::View(n) => {
            let query = format!(
                "query($id: String!) {{ issue(id: $id) {{ {FIELDS} description assignee {{ name }} comments {{ nodes {{ body createdAt user {{ name }} }} }} }} }}"
            );
            Answer::Viewed(n, ask(&query, &json!({ "id": id(n) })).map(|d| Box::new(detail(&d["issue"]))))
        }
        Ask::Diff(n) => Answer::Diffed(n, Ok(Vec::new())),
        Ask::Act(n, act) => {
            let update = |input: Value| {
                ask(
                    "mutation($id: String!, $input: IssueUpdateInput!) { issueUpdate(id: $id, input: $input) { success } }", &json!({ "id": id(n), "input": input }),
                )
            };
            let done = match act {
                Act::Comment(body) => ask(
                    "mutation($input: CommentCreateInput!) { commentCreate(input: $input) { success } }", &json!({ "input": { "issueId": id(n), "body": body } }),
                ),
                Act::Edit { title, body } => update(json!({ "title": title, "description": body })),
                Act::Close | Act::Reopen => {
                    let kind = if matches!(act, Act::Close) { "completed" } else { "unstarted" };
                    state_of(team, kind).and_then(|state| update(json!({ "stateId": state })))
                }
                _ => Err("Linear takes this on its own page".into()),
            };
            Answer::Acted(Some(n), done.map(|_| "Done".into()))
        }
        Ask::Create { title, body } => Answer::Acted(
            None,
            ask(
                "query($key: String!) { teams(filter: { key: { eq: $key } }) { nodes { id } } }", &json!({ "key": team }),
            )
            .and_then(|d| {
                let team = text(&d, "/teams/nodes/0/id");
                ask(
                    "mutation($input: IssueCreateInput!) { issueCreate(input: $input) { issue { url } } }", &json!({ "input": { "teamId": team, "title": title, "description": body } }),
                )
            })
            .map(|d| text(&d, "/issueCreate/issue/url")),
        ),
    }
}

/// The id of the team's first workflow state of `kind` (completed,
/// unstarted).
fn state_of(team: &str, kind: &str) -> Result<String, String> {
    let d = ask(
        "query($key: String!, $type: String!) { workflowStates(filter: { team: { key: { eq: $key } }, type: { eq: $type } }) { nodes { id } } }",
        &json!({ "key": team, "type": kind }),
    )?;
    Some(text(&d, "/workflowStates/nodes/0/id"))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{team} has no {kind} state"))
}

/// One listed, as Linear says it.
fn item(v: &Value) -> Item {
    Item {
        number: int(v, "/number"),
        title: text(v, "/title"),
        state: match text(v, "/state/type").as_str() {
            "completed" | "canceled" => "closed".into(),
            _ => "open".into(),
        },
        draft: false,
        author: text(v, "/creator/name"),
        labels: list(v, "/labels/nodes", "/name"),
        updated: text(v, "/updatedAt"),
        url: text(v, "/url"),
        branch: String::new(),
        base: String::new(),
        review: String::new(),
        checks: (0, 0, 0),
        comments: v
            .pointer("/comments/nodes")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    }
}

/// One read whole.
fn detail(v: &Value) -> Detail {
    Detail {
        item: item(v),
        body: text(v, "/description"),
        said: v
            .pointer("/comments/nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|c| said(c, "/user/name", "/createdAt", "commented", "/body"))
            .collect(),
        checks: Vec::new(),
        assignees: [text(v, "/assignee/name")]
            .into_iter()
            .filter(|a| !a.is_empty())
            .collect(),
        reviewers: Vec::new(),
        conflicts: false,
        head: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linear_issue_reads_as_an_issue_does() {
        let v = json!({"number": 12, "title": "Crash", "url": "u", "updatedAt": "t",
            "state": {"type": "canceled"}, "creator": {"name": "Ann"},
            "labels": {"nodes": [{"name": "bug"}]}, "description": "why",
            "assignee": {"name": "Bo"},
            "comments": {"nodes": [{"body": "seen", "createdAt": "t", "user": {"name": "Cy"}}]}});
        let d = detail(&v);
        assert_eq!((d.item.number, d.item.state.as_str()), (12, "closed"));
        assert_eq!(
            (d.item.labels.clone(), d.item.comments),
            (vec!["bug".to_owned()], 1)
        );
        assert_eq!(
            (d.assignees.clone(), d.said[0].who.as_str()),
            (vec!["Bo".to_owned()], "Cy")
        );
    }
}
