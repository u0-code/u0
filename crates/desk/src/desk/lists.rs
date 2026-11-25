//! Where sessions stand in your lists, as t3code keeps them: a name you
//! give one, pinned on top, unread, done for now (settled), out of the way
//! until later (snoozed), archived, deleted after a moment to take it back.

use std::{
    collections::HashSet,
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use gpui::{
    AppContext as _, Context, Entity, Focusable as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, px,
};

use agent_client_protocol::v1::schema::MessageId;

use super::Desk;
use crate::{
    hub,
    input::{Cancelled, Changed, TextInput},
    proto::Call,
    session::{Item, Session, State},
    store, when,
};

/// What is being renamed, in the desk's line for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Renaming {
    /// A session: in the sidebar, or (`here`) where its title is in the
    /// top bar.
    Session { key: u64, here: bool },
    /// A project, in the sidebar.
    Project(PathBuf),
}

/// The search over sessions: whether it is open, what is typed, and the
/// sessions whose records hold it.
pub struct Search {
    pub open: bool,
    pub input: Entity<TextInput>,
    pub found: HashSet<u64>,
}

impl Search {
    pub(super) fn new(cx: &mut Context<Desk>) -> Self {
        let input = cx.new(|cx| TextInput::new(cx, "Search sessions").padded(4.).keeping());
        cx.subscribe(&input, |this, _, _: &Changed, cx| {
            this.search_records(cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&input, |this, _, _: &Cancelled, cx| {
            this.search.open = false;
            cx.notify();
        })
        .detach();
        Self {
            open: false,
            input,
            found: HashSet::new(),
        }
    }
}

/// How long a deleted or archived session can be taken back.
pub const UNDO_FOR: Duration = Duration::from_secs(5);

/// When a snoozed session comes back, or what you wrote is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Later {
    /// In so many minutes.
    In(u64),
    /// Tomorrow at nine.
    Tomorrow,
    /// Next Monday at nine.
    NextWeek,
    /// At the next nine o'clock, and each day after.
    Daily,
}

impl Later {
    /// The time it says, from `now`.
    pub fn at(self, now: SystemTime) -> SystemTime {
        match self {
            Self::In(minutes) => now + Duration::from_secs(minutes * 60),
            Self::Tomorrow => when::at_hour(now, 1, 9),
            Self::NextWeek => when::at_hour(now, when::to_monday(now), 9),
            Self::Daily => match when::at_hour(now, 0, 9) {
                today if today > now => today,
                _ => when::at_hour(now, 1, 9),
            },
        }
    }
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
        self.sessions.at_mut(key).map(|s| &mut s.meta.flags)
    }

