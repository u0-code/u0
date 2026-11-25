//! Where sessions stand in your lists, as t3code keeps them: a name you
//! give one, pinned on top, unread, done for now (settled), out of the way
//! until later (snoozed), archived, deleted after a moment to take it back.

use std::time::{Duration, Instant, SystemTime};

use gpui::{
    Context, Focusable as _, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
    Window, px,
};

use agent_client_protocol_schema::v1::MessageId;

use super::Desk;
use crate::{
    hub,
    session::{Item, Session, State},
    store, when,
};

/// How long a deleted or archived session can be taken back.
pub const UNDO_FOR: Duration = Duration::from_secs(5);

/// When a snoozed session comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Later {
    Hour,
    Tomorrow,
    NextWeek,
}

/// Something asked of an agent that is not running: done once it starts.
pub enum Waiting {
    /// Branch the session off, whole or before an item of its thread.
    Branch(u64, Option<(usize, MessageId)>),
    /// Delete the session from the agent too.
    Delete(u64),
}

/// What can still be taken back, and since when.
pub struct Undo {
    pub key: u64,
    pub what: Undone,
    pub at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undone {
    /// Deleted once the moment passes; until then only hidden.
    Delete,
    Archive,
}

impl Desk {
    fn flags_mut(&mut self, key: u64) -> Option<&mut store::Flags> {
        self.session_mut(key).map(|s| &mut s.flags)
    }

    /// Change where the session `key` stands, and keep it.
    fn mark(&mut self, key: u64, cx: &mut Context<Self>, f: impl FnOnce(&mut store::Flags)) {
        if let Some(flags) = self.flags_mut(key) {
            f(flags);
        }
        self.popup = super::Popup::Closed;
        self.save_index();
        cx.notify();
    }

    /// Pin the session `key` on top, or take the pin off.
    pub fn toggle_pin(&mut self, key: u64, cx: &mut Context<Self>) {
        let last = self
            .sessions
            .iter()
            .filter_map(|s| s.flags.pinned)
            .max()
            .unwrap_or(0);
        self.mark(key, cx, |f| {
            f.pinned = match f.pinned {
                Some(_) => None,
                None => Some(last + 1),
            };
        });
    }

    pub fn mark_unread(&mut self, key: u64, cx: &mut Context<Self>) {
        self.mark(key, cx, |f| f.unread = true);
    }

    /// Done for now, or back among the rest.
    pub fn toggle_settled(&mut self, key: u64, cx: &mut Context<Self>) {
        let now = store::secs(SystemTime::now());
        self.mark(key, cx, |f| {
            f.settled = match f.settled {
                Some(_) => None,
                None => Some(now),
            };
            f.snoozed = None;
        });
    }

    pub fn snooze(&mut self, key: u64, until: Later, cx: &mut Context<Self>) {
        let now = SystemTime::now();
        let at = match until {
            Later::Hour => now + Duration::from_secs(3600),
            Later::Tomorrow => when::at_hour(now, 1, 9),
            Later::NextWeek => when::at_hour(now, when::to_monday(now), 9),
        };
        self.mark(key, cx, |f| f.snoozed = Some(store::secs(at)));
    }

    pub fn wake(&mut self, key: u64, cx: &mut Context<Self>) {
        self.mark(key, cx, |f| f.snoozed = None);
    }

    /// Archive the session `key`: hidden and read only until restored.
    pub fn archive(&mut self, key: u64, cx: &mut Context<Self>) {
        // A delete still waiting is done first, not brought back.
        self.finish_undo(cx);
        self.mark(key, cx, |f| f.archived = true);
        self.undo = Some(Undo {
            key,
            what: Undone::Archive,
            at: Instant::now(),
        });
        Self::after_undo(cx);
        if self.current == Some(key) {
            self.current = None;
        }
    }

    pub fn restore(&mut self, key: u64, cx: &mut Context<Self>) {
        self.mark(key, cx, |f| f.archived = false);
    }

