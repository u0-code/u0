//! What you say in a session: sent now, to every agent, later or queued
//! behind the turn going on; the session opened for it; the turn stopped;
//! the agent's questions answered and its mode and options chosen.

use gpui::{Context, Window};

use agent_client_protocol::v1::schema::{RequestPermissionOutcome, SelectedPermissionOutcome};

use super::{Desk, Later, Popup};
use crate::{
    hub::{self, Slot, Stage},
    proto::Call,
    session::{Item, State},
};

impl Desk {
    /// Say `text` in the session on screen, as if typed.
    pub fn say(&mut self, text: &str, cx: &mut Context<Self>) {
        self.send(text, cx);
    }

    /// Say `text` in the session on screen; a draft set to go to every
    /// agent goes to each of them, a session of its own for each.
    pub(crate) fn send(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let draft = self
            .session()
            .is_some_and(|s| s.state == State::Draft && s.items.is_empty());
        self.send_in(key, text, cx);
        if draft && self.composer.fanout {
            self.composer.fanout = false;
            self.fan_out(key, text, cx);
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

    /// Begin, beside the session `key`, a session with each other agent
    /// that can run, in the same project, and say `text` in each.
    pub(crate) fn fan_out(&mut self, key: u64, text: &str, cx: &mut Context<Self>) {
        let Some(origin) = self.sessions.at(key) else {
            return;
        };
        // Each goes as the first did: begun the same way (its worktree, the
        // machine it is on) and with the same images and files named.
        let (agent, folder, shared) = (
            origin.meta.agent.clone(),
            origin.meta.project.clone(),
            origin.attached.clone(),
        );
        let others: Vec<String> = self
            .slots
            .iter()
            .filter(|s| s.entry.id != agent && s.usable())
            .map(|s| s.entry.id.clone())
            .collect();
        for other in others {
            let new = self.sessions.take_key();
            let mut s = self.begun_in(new, &other, folder.clone());
            s.attached.clone_from(&shared);
            self.sessions.all.push(s);
            self.send_in(new, text, cx);
        }
    }

    /// Say what was written for later and is due now, in its session;
    /// one said each day is kept for the next.
    pub(crate) fn send_due(&mut self, cx: &mut Context<Self>) {
        let now = crate::store::secs(std::time::SystemTime::now());
        let mut due = Vec::new();
        for s in &mut self.sessions.all {
            for l in &mut s.meta.later {
                if l.at <= now {
                    due.push((s.key, l.text.clone()));
                    while l.daily && l.at <= now {
                        l.at += 86_400;
                    }
                }
            }
            s.meta.later.retain(|l| l.at > now);
        }
        if due.is_empty() {
            return;
        }
        for (key, text) in due {
            match self.sessions.at_mut(key) {
                // A turn going on: it waits its turn, as one queued.
                Some(s) if matches!(s.state, State::Working | State::Opening) => {
                    s.meta.queued.push(text);
                }
                Some(_) => self.send_in(key, &text, cx),
                None => {}
            }
        }
        self.save_index();
        cx.notify();
    }

    /// Say what is in the composer later in the session on screen, when
    /// `when` says.
    pub fn send_later(&mut self, when: Later, cx: &mut Context<Self>) {
        let text = self.composer.input.read(cx).text().trim().to_owned();
        let now = std::time::SystemTime::now();
        let at = when.at(now);
        let Some(key) = self.current else { return };
        if text.is_empty() {
            return;
        }
        if let Some(s) = self.sessions.at_mut(key) {
            s.meta.later.push(crate::store::Later {
                at: crate::store::secs(at),
                text,
                daily: when == Later::Daily,
            });
            s.meta.later.sort_by_key(|l| l.at);
        }
        self.composer.input.update(cx, |i, cx| i.set_text("", cx));
        self.popup = Popup::Closed;
        self.save_index();
        self.done(
            format!(
                "To be said at {}",
                crate::when::said_at(at, now, self.settings.twelve)
            ),
            None,
        );
        cx.notify();
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
        if session.state == State::Working || session.state == State::Opening {
            if !on_screen {
                return;
            }
            // Said while a turn goes on: it waits its turn, and is said as
            // the one going on ends, with what goes with it.
            session.meta.queued.push(text.clone());
            let last = session.meta.queued.len() - 1;
            let attached = self.attach(key, &text);
            if let Some(s) = self.sessions.at_mut(key) {
                s.attached.extend(attached);
            }
            self.save_index();
            // Or it goes in now, the turn stopped for it, as you asked.
            if self.settings.steer {
                self.steer(last, cx);
            }
            cx.notify();
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
            let agent = session.meta.agent.clone();
            // What goes with it (images, files named) goes with it.
            if on_screen {
                let attached = self.attach(key, &text);
                if let Some(s) = self.sessions.at_mut(key) {
                    s.attached.extend(attached);
                }
            }
            self.open_when_running(&agent, key);
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
        let pictures: Vec<_> = attached
            .iter()
            .filter(|b| matches!(b, agent_client_protocol::v1::schema::ContentBlock::Image(_)))
            .cloned()
            .collect();
        session.attached.extend(attached);
        // Something new said in a session done for now brings it back.
        session.meta.flags.settled = None;
        session.meta.flags.snoozed = None;
        session.said(&text);
        // The images it took show under it, and are kept with its record.
        for p in pictures {
            session.apply(
                agent_client_protocol::v1::schema::SessionUpdate::UserMessageChunk(
                    agent_client_protocol::v1::schema::ContentChunk::new(p),
                ),
            );
        }
        let (agent, folder, id) = (
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
            self.settings.used(&folder);
            self.save_settings();
            if worktree {
                let from = self.sessions.at(key).and_then(|s| s.begin.clone());
                hub::make_worktree(
                    folder,
                    text,
                    self.home.clone(),
                    self.settings.branch_prefix.clone(),
                    from,
                    key,
                    self.tx.clone(),
                );
            } else {
                self.open_when_running(&agent, key);
            }
        }
        self.to_bottom();
        cx.notify();
    }

    /// Open the session `key` once its agent runs, starting it if need be.
    pub(crate) fn open_when_running(&mut self, agent: &str, key: u64) {
        // One on another machine has no slot after a restart until wanted.
        if self.slot(agent).is_none()
            && let (base, Some(host)) = crate::remote::split(agent)
        {
            let (base, host) = (base.to_owned(), host.to_owned());
            self.remote_slot(&base, &host);
        }
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
        let Some(slot) = self.slots.iter_mut().find(|s| s.entry.id == agent) else {
            return;
        };
        let (resume, load) = slot.caps().map_or((false, false), |c| {
            (c.session_capabilities.resume.is_some(), c.load_session)
        });
        match &slot.stage {
            Stage::Running { .. } => match id {
                Some(id) if recorded && resume => {
                    self.transport.send(Call::Reopen {
                        agent: agent.to_owned(),
                        key,
                        session: id,
                        folder,
                        also,
                        resumed: true,
                    });
                }
                Some(id) if load => {
                    if recorded {
                        self.drop_record(key);
                    }
                    self.transport.send(Call::Reopen {
                        agent: agent.to_owned(),
                        key,
                        session: id,
                        folder,
                        also,
                        resumed: false,
                    });
                }
                Some(_) => {
                    if let Some(s) = self.sessions.at_mut(key) {
                        s.state = State::Broken("The agent cannot open a session again".into());
                    }
                }
                None => self.transport.send(Call::Open {
                    agent: agent.to_owned(),
                    key,
                    folder,
                    also,
                }),
            },
            Stage::Starting => {}
            _ => {
                slot.stage = Stage::Starting;
                self.transport.send(Call::Start {
                    entry: slot.entry.clone(),
                    command: slot.command.clone(),
                });
            }
        }
    }

    /// Stop the turn going on in the session on screen.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let (agent, id) = (s.meta.agent.clone(), s.meta.id.clone());
        if let (true, Some(id)) = (self.slot(&agent).is_some_and(Slot::running), id) {
            // Questions still open are answered as cancelled, as ACP asks.
            let open: Vec<_> = s
                .items
                .iter()
                .filter_map(|i| match i {
                    Item::Asked(a) if a.chosen.is_none() => Some(a.id),
                    _ => None,
                })
                .collect();
            for request in open {
                self.transport.send(Call::Answer {
                    agent: agent.clone(),
                    request,
                    outcome: RequestPermissionOutcome::Cancelled,
                });
            }
            self.transport.send(Call::Cancel { agent, session: id });
            if self.stopping.is_none_or(|(k, _)| k != s.key) {
                self.stopping = Some((s.key, std::time::Instant::now()));
            }
        }
        cx.notify();
    }

    /// Stop the task `task` the agent left going in the session `key`.
    pub fn stop_task(&mut self, key: u64, task: String, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        if let (true, Some(id)) = (
            self.slot(&s.meta.agent).is_some_and(Slot::running),
            s.meta.id.clone(),
        ) {
            self.transport.send(Call::StopTask {
                agent: s.meta.agent.clone(),
                session: id,
                task,
            });
        }
        cx.notify();
    }

    /// Stop the work of the subagent `key`, where its agent lets it be.
    pub fn stop_sub(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(c) = self.sessions.at(key) else {
            return;
        };
        if let (true, Some(id)) = (
            self.slot(&c.meta.agent).is_some_and(Slot::running),
            c.meta.id.clone(),
        ) {
            self.transport.send(Call::Cancel {
                agent: c.meta.agent.clone(),
                session: id,
            });
        }
        cx.notify();
    }

    /// Stop by force the agent of the session `key`, which would not stop
    /// its turn: its process is ended, and started again when wanted.
    pub fn force_stop(&mut self, key: u64, cx: &mut Context<Self>) {
        self.stopping = None;
        let agent = self.sessions.at(key).map(|s| s.meta.agent.clone());
        if let Some(agent) = agent.filter(|a| self.slot(a).is_some_and(Slot::running)) {
            self.transport.send(Call::Kill { agent });
        }
        cx.notify();
    }

    /// Answer the question asked under `request` with the option `option`.
    pub fn choose(
        &mut self,
        request: u64,
        option: &agent_client_protocol::v1::schema::PermissionOption,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.current else { return };
        let Some(agent) = self.session().map(|s| s.meta.agent.clone()) else {
            return;
        };
        if self.slot(&agent).is_some_and(Slot::running) {
            self.transport.send(Call::Answer {
                agent,
                request,
                outcome: RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    option.option_id.clone(),
                )),
            });
        }
        if let Some(s) = self.sessions.at_mut(key) {
            s.answered(request, option);
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
                text: "You stopped the turn to say what follows.".into(),
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

    /// Take the queued `n` back into the line.
    pub fn unqueue(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(key) = self.current else { return };
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        if n >= s.meta.queued.len() {
            return;
        }
        let text = s.meta.queued.remove(n);
        self.composer
            .input
            .update(cx, |i, cx| i.set_text(&text, cx));
        self.save_index();
        cx.notify();
    }

    /// Put the session on screen in the mode `mode`.
    pub fn set_mode(&mut self, mode: String, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let Some(key) = self.current else { return };
        let Some(s) = self.session() else { return };
        let (agent, id) = (s.meta.agent.clone(), s.meta.id.clone());
        if id.is_none() {
            self.sessions
                .at_mut(key)
                .expect("on screen")
                .choose_mode(&mode);
        } else if let (true, Some(id)) = (self.slot(&agent).is_some_and(Slot::running), id) {
            self.transport.send(Call::SetMode {
                agent,
                key,
                session: id,
                mode: mode.clone(),
            });
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
        let Some(s) = self.session() else { return };
        let (agent, id) = (s.meta.agent.clone(), s.meta.id.clone());
        if id.is_none() {
            self.sessions
                .at_mut(key)
                .expect("on screen")
                .choose(&config, &value);
        } else if let (true, Some(id)) = (self.slot(&agent).is_some_and(Slot::running), id) {
            self.transport.send(Call::SetOption {
                agent,
                key,
                session: id,
                config,
                value,
            });
        }
        cx.notify();
    }
}
