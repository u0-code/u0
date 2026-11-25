//! Where sessions stand in your lists, as t3code keeps them: a name you
//! give one, pinned on top, unread, done for now (settled), out of the way
//! until later (snoozed), archived, deleted after a moment to take it back.

use std::{
    collections::HashSet,
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use gpui::{AppContext as _, Context, Entity, Focusable as _, Window};

use agent_client_protocol::v1::schema::MessageId;

use gpui::component::input::{InputEvent, InputState};

use super::{Desk, mention};
use crate::{
    hub,
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

/// The search over sessions: what is typed, and the sessions whose
/// records hold it.
pub struct Search {
    pub input: Entity<InputState>,
    pub found: HashSet<u64>,
}

impl Search {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        cx.subscribe(&input, |this, _, e: &InputEvent, cx| {
            if let InputEvent::Change = e {
                this.search_records(cx);
                cx.notify();
            }
        })
        .detach();
        Self {
            input,
            found: HashSet::new(),
        }
    }
}

/// How long what was done to sessions can be taken back.
pub const UNDO_FOR: Duration = Duration::from_secs(5);

/// The ways to snooze offered first, as t3code's presets.
pub const SNOOZES: [(&str, Later); 3] = [
    ("For an hour", Later::In(60)),
    ("Until tomorrow", Later::Tomorrow),
    ("Until next week", Later::NextWeek),
];

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
    /// At a time said: when a limit the agent hit resets.
    At(SystemTime),
}

