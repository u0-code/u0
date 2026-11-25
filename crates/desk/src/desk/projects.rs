//! What you set for a project, from its menu in the sidebar: its name,
//! pinned on top or archived out of the list, the agent and the way of
//! working its new sessions take; a project cloned, made, or on another
//! machine; and the scratch sessions that have no project yet, each in a
//! folder of its own until it becomes one.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};

use super::{Desk, Popup};
use crate::settings::Prefs;

/// A project's icon: its own icon file where it has one, else a folder,
/// or a server for one on another machine; never one made up.
pub fn project_icon(folder: &Path, icon: Option<PathBuf>, size: f32) -> gpui::AnyElement {
    use gpui::{IntoElement as _, ParentElement as _, Styled as _};
    match (icon, crate::remote::parse(folder).is_some()) {
        (Some(icon), false) => gpui::div()
            .size(gpui::px(size))
            .flex_none()
            .child(gpui::img(icon).size(gpui::px(size)).rounded(gpui::px(3.)))
            .into_any_element(),
        (_, remote) => crate::ui::icon(if remote { "server" } else { "folder" }, size)
            .flex_none()
            .text_color(crate::theme::muted())
            .into_any_element(),
    }
}

/// Where a project made or cloned in Ghost goes unless you choose
/// another folder: ~/Documents/Ghost, as Codex keeps its own in
/// ~/Documents/Codex. It is made with the first project.
fn made_here() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("/"), PathBuf::from)
        .join("Documents")
        .join("Ghost")
}

/// Adding a project from the palette: the machines your SSH configuration
/// names, and a clone going on, from the address `url` (none for a new
/// project being made).
#[derive(Default)]
pub struct Adding {
    pub hosts: Vec<String>,
    pub cloning: Option<crate::hub::Install>,
    pub url: Option<String>,
}

/// The hosts whose page of a repository (a branch, a file) names it in
/// its first parts: GitLab's up to its `-`, the others' the first two.
const FORGES: [&str; 5] = [
    "github.com",
    "gitlab.com",
    "codeberg.org",
    "bitbucket.org",
    "gitcafe.com",
];

/// A repository to clone, from what was pasted: the address to clone, the
/// host it is on, and its path there (`owner`, `repo`), its name last.
#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub host: Option<String>,
    pub path: Vec<String>,
}

impl Source {
    /// Read what was pasted: a clone address (HTTPS, SSH, a folder), or a
    /// page of a repository on a known host, which clones that
    /// repository's default branch; none for what names no repository.
    pub fn read(typed: &str) -> Option<Self> {
        let typed = typed.trim().trim_end_matches('/');
        let web = ["https://", "http://", "ssh://"]
            .iter()
            .find_map(|p| typed.strip_prefix(p));
        let (host, rest) = match web {
            Some(rest) => {
                let (authority, path) = rest.split_once('/')?;
                let host = authority.rsplit('@').next()?;
                (host.split(':').next(), path)
            }
            None => match typed.split_once(':') {
                Some((who, path)) if who.contains('@') && !path.starts_with('/') => {
                    (who.split_once('@').map(|(_, h)| h), path)
                }
                _ if typed.contains('/') => (None, typed),
                _ => return None,
            },
        };
        let mut path: Vec<String> = rest
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let forge = host.filter(|h| FORGES.contains(h));
        if let Some(h) = forge {
            let end = path.iter().position(|p| p == "-").unwrap_or(path.len());
            path.truncate(if h == "gitlab.com" { end } else { end.min(2) });
        }
        let last = path.last_mut()?;
        *last = last.trim_end_matches(".git").to_owned();
        if last.is_empty() {
            return None;
        }
        let url = match forge {
            Some(h) if web.is_some() && !typed.starts_with("ssh://") => {
                format!("https://{h}/{}.git", path.join("/"))
            }
            _ => typed.to_owned(),
        };
        if host.is_none() {
            path.drain(..path.len() - 1);
        }
        Some(Self {
            url,
            host: host.map(str::to_owned),
            path,
        })
    }

    /// Its name: the folder it is cloned into.
    pub fn name(&self) -> &str {
        self.path.last().map_or("", String::as_str)
    }

    /// Where it is cloned under `base`: in a folder of its name, or, with
    /// `host_folders`, in `<host>/<owner>/<repo>`, as ghq lays them out.
    pub fn dir(&self, base: &Path, host_folders: bool) -> PathBuf {
        match (&self.host, host_folders) {
            (Some(host), true) => self.path.iter().fold(base.join(host), |d, p| d.join(p)),
            _ => base.join(self.name()),
        }
    }
}