    /// Change where the session `key` stands, and keep it.
    pub(crate) fn mark(
        &mut self,
        key: u64,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut store::Flags),
    ) {
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
            .all
            .iter()
            .filter_map(|s| s.meta.flags.pinned)
            .max()
            .unwrap_or(0);
        self.mark(key, cx, |f| {
            f.pinned = match f.pinned {
                Some(_) => None,
                None => Some(last + 1),
            };
        });
    }

    /// Put the session `moved` just before `before` in the list: among the
    /// pinned if that one is pinned, else among the rest of its project.
    pub fn move_session(&mut self, moved: u64, before: u64, cx: &mut Context<Self>) {
        if moved == before {
            return;
        }
        let Some(target) = self.sessions.at(before) else {
            return;
        };
        let pinned = target.meta.flags.pinned.is_some();
        let project = target.meta.project.clone();
        let whole = pinned && self.settings.by_status;
        let mut group: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                s.key != moved
                    && s.meta.flags.pinned.is_some() == pinned
                    && (whole || s.meta.project == project)
                    && s.parent.is_none()
            })
            .collect();
        group.sort_by_key(|s| (s.meta.flags.pinned, s.meta.flags.order, s.key));
        let mut keys: Vec<u64> = group.iter().map(|s| s.key).collect();
        let mut orders: Vec<i64> = group.iter().map(|s| s.meta.flags.order).collect();
        if let Some(m) = self.sessions.at(moved) {
            orders.push(m.meta.flags.order);
        }
        orders.sort_unstable();
        let at = keys.iter().position(|k| *k == before).unwrap_or(keys.len());
        keys.insert(at, moved);
        for (n, key) in keys.into_iter().enumerate() {
            if let Some(s) = self.sessions.at_mut(key) {
                if pinned {
                    s.meta.flags.pinned = Some(i64::try_from(n).unwrap_or(0) + 1);
                } else {
                    s.meta.flags.pinned = None;
                    s.meta.flags.order = orders.get(n).copied().unwrap_or(s.meta.flags.order);
                }
            }
        }
        self.save_index();
        cx.notify();
    }

    /// Change the flags of each session chosen with ⌘-click, then choose
    /// none.
    pub(crate) fn for_chosen(&mut self, cx: &mut Context<Self>, f: impl Fn(&mut store::Flags)) {
        for key in std::mem::take(&mut self.chosen) {
            if let Some(s) = self.sessions.at_mut(key) {
                f(&mut s.meta.flags);
            }
        }
        self.save_index();
        cx.notify();
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
        let at = until.at(SystemTime::now());
        self.mark(key, cx, |f| f.snoozed = Some(store::secs(at)));
    }

    /// Snooze the session `key` for the while `text` says ("3h", "2d").
    pub fn snooze_for(&mut self, key: u64, text: &str, cx: &mut Context<Self>) {
        match when::span(text) {
            Some(d) => {
                self.popup = super::Popup::Closed;
                let at = SystemTime::now() + d;
                self.mark(key, cx, |f| f.snoozed = Some(store::secs(at)));
            }
            None => self.trouble(
                format!("“{}” is not a while Ghost reads", text.trim()),
                Some("Say it as 45m, 3h, 2d or 1w.".into()),
                None,
            ),
        }
        cx.notify();
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
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let (agent, id) = (s.meta.agent.clone(), s.meta.id.clone());
        let Some(id) = id else {
            self.forget_session(key, false, cx);
            return;
        };
        if !self.can_delete(&agent) {
            self.gone.push((agent, id.0.to_string()));
            self.forget_session(key, true, cx);
            return;
        }
        if self.slot(&agent).is_some_and(hub::Slot::running) {
            self.transport.send(Call::Delete {
                agent,
                key,
                session: id,
            });
        } else {
            self.waiting.push((agent.clone(), Waiting::Delete(key)));
            self.start_for(&agent);
        }
    }

    /// Do what waited for an agent to start.
    pub(crate) fn do_waiting(&mut self, waiting: Waiting) {
        match waiting {
            Waiting::Branch(key, upto) => {
                if let Some(s) = self.sessions.at(key)
                    && let Some(id) = s.meta.id.clone()
                    && self.slot(&s.meta.agent).is_some_and(hub::Slot::running)
                {
                    self.transport.send(Call::Fork {
                        agent: s.meta.agent.clone(),
                        from: key,
                        session: id,
                        folder: s.meta.folder.clone(),
                        upto,
                    });
                }
            }
            Waiting::Delete(key) => {
                if let Some(s) = self.sessions.at(key)
                    && let Some(id) = s.meta.id.clone()
                    && self.slot(&s.meta.agent).is_some_and(hub::Slot::running)
                {
                    self.transport.send(Call::Delete {
                        agent: s.meta.agent.clone(),
                        key,
                        session: id,
                    });
                }
            }
        }
    }

    /// Begin renaming `what`, its name as it is in the line for it.
    pub fn begin_rename(&mut self, what: Renaming, window: &mut Window, cx: &mut Context<Self>) {
        let name = match &what {
            Renaming::Session { key, .. } => self
                .sessions
                .at(*key)
                .map(|s| s.meta.title.clone())
                .unwrap_or_default(),
            Renaming::Project(folder) => self.project_name(folder),
        };
        self.popup = super::Popup::Closed;
        self.renaming = Some(what);
        self.rename.update(cx, |i, cx| {
            i.set_text(&name, cx);
            i.select_everything(cx);
        });
        window.focus(&self.rename.focus_handle(cx));
        cx.notify();
    }

    /// The name typed for what is being renamed: it sticks. An empty one
    /// leaves a session's as it was, and gives a project back its
    /// folder's.
    pub fn finish_rename(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim().to_owned();
        match self.renaming.take() {
            Some(Renaming::Session { key, .. }) => {
                if !text.is_empty()
                    && let Some(s) = self.sessions.at_mut(key)
                {
                    s.meta.title = text;
                    s.meta.renamed = true;
                }
                self.save_index();
            }
            Some(Renaming::Project(folder)) => {
                self.set_prefs(&folder, cx, |p| p.name = (!text.is_empty()).then_some(text));
            }
            None => return,
        }
        cx.notify();
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
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
            .at(u.key)
            .map(|s| crate::ui::fit(&s.meta.title, 32))
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
        s.meta.id.is_some()
            && self
                .slot(&s.meta.agent)
                .and_then(crate::hub::Slot::caps)
                .is_some_and(|k| k.session_capabilities.fork.is_some())
    }

    /// Branch the session `key` off into a new one with all that was said
    /// in it, to go another way from there.
    pub fn fork(&mut self, key: u64, cx: &mut Context<Self>) {
        self.popup = super::Popup::Closed;
        self.branch(key, None, cx);
    }

    /// Ask a side question of the session `key`: a branch of it whole, on
    /// screen, not listed, so that what is asked there does not go in it.
    pub fn ask_beside(&mut self, key: u64, cx: &mut Context<Self>) {
        self.popup = super::Popup::Closed;
        self.asking_beside = Some(key);
        self.branch(key, None, cx);
    }

    /// Take the last answer of the side question `key` back to the session
    /// it was asked of, quoted in what you are writing there.
    pub fn answer_back(&mut self, key: u64, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let Some(to) = s.beside else { return };
        let answer = s.items.iter().rev().find_map(|i| match i {
            Item::Agent(t) => Some(t.clone()),
            _ => None,
        });
        self.show(to, window, cx);
        if let Some(answer) = answer {
            self.quote(&answer, window, cx);
        }
    }

    /// Branch the session `key` off before your message at `item` of its
    /// thread: a new session with what was said up to it, and the message
    /// in the composer to say another way.
    pub fn fork_at(&mut self, key: u64, item: usize, cx: &mut Context<Self>) {
        let last = self.sessions.at(key).and_then(|s| s.fork_point(item));
        if let Some(last) = last {
            self.branch(key, Some((item, last)), cx);
        }
    }

    /// Branch the session `key` off after the agent's message at `item`:
    /// a new session with what was said through it.
    pub fn fork_after(&mut self, key: u64, item: usize, cx: &mut Context<Self>) {
        let last = self.sessions.at(key).and_then(|s| s.through(item));
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
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let point = s.fork_point(item);
        let first = s.items.iter().position(|i| matches!(i, Item::You(_))) == Some(item);
        if point.is_none() && !first {
            return;
        }
        if files
            && let Some(turn) = s.turn_of(item)
            && let Some(before) = s.turns.get(turn).and_then(|t| t.before.clone())
        {
            hub::restore(
                s.meta.folder.clone(),
                before,
                format!("{}/{turn}-rewind", s.uid()),
                s.first().map(str::to_owned),
                key,
                turn,
                true,
                self.settings.no_spacing,
                self.tx.clone(),
            );
        }
        match point {
            Some(point) => {
                self.rewinding = Some(key);
                self.branch(key, Some((item, point)), cx);
            }
            // The first message has nothing before it to branch off: the
            // session begins again, its words in the composer, in its
            // place; the old one is archived.
            None => self.start_over(key, item, cx),
        }
    }

    fn start_over(&mut self, key: u64, item: usize, cx: &mut Context<Self>) {
        let Some(mut s) = self.beside(key) else {
            return;
        };
        if let Some(Item::You(text)) = self.sessions.at(key).and_then(|o| o.items.get(item)) {
            s.meta.unsent.clone_from(text);
        }
        self.replace(key, &mut s);
        let new = s.key;
        self.sessions.all.push(s);
        self.save_index();
        self.bring(new, cx);
    }

    fn branch(&mut self, key: u64, upto: Option<(usize, MessageId)>, cx: &mut Context<Self>) {
        let Some(agent) = self.sessions.at(key).map(|s| s.meta.agent.clone()) else {
            return;
        };
        if self.slot(&agent).is_some_and(hub::Slot::running) {
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
            Box<agent_client_protocol::v1::schema::ForkSessionResponse>,
            crate::acp::Error,
        >,
        cx: &mut Context<Self>,
    ) {
        let Some(origin) = self.sessions.at(from).cloned() else {
            return;
        };
        let r = match result {
            Ok(r) => r,
            Err(f) => {
                self.rewinding = None;
                self.asking_beside = None;
                self.trouble(
                    format!(
                        "Couldn't branch off “{}”",
                        crate::ui::fit(&origin.meta.title, 40)
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
        // Taken back to a message: the branch is the session now, in its
        // place; the old one is archived.
        let rewound = self.rewinding.take_if(|k| *k == from).is_some();
        let side = self.asking_beside.take_if(|k| *k == from).is_some();
        let Some(mut s) = self.beside(from) else {
            return;
        };
        let key = s.key;
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
            s.meta.unsent.clone_from(text);
        }
        if rewound {
            s.meta.renamed = origin.meta.renamed;
            s.meta.from.clone_from(&origin.meta.from);
            self.replace(from, &mut s);
        } else {
            s.meta.title = if side {
                format!("Beside “{}”", origin.meta.title)
            } else {
                format!("{} (fork)", origin.meta.title)
            };
            s.meta.renamed = true;
            s.meta.from = origin.meta.id.as_ref().map(|id| id.0.to_string());
            s.beside = side.then_some(from);
        }
        s.meta.id = Some(r.session_id.clone());
        s.modes = r.modes;
        s.options = r.config_options.unwrap_or_default();
        s.state = State::Idle;
        // Its record: the origin's, with what is not yet written, up to
        // where it was branched off.
        if let Some(id) = origin.meta.id {
            self.flush(from);
            self.transport.send(Call::CopyRecord {
                agent: origin.meta.agent,
                from: id,
                to: r.session_id.clone(),
                yours,
            });
            s.recorded = true;
        }
        self.sessions.all.push(s);
        self.save_index();
        self.bring(key, cx);
    }

    /// Carry out the plan the session `key` made: out of the mode that only
    /// plans (to its default, or the first that does not plan), then on.
    pub fn run_plan(&mut self, key: u64, cx: &mut Context<Self>) {
        let plan = |s: &str| s.to_lowercase().contains("plan");
        let Some(s) = self.sessions.at(key) else {
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
            agent_client_protocol::v1::schema::SessionConfigKind::Select(sel)
                if plan(&sel.current_value.0) =>
            {
                let values: Vec<_> = match &sel.options {
                    agent_client_protocol::v1::schema::SessionConfigSelectOptions::Ungrouped(v) => {
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

    /// Open or close the search over sessions.
    pub fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.open = !self.search.open;
        if self.search.open {
            window.focus(&self.search.input.focus_handle(cx));
        } else {
            self.search.input.update(cx, |s, cx| s.set_text("", cx));
        }
        cx.notify();
    }

    /// Search what was said in each session for what is typed in the
    /// search, a moment after typing stops.
    pub(crate) fn search_records(&mut self, cx: &mut Context<Self>) {
        let query = self.search.input.read(cx).text().trim().to_lowercase();
        self.search.found.clear();
        if query.chars().count() < 2 {
            return;
        }
        // What is open now is searched where it is.
        for s in &self.sessions.all {
            let said = s.items.iter().any(|i| match i {
                crate::session::Item::You(t) | crate::session::Item::Agent(t) => {
                    t.to_lowercase().contains(&query)
                }
                _ => false,
            });
            if said {
                self.search.found.insert(s.key);
            }
        }
        let records: Vec<_> = (self.sessions.all.iter())
            .filter_map(|s| Some((s.key, s.meta.agent.clone(), s.meta.id.clone()?)))
            .collect();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.search.input.read(cx).text().trim().to_lowercase() == query {
                    this.transport.send(Call::Search { query, records });
                }
            });
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
        for s in &mut self.sessions.all {
            let idle = now.duration_since(s.meta.updated).unwrap_or_default() >= after;
            if idle
                && !s.meta.flags.kept
                && s.meta.flags.pinned.is_none()
                && s.meta.flags.settled.is_none()
                && matches!(s.state, State::Closed | State::Idle)
            {
                s.meta.flags.settled = Some(store::secs(s.meta.updated + after));
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
        s.parent.is_none()
            && s.beside.is_none()
            && !s.meta.flags.archived
            && !self.deleting(s.key)
            && s.meta.flags.snoozed.is_none_or(|t| t <= now)
            && !self.prefs(&s.meta.project).archived
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_later_is_ahead_and_each_day_is_within_a_day() {
        let now = SystemTime::now();
        for later in [
            Later::In(30),
            Later::Tomorrow,
            Later::NextWeek,
            Later::Daily,
        ] {
            assert!(later.at(now) > now, "{later:?}");
        }
        let day = Duration::from_secs(86_400 + 3600);
        assert!(Later::Daily.at(now) <= now + day);
        assert_eq!(Later::In(30).at(now), now + Duration::from_secs(1800));
    }
}
