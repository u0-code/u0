//! u0's tools for a project's issues, from where the Issues page reads
//! them (`forge::answer`, what `Call::Forge` asks): the host of its remote
//! (GitHub, GitLab, Bitbucket, Azure DevOps), or the Linear team the
//! project is bound to. Listed and read by any caller where it may read;
//! opened, commented on, closed and reopened by a session, asked of you
//! first.

use std::path::{Path, PathBuf};

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{
    Tools,
    forge::{acted, named},
    off, said,
};
use crate::{
    github::{Act, Answer, Ask, Item, Kind},
    settings::Settings,
};

#[derive(Deserialize, schemars::JsonSchema)]
struct ListIssues {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    /// open (none), closed or all.
    state: Option<String>,
    /// As the host's search reads it (`label:bug`, `assignee:@me`).
    #[serde(default)]
    search: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct Numbered {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct CreateIssue {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    title: String,
    body: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct CommentOnIssue {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
    body: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct SetIssueState {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
    /// Open it again; else close it.
    #[serde(default)]
    open: bool,
}

/// The Linear team the project `folder` is in reads its issues from, where
/// it is bound to one: the nearest project holding it.
fn team(home: &Path, folder: &Path) -> Option<String> {
    let settings = Settings::load(home).0;
    (settings.prefs.iter())
        .filter(|(project, _)| folder.starts_with(project))
        .max_by_key(|(project, _)| project.as_os_str().len())
        .and_then(|(_, prefs)| prefs.linear.clone())
}

/// The issue `number` of the project `folder`, as it is now: what its card
/// names. Asked only of an agent in a session of u0's, for none other
/// may act.
async fn about(tools: &Tools, folder: PathBuf, number: u64) -> Result<Item, String> {
    tools.session().ok_or(super::OUTSIDE)?;
    let Answer::Viewed(_, read) = tools.issues(folder, Ask::View(number)).await? else {
        return Err("The host said nothing of it".into());
    };
    Ok(read?.item)
}

impl Tools {
    /// What the host of the project `folder`'s issues answers to `ask`.
    async fn issues(&self, folder: PathBuf, ask: Ask) -> Result<Answer, String> {
        let linear = team(&self.shared.home, &folder);
        off(move || crate::forge::answer(&folder, Kind::Issue, ask, linear.as_deref())).await
    }
}

#[tool_router(router = issue_router, vis = "pub(super)")]
impl Tools {
    /// A project's issues, from where u0 reads them (the host of its
    /// remote, or its Linear team): each one's number, title, state,
    /// author, labels and address.
    #[tool]
    async fn list_issues(&self, Parameters(ask): Parameters<ListIssues>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        let state = ask.state.unwrap_or_else(|| "open".into());
        if !["open", "closed", "all"].contains(&state.as_str()) {
            return Err("state is open, closed or all".into());
        }
        let search = ask.search;
        match self.issues(folder, Ask::List { state, search }).await? {
            Answer::Listed(listed) => said(&listed?),
            _ => Err("The host listed nothing".into()),
        }
    }

    /// An issue read whole: what it says, what was said on it, who it is
    /// assigned to.
    #[tool]
    async fn read_issue(&self, Parameters(ask): Parameters<Numbered>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        match self.issues(folder, Ask::View(ask.number)).await? {
            Answer::Viewed(_, read) => said(&read?),
            _ => Err("The host said nothing of it".into()),
        }
    }

    /// Open a new issue. You are asked first.
    #[tool]
    async fn create_issue(
        &self,
        Parameters(ask): Parameters<CreateIssue>,
    ) -> Result<String, String> {
        let folder = self.folder(ask.folder.clone())?;
        let title = format!("Open the issue “{}” in {}", ask.title, folder.display());
        self.allowed("create_issue", title, &ask).await?;
        let create = Ask::Create {
            title: ask.title,
            body: ask.body,
        };
        match self.issues(folder, create).await? {
            Answer::Acted(_, done) => done,
            _ => Err("The host said nothing of it".into()),
        }
    }

    /// Comment on an issue. You are asked first.
    #[tool]
    async fn comment_on_issue(
        &self,
        Parameters(ask): Parameters<CommentOnIssue>,
    ) -> Result<String, String> {
        let folder = self.folder(ask.folder.clone())?;
        let issue = about(self, folder.clone(), ask.number).await?;
        let title = format!("Comment on {}", named("issue", &issue));
        self.allowed("comment_on_issue", title, &ask).await?;
        let act = Act::Comment(ask.body);
        acted(
            self.issues(folder, Ask::Act(ask.number, act)).await?,
            ask.number,
        )
    }

    /// Close an issue, or open it again (`open`). You are asked first.
    #[tool]
    async fn set_issue_state(
        &self,
        Parameters(ask): Parameters<SetIssueState>,
    ) -> Result<String, String> {
        let folder = self.folder(ask.folder.clone())?;
        let issue = about(self, folder.clone(), ask.number).await?;
        let (doing, act) = if ask.open {
            ("Reopen", Act::Reopen)
        } else {
            ("Close", Act::Close)
        };
        let title = format!("{doing} {}", named("issue", &issue));
        self.allowed("set_issue_state", title, &ask).await?;
        acted(
            self.issues(folder, Ask::Act(ask.number, act)).await?,
            ask.number,
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{
        NOT_HERE, OUTSIDE,
        tests::{Client, repo_at},
    };
    use crate::server::tests::server;

    #[test]
    fn a_projects_linear_team_is_its_nearest_projects() {
        let home = tempfile::tempdir().unwrap();
        let settings = json!({"enabled": [], "folders": ["/p", "/p/inner"], "prefs": {
            "/p": {"linear": "ENG"}, "/p/inner": {}}});
        std::fs::write(home.path().join("settings.json"), settings.to_string()).unwrap();
        let team = |f: &str| super::team(home.path(), std::path::Path::new(f));
        assert_eq!(team("/p/wt/src").as_deref(), Some("ENG"));
        assert_eq!(
            team("/p/inner/x"),
            None,
            "the nearest project is bound to none"
        );
        assert_eq!(team("/q"), None);
    }

    #[test]
    fn an_agent_outside_u0_reads_your_projects_issues_alone_and_changes_none() {
        let home = tempfile::tempdir().unwrap();
        let (project, elsewhere) = (home.path().join("p"), home.path().join("q"));
        repo_at(&project, &[("a.txt", "a")]);
        repo_at(&elsewhere, &[("a.txt", "a")]);
        let settings = json!({"enabled": [], "folders": [project]});
        std::fs::write(home.path().join("settings.json"), settings.to_string()).unwrap();
        let (_server, _said) = server(home.path());
        let mut client = Client::new(home.path(), None);
        for (tool, args) in [
            ("list_issues", json!({"folder": elsewhere})),
            ("read_issue", json!({"folder": elsewhere, "number": 1})),
        ] {
            client.call(tool, &args);
            assert_eq!(client.result(), (NOT_HERE.to_owned(), true), "{tool}");
        }
        for (tool, args) in [
            (
                "create_issue",
                json!({"folder": project, "title": "t", "body": "b"}),
            ),
            (
                "comment_on_issue",
                json!({"folder": project, "number": 1, "body": "b"}),
            ),
            ("set_issue_state", json!({"folder": project, "number": 1})),
        ] {
            client.call(tool, &args);
            assert_eq!(client.result(), (OUTSIDE.to_owned(), true), "{tool}");
        }
    }
}
