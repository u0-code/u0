//! What you say in a session: sent now, to every agent, later or queued
//! behind the turn going on; the session opened for it; the turn stopped;
//! the agent's questions answered and its mode and options chosen.

use gpui::{Context, Window};

use agent_client_protocol::v1::schema::{RequestPermissionOutcome, SelectedPermissionOutcome};

use super::{Desk, Later, Place, Popup};
use crate::{
    hub::{Host, Slot, Stage},
    proto::Call,
    session::{Item, State},
    tasks::{Schedule, Task},
};

impl Desk {
    /// Say `text` in the session on screen, as if typed.
    pub fn say(&mut self, text: &str, cx: &mut Context<Self>) {
        self.send(text, cx);
    }

    /// Say `text` in the session on screen; a draft set to run on additional
    /// models/agents starts a session of its own for each.
    pub(crate) fn send(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let draft = self
            .session()
            .is_some_and(|s| s.state == State::Draft && s.items.is_empty());
        self.send_in(key, text, cx);
        if draft && !self.composer.also_run.is_empty() {
            let targets = std::mem::take(&mut self.composer.also_run);
            self.fan_out(key, &targets, text, cx);
        }
    }

    /// Say `text` and leave it to go on: a new session is begun beside it,
    /// in the same project, to write the next thing in (⌘Return).
    pub(crate) fn send_aside(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.send(text, cx);
        if let Some(project) = self.project() {
            self.new_in(project, window, cx);
        }
    }

    /// Begin, beside the session `key`, a session for each `(agent, model)`
    /// target in the same project (and worktree mode), and say `text` in each.
    pub(crate) fn fan_out(
        &mut self,
        key: u64,
        targets: &[(String, Option<String>)],
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(origin) = self.sessions.at(key) else {
            return;
        };
        let (project, shared, wants_wt, begin, repo, repos, chosen_repo) = (
            Place::project_of(origin),
            origin.attached.clone(),
            origin.wants_worktree,
            origin.begin.clone(),
            origin.repo,
            origin.git.repos.clone(),
            origin.git.repo.clone(),
        );
        for (other, model) in targets {
            let new = self.sessions.take_key();
            let mut s = self.begun_in(new, other, project.clone());
            s.wants_worktree = wants_wt;
            s.begin.clone_from(&begin);
            s.repo = repo;
            s.git.repos.clone_from(&repos);
            s.git.repo.clone_from(&chosen_repo);
            if let Some(model) = model {
                let config = s
                    .options
                    .iter()
                    .find(|o| {
                        o.category
                            == Some(
                                agent_client_protocol::v1::schema::SessionConfigOptionCategory::Model,
                            )
                    })
                    .map_or_else(|| "model".to_owned(), |o| o.id.0.to_string());
                s.choose(&config, model);
            }
            s.attached.clone_from(&shared);
            self.sessions.all.push(s);
            self.send_in(new, text, cx);
        }
    }

    /// Say "Continue" in the session `key` at `at`: when a limit it hit
    /// resets.
    pub fn retry_at(&mut self, key: u64, at: std::time::SystemTime, cx: &mut Context<Self>) {
        self.say_later(key, "Continue".into(), Later::At(at), cx);
    }

    /// Say what is in the composer later in the session on screen, when
    /// `when` says.
    pub fn send_later(&mut self, when: Later, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.typed(cx);
        let Some(key) = self.current else { return };
        if text.is_empty() || !self.say_later(key, text, when, cx) {
            return;
        }
        self.put_line("", window, cx);
        self.popup = Popup::Closed;
        cx.notify();
    }

    /// Say `text` in the session `key` when `when` says: a scheduled task
    /// of that session, which the server runs. Whether there was one to
    /// say it in: a session is one once its agent has it.
    fn say_later(&mut self, key: u64, text: String, when: Later, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.sessions.at(key).filter(|s| s.meta.id.is_some()) else {
            self.trouble(
                crate::tr("Say something first: a thread is kept once its agent has it"),
                None,
                None,
            );
            return false;
        };
        let now = std::time::SystemTime::now();
        let at = when.at(now);
        let schedule = if when == Later::Daily {
            Schedule::Daily {
                minute: crate::when::minute_of_day(at),
                days: Vec::new(),
            }
        } else {
            Schedule::Once(crate::store::secs(at))
        };
        let host = s.host;
        let task = Task {
            id: 0,
            title: crate::ui::fit(text.lines().next().unwrap_or_default(), 40),
            prompt: text,
            agent: s.meta.agent.clone(),
            model: None,
            mode: None,
            project: s.meta.project.clone(),
            schedule,
            paused: false,
            same: true,
            worktree: false,
            if_running: crate::tasks::Overlap::Skip,
            timeout_mins: None,
            notify: true,
            workflow: None,
            session: s.meta.id.clone(),
            next: None,
            last: None,
            runs: 0,
        };
        self.call(host, Call::SetTask(task));
        self.done(
            crate::tr_with(
                "To be said at %{when}",
                &[("when", &crate::when::said_at(at, now, self.settings.twelve))],
            ),
            None,
        );
        cx.notify();
        true
    }