impl Later {
    /// The time it says, from `now`.
    pub fn at(self, now: SystemTime) -> SystemTime {
        match self {
            Self::In(minutes) => now + Duration::from_secs(minutes * 60),
            Self::Tomorrow => when::at_hour(now, 1, 9),
            Self::At(at) => at,
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
    /// Have it write something for the session.
    Write(u64, crate::writer::Ask),
}

/// What can still be taken back, and since when: one action, done to
/// one session or several, each with where it stood before.
pub struct Undo {
    pub what: Undone,
    pub before: Vec<(u64, store::Flags)>,
    pub at: Instant,
}

/// The actions that can be taken back, as t3code's notice names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undone {
    Unpin,
    Settle,
    Snooze,
    Archive,
    /// Deleted once the moment passes; until then only hidden.
    Delete,
}

impl Undone {
    fn word(self) -> &'static str {
        match self {
            Self::Unpin => "Unpinned",
            Self::Settle => "Settled",
            Self::Snooze => "Snoozed",
            Self::Archive => "Archived",
            Self::Delete => "Deleted",
        }
    }
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
        if self.flags_mut(key).is_some_and(|f| f.pinned.is_some()) {
            self.unpin(vec![key], cx);
            return;
        }
        let last = self
            .sessions
            .all
            .iter()
            .filter_map(|s| s.meta.flags.pinned)
            .max()
            .unwrap_or(0);
        self.mark(key, cx, |f| f.pinned = Some(last + 1));
    }

    /// Take the pin off the sessions `keys`, asking first if you said to.
    pub fn unpin(&mut self, keys: Vec<u64>, cx: &mut Context<Self>) {
        self.ask_first(self.settings.confirm.unpin, Undone::Unpin, keys, cx);
    }

    /// Put the session `moved` just before `before` in the list: among the
    /// pinned (of every project) if that one is pinned, else among the
    /// rest of its project.
    pub fn move_session(&mut self, moved: u64, before: u64, cx: &mut Context<Self>) {
        if moved == before {
            return;
        }
        let Some(target) = self.sessions.at(before) else {
            return;
        };
        let pinned = target.meta.flags.pinned.is_some();
        let project = target.meta.project.clone();
        let mut group: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                s.key != moved
                    && s.meta.flags.pinned.is_some() == pinned
                    && (pinned || s.meta.project == project)
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

    /// Do `what` to the sessions `keys`, to be taken back for a moment: an
    /// action like the last one, still to be taken back, joins it.
    pub(crate) fn undoable(&mut self, what: Undone, keys: &[u64], cx: &mut Context<Self>) {
        if self
            .undo
            .as_ref()
            .is_some_and(|u| u.what != what || u.at.elapsed() >= UNDO_FOR)
        {
            self.finish_undo(cx);
        }
        let now = store::secs(SystemTime::now());
        let mut before = self.undo.take().map(|u| u.before).unwrap_or_default();
        for &key in keys {
            let Some(flags) = self.flags_mut(key) else {
                continue;
            };
            let was = flags.clone();
            match what {
                Undone::Unpin => flags.pinned = None,
                Undone::Settle => {
                    flags.settled = Some(now);
                    flags.snoozed = None;
                }
                Undone::Archive => flags.archived = true,
                Undone::Snooze | Undone::Delete => {}
            }
            if !before.iter().any(|(k, _)| *k == key) {
                before.push((key, was));
            }
            if self.current == Some(key) && matches!(what, Undone::Archive | Undone::Delete) {
                self.current = None;
            }
        }
        self.chosen.clear();
        self.undo = Some(Undo {
            what,
            before,
            at: Instant::now(),
        });
        self.popup = super::Popup::Closed;
        self.save_index();
        self.say_undo();
        Self::after_undo(cx);
        cx.notify();
    }

    /// Do `what` to the sessions `keys`; with `ask`, once you say so.
    fn ask_first(&mut self, ask: bool, what: Undone, keys: Vec<u64>, cx: &mut Context<Self>) {
        if !ask {
            self.undoable(what, &keys, cx);
            return;
        }
        let title = match keys.as_slice() {
            [key] => format!(
                "{} \u{201c}{}\u{201d}?",
                match what {
                    Undone::Unpin => "Unpin",
                    Undone::Archive => "Archive",
                    _ => "Delete session",
                },
                self.sessions
                    .at(*key)
                    .map(|s| s.meta.title.as_str())
                    .unwrap_or_default()
            ),
            _ => format!(
                "{} {} sessions?",
                match what {
                    Undone::Unpin => "Unpin",
                    Undone::Archive => "Archive",
                    _ => "Delete",
                },
                keys.len()
            ),
        };
        self.confirming = Some((title, what, keys));
        self.popup = super::Popup::Closed;
        cx.notify();
    }

    pub fn mark_unread(&mut self, key: u64, cx: &mut Context<Self>) {
        self.mark(key, cx, |f| f.unread = true);
    }

    /// Done for now, or back among the rest.
    pub fn toggle_settled(&mut self, key: u64, cx: &mut Context<Self>) {
        if self.flags_mut(key).is_some_and(|f| f.settled.is_some()) {
            self.mark(key, cx, |f| f.settled = None);
        } else {
            self.undoable(Undone::Settle, &[key], cx);
        }
    }

    /// Put the sessions `keys` out of the way until `until`.
    pub fn snooze(&mut self, keys: &[u64], until: Later, cx: &mut Context<Self>) {
        let at = store::secs(until.at(SystemTime::now()));
        self.undoable(Undone::Snooze, keys, cx);
        for &key in keys {
            if let Some(f) = self.flags_mut(key) {
                f.snoozed = Some(at);
            }
        }
        self.save_index();
    }

    /// Bring the session `key` back from being snoozed.
    pub fn wake(&mut self, key: u64, cx: &mut Context<Self>) {
        self.mark(key, cx, |f| f.snoozed = None);
    }

    /// Archive the session `key`: hidden and read only until restored;
    /// asking first if you said to.
    pub fn archive(&mut self, key: u64, cx: &mut Context<Self>) {
        self.ask_first(
            self.settings.confirm.archive,
            Undone::Archive,
            vec![key],
            cx,
        );
    }

    pub fn restore(&mut self, key: u64, cx: &mut Context<Self>) {
        self.mark(key, cx, |f| f.archived = false);
    }

    /// Ask before deleting the sessions `keys`, in t3code's words, if you
    /// said to; and for one that alone works in its worktree, whether that
    /// goes too. Each is deleted after a moment in which it can be taken
    /// back; until then it is only hidden.
    pub fn ask_delete(&mut self, keys: Vec<u64>, window: &mut Window, cx: &mut Context<Self>) {
        // Its worktree, where no other session works in it.
        let alone = match keys.as_slice() {
            [key] => self
                .sessions
                .at(*key)
                .and_then(|s| s.meta.worktree.clone())
                .filter(|wt| {
                    self.sessions
                        .all
                        .iter()
                        .filter(|o| o.meta.worktree.as_ref().is_some_and(|w| w.path == wt.path))
                        .count()
                        == 1
                }),
            _ => None,
        };
        match alone {
            Some(wt) => Self::ask_worktree(keys[0], &wt.path, window, cx),
            None => self.ask_first(self.settings.confirm.delete, Undone::Delete, keys, cx),
        }
    }

    /// What waits to be confirmed, as gpui-kit's alert: done on OK.
    pub(crate) fn show_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui::component::WindowExt as _;
        let Some((title, what, keys)) = self.confirming.take() else {
            return;
        };
        let (label, about) = match what {
            Undone::Unpin => ("Unpin", "It goes back among its project's sessions."),
            Undone::Archive => ("Archive", "It is hidden until you restore it."),
            _ => (
                "Delete",
                "This permanently clears conversation history for this session.",
            ),
        };
        let desk = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (desk, keys) = (desk.clone(), keys.clone());
            alert
                .title(title.clone())
                .description(about)
                .ok_text(label)
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    desk.update(cx, |d, cx| d.undoable(what, &keys, cx));
                    true
                })
        });
    }

    /// Whether the worktree at `path`, which the session `key` alone works
    /// in, is deleted with it; then the session is.
    fn ask_worktree(key: u64, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        use gpui::component::WindowExt as _;
        let desk = cx.entity();
        let said = format!(
            "This session is the only one linked to this worktree: {}. Delete the worktree too?",
            Desk::shown(path)
        );
        window.defer(cx, move |window, cx| {
            window.open_alert_dialog(cx, move |alert, _, _| {
                let (yes, no) = (desk.clone(), desk.clone());
                alert
                    .title("Delete the worktree too?")
                    .description(said.clone())
                    .ok_text("Delete worktree")
                    .cancel_text("Keep it")
                    .show_cancel(true)
                    .on_ok(move |_, _, cx| {
                        yes.update(cx, |d, cx| d.undoable(Undone::Delete, &[key], cx));
                        true
                    })
                    .on_cancel(move |_, _, cx| {
                        no.update(cx, |d, cx| {
                            d.keep_worktree.insert(key);
                            d.undoable(Undone::Delete, &[key], cx);
                        });
                        true
                    })
            });
        });
    }

    /// Say what was just done, with Undo, for as long as it can be taken
    /// back: one notice, said again as more joins it.
    fn say_undo(&mut self) {
        let Some(u) = &self.undo else { return };
        let what = match u.before.as_slice() {
            [(key, _)] => format!(
                "{} \u{201c}{}\u{201d}",
                u.what.word(),
                (self.sessions.at(*key))
                    .map(|s| crate::ui::fit(&s.meta.title, 32))
                    .unwrap_or_default()
            ),
            all => format!("{} {} sessions", u.what.word(), all.len()),
        };
        self.done(what, Some(super::notices::Way::Undo));
    }

    /// Take back what was just done: each session as it stood before.
    pub fn take_back(&mut self, cx: &mut Context<Self>) {
        let Some(u) = self.undo.take() else { return };
        for (key, flags) in u.before {
            if let Some(f) = self.flags_mut(key) {
                *f = flags;
            }
        }
        self.save_index();
        cx.notify();
    }

    /// Whether the session `key` waits to be deleted.
    pub fn deleting(&self, key: u64) -> bool {
        self.undo
            .as_ref()
            .is_some_and(|u| u.what == Undone::Delete && u.before.iter().any(|(k, _)| *k == key))
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
            for (key, _) in u.before {
                self.delete_now(key, cx);
            }
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
            Waiting::Write(key, ask) => self.ask_writer(key, ask),
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
            i.set_value(&name, window, cx);
            i.select_all(window, cx);
        });
        window.focus(&self.rename.focus_handle(cx), cx);
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

    /// The other agents a session of `agent` could go on with: each that
    /// can run, by id and name.
    pub(crate) fn others(&self, agent: &str) -> Vec<(String, String)> {
        (self.slots.iter())
            .filter(|s| s.entry.id != agent && s.usable())
            .map(|s| (s.entry.id.clone(), s.entry.name.clone()))
            .collect()
    }

    /// Go on with the session `key` with another agent, `agent`: a new
    /// session of it where `key` works (its folder, its worktree), the
    /// conversation named in the line (`@`, as a session is) to go with
    /// what you say first. Nothing is sent until you send it.
    pub fn hand_off(&mut self, key: u64, agent: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.popup = super::Popup::Closed;
        let Some(origin) = self.sessions.at(key) else {
            return;
        };
        let (title, folder, project) = (
            origin.meta.title.clone(),
            origin.meta.folder.clone(),
            origin.meta.project.clone(),
        );
        let new = self.sessions.take_key();
        let mut s = self.begun_in(new, agent, project);
        s.wants_worktree = false;
        if s.meta.folder != folder {
            s.meta.folder.clone_from(&folder);
            self.transport.send(Call::Look { folder, key: new });
        }
        let token = mention::token_of(&title);
        s.meta.unsent = format!("Go on with the work in {token} ");
        self.sessions.all.push(s);
        self.composer.mentions.push(mention::Mention {
            token,
            what: mention::Offer::Session { key, title },
            text: None,
        });
        self.show(new, window, cx);
    }

    /// Bring what was done in the branch `key` back to the session it was
    /// branched off: that one on screen, the branch named in its line.
    pub fn bring_back(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let Some(origin) = self.origin_of(s) else {
            return;
        };
        let title = s.meta.title.clone();
        let token = mention::token_of(&title);
        self.show(origin, window, cx);
        let typed = self.typed(cx);
        let line = format!("{typed}What was done in a branch of this session, {token}: ");
        self.put_line(line.trim_start(), window, cx);
        self.composer.mentions.push(mention::Mention {
            token,
            what: mention::Offer::Session { key, title },
            text: None,
        });
        cx.notify();
    }

    /// The session `s` was branched off, when it was and that is here.
    pub(crate) fn origin_of(&self, s: &Session) -> Option<u64> {
        let from = s.meta.from.as_ref()?;
        (self.sessions.all.iter())
            .find(|o| {
                o.meta.agent == s.meta.agent && o.meta.id.as_ref().is_some_and(|i| *i.0 == **from)
            })
            .map(|o| o.key)
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
    pub fn rewind(
        &mut self,
        key: u64,
        item: usize,
        files: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            self.transport.send(Call::Restore {
                dir: s.meta.folder.clone(),
                target: before,
                name: format!("{}/{turn}-rewind", s.uid()),
                first: s.first().map(str::to_owned),
                key,
                turn,
                undo: true,
                no_spacing: self.settings.no_spacing,
            });
        }
        match point {
            Some(point) => {
                self.rewinding = Some(key);
                self.branch(key, Some((item, point)), cx);
            }
            // The first message has nothing before it to branch off: the
            // session begins again, its words in the composer, in its
            // place; the old one is archived.
            None => self.start_over(key, item, window, cx),
        }
    }

    fn start_over(&mut self, key: u64, item: usize, window: &mut Window, cx: &mut Context<Self>) {
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
        self.bring(new, window, cx);
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
        window: &mut Window,
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
        // A side chat opens as a tab beside the session it was asked of.
        if side {
            self.columns
                .entry(from)
                .or_default()
                .open(super::Surface::Side(key));
            self.panes.right = true;
            cx.notify();
        } else {
            self.bring(key, window, cx);
        }
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

    /// Show the project `folder` in the sidebar among those chosen, or no
    /// longer; with none chosen, all are shown.
    pub fn toggle_filter(&mut self, folder: &std::path::Path, cx: &mut Context<Self>) {
        if let Some(at) = self.filter.iter().position(|f| f == folder) {
            self.filter.remove(at);
        } else {
            self.filter.push(folder.to_path_buf());
        }
        cx.notify();
    }

    /// Search what was said in each session for what is typed in the
    /// search, a moment after typing stops.
    pub(crate) fn search_records(&mut self, cx: &mut Context<Self>) {
        let query = self.search.input.read(cx).value().trim().to_lowercase();
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
                if this.search.input.read(cx).value().trim().to_lowercase() == query {
                    this.transport.send(Call::Search { query, records });
                }
            });
        })
        .detach();
    }

    /// Settle what has been left as many days as you said, unless it is
    /// pinned or settled, or has a pull request open and you said to keep
    /// those.
    pub(crate) fn settle_idle(&mut self) {
        let Some(days) = self.settings.settle_days else {
            return;
        };
        let after = Duration::from_secs(u64::from(days) * 24 * 3600);
        let now = SystemTime::now();
        let keep_with_pr = self.settings.keep_with_pr;
        let mut changed = false;
        for s in &mut self.sessions.all {
            let idle = now.duration_since(s.meta.updated).unwrap_or_default() >= after;
            let open_pr = s.git.pr.as_ref().is_some_and(|p| p.state == "open");
            if idle
                && !s.meta.flags.kept
                && !(keep_with_pr && open_pr)
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

/// Where a session stands, as its row says at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Working,
    /// A permission it asks for.
    Approval,
    SignIn,
    Failed,
    /// It stopped at a limit of the agent's: usage, or a turn's.
    Limited,
    /// A plan ready for you.
    Plan,
    /// Finished, and not read since.
    Done,
    /// Back from being snoozed, and not looked at since.
    Woke,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Approval => "Approval",
            Self::SignIn => "Sign in",
            Self::Failed => "Failed",
            Self::Limited => "Limited",
            Self::Plan => "Plan",
            Self::Done => "Done",
            Self::Woke => "Woke",
        }
    }

    pub fn color(self) -> gpui::Hsla {
        match self {
            Self::Working | Self::Plan | Self::Woke => crate::theme::link(),
            Self::Approval | Self::SignIn | Self::Limited => crate::theme::accent(),
            Self::Failed => crate::theme::coral(),
            Self::Done => crate::theme::ok(),
        }
    }
}

