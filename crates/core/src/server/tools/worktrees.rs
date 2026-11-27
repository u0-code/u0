//! u0's tools for the worktrees it makes of your projects: listed as
//! the Storage page measures them (`git::worktrees` in u0's folders),
//! made as a session's is (`Call::MakeWorktree`), and taken away as tidying
//! takes one (`git::remove_worktree`), never one a session works in.

use std::path::PathBuf;

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{Tools, off, said};
use crate::{git, proto::Event, settings::Settings};

#[derive(Deserialize, schemars::JsonSchema)]
struct ListWorktrees {
    /// The project's folder; none, where this thread works.
    project: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct MakeWorktree {
    /// The project's folder; none, where this thread works.
    project: Option<PathBuf>,
    /// What it is for, in a few words: its branch is named for it.
    about: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct RemoveWorktree {
    /// The worktree, by its path as `list_worktrees` gives it.
    path: PathBuf,
}

#[tool_router(router = worktree_router, vis = "pub(super)")]
impl Tools {
    /// The worktrees u0 made of a project, in its worktrees' folders:
    /// each one's repository, path and branch.
    #[tool]
    async fn list_worktrees(
        &self,
        Parameters(ask): Parameters<ListWorktrees>,
    ) -> Result<String, String> {
        let project = self.folder(ask.project)?;
        let home = self.shared.home.clone();
        let found = off(move || {
            let under = Settings::load(&home).0.worktree_folders(&home);
            (git::project_repos(&project).iter())
                .flat_map(|repo| git::worktrees(repo, &under))
                .collect::<Vec<_>>()
        })
        .await?;
        said(&found)
    }

    /// Make a worktree of a project, on a branch of its own from the
    /// remote's default branch, as a new thread's is: its path and
    /// branch. You are asked first.
    #[tool]
    async fn make_worktree(
        &self,
        Parameters(ask): Parameters<MakeWorktree>,
    ) -> Result<String, String> {
        let project = self.folder(ask.project.clone())?;
        let title = format!("Make a worktree for “{}”", ask.about);
        self.allowed("make_worktree", title, &ask).await?;
        let home = self.shared.home.clone();
        let made = off(move || {
            let settings = Settings::load(&home).0;
            let under = settings.worktree_folders(&home).remove(0);
            let submodules = settings.of(&project).submodules;
            let prefix = &settings.branch_prefix;
            crate::repo::make_worktree(&project, &ask.about, &under, prefix, None, submodules, 0)
        })
        .await?;
        match made {
            Event::Worktree { result, .. } => said(&result?.0),
            _ => Err("No worktree was made".into()),
        }
    }

    /// Take away a worktree u0 made, where no thread of u0's works
    /// in it and nothing in it is uncommitted (git keeps it, and says so);
    /// its branch stays. You are asked first.
    #[tool]
    async fn remove_worktree(
        &self,
        Parameters(ask): Parameters<RemoveWorktree>,
    ) -> Result<String, String> {
        let in_use = (self.shared.index.lock().expect("index").as_ref())
            .is_ok_and(|i| i.sessions.iter().any(|m| m.folder.starts_with(&ask.path)));
        if in_use {
            return Err("A thread of u0's works in it: archive and tidy it instead".into());
        }
        let home = self.shared.home.clone();
        let (path, projects) = (ask.path.clone(), Settings::load(&home).0.folders);
        let found = off(move || {
            let under = Settings::load(&home).0.worktree_folders(&home);
            (projects.iter().flat_map(|p| git::project_repos(p)))
                .flat_map(|repo| git::worktrees(&repo, &under))
                .find(|wt| wt.path == path)
        })
        .await?
        .ok_or("No worktree u0 made of a project of yours is there")?;
        let title = format!("Take away the worktree {}", ask.path.display());
        self.allowed("remove_worktree", title, &ask).await?;
        off(move || git::remove_worktree(&found)).await??;
        Ok("Taken away.".into())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::tests::{answer_card, in_session, meta, repo_at};
    use crate::proto::Call;

    #[test]
    fn a_worktree_is_made_listed_and_taken_away_as_you_allow_but_not_one_worked_in() {
        let dir = tempfile::tempdir().unwrap();
        // As git names it, links read.
        let home = dir.path().canonicalize().unwrap();
        let project = home.join("p");
        repo_at(&project, &[("a.txt", "a")]);
        let settings = json!({"enabled": [], "folders": [project]});
        std::fs::write(home.join("settings.json"), settings.to_string()).unwrap();
        let (server, said, mut client) = in_session(&home, &project);
        client.call("make_worktree", &json!({"about": "Try a thing"}));
        let title = answer_card(&server, &said, "once");
        assert_eq!(title, "Make a worktree for “Try a thing”");
        let (made, failed) = client.result();
        assert!(!failed, "{made}");
        let made: serde_json::Value = serde_json::from_str(&made).unwrap();
        let path = std::path::PathBuf::from(made["path"].as_str().unwrap());
        assert!(path.starts_with(home.join("worktrees")) && path.join("a.txt").exists());
        client.call("list_worktrees", &json!({}));
        let (listed, _) = client.result();
        assert!(listed.contains(&*path.to_string_lossy()), "{listed}");
        // While a session works in it, it stays, and nothing is asked.
        server.handle(Call::Keep(vec![meta(&path, "Work")]));
        client.call("remove_worktree", &json!({"path": path}));
        let (why, failed) = client.result();
        assert!(failed && why.contains("A thread"), "{why}");
        server.handle(Call::Keep(vec![meta(&project, "Work")]));
        client.call("remove_worktree", &json!({"path": path}));
        let title = answer_card(&server, &said, "once");
        assert!(title.starts_with("Take away the worktree"), "{title}");
        assert_eq!(client.result(), ("Taken away.".to_owned(), false));
        assert!(!path.exists());
        client.call("remove_worktree", &json!({"path": project}));
        assert!(client.result().1, "a project is no worktree of u0's");
    }
}
