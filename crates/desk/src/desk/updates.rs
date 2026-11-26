//! Ghost's own updates, as the window has them: looked for as Ghost
//! starts and every four hours where you let it, and from Check for
//! Updates…; downloaded and put in place on Install; Ghost opened again on
//! Restart.

use std::time::{Duration, Instant};

use gpui::{Context, Window};

use super::{Desk, notices::Way};
use crate::{hub, pages::Tab, update};

/// How often Ghost looks for an update by itself.
const EVERY: Duration = Duration::from_hours(4);

/// Where an update stands.
#[derive(Clone, PartialEq, Eq)]
pub enum Updating {
    /// Not looked for yet.
    Idle,
    Checking,
    /// This build is the newest on its channel.
    Newest,
    Found(update::Found),
    /// Downloading or putting it in place: what it is doing.
    Installing(String),
    /// In place: Ghost runs it once opened again.
    Ready,
    Failed(String),
}

/// What comes back of an update.
pub enum Update {
    Checked(Result<Option<update::Found>, String>),
    Doing(String),
    Installed(Result<(), String>),
}

#[derive(Clone)]
pub struct Updates {
    pub now: Updating,
    /// When it was last looked for, on which channel, and whether you
    /// asked.
    last: Option<(Instant, update::Channel, bool)>,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            now: Updating::Idle,
            last: None,
        }
    }
}

impl Desk {
    /// Look for an update where it is due: as Ghost starts, then every
    /// four hours, unless you said not to.
    pub(crate) fn update_due(&mut self) {
        let channel = self.settings.channel;
        let due =
            (self.updates.last).is_none_or(|(at, on, _)| at.elapsed() >= EVERY || on != channel);
        let quiet = matches!(
            self.updates.now,
            Updating::Idle | Updating::Newest | Updating::Failed(_)
        );
        // Another channel is looked on at once, as you chose it.
        let moved = self.updates.last.is_some_and(|(_, on, _)| on != channel);
        if (self.settings.check_updates || moved) && due && (quiet || moved) {
            self.check_update(false);
        }
    }

    /// Look for an update on your channel; `asked`, you asked, and what is
    /// found (or not) is said.
    pub(crate) fn check_update(&mut self, asked: bool) {
        if matches!(
            self.updates.now,
            Updating::Checking | Updating::Installing(_)
        ) {
            return;
        }
        self.updates.last = Some((Instant::now(), self.settings.channel, asked));
        self.updates.now = Updating::Checking;
        hub::check_update(self.settings.channel, self.tx.clone());
    }

    /// Ghost › Check for Updates…: the About page, where it says.
    pub(crate) fn check_update_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab(Tab::About, window, cx);
        self.check_update(true);
        cx.notify();
    }

    pub(super) fn updated(&mut self, update: Update, cx: &mut Context<Self>) {
        let asked = self.updates.last.is_some_and(|(_, _, asked)| asked);
        self.updates.now = match update {
            Update::Checked(Ok(Some(found))) => {
                if self.updates.now != Updating::Found(found.clone()) && !asked {
                    self.done(
                        crate::tr_with("Ghost %{version} is out", &[("version", &found.name)]),
                        Some(Way::Update),
                    );
                }
                Updating::Found(found)
            }
            Update::Checked(Ok(None)) => Updating::Newest,
            Update::Checked(Err(why)) => Updating::Failed(why),
            Update::Doing(what) => Updating::Installing(what),
            Update::Installed(Ok(())) => Updating::Ready,
            Update::Installed(Err(why)) => {
                self.trouble(crate::tr("Ghost couldn't update"), Some(why.clone()), None);
                Updating::Failed(why)
            }
        };
        cx.notify();
    }

    /// Download the update found and put it in place of this Ghost.app.
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        let Updating::Found(found) = &self.updates.now else {
            return;
        };
        let Some(app) = update::running_app() else {
            self.trouble(
                crate::tr("Ghost updates itself only as Ghost.app"),
                Some(
                    crate::tr("This Ghost runs outside an app bundle: build it with bundle.sh.")
                        .into(),
                ),
                None,
            );
            return;
        };
        hub::install_update(found.clone(), app, self.home.clone(), self.tx.clone());
        self.updates.now = Updating::Installing(crate::tr("Downloading").into());
        cx.notify();
    }

    /// Quit, and open Ghost again once this one has gone: the updated
    /// Ghost.app. Run outside an app, the program itself is run again, as
    /// it was.
    pub(crate) fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (again, what) = match update::running_app() {
            Some(app) => ("open \"$1\"", app),
            None => match std::env::current_exe() {
                Ok(exe) => ("exec \"$1\"", exe),
                Err(e) => {
                    return self.trouble(
                        crate::tr("Ghost couldn't restart"),
                        Some(e.to_string()),
                        None,
                    );
                }
            },
        };
        let waiting = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "while kill -0 \"$0\" 2>/dev/null; do sleep 0.2; done; {again}"
            ))
            .arg(std::process::id().to_string())
            .arg(&what)
            .spawn();
        match waiting {
            Ok(_) => window.dispatch_action(Box::new(crate::keys::Quit), cx),
            Err(e) => self.trouble(
                crate::tr("Ghost couldn't restart"),
                Some(e.to_string()),
                None,
            ),
        }
    }
}
