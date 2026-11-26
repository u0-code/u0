//! Ghost's tools for your projects: listed and added, as the sidebar
//! has them; and what is in them found, as Ghost finds it: the sessions
//! kept (`Call::Search`), a file by its letters (`repo::paths`), words in
//! the files git sees (`Call::Grep`).

use std::path::PathBuf;

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{Listed, Tools, off, said};
use crate::{git, proto::Event, settings::Settings, store};

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct AddProject {
    /// The folder, by its whole path.
    folder: PathBuf,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct SearchSessions {
    /// What to look for, case aside: in a session's title, and in all
    /// that was said in it.
    query: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct FindFiles {
    /// The folder; none, where this session works.
    folder: Option<PathBuf>,
    /// Letters of the path, in order, as typed to Ghost's file finder
    /// (`smr` finds `src/main.rs`).
    query: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct SearchCode {
    /// The folder; none, where this session works.
    folder: Option<PathBuf>,
    /// What to look for.
    words: String,
    /// In its case; none, case aside.
    #[serde(default)]
    case: bool,
    /// As a whole word.
    #[serde(default)]
    word: bool,
    /// As an extended regular expression.
    #[serde(default)]
    regex: bool,
}

/// Why a folder is not added as a project.
const TOO_WIDE: &str = "That folder is too wide to be a project: not the root, your home folder or a folder holding it, nor Ghost's own";

/// Whether `folder`, as it really is, is too wide to be made a project by
/// an agent: a project is read by agents outside Ghost and devices that
/// may only read, so it is never the root, your home folder or one that
/// holds it, nor Ghost's home (`ghost`: its keys and records) or a folder
/// in it or holding it.
fn too_wide(folder: &std::path::Path, ghost: &std::path::Path) -> bool {
    let real = |p: &std::path::Path| std::fs::canonicalize(p).ok();
    let Some(folder) = real(folder) else {
        return true;
    };
    let yours = std::env::var_os("HOME").and_then(|h| real(h.as_ref()));
    let ghost = real(ghost);
    folder.parent().is_none()
        || yours.is_some_and(|h| h.starts_with(&folder))
        || ghost.is_some_and(|g| g.starts_with(&folder) || folder.starts_with(&g))
}

/// The settings kept, or why they are not read (then none are written).
fn settings(home: &std::path::Path) -> Result<Settings, String> {
    match Settings::load(home) {
        (settings, None) => Ok(settings),
        (_, Some(why)) => Err(why),
    }
}

#[tool_router(router = project_router, vis = "pub(super)")]
impl Tools {
    /// Your projects in Ghost: their folders, as its sidebar lists them.
    #[tool]
    async fn list_projects(&self) -> Result<String, String> {
        said(&settings(&self.shared.home)?.folders)
    }

    /// Make a folder one of your projects in Ghost, first in its list.
    /// You are asked first.
    #[tool]
    async fn add_project(&self, Parameters(ask): Parameters<AddProject>) -> Result<String, String> {
        if !ask.folder.is_absolute() || !ask.folder.is_dir() {
            return Err("Name a folder that is there, by its whole path".into());
        }
        if too_wide(&ask.folder, &self.shared.home) {
            return Err(TOO_WIDE.into());
        }
        let shown = ask.folder.display().to_string();
        self.allowed("add_project", format!("Add the project {shown}"), &ask)
            .await?;
        let mut settings = settings(&self.shared.home)?;
        if settings.folders.contains(&ask.folder) {
            return Ok(format!("{shown} is a project already."));
        }
        settings.used(&ask.folder);
        settings.save(&self.shared.home);
        (self.shared.send)(Event::Settings(Box::new(settings)));
        Ok(format!("{shown} is a project now."))
    }

    /// The sessions Ghost keeps whose title, or what was said in them,
    /// holds `query` (case aside), the latest first; as `list_sessions`
    /// gives them.
    #[tool]
    async fn search_sessions(
        &self,
        Parameters(ask): Parameters<SearchSessions>,
    ) -> Result<String, String> {
        let query = ask.query.trim().to_lowercase();
        if query.is_empty() {
            return Err("Say what to look for".into());
        }
        let index = self.shared.index.lock().expect("index").clone()?;
        let home = self.shared.home.clone();
        let found = off(move || {
            let holds = |m: &&store::Meta| {
                m.title.to_lowercase().contains(&query)
                    || (m.id.as_ref())
                        .is_some_and(|id| store::holds(&home, &m.agent, &id.0, &query))
            };
            Listed::latest(index.sessions.iter().filter(holds))
        })
        .await?;
        said(&found)
    }

    /// The files git sees in a folder whose paths hold the letters of
    /// `query` in order, the best first (fifty at most).
    #[tool]
    async fn find_files(&self, Parameters(ask): Parameters<FindFiles>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        let found = off(move || {
            let files = git::files(&folder);
            (crate::repo::paths(&files, &ask.query).into_iter())
                .take(50)
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>()
        })
        .await?;
        said(&found)
    }

    /// Where words are in the files git sees in a folder: each line found
    /// (`path:line` and the line), fifty at most.
    #[tool]
    async fn search_code(&self, Parameters(ask): Parameters<SearchCode>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        let how = git::Match {
            case: ask.case,
            word: ask.word,
            regex: ask.regex,
        };
        let found = off(move || git::grep(&folder, &ask.words, how)).await?;
        said(&found)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::super::{
        NOT_HERE, OUTSIDE,
        tests::{Client, answer_card, in_session, meta, repo_at},
    };
    use crate::{proto::Call, server::tests::server};

    #[test]
    fn an_agent_outside_ghost_finds_in_your_projects_alone_and_adds_none() {
        let home = tempfile::tempdir().unwrap();
        let (project, elsewhere) = (home.path().join("p"), home.path().join("q"));
        repo_at(&project, &[("src/main.rs", "fn main() { build(); }\n")]);
        repo_at(&elsewhere, &[("secret.rs", "build()\n")]);
        let settings = json!({"enabled": [], "folders": [project]});
        std::fs::write(home.path().join("settings.json"), settings.to_string()).unwrap();
        let record = crate::store::record_path(home.path(), "a", "s1");
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, "{\"You\":\"the test is Flaky\"}\n").unwrap();
        let (server, _said) = server(home.path());
        server.handle(Call::Keep(vec![meta(&project, "Fix the build")]));
        let mut client = Client::new(home.path(), None);
        let mut said = |tool: &str, args: serde_json::Value| {
            client.call(tool, &args);
            client.result()
        };
        let (projects, _) = said("list_projects", json!({}));
        assert!(projects.contains(&*project.to_string_lossy()), "{projects}");
        // By its title, and by what was said in it, case aside.
        for words in ["BUILD", "flaky"] {
            let (found, _) = said("search_sessions", json!({"query": words}));
            assert!(found.contains("Fix the build"), "{words}: {found}");
        }
        let none = said("search_sessions", json!({"query": "none such"}));
        assert_eq!(none.0, "[]");
        let (files, _) = said("find_files", json!({"folder": project, "query": "smr"}));
        assert!(files.contains("src/main.rs"), "{files}");
        let grep = json!({"folder": project, "words": "build()"});
        let (lines, _) = said("search_code", grep);
        assert!(lines.contains("src/main.rs:1"), "{lines}");
        // Not in a project of yours: not read.
        let refused = (NOT_HERE.to_owned(), true);
        let find = json!({"folder": elsewhere, "query": "s"});
        assert_eq!(said("find_files", find), refused);
        let grep = json!({"folder": elsewhere, "words": "build"});
        assert_eq!(said("search_code", grep), refused);
        let away = tempfile::tempdir().unwrap();
        let add = json!({"folder": away.path()});
        assert_eq!(said("add_project", add), (OUTSIDE.to_owned(), true));
    }

    #[test]
    fn a_session_adds_a_project_once_you_allow_it_and_the_desk_is_told() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let other = elsewhere.path().join("other");
        std::fs::create_dir(&other).unwrap();
        let other = other.canonicalize().unwrap();
        let (server, said, mut client) = in_session(home.path(), home.path());
        client.call("add_project", &json!({"folder": other}));
        let title = answer_card(&server, &said, "once");
        assert_eq!(title, format!("Add the project {}", other.display()));
        let (done, failed) = client.result();
        assert!(!failed, "{done}");
        let told = loop {
            if let crate::proto::Event::Settings(s) =
                said.recv_timeout(Duration::from_secs(10)).unwrap()
            {
                break s.folders;
            }
        };
        assert_eq!(told, std::slice::from_ref(&other));
        let kept =
            || std::fs::read_to_string(home.path().join("settings.json")).unwrap_or_default();
        let other_text = other.to_string_lossy().into_owned();
        for _ in 0..250 {
            if kept().contains(&other_text) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(kept().contains(&other_text), "written");
        client.call("add_project", &json!({"folder": "not/whole"}));
        assert!(client.result().1, "a folder by its whole path alone");
        let mut wide = vec![
            std::path::PathBuf::from("/"),
            home.path().to_path_buf(),
            home.path().join("transcripts"),
        ];
        wide.extend(std::env::var_os("HOME").map(std::path::PathBuf::from));
        std::fs::create_dir_all(home.path().join("transcripts")).unwrap();
        for folder in wide {
            client.call("add_project", &json!({"folder": folder}));
            let refused = client.result();
            assert_eq!(
                refused,
                (super::TOO_WIDE.to_owned(), true),
                "{}",
                folder.display()
            );
        }
    }
}