    /// Delete the session `key` after a moment in which it can be taken
    /// back; until then it is only hidden.
    pub fn delete_later(&mut self, key: u64, cx: &mut Context<Self>) {
        self.popup = super::Popup::Closed;
        self.finish_undo(cx);
        self.undo = Some(Undo {
            key,
            what: Undone::Delete,
            at: Instant::now(),
        });
        if self.current == Some(key) {
            self.current = None;
        }
        Self::after_undo(cx);
        cx.notify();
    }

    /// Take back what was just deleted or archived.
    pub fn take_back(&mut self, cx: &mut Context<Self>) {
        if let Some(u) = self.undo.take()
            && u.what == Undone::Archive
        {
            self.mark(u.key, cx, |f| f.archived = false);
        }
        cx.notify();
    }

    /// Whether the session `key` waits to be deleted.
    pub fn deleting(&self, key: u64) -> bool {
        self.undo
            .as_ref()
            .is_some_and(|u| u.key == key && u.what == Undone::Delete)
    }

    /// Once the moment to take it back is over, do what waits.
    fn after_undo(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_FOR).await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .undo
                    .as_ref()
                    .is_some_and(|u| u.at.elapsed() >= UNDO_FOR)
                {
                    this.finish_undo(cx);
                }
            });
        })
        .detach();
    }

    /// Do now what waits to be taken back.
    fn finish_undo(&mut self, cx: &mut Context<Self>) {
        let Some(u) = self.undo.take() else { return };
        if u.what == Undone::Delete {
            self.delete_now(u.key, cx);
        }
        cx.notify();
    }

    /// Delete the session `key` now: from its agent too, where it can
    /// (started for it if need be); where it cannot, the agent keeps it and
    /// Ghost leaves it out of the list from now on.
    fn delete_now(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.iter().find(|s| s.key == key) else {
            return;
        };
        let (agent, id) = (s.agent.clone(), s.id.clone());
        let Some(id) = id else {
            self.forget_session(key, cx);
            return;
        };
        if !self.can_delete(&agent) {
            self.gone.push((agent, id.0.to_string()));
            self.forget_session(key, cx);
            return;
        }
        if let Some(running) = self.slot(&agent).and_then(hub::Slot::running) {
            hub::delete(running.clone(), key, id, self.tx.clone());
        } else {
            self.waiting.push((agent.clone(), Waiting::Delete(key)));
            self.start_for(&agent);
        }
    }

    /// Do what waited for an agent to start.
    pub(crate) fn do_waiting(&mut self, waiting: Waiting) {
        match waiting {
            Waiting::Branch(key, upto) => {
                if let Some(s) = self.sessions.iter().find(|s| s.key == key)
                    && let (Some(id), Some(running)) = (
                        s.id.clone(),
                        self.slot(&s.agent).and_then(hub::Slot::running),
                    )
                {
                    hub::fork(
                        running.clone(),
                        key,
                        id,
                        s.folder.clone(),
                        upto,
                        self.tx.clone(),
                    );
                }
            }
            Waiting::Delete(key) => {
                if let Some(s) = self.sessions.iter().find(|s| s.key == key)
                    && let (Some(id), Some(running)) = (
                        s.id.clone(),
                        self.slot(&s.agent).and_then(hub::Slot::running),
                    )
                {
                    hub::delete(running.clone(), key, id, self.tx.clone());
                }
            }
        }
    }

    /// Begin renaming the session `key` in the sidebar.
    pub fn begin_rename(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let title = self
            .sessions
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.title.clone())
            .unwrap_or_default();
        self.popup = super::Popup::Closed;
        self.renaming = Some(key);
        self.rename.update(cx, |i, cx| {
            i.set_text(&title, cx);
            i.select_everything(cx);
        });
        window.focus(&self.rename.focus_handle(cx));
        cx.notify();
    }

    /// The name typed for the session being renamed: it sticks.
    pub fn finish_rename(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.finish_rename_project(text, cx) {
            return;
        }
        let Some(key) = self.renaming.take() else {
            return;
        };
        let text = text.trim();
        if !text.is_empty()
            && let Some(s) = self.session_mut(key)
        {
            text.clone_into(&mut s.title);
            s.renamed = true;
        }
        self.save_index();
        cx.notify();
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        self.renaming_project = None;
        cx.notify();
    }

    /// The note at the foot of the window while what was just deleted or
    /// archived can be taken back.
    pub(crate) fn undo_notice(
        &self,
        faces: &crate::fonts::Faces,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let u = self.undo.as_ref()?;
        let title = self
            .sessions
            .iter()
            .find(|s| s.key == u.key)
            .map(|s| crate::ui::fit(&s.title, 32))
            .unwrap_or_default();
        let what = match u.what {
            Undone::Delete => "Deleted",
            Undone::Archive => "Archived",
        };
        Some(
            super::notices::frame()
                .items_center()
                .py(px(8.))
                .child(
                    crate::ui::icon(
                        match u.what {
                            Undone::Delete => "trash",
                            Undone::Archive => "archive",
                        },
                        16.,
                    )
                    .text_color(crate::theme::muted()),
                )
                .child(
                    crate::ui::one_line(
                        crate::ui::text(faces, format!("{what} “{title}”")).text_size(px(13.)),
                    )
                    .flex_1()
                    .min_w(px(0.)),
                )
                .child(
                    crate::ui::button(faces, "undo", "Undo", crate::ui::Weight::Secondary)
                        .on_click(cx.listener(|this, _, _, cx| this.take_back(cx))),
                ),
        )
    }

    /// Whether the agent of `s` can branch a session off, as it said when
    /// it last started.
    pub fn can_fork(&self, s: &Session) -> bool {
        s.id.is_some()
            && self
                .slot(&s.agent)
                .and_then(|x| x.known.as_ref())
                .is_some_and(|k| k.session_capabilities.fork.is_some())
    }

    /// Branch the session `key` off into a new one with all that was said
    /// in it, to go another way from there.
    pub fn fork(&mut self, key: u64, cx: &mut Context<Self>) {
        self.popup = super::Popup::Closed;
        self.branch(key, None, cx);
    }

    /// Branch the session `key` off before your message at `item` of its
    /// thread: a new session with what was said up to it, and the message
    /// in the composer to say another way.
    pub fn fork_at(&mut self, key: u64, item: usize, cx: &mut Context<Self>) {
        let last = self
            .sessions
            .iter()
            .find(|s| s.key == key)
            .and_then(|s| s.fork_point(item));
        if let Some(last) = last {
            self.branch(key, Some((item, last)), cx);
        }
    }

    /// Branch the session `key` off after the agent's message at `item`:
    /// a new session with what was said through it.
    pub fn fork_after(&mut self, key: u64, item: usize, cx: &mut Context<Self>) {
        let last = self
            .sessions
            .iter()
            .find(|s| s.key == key)
            .and_then(|s| s.through(item));
        if let Some(last) = last {
            self.branch(key, Some((item + 1, last)), cx);
        }
    }

    /// Take the session `key` back to your message at `item`, to say it
    /// again: what came after is left behind (the session is branched off
    /// before it, and the branch takes its place), and with `files`, the
    /// files go back to how they were before that turn.
    pub fn rewind(&mut self, key: u64, item: usize, files: bool, cx: &mut Context<Self>) {
        self.popup = super::Popup::Closed;
        let Some(s) = self.sessions.iter().find(|s| s.key == key) else {
            return;
        };
        let Some(point) = s.fork_point(item) else {
            return;
        };
        if files
            && let Some(turn) = s.turn_of(item)
            && let Some(before) = s.turns.get(turn).and_then(|t| t.before.clone())
        {
            hub::restore(
                s.folder.clone(),
                before,
                format!("{}/{turn}-rewind", s.uid()),
                s.first().map(str::to_owned),
                key,
                turn,
                true,
                self.tx.clone(),
            );
        }
        self.rewinding = Some(key);
        self.branch(key, Some((item, point)), cx);
    }

    fn branch(&mut self, key: u64, upto: Option<(usize, MessageId)>, cx: &mut Context<Self>) {
        let Some(agent) = self
            .sessions
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.agent.clone())
        else {
            return;
        };
        if self.slot(&agent).and_then(hub::Slot::running).is_some() {
            self.do_waiting(Waiting::Branch(key, upto));
        } else {
            self.waiting
                .push((agent.clone(), Waiting::Branch(key, upto)));
            self.start_for(&agent);
        }
        cx.notify();
    }

    /// The session branched off `from`, whole or before the item `at` of
    /// its thread: on screen, with its thread and its record up to there.
    pub(crate) fn forked(
        &mut self,
        from: u64,
        at: Option<usize>,
        result: Result<
            Box<agent_client_protocol_schema::v1::ForkSessionResponse>,
            crate::acp::Failure,
        >,
        cx: &mut Context<Self>,
    ) {
        let Some(origin) = self.sessions.iter().find(|s| s.key == from).cloned() else {
            return;
        };
        let r = match result {
            Ok(r) => r,
            Err(f) => {
                self.rewinding = None;
                self.trouble(
                    format!(
                        "Couldn't branch off “{}”",
                        crate::ui::fit(&origin.title, 40)
                    ),
                    Some(f.message),
                    None,
                );
                cx.notify();
                return;
            }
        };
        let end = at.unwrap_or(origin.items.len());
        let yours = origin.items[..end]
            .iter()
            .filter(|i| matches!(i, Item::You(_)))
            .count();
        let key = self.take_key();
        // Taken back to a message: the branch is the session now, in its
        // place; the old one is archived.
        let rewound = self.rewinding.take_if(|k| *k == from).is_some();
        let mut s = Session::new(key, &origin.agent, origin.folder.clone());
        s.project.clone_from(&origin.project);
        s.worktree.clone_from(&origin.worktree);
        s.items = origin.items[..end].to_vec();
        s.marks = origin
            .marks
            .iter()
            .filter(|(i, _)| *i < end)
            .cloned()
            .collect();
        if at.is_none() {
            s.plan = origin.plan;
        }
        if let Some(Item::You(text)) = at.and_then(|i| origin.items.get(i)) {
            s.unsent.clone_from(text);
        }
        if rewound {
            s.title.clone_from(&origin.title);
            s.renamed = origin.renamed;
            s.flags.order = origin.flags.order;
            s.flags.pinned = origin.flags.pinned;
            s.from.clone_from(&origin.from);
            if let Some(o) = self.session_mut(from) {
                o.flags.archived = true;
                o.flags.pinned = None;
            }
        } else {
            s.title = format!("{} (fork)", origin.title);
            s.renamed = true;
            s.from = origin.id.as_ref().map(|id| id.0.to_string());
        }
        s.id = Some(r.session_id.clone());
        s.modes = r.modes;
        s.options = r.config_options.unwrap_or_default();
        s.state = State::Idle;
        // Its record: the origin's, with what is not yet written, up to
        // where it was branched off.
        if let Some(id) = &origin.id {
            let mut events = store::read(&store::record_path(&self.home, &origin.agent, &id.0))
                .unwrap_or_default();
            events.extend(origin.log);
            let events = Session::record_before(events, yours);
            if !events.is_empty() {
                let to = store::record_path(&self.home, &origin.agent, &r.session_id.0);
                store::queue(store::Job::Append(to, events));
                s.recorded = true;
            }
        }
        self.sessions.push(s);
        self.save_index();
        self.bring(key, cx);
    }

    /// Carry out the plan the session `key` made: out of the mode that only
    /// plans (to its default, or the first that does not plan), then on.
    pub fn run_plan(&mut self, key: u64, cx: &mut Context<Self>) {
        let plan = |s: &str| s.to_lowercase().contains("plan");
        let Some(s) = self.sessions.iter().find(|s| s.key == key) else {
            return;
        };
        let mode = s
            .modes
            .as_ref()
            .filter(|m| plan(&m.current_mode_id.0))
            .and_then(|m| {
                m.available_modes
                    .iter()
                    .find(|x| &*x.id.0 == "default")
                    .or_else(|| m.available_modes.iter().find(|x| !plan(&x.id.0)))
                    .map(|x| x.id.0.to_string())
            });
        // An option that plans (a way of working) goes back to its first value
        // that does not.
        let option = s.options.iter().find_map(|o| match &o.kind {
            agent_client_protocol_schema::v1::SessionConfigKind::Select(sel)
                if plan(&sel.current_value.0) =>
            {
                let values: Vec<_> = match &sel.options {
                    agent_client_protocol_schema::v1::SessionConfigSelectOptions::Ungrouped(v) => {
                        v.iter().map(|x| x.value.0.to_string()).collect()
                    }
                    _ => Vec::new(),
                };
                values
                    .into_iter()
                    .find(|v| !plan(v))
                    .map(|v| (o.id.0.to_string(), v))
            }
            _ => None,
        });
        self.current = Some(key);
        if let Some(m) = mode {
            self.set_mode(m, cx);
        }
        if let Some((config, value)) = option {
            self.set_option(config, value, cx);
        }
        self.send_in(key, "Carry out the plan.", cx);
    }

    /// List sessions by where they stand, or by project.
    pub fn toggle_grouping(&mut self, cx: &mut Context<Self>) {
        self.settings.by_status = !self.settings.by_status;
        self.save_settings();
        cx.notify();
    }

    /// Show, by status, the sessions of `project` only, or of all.
    pub fn show_only(&mut self, project: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        self.settings.only = project;
        self.popup = super::Popup::Closed;
        self.save_settings();
        cx.notify();
    }

    /// Search what was said in each session for what is typed in the
    /// search, a moment after typing stops.
    pub(crate) fn search_records(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).text().trim().to_lowercase();
        self.found.clear();
        if query.chars().count() < 2 {
            return;
        }
        // What is open now is searched where it is.
        for s in &self.sessions {
            let said = s.items.iter().any(|i| match i {
                crate::session::Item::You(t) | crate::session::Item::Agent(t) => {
                    t.to_lowercase().contains(&query)
                }
                _ => false,
            });
            if said {
                self.found.insert(s.key);
            }
        }
        let paths: Vec<(u64, std::path::PathBuf)> = self
            .sessions
            .iter()
            .filter_map(|s| {
                let id = s.id.as_ref()?;
                Some((s.key, store::record_path(&self.home, &s.agent, &id.0)))
            })
            .collect();
        let tx = self.tx.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let still = this
                .update(cx, |this, cx| {
                    this.search.read(cx).text().trim().to_lowercase() == query
                })
                .unwrap_or(false);
            if still {
                hub::search(query, paths, tx);
            }
        })
        .detach();
    }

    /// Settle what has been left as many days as you said, unless it is
    /// pinned or settled.
    pub(crate) fn settle_idle(&mut self) {
        let Some(days) = self.settings.settle_days else {
            return;
        };
        let after = Duration::from_secs(u64::from(days) * 24 * 3600);
        let now = SystemTime::now();
        let mut changed = false;
        for s in &mut self.sessions {
            let idle = now.duration_since(s.updated).unwrap_or_default() >= after;
            if idle
                && s.flags.pinned.is_none()
                && s.flags.settled.is_none()
                && matches!(s.state, State::Closed | State::Idle)
            {
                s.flags.settled = Some(store::secs(s.updated + after));
                changed = true;
            }
        }
        if changed {
            self.save_index();
        }
    }

    /// Whether the session shows in the lists now: not archived, not
    /// deleted, not snoozed until later.
    pub fn listed_now(&self, s: &Session) -> bool {
        let now = store::secs(SystemTime::now());
        !s.flags.archived
            && !self.deleting(s.key)
            && s.flags.snoozed.is_none_or(|t| t <= now)
            && !self.prefs(&s.project).archived
    }
}