/// Where the session `s` stands, when that is anything to say.
pub fn status(s: &Session) -> Option<Status> {
    match &s.state {
        State::Working | State::Opening if s.asks() => Some(Status::Approval),
        State::Working | State::Opening => Some(Status::Working),
        State::SignIn => Some(Status::SignIn),
        State::Broken(_) => Some(Status::Failed),
        State::Idle | State::Closed => match s.outcome() {
            Some("failed") if limited(s) => Some(Status::Limited),
            Some("failed") => Some(Status::Failed),
            Some(_) => Some(Status::Plan),
            None if s.meta.flags.unread => Some(Status::Done),
            None => woke(s).then_some(Status::Woke),
        },
        State::Draft => None,
    }
}

/// Whether the last turn of `s` ended at a limit the agent said it hit.
fn limited(s: &Session) -> bool {
    let start = s.items.iter().rposition(|i| matches!(i, Item::You(_)));
    s.items[start.unwrap_or(0)..].iter().any(|i| {
        matches!(i, Item::Note { text, trouble: true } if text.to_lowercase().contains("limit"))
    })
}

/// Whether `s` was snoozed until a time now past, and not looked at since.
pub fn woke(s: &Session) -> bool {
    s.meta
        .flags
        .snoozed
        .is_some_and(|t| t <= store::secs(SystemTime::now()))
}

/// When you last used the session `s`: your last message in it, where its
/// thread is here; else when anything last happened in it.
pub fn used(s: &Session) -> SystemTime {
    s.items
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, i)| matches!(i, Item::You(_)))
        .find_map(|(n, _)| s.time_of(n))
        .unwrap_or(s.meta.updated)
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
        let day = Duration::from_hours(25);
        assert!(Later::Daily.at(now) <= now + day);
        assert_eq!(Later::In(30).at(now), now + Duration::from_mins(30));
    }

    #[test]
    fn a_turn_stopped_at_a_limit_says_limited_and_a_past_snooze_says_woke() {
        let mut s = Session::new(1, "x", "/r".into());
        s.state = State::Idle;
        s.items.push(Item::You("go".into()));
        s.items.push(Item::Note {
            text: "You've hit your usage limit".into(),
            trouble: true,
        });
        assert_eq!(status(&s), Some(Status::Limited));
        s.items.push(Item::You("again".into()));
        assert_eq!(status(&s), None);
        s.meta.flags.snoozed = Some(store::secs(SystemTime::now()) - 60);
        assert_eq!(status(&s), Some(Status::Woke));
    }
}
