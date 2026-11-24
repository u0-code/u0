//! Where sessions stand in your lists, as t3code keeps them: a name you
//! give one, pinned on top, unread, done for now (settled), out of the way
//! until later (snoozed), archived, deleted after a moment to take it back.

use std::time::{Duration, Instant, SystemTime};

use gpui::{
    Context, Focusable as _, IntoElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};

use super::Desk;
use crate::{
    hub,
    session::{Session, State},
    store, when,
};

/// A session idle this long is settled on its own, unless it is pinned.
const SETTLE_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);

/// How long a deleted or archived session can be taken back.
pub const UNDO_FOR: Duration = Duration::from_secs(5);

/// When a snoozed session comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Later {
    Hour,
    Tomorrow,
    NextWeek,
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

    /// Delete the session `key` now: from its agent too, where it can.
    fn delete_now(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.iter().find(|s| s.key == key) else {
            return;
        };
        match (
            s.id.clone(),
            self.slot(&s.agent).and_then(hub::Slot::running),
        ) {
            (Some(id), Some(running)) if self.can_delete(&s.agent) => {
                hub::delete(running.clone(), key, id, self.tx.clone());
            }
            _ => self.forget_session(key, cx),
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
        self.rename.update(cx, |i, cx| i.set_text(&title, cx));
        window.focus(&self.rename.focus_handle(cx));
        cx.notify();
    }

    /// The name typed for the session being renamed: it sticks.
    pub fn finish_rename(&mut self, text: &str, cx: &mut Context<Self>) {
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
        cx.notify();
    }

    /// The note at the foot of the window while what was just deleted or
    /// archived can be taken back.
    pub(crate) fn undo_toast(
        &self,
        faces: &crate::fonts::Faces,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
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
            div()
                .absolute()
                .bottom(px(18.))
                .left(px(18.))
                .flex()
                .items_center()
                .gap(px(12.))
                .pl(px(14.))
                .pr(px(6.))
                .py(px(6.))
                .rounded(px(10.))
                .bg(crate::theme::panel())
                .border_1()
                .border_color(crate::theme::border2())
                .shadow_lg()
                .child(crate::ui::text(faces, format!("{what} “{title}”")).text_size(px(13.)))
                .child(
                    crate::ui::button(faces, "undo", "Undo", crate::ui::Weight::Secondary)
                        .on_click(cx.listener(|this, _, _, cx| this.take_back(cx))),
                )
                .into_any_element(),
        )
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

    /// Settle what has been idle a week, unless it is pinned or settled.
    pub(crate) fn settle_idle(&mut self) {
        let now = SystemTime::now();
        let mut changed = false;
        for s in &mut self.sessions {
            let idle = now.duration_since(s.updated).unwrap_or_default() >= SETTLE_AFTER;
            if idle
                && s.flags.pinned.is_none()
                && s.flags.settled.is_none()
                && matches!(s.state, State::Closed | State::Idle)
            {
                s.flags.settled = Some(store::secs(s.updated + SETTLE_AFTER));
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
        !s.flags.archived && !self.deleting(s.key) && s.flags.snoozed.is_none_or(|t| t <= now)
    }
}
