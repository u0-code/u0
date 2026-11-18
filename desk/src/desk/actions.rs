//! What the desk does to the daemons it follows and the core it runs:
//! adding and forgetting connections, trying a daemon again, starting the
//! core, and restarting it with a new brain.

use gpui::{Context, Focusable as _, Window};

use super::{Desk, Notice, Popup, listening};
use crate::{
    daemons::{Daemon, Host},
    settings::{Connection, SOLVER_URL},
};

impl Desk {
    /// Add the connection the form describes, if it is a good one, and
    /// connect to it; or say in the form why not.
    pub(crate) fn add_connection(&mut self, cx: &mut Context<Self>) {
        let [name, url, var] = self.form.fields.clone().map(|f| f.read(cx).text());
        match Connection::check(&name, &url, &var, &self.settings.connections) {
            Ok(connection) => {
                self.daemons.push(Daemon::remote(&connection));
                self.settings.connections.push(connection);
                self.settings.save();
                for f in &self.form.fields {
                    f.update(cx, |f, cx| f.set_text("", cx));
                }
                self.form.open = false;
                self.form.refused = None;
            }
            Err(why) => self.form.refused = Some(why),
        }
        cx.notify();
    }

    /// Open the form for a connection, or close it.
    pub(crate) fn toggle_adding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form.open = !self.form.open;
        self.form.refused = None;
        if self.form.open {
            self.form.fields[0].focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    /// Forget the connection `name`: the desk stops following it, and her
    /// thread closes if she was on it. Its daemon is not touched.
    pub(crate) fn remove_connection(&mut self, name: &str, cx: &mut Context<Self>) {
        let host = Host::Remote(name.to_owned());
        self.settings.connections.retain(|c| c.name != name);
        self.settings.save();
        self.daemons.retain(|d| d.host != host);
        if self.open.as_ref().is_some_and(|o| o.host == host) {
            self.close_character(cx);
        }
        self.popup = Popup::Closed;
        cx.notify();
    }

    /// Whether the solver's daemon is the core on this machine, which the
    /// desk looks after and can start.
    pub(crate) fn core_is_local(&self) -> bool {
        self.settings.solver_url == SOLVER_URL
    }

    /// Try the solver's daemon again now, and ask the core who it runs.
    pub(crate) fn retry_solver(&mut self, cx: &mut Context<Self>) {
        self.solver.retry();
        self.retry_daemon(&self.settings.solver_url.clone());
        cx.notify();
    }

    /// Try her daemon again now, and ask it who it runs.
    pub(crate) fn retry_her(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = &self.open {
            open.her.retry();
            let url = open.url.clone();
            self.retry_daemon(&url);
        }
        cx.notify();
    }

    /// Ask the daemon `host` names who it runs, now.
    pub(crate) fn retry_host(&mut self, host: &crate::daemons::Host, cx: &mut Context<Self>) {
        for d in self.daemons.iter().filter(|d| d.host == *host) {
            d.retry();
        }
        cx.notify();
    }

    fn retry_daemon(&self, url: &str) {
        for d in self.daemons.iter().filter(|d| d.url == url) {
            d.retry();
        }
    }

    /// Start the core on this machine, or start it again when the desk
    /// runs it and it cannot be reached; then try it at once. Something
    /// else listening where it would is said, not fought.
    pub(crate) fn start_core(&mut self, cx: &mut Context<Self>) {
        // The desk is still starting it as it opens.
        if !self.core_ready {
            return;
        }
        let url = self.settings.solver_url.clone();
        let started = if self.deployment.owns_running() {
            self.deployment.restart()
        } else if listening(&url) {
            Err(anyhow::anyhow!(
                "something the desk did not start is already listening at {}",
                url.trim_start_matches("ws://")
            ))
        } else {
            self.deployment.start()
        };
        self.notice = started
            .err()
            .map(|e| Notice::trouble(format!("Could not start the core: {e:#}")));
        self.retry_solver(cx);
    }
}