    /// Say `text` in the session `key`.
    pub(crate) fn send_in(&mut self, key: u64, text: &str, cx: &mut Context<Self>) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        let on_screen = self.current == Some(key);
        let Some(session) = self.sessions.at_mut(key) else {
            return;
        };
        session.ran_on.clone_from(&session.branch);
        if session.state == State::Working || session.state == State::Opening {
            if on_screen {
                let waiting_reject =
                    (session.waiting()).and_then(|a| Some((a.id, a.no()?.clone())));
                if let Some((request, no)) = waiting_reject {
                    self.telling = None;
                    self.reject_and_say(key, request, &no, &text, cx);
                } else {
                    self.follow_up(key, &text, self.settings.steer, cx);
                }
            }
            return;
        }
        if session.state == State::Closed {
            // Its agent opens it again (resumed from the record, or loaded),
            // and then it goes; after what was queued before, if anything
            // was (kept from an earlier run).
            if !session.recorded {
                session.forget();
            }
            let text = if session.meta.queued.is_empty() {
                text
            } else {
                session.meta.queued.push(text);
                session.meta.queued.remove(0)
            };
            session.pending = Some(text.clone());
            session.state = State::Opening;
            let (host, agent) = (session.host, session.meta.agent.clone());
            // What goes with it (images, files named) goes with it.
            if on_screen {
                let attached = self.attach(key, &text);
                if let Some(s) = self.sessions.at_mut(key) {
                    s.attached.extend(attached);
                }
            }
            self.open_when_running(host, &agent, key);
            cx.notify();
            return;
        }
        let attached = if on_screen {
            self.attach(key, &text)
        } else {
            Vec::new()
        };
        let session = self.sessions.at_mut(key).expect("found above");
        session.meta.unsent.clear();
        session.attached.extend(attached);
        // Something new said in a session done for now brings it back.
        session.meta.flags.settled = None;
        session.meta.flags.snoozed = None;
        session.said(&text);
        let (host, agent, folder, id) = (
            session.host,
            session.meta.agent.clone(),
            session.meta.folder.clone(),
            session.meta.id.clone(),
        );
        if id.is_some() {
            // What you said goes in its record before what the agent says.
            self.flush(key);
            self.begin_turn(key, text);
        } else {
            let session = self.sessions.at_mut(key).expect("on screen");
            session.state = State::Opening;
            session.pending = Some(text.clone());
            let worktree = session.needs_worktree();
            self.settings_on_mut(host).used(&folder);
            self.save_settings_on(host);
            if worktree {
                let from = self.sessions.at(key).and_then(|s| s.begin.clone());
                // In a folder of repositories, a worktree of each.
                let settings = self.settings_on(host);
                let call = Call::MakeWorktree {
                    submodules: settings.of(&folder).submodules,
                    dir: folder,
                    about: text,
                    under: self.worktree_folders(host).remove(0),
                    prefix: settings.branch_prefix.clone(),
                    from,
                    key,
                };
                self.call(host, call);
            } else {
                self.open_when_running(host, &agent, key);
            }
        }
        self.to_bottom(cx);
        cx.notify();
    }

    /// Say `text` while a turn goes on in the session `key`: it waits its
    /// turn, and is said as the one going on ends, with what goes with
    /// it; or (`steer`) it goes in now, the turn stopped for it.
    pub(crate) fn follow_up(&mut self, key: u64, text: &str, steer: bool, cx: &mut Context<Self>) {
        let Some(last) = self.sessions.at(key).map(|s| s.meta.queued.len()) else {
            return;
        };
        self.queue(key, text, last);
        if steer {
            self.steer(last, cx);
        }
        cx.notify();
    }

    /// Turn the question `request` in the session `key` down with `no`,
    /// and say `text` next: before anything queued already, since it
    /// answers the step the agent is on, not the ones queued after it.
    fn reject_and_say(
        &mut self,
        key: u64,
        request: u64,
        no: &agent_client_protocol::v1::schema::PermissionOption,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        self.choose(key, request, no, cx);
        self.queue(key, text, 0);
        cx.notify();
    }

    /// Queue `text` in the session `key` at `at`, with what goes with it.
    fn queue(&mut self, key: u64, text: &str, at: usize) {
        let Some(session) = self.sessions.at_mut(key) else {
            return;
        };
        let at = at.min(session.meta.queued.len());
        session.meta.queued.insert(at, text.to_owned());
        let attached = self.attach(key, text);
        if let Some(s) = self.sessions.at_mut(key) {
            s.attached.extend(attached);
        }
        self.save_index();
    }

    /// Open the session `key` once its agent on `host` runs, starting it
    /// if need be.
    pub(crate) fn open_when_running(&mut self, host: Host, agent: &str, key: u64) {
        let ((folder, also), id, recorded) = self
            .sessions
            .at(key)
            .map(|s| {
                (
                    (s.meta.folder.clone(), s.meta.also.clone()),
                    s.meta.id.clone(),
                    s.recorded,
                )
            })
            .unwrap_or_default();
        let Some(slot) = self.slot(host, agent) else {
            return;
        };
        let (resume, load) = slot.caps().map_or((false, false), |c| {
            (c.session_capabilities.resume.is_some(), c.load_session)
        });
        match &slot.stage {
            Stage::Running { .. } => match id {
                Some(id) if recorded && resume => {
                    let call = Call::Reopen {
                        agent: agent.to_owned(),
                        key,
                        session: id,
                        folder,
                        also,
                        resumed: true,
                    };
                    self.call(host, call);
                }
                Some(id) if load => {
                    if recorded {
                        self.drop_record(key);
                    }
                    let call = Call::Reopen {
                        agent: agent.to_owned(),
                        key,
                        session: id,
                        folder,
                        also,
                        resumed: false,
                    };
                    self.call(host, call);
                }
                Some(_) => {
                    if let Some(s) = self.sessions.at_mut(key) {
                        s.state =
                            State::Broken(crate::tr("The agent cannot open a thread again").into());
                    }
                }
                None => {
                    let call = Call::Open {
                        agent: agent.to_owned(),
                        key,
                        folder,
                        also,
                    };
                    self.call(host, call);
                }
            },
            Stage::Starting => {}
            _ => {
                // One failed is tried again, as one ready is started.
                if let Some(slot) = self.slot_mut(host, agent) {
                    slot.stage = Stage::Ready;
                }
                self.start_for(host, agent);
            }
        }
    }

    /// The server the session `key` is on, and its agent, while that
    /// agent runs: what is asked of it then goes there.
    fn running(&self, key: u64) -> Option<(Host, String)> {
        let s = self.sessions.at(key)?;
        (self.slot(s.host, &s.meta.agent).is_some_and(Slot::running))
            .then(|| (s.host, s.meta.agent.clone()))
    }

    /// Stop the turn going on in the session on screen.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        if let (Some((host, agent)), Some(id)) = (self.running(s.key), s.meta.id.clone()) {
            // Questions still open are answered as cancelled, as ACP asks.
            let open: Vec<_> = s
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Asked(a) if a.chosen.is_none() => Some((a.id, false)),
                    Item::Elicited(e) if e.answered.is_none() => Some((e.id, true)),
                    _ => None,
                })
                .collect();
            let key = s.key;
            for (request, form) in open {
                let call = Call::Answer {
                    agent: agent.clone(),
                    request,
                    answer: crate::acp::Answer::cancelled(form),
                };
                self.call(host, call);
                if form {
                    self.forms.remove(&request);
                    if let Some(s) = self.sessions.at_mut(key) {
                        s.elicit_answered(request, "Cancelled");
                    }
                }
            }
            self.call(host, Call::Cancel { agent, session: id });
            if self.stopping.is_none_or(|(k, _)| k != key) {
                self.stopping = Some((key, std::time::Instant::now()));
            }
            // Its subagents stop with it, as t3code's do.
            let children: Vec<u64> = (self.sessions.all.iter())
                .filter(|c| c.parent == Some(key) && matches!(c.state, State::Working))
                .map(|c| c.key)
                .collect();
            for child in children {
                self.stop_sub(child, cx);
            }
        }
        cx.notify();
    }

    /// Stop the task `task` the agent left going in the session `key`.
    pub fn stop_task(&mut self, key: u64, task: String, cx: &mut Context<Self>) {
        let id = self.sessions.at(key).and_then(|s| s.meta.id.clone());
        if let (Some((host, agent)), Some(id)) = (self.running(key), id) {
            let call = Call::StopTask {
                agent,
                session: id,
                task,
            };
            self.call(host, call);
        }
        cx.notify();
    }

    /// Stop the work of the subagent `key`, where its agent lets it be.
    pub fn stop_sub(&mut self, key: u64, cx: &mut Context<Self>) {
        let id = self.sessions.at(key).and_then(|c| c.meta.id.clone());
        if let (Some((host, agent)), Some(id)) = (self.running(key), id) {
            self.call(host, Call::Cancel { agent, session: id });
        }
        cx.notify();
    }

    /// Stop by force the agent of the session `key`, which would not stop
    /// its turn: its process is ended, and started again when wanted.
    pub fn force_stop(&mut self, key: u64, cx: &mut Context<Self>) {
        self.stopping = None;
        if let Some((host, agent)) = self.running(key) {
            self.call(host, Call::Kill { agent });
        }
        cx.notify();
    }

    /// Answer the question asked in the session `key` under `request`
    /// with the option `option`.
    pub fn choose(
        &mut self,
        key: u64,
        request: u64,
        option: &agent_client_protocol::v1::schema::PermissionOption,
        cx: &mut Context<Self>,
    ) {
        if let Some((host, agent)) = self.running(key) {
            let call = Call::Answer {
                agent,
                request,
                answer: crate::acp::Answer::Permission(RequestPermissionOutcome::Selected(
                    SelectedPermissionOutcome::new(option.option_id.clone()),
                )),
            };
            self.call(host, call);
        }
        if let Some(s) = self.sessions.at_mut(key) {
            s.answered(request, option);
        }
        if self
            .telling
            .is_some_and(|(k, r, _)| k == key && r == request)
        {
            self.telling = None;
        }
        cx.notify();
    }

    /// Open the inline note box on a waiting permission card (`revise` for
    /// plan changes, else `"No, and tell it…"`).
    pub fn begin_tell(
        &mut self,
        key: u64,
        request: u64,
        revise: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = if revise {
            crate::tr("What to change in the plan")
        } else {
            let name = self
                .sessions
                .at(key)
                .map(|s| self.name_of(&s.meta.agent))
                .unwrap_or_default();
            crate::tr_with("Tell %{name} what to do instead", &[("name", &name)])
        };
        self.telling = Some((key, request, revise));
        self.tell_input.update(cx, |i, cx| {
            i.set_value("", window, cx);
            i.set_placeholder(placeholder, window, cx);
            window.focus(&gpui::Focusable::focus_handle(&*i, cx), cx);
        });
        cx.notify();
    }

    /// Close the inline permission note box and return focus to the composer.
    pub fn cancel_tell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.telling = None;
        self.tell_input
            .update(cx, |i, cx| i.set_value("", window, cx));
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    /// Reject the waiting permission `request` once and say what was typed
    /// in `tell_input` next, before anything queued.
    pub fn reject_and_tell(
        &mut self,
        key: u64,
        request: u64,
        _revise: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.tell_input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let no = self.sessions.at(key).and_then(|s| {
            s.items.iter().rev().find_map(|i| match i {
                Item::Asked(a) if a.id == request && a.chosen.is_none() => a.no().cloned(),
                _ => None,
            })
        });
        self.telling = None;
        self.tell_input
            .update(cx, |i, cx| i.set_value("", window, cx));
        if let Some(no) = no {
            self.reject_and_say(key, request, &no, &text, cx);
        } else {
            self.send_in(key, &text, cx);
        }
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    /// Answer what was asked under `id` in the session `key`, and say so
    /// in its thread as `words`.
    pub(crate) fn answer_form(
        &mut self,
        agent: &str,
        key: u64,
        id: u64,
        answer: crate::acp::Answer,
        words: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self.sessions.at(key).map(|s| s.host) {
            let call = Call::Answer {
                agent: agent.to_owned(),
                request: id,
                answer,
            };
            self.call(host, call);
        }
        self.forms.remove(&id);
        if let Some(s) = self.sessions.at_mut(key) {
            s.elicit_answered(id, words);
        }
        cx.notify();
    }

    /// Say the next thing queued in the session `key`, if one is.
    pub(crate) fn say_queued(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        if s.state != State::Idle || s.meta.queued.is_empty() {
            return;
        }
        let text = s.meta.queued.remove(0);
        self.send_in(key, &text, cx);
    }

    /// Say the queued `n` now: the turn going on is stopped, and it is
    /// said as soon as that has ended.
    /// With no turn going on (the session closed, as after a restart),
    /// it is said at once.
    pub fn steer(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        if n >= s.meta.queued.len() {
            return;
        }
        let text = s.meta.queued.remove(n);
        if matches!(s.state, State::Working | State::Opening) {
            s.meta.queued.insert(0, text);
            s.items.push(Item::Note {
                text: crate::tr("You stopped the turn to say what follows.").into(),
                trouble: false,
            });
            self.stop(cx);
        } else {
            self.send_in(key, &text, cx);
        }
    }

    /// Say the queued `n` one sooner.
    pub fn queue_earlier(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        if let Some(s) = self.sessions.at_mut(key)
            && n > 0
            && n < s.meta.queued.len()
        {
            s.meta.queued.swap(n, n - 1);
            self.save_index();
        }
        cx.notify();
    }

    /// Edit the queued message `n` in place in the composer.
    pub fn unqueue(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let Some(text) = s.meta.queued.get(n).cloned() else {
            return;
        };
        self.composer.editing_queued = Some(n);
        self.put_line(&text, window, cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    /// Update the queued message `n` in place with `text`.
    pub fn save_queued(&mut self, n: usize, text: &str, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        if let Some(s) = self.sessions.at_mut(key) {
            if n < s.meta.queued.len() {
                text.clone_into(&mut s.meta.queued[n]);
            } else {
                s.meta.queued.push(text.to_owned());
            }
            self.save_index();
        }
        cx.notify();
    }

    /// Remove the queued message `n`.
    pub fn remove_queued(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        if let Some(s) = self.sessions.at_mut(key)
            && n < s.meta.queued.len()
        {
            s.meta.queued.remove(n);
            self.save_index();
        }
        if let Some(ed) = self.composer.editing_queued {
            if ed == n {
                self.composer.editing_queued = None;
            } else if ed > n {
                self.composer.editing_queued = Some(ed - 1);
            }
        }
        cx.notify();
    }

    /// Put the session on screen in the mode `mode`.
    pub fn set_mode(&mut self, mode: String, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let Some(key) = self.current else { return };
        self.remember(|c| c.mode = Some(mode.clone()));
        let Some(s) = self.session() else { return };
        let id = s.meta.id.clone();
        if id.is_none() {
            self.sessions
                .at_mut(key)
                .expect("on screen")
                .choose_mode(&mode);
        } else if let (Some((host, agent)), Some(id)) = (self.running(key), id) {
            let call = Call::SetMode {
                agent,
                key,
                session: id,
                mode: mode.clone(),
            };
            self.call(host, call);
            if let Some(modes) = &mut self.sessions.at_mut(key).expect("on screen").modes {
                modes.current_mode_id = agent_client_protocol::v1::schema::SessionModeId::new(mode);
            }
        }
        cx.notify();
    }

    /// Choose `value` for the session's option `config`.
    pub fn set_option(&mut self, config: String, value: String, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let Some(key) = self.current else { return };
        self.remember(|c| {
            c.options.insert(config.clone(), value.clone());
        });
        let Some(s) = self.session() else { return };
        let id = s.meta.id.clone();
        if id.is_none() {
            self.sessions
                .at_mut(key)
                .expect("on screen")
                .choose(&config, &value);
        } else if let (Some((host, agent)), Some(id)) = (self.running(key), id) {
            let call = Call::SetOption {
                agent,
                key,
                session: id,
                config,
                value,
            };
            self.call(host, call);
        }
        cx.notify();
    }

    /// Keep what `chose` sets as what the next session of the agent on
    /// screen, in its project, begins with: in its server's settings.
    fn remember(&mut self, chose: impl FnOnce(&mut crate::settings::Chosen)) {
        let Some(s) = self.session() else { return };
        let (host, project, agent) = (s.host, s.meta.project.clone(), s.meta.agent.clone());
        let prefs = self.settings_on_mut(host).prefs.entry(project).or_default();
        chose(prefs.last.entry(agent).or_default());
        self.save_settings_on(host);
    }
}