/// A project on another machine as typed, `host:/path`: its machine and
/// folder there.
pub fn remote_typed(typed: &str) -> Option<(&str, &str)> {
    let (host, path) = typed.trim().split_once(':')?;
    (!host.is_empty() && path.starts_with('/')).then_some((host, path))
}

impl Desk {
    /// Change what is set for the project `folder`, and keep it.
    pub(crate) fn set_prefs(
        &mut self,
        folder: &Path,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Prefs),
    ) {
        let mut prefs = self.prefs(folder);
        f(&mut prefs);
        if prefs == Prefs::default() {
            self.settings.prefs.remove(folder);
        } else {
            self.settings.prefs.insert(folder.to_path_buf(), prefs);
        }
        self.popup = Popup::Closed;
        self.save_settings();
        cx.notify();
    }

    pub fn toggle_project_pin(&mut self, folder: &Path, cx: &mut Context<Self>) {
        self.set_prefs(folder, cx, |p| p.pinned = !p.pinned);
    }

    /// Archive the project `folder`: out of the list, its sessions with it,
    /// until it is restored from the settings.
    pub fn archive_project(&mut self, folder: &Path, cx: &mut Context<Self>) {
        self.set_prefs(folder, cx, |p| p.archived = true);
    }

    pub fn restore_project(&mut self, folder: &Path, cx: &mut Context<Self>) {
        self.set_prefs(folder, cx, |p| p.archived = false);
    }

    /// The agent new sessions in the project begin with; `None`, the one
    /// on screen or the first that can run.
    pub fn set_project_agent(
        &mut self,
        folder: &Path,
        agent: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.set_prefs(folder, cx, |p| p.agent = agent);
    }

    /// Whether new sessions in the project work in a worktree of their own.
    pub fn set_project_worktree(&mut self, folder: &Path, on: bool, cx: &mut Context<Self>) {
        self.set_prefs(folder, cx, |p| p.worktree = Some(on));
    }

    /// The folder new and cloned projects go in.
    pub fn projects_in(&self) -> PathBuf {
        self.settings.projects_in.clone().unwrap_or_else(made_here)
    }

    /// Clone what was typed, `typed`, into the folder projects go in, its
    /// progress shown; it becomes a project once cloned.
    pub fn start_clone(&mut self, typed: &str, cx: &mut Context<Self>) {
        if let Some(source) = Source::read(typed) {
            let dir = source.dir(&self.projects_in(), self.settings.host_folders);
            self.clone_into(source.url, dir, cx);
        }
    }

    /// Clone `url` into `dir`, unless a clone goes on already.
    pub fn clone_into(&mut self, url: String, dir: PathBuf, cx: &mut Context<Self>) {
        if self.adding.cloning.is_some() {
            return;
        }
        self.adding.cloning = Some(crate::hub::Install {
            what: "Starting".into(),
            share: None,
        });
        self.adding.url = Some(url.clone());
        self.transport.send(crate::proto::Call::Clone { url, dir });
        cx.notify();
    }

    pub fn cancel_clone(&mut self, cx: &mut Context<Self>) {
        if self.adding.cloning.is_some() {
            self.transport.send(crate::proto::Call::CancelClone);
        }
        cx.notify();
    }

    /// Make the new project `name` in the folder projects go in, and
    /// (`publish`) a private repository on GitHub for it.
    pub fn create_project(&mut self, name: &str, publish: bool, cx: &mut Context<Self>) {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.adding.url = None;
        self.transport.send(crate::proto::Call::Create {
            dir: self.projects_in().join(&name),
            name,
            publish,
        });
        cx.notify();
    }

    /// Add the project typed as `host:/path`, on a machine reached over
    /// SSH; its sessions are those of the agents there.
    pub fn add_remote(&mut self, typed: &str, cx: &mut Context<Self>) {
        if let Some((host, path)) = remote_typed(typed) {
            self.adopt(&crate::remote::project(host, path));
        }
        cx.notify();
    }

    /// The folders the agents have sessions in that are not projects yet:
    /// how many sessions each, and which agents; most first.
    pub(crate) fn found_elsewhere(&self) -> Vec<(PathBuf, usize, Vec<String>)> {
        let projects = self.projects();
        let mut found = std::collections::BTreeMap::<PathBuf, (usize, Vec<String>)>::new();
        for (agent, folders) in &self.elsewhere {
            for (folder, n) in folders {
                if projects.contains(folder) || self.settings.prefs.contains_key(folder) {
                    continue;
                }
                let at = found.entry(folder.clone()).or_default();
                at.0 += n;
                at.1.push(agent.clone());
            }
        }
        let mut found: Vec<_> = found.into_iter().map(|(f, (n, a))| (f, n, a)).collect();
        found.sort_by_key(|f| std::cmp::Reverse(f.1));
        found
    }

    /// Ask for the folder new and cloned projects go in.
    pub fn choose_parent(cx: &mut Context<Self>) {
        Self::ask_folder(
            "Put them here",
            |d, folder| {
                d.settings.projects_in = Some(folder);
                d.save_settings();
            },
            cx,
        );
    }

    /// Ask for a folder, the button saying `prompt`, and hand the one
    /// chosen to `then`.
    pub fn ask_folder(prompt: &str, then: fn(&mut Self, PathBuf), cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.to_owned().into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    then(this, folder);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Make `dir` a project, listed with the agents' sessions in it.
    pub(crate) fn adopt(&mut self, dir: &Path) {
        self.settings.used(dir);
        self.save_settings();
        let agents: Vec<String> = self.slots.iter().map(|s| s.entry.id.clone()).collect();
        for agent in agents {
            self.list_sessions(&agent);
        }
        self.page = super::Page::Thread;
        self.current = None;
    }

    /// Fetch each project here from its remote, as often as you said
    /// (`fetch_every`), those you said pulled on their default branch; at
    /// the first, those you said pulled even with fetching off.
    pub(crate) fn refresh_projects(&mut self) {
        let first = self.fetched.is_none();
        let due = self.settings.fetch_every.is_some_and(|m| {
            self.fetched
                .is_none_or(|t| t.elapsed().as_secs() >= u64::from(m) * 60)
        });
        if !due && !first {
            return;
        }
        self.fetched = Some(std::time::Instant::now());
        let projects: Vec<(PathBuf, bool)> = self
            .projects()
            .into_iter()
            .filter(|p| crate::remote::parse(p).is_none() && !self.is_scratch(p))
            .map(|p| {
                let pull = self.prefs(&p).pull;
                (p, pull)
            })
            .filter(|(_, pull)| due || *pull)
            .collect();
        if !projects.is_empty() {
            self.transport
                .send(crate::proto::Call::Refresh { projects });
        }
    }

    /// Where scratch sessions are begun: one folder each, under the home.
    pub fn scratch_dir(&self) -> PathBuf {
        self.home.join("scratch")
    }

    /// Whether `folder` is a scratch session's, not yet named: the sidebar
    /// lists its sessions together under Scratch, as t3code does.
    pub fn is_scratch(&self, folder: &Path) -> bool {
        folder.parent() == Some(self.scratch_dir().as_path()) && self.prefs(folder).name.is_none()
    }

    /// The folder the sidebar lists `folder`'s sessions under: Scratch
    /// for a scratch session's, else its own.
    pub fn listed_under(&self, folder: &Path) -> PathBuf {
        if self.is_scratch(folder) {
            self.scratch_dir()
        } else {
            folder.to_path_buf()
        }
    }

    /// Begin a session in a folder of its own, with no project yet: it is
    /// listed under Scratch until you name it, which makes it a project where
    /// it is, as t3code does.
    pub fn new_scratch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(folder) = self.scratch() {
            self.new_in(folder, window, cx);
        }
    }

    /// Make a scratch folder, listed as Scratch: none when it could not be
    /// made.
    pub(crate) fn scratch(&mut self) -> Option<PathBuf> {
        let at = crate::when::stamp(std::time::SystemTime::now());
        let folder = self.scratch_dir().join(&at);
        if let Err(e) = std::fs::create_dir_all(&folder) {
            tracing::warn!("a scratch folder could not be made: {e}");
            return None;
        }
        self.settings.used(&folder);
        self.popup = Popup::Closed;
        self.save_settings();
        Some(folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_of_a_repository_clones_the_repository_into_its_host_folders() {
        let s =
            Source::read("https://github.com/zed-industries/zed/tree/main/crates/gpui").unwrap();
        assert_eq!(s.url, "https://github.com/zed-industries/zed.git");
        assert_eq!(
            s.dir(Path::new("/g"), true),
            Path::new("/g/github.com/zed-industries/zed")
        );
        assert_eq!(s.dir(Path::new("/g"), false), Path::new("/g/zed"));
        let s = Source::read("https://gitlab.com/group/sub/app/-/blob/main/x.rs").unwrap();
        assert_eq!(s.url, "https://gitlab.com/group/sub/app.git");
        let s = Source::read("git@github.com:o/r.git").unwrap();
        assert_eq!((s.url.as_str(), s.name()), ("git@github.com:o/r.git", "r"));
        assert_eq!(s.dir(Path::new("/g"), true), Path::new("/g/github.com/o/r"));
        let s = Source::read("/Users/me/code/thing.git").unwrap();
        assert_eq!(s.dir(Path::new("/g"), true), Path::new("/g/thing"));
        assert_eq!(Source::read("just words"), None);
    }
}
