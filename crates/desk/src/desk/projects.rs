//! What you set for a project, from its menu in the sidebar: its name,
//! pinned on top or archived out of the list, the agent and the way of
//! working its new sessions take; and the scratch sessions that have no
//! project yet, each in a folder of its own until it becomes one.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use gpui::{AppContext as _, Context, Entity, Focusable as _, Window};

use super::{Desk, Popup};
use crate::{
    input::{Submitted, TextInput},
    settings::Prefs,
};

/// Where a project made or cloned in Ghost goes unless you choose
/// another folder: ~/Documents/Ghost, as Codex keeps its own in
/// ~/Documents/Codex. It is made with the first project.
fn made_here() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("/"), PathBuf::from)
        .join("Documents")
        .join("Ghost")
}

/// Adding a project: the address to clone, the name of one to make, a
/// project on another machine typed as `host:/path` and the machines your
/// SSH configuration names; the folder a clone or a new one goes in, a
/// clone going on and what went wrong.
pub struct Adding {
    pub url: Entity<TextInput>,
    pub name: Entity<TextInput>,
    pub remote: Entity<TextInput>,
    pub hosts: Vec<String>,
    pub parent: PathBuf,
    /// A clone going on, and the switch that stops it.
    pub cloning: Option<(crate::hub::Install, Arc<AtomicBool>)>,
    pub trouble: Option<String>,
}

impl Adding {
    pub(super) fn new(cx: &mut Context<Desk>) -> Self {
        let url = cx.new(|cx| {
            TextInput::new(cx, "https://github.com/owner/repo.git")
                .padded(6.)
                .keeping()
        });
        cx.subscribe(&url, |this, _, Submitted(_), cx| this.start_clone(cx))
            .detach();
        let name = cx.new(|cx| TextInput::new(cx, "my-project").padded(6.).keeping());
        cx.subscribe(&name, |this, _, Submitted(_), cx| this.create_project(cx))
            .detach();
        let remote = cx.new(|cx| {
            TextInput::new(cx, "devbox:/home/me/app")
                .padded(6.)
                .keeping()
        });
        cx.subscribe(&remote, |this, _, Submitted(_), cx| this.add_remote(cx))
            .detach();
        Self {
            url,
            name,
            remote,
            hosts: Vec::new(),
            parent: made_here(),
            cloning: None,
            trouble: None,
        }
    }
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

    /// Clone the address typed into a folder of its name under the folder
    /// chosen, with its progress shown; it becomes a project once cloned.
    pub fn start_clone(&mut self, cx: &mut Context<Self>) {
        let url = self.adding.url.read(cx).text().trim().to_owned();
        let name = url
            .trim_end_matches('/')
            .rsplit(['/', ':'])
            .next()
            .unwrap_or_default()
            .trim_end_matches(".git")
            .to_owned();
        if url.is_empty() || name.is_empty() || self.adding.cloning.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let install = crate::hub::Install {
            what: "Starting".into(),
            share: None,
        };
        self.adding.cloning = Some((install, stop.clone()));
        self.adding.trouble = None;
        crate::hub::clone(url, self.adding.parent.join(name), stop, self.tx.clone());
        cx.notify();
    }

    pub fn cancel_clone(&mut self, cx: &mut Context<Self>) {
        if let Some((_, stop)) = &self.adding.cloning {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Make a new project of the name typed under the folder chosen.
    pub fn create_project(&mut self, cx: &mut Context<Self>) {
        let name = self.adding.name.read(cx).text().trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.adding.trouble = None;
        crate::hub::create(self.adding.parent.join(&name), name, self.tx.clone());
        cx.notify();
    }

    /// Add the project typed as `host:/path`, on a machine reached over
    /// SSH; its sessions are those of the agents there.
    pub fn add_remote(&mut self, cx: &mut Context<Self>) {
        let typed = self.adding.remote.read(cx).text();
        match typed.trim().split_once(':') {
            Some((host, path)) if !host.is_empty() && path.starts_with('/') => {
                self.adding.remote.update(cx, |i, cx| i.set_text("", cx));
                self.adding.trouble = None;
                self.adopt(&crate::remote::project(host, path));
            }
            _ => {
                self.adding.trouble =
                    Some("Write the machine and the folder on it as host:/path.".into());
            }
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
        found.sort_by(|a, b| b.1.cmp(&a.1));
        found
    }

    /// Begin typing a project on `host`.
    pub fn pick_host(&mut self, host: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.adding
            .remote
            .update(cx, |i, cx| i.set_text(&format!("{host}:/"), cx));
        window.focus(&self.adding.remote.focus_handle(cx));
    }

    /// Ask for the folder new and cloned projects go in.
    pub fn choose_parent(cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Put it here".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.adding.parent = folder;
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

    /// Where scratch sessions are begun: one folder each, under the home.
    pub fn scratch_dir(&self) -> PathBuf {
        self.home.join("scratch")
    }

    /// Begin a session in a folder of its own, with no project yet: it is
    /// listed as Scratch until you name it, which makes it a project where
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
        self.settings.prefs.insert(
            folder.clone(),
            Prefs {
                name: Some(format!("Scratch {at}")),
                ..Prefs::default()
            },
        );
        self.popup = Popup::Closed;
        self.save_settings();
        Some(folder)
    }
}
