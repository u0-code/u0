//! The sidebar's list, as t3code's when it groups by project: the pinned
//! sessions of every project on top, in the order you dragged them into;
//! each project a folder of its sessions not done, by your last message,
//! the projects by theirs; and at the foot two shelves, folded, of the
//! sessions snoozed (soonest back first) and settled (latest first, ten,
//! then 25 more at a time). It is gpui-kit's Tree: ↑ ↓ move, → ← open and
//! fold, Return opens; every row is one height.

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use gpui::component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    list::ListItem,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    tree::{TreeEntry, TreeItem, TreeState},
};
use gpui::{App, Context, Entity, SharedString, Window, div, prelude::*, px};

use super::{Dragged, gist};
use crate::{
    desk::{Desk, Renaming, lists, rank},
    fonts::Weighted as _,
    session::{Session, State},
    settings::Sort,
    store, theme, ui,
};

/// A title with its tags after it, as search reads it and a row shows
/// them: "Fix the build #ci #urgent".
fn tagged(title: &str, tags: &[String]) -> String {
    tags.iter()
        .fold(title.to_owned(), |t, tag| format!("{t} #{tag}"))
}

/// How many settled sessions show at first, and how many more each time.
const SETTLED: usize = 10;
const SETTLED_MORE: usize = 25;

/// What a row of the list is, as its id in the tree says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Project(PathBuf),
    Session(u64),
    Shelf(Shelf),
    /// More of the settled.
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Snoozed,
    Settled,
}

impl Row {
    fn id(&self) -> SharedString {
        match self {
            Self::Project(p) => format!("p:{}", p.display()).into(),
            Self::Session(key) => format!("s:{key}").into(),
            Self::Shelf(Shelf::Snoozed) => "h:snoozed".into(),
            Self::Shelf(Shelf::Settled) => "h:settled".into(),
            Self::More => "more".into(),
        }
    }

    pub fn of(id: &str) -> Option<Self> {
        Some(match id.split_once(':') {
            Some(("p", path)) => Self::Project(PathBuf::from(path)),
            Some(("s", key)) => Self::Session(key.parse().ok()?),
            Some(("h", "snoozed")) => Self::Shelf(Shelf::Snoozed),
            Some(("h", "settled")) => Self::Shelf(Shelf::Settled),
            _ if id == "more" => Self::More,
            _ => return None,
        })
    }
}

/// The key a shelf's being open is kept under in `Desk::expanded`.
fn shelf_key(shelf: Shelf) -> String {
    format!("shelf-{shelf:?}")
}

/// What the tree's shape is: each row's id and whether it is open, to
/// tell when it has to be given its rows again.
fn shape(items: &[TreeItem], out: &mut String) {
    for item in items {
        out.push_str(&item.id);
        out.push(if item.is_expanded() { '+' } else { '-' });
        shape(&item.children, out);
        out.push('\n');
    }
}

/// When, compactly: "now", "5m", "3h", "2d".
pub fn compact(t: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(t).map_or(0, |d| d.as_secs());
    match secs {
        0..60 => "now".into(),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

impl Desk {
    /// Give the sidebar's tree its rows, where they changed; the row
    /// chosen is the session on screen, but while the keys move it.
    pub(crate) fn sync_side(&mut self, window: &Window, cx: &mut Context<Self>) {
        let (items, order) = self.side_items(cx);
        let mut drawn = String::new();
        shape(&items, &mut drawn);
        let current = self.current;
        let away = {
            let tree = self.side.read(cx);
            let open = current.and_then(|k| tree.index_of(&Row::Session(k).id()));
            !gpui::Focusable::focus_handle(tree, cx).contains_focused(window, cx)
                && tree.selected_index() != open
        };
        if drawn != self.side_shape || current != self.side_current || away {
            let reshaped = drawn != self.side_shape;
            self.side_shape = drawn;
            self.side_current = current;
            self.side.update(cx, |tree, cx| {
                if reshaped {
                    tree.set_items(items, cx);
                }
                let at = current.and_then(|k| tree.index_of(&Row::Session(k).id()));
                tree.set_selected_index(at, cx);
            });
        }
        self.side_order = order;
    }

    /// A folder opened or folded in the tree: kept, as the desk's.
    pub(crate) fn side_folded(&mut self, id: &str, open: bool, cx: &mut Context<Self>) {
        match Row::of(id) {
            Some(Row::Project(p)) if open => {
                self.closed.remove(&p);
            }
            Some(Row::Project(p)) => {
                self.closed.insert(p);
            }
            Some(Row::Shelf(kind)) if open => {
                self.expanded.insert(shelf_key(kind));
            }
            Some(Row::Shelf(kind)) => {
                self.expanded.remove(&shelf_key(kind));
            }
            _ => return,
        }
        // The row chosen in the tree goes back to the session open.
        self.side_current = None;
        cx.notify();
    }

    /// Whether the session `s` answers what is typed in the search: by
    /// its title, branch, pull request or project (`rank`), or by what
    /// was said in it.
    fn answers(&self, s: &Session, query: &rank::Query, now: SystemTime) -> bool {
        query.is_empty()
            || self.search.found.contains(&s.key)
            || rank::session(
                query,
                &tagged(&s.meta.title, &s.meta.tags),
                s.git.pr.as_ref().map(|p| p.number),
                s.branch.as_deref(),
                &self.project_name(&s.meta.project),
                lists::used(s),
                now,
                true,
            )
            .is_some()
    }

    /// The sessions of `folder` its folder lists: not pinned, settled,
    /// snoozed or put away; by your last message, the latest first.
    pub(crate) fn in_project(&self, folder: &Path) -> Vec<&Session> {
        let mut here: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                self.listed_under(&s.meta.project) == folder
                    && self.listed_now(s)
                    && s.meta.flags.settled.is_none()
                    && s.meta.flags.pinned.is_none()
            })
            .collect();
        here.sort_by(|a, b| self.before(a, b));
        here
    }

    /// Which of `a` and `b` the sidebar lists first, as `settings.sort`
    /// says: the latest first, or by name.
    fn before(&self, a: &Session, b: &Session) -> std::cmp::Ordering {
        match self.settings.sort {
            Sort::Name => (a.meta.title.to_lowercase()).cmp(&b.meta.title.to_lowercase()),
            _ => self.sorted_at(b).cmp(&self.sorted_at(a)),
        }
    }

    /// The time `s` is ordered by: your last message in it, the last thing
    /// that happened in it, or when it began.
    fn sorted_at(&self, s: &Session) -> SystemTime {
        match self.settings.sort {
            Sort::Used | Sort::Name => lists::used(s),
            Sort::Updated => s.meta.updated,
            Sort::Begun => s.meta.begun,
        }
    }

    /// The tree's rows, and the sessions in the order they are listed.
    fn side_items(&self, cx: &App) -> (Vec<TreeItem>, Vec<u64>) {
        let typed = self.search.input.read(cx).value().trim().to_owned();
        let query = rank::Query::new(&typed);
        let searching = !query.is_empty();
        let now = SystemTime::now();
        let secs = store::secs(now);
        let shown = |s: &&Session| {
            (self.filter.is_empty() || self.filter.contains(&s.meta.project))
                && self.answers(s, &query, now)
        };
        let leaf = |s: &Session| TreeItem::new(Row::Session(s.key).id(), s.meta.title.clone());
        let mut order = Vec::new();
        let mut items = Vec::new();

        let mut pinned: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| s.meta.flags.pinned.is_some() && s.meta.flags.settled.is_none())
            .filter(|s| self.listed_now(s))
            .filter(shown)
            .collect();
        pinned.sort_by_key(|s| s.meta.flags.pinned);
        for s in pinned {
            order.push(s.key);
            items.push(leaf(s));
        }

        // Pinned projects first, then by your last message in any of
        // their sessions, then by name.
        let mut folders: Vec<PathBuf> = Vec::new();
        for p in self.projects() {
            let p = self.listed_under(&p);
            if !folders.contains(&p) {
                folders.push(p);
            }
        }
        let mut projects: Vec<(PathBuf, SystemTime)> = folders
            .into_iter()
            .filter(|p| self.filter.is_empty() || self.filter.contains(p))
            .map(|p| {
                let last = self
                    .sessions
                    .all
                    .iter()
                    .filter(|s| self.listed_under(&s.meta.project) == p)
                    .map(|s| self.sorted_at(s))
                    .max()
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (p, last)
            })
            .collect();
        // Pinned first; then the latest first, or by name alone.
        let named = self.settings.sort == Sort::Name;
        projects.sort_by(|(a, at), (b, bt)| {
            let when = |t: SystemTime| std::cmp::Reverse((!named).then_some(t));
            (
                !self.prefs(a).pinned,
                when(*at),
                self.project_name(a).to_lowercase(),
            )
                .cmp(&(
                    !self.prefs(b).pinned,
                    when(*bt),
                    self.project_name(b).to_lowercase(),
                ))
        });
        for (folder, _) in projects {
            let sessions: Vec<&Session> =
                self.in_project(&folder).into_iter().filter(shown).collect();
            if searching && sessions.is_empty() {
                continue;
            }
            let open = searching || !self.closed.contains(&folder);
            if open {
                order.extend(sessions.iter().map(|s| s.key));
            }
            items.push(
                TreeItem::new(
                    Row::Project(folder.clone()).id(),
                    self.project_name(&folder),
                )
                .expanded(open)
                .children(sessions.into_iter().map(leaf)),
            );
        }

        let mut snoozed: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| s.parent.is_none() && s.beside.is_none() && !s.meta.flags.archived)
            .filter(|s| s.meta.flags.snoozed.is_some_and(|t| t > secs))
            .filter(shown)
            .collect();
        snoozed.sort_by_key(|s| s.meta.flags.snoozed);
        let mut settled: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| s.meta.flags.settled.is_some() && self.listed_now(s))
            .filter(shown)
            .collect();
        settled.sort_by_key(|s| std::cmp::Reverse(s.meta.flags.settled));
        for (shelf, list) in [(Shelf::Snoozed, snoozed), (Shelf::Settled, settled)] {
            if list.is_empty() {
                continue;
            }
            let open = searching || self.expanded.contains(&shelf_key(shelf));
            let most = match shelf {
                Shelf::Settled => self.settled_shown.max(SETTLED),
                Shelf::Snoozed => usize::MAX,
            };
            let mut children: Vec<TreeItem> = list.iter().take(most).map(|s| leaf(s)).collect();
            if list.len() > most {
                children.push(TreeItem::new(Row::More.id(), "More"));
            }
            if open {
                order.extend(list.iter().take(most).map(|s| s.key));
            }
            items.push(
                TreeItem::new(Row::Shelf(shelf).id(), format!("{shelf:?}"))
                    .expanded(open)
                    .children(children),
            );
        }
        (items, order)
    }

    /// A row of the tree, drawn from what the desk knows now. `chosen`:
    /// the row the keys move from; `focused`: the tree has the keys.
    pub(crate) fn side_row(
        &self,
        entry: &TreeEntry,
        chosen: bool,
        focused: bool,
        desk: &Entity<Self>,
        cx: &App,
    ) -> ListItem {
        let faces = crate::fonts::faces(cx).clone();
        let id = entry.item().id.clone();
        let open = matches!(Row::of(&id), Some(Row::Session(k)) if self.current == Some(k));
        // Rows stand a little apart, so that the ground of one lit (hovered,
        // chosen) never runs into the next: 36 from one to the next, as
        // t3code's.
        let item = ListItem::new(SharedString::from(format!("row-{id}")))
            .h(px(32.))
            .my(px(2.))
            .px(px(10.))
            .rounded(px(8.))
            // The session open keeps the open row's ground wherever the
            // keys are (gpui-kit gives it to the row they move from too).
            .when(open, |i| i.bg(theme::row_open()))
            // While the tree has the keys, the row they move from is ringed,
            // over the whole row.
            .when(chosen && focused, |i| {
                i.suffix(|_, _| {
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(8.))
                        .border_2()
                        .border_color(theme::faint())
                })
            });
        match Row::of(&id) {
            Some(Row::Project(folder)) => self.project_row(&faces, item, &folder, entry, desk),
            Some(Row::Session(key)) => match self.sessions.at(key) {
                Some(s) => self.session_row(
                    &faces,
                    item,
                    s,
                    entry.depth() > 0 && !Self::on_shelf(s),
                    desk,
                ),
                None => item,
            },
            Some(Row::Shelf(kind)) => {
                let count = entry.item().children.len();
                let more = matches!(kind, Shelf::Settled)
                    && entry
                        .item()
                        .children
                        .iter()
                        .any(|c| c.id.as_ref() == "more");
                let count = if more { count - 1 } else { count };
                let drop = desk.clone();
                item.child(
                    div()
                        .id(SharedString::from(format!("shelf-{kind:?}")))
                        .drag_over::<Dragged>(|st, _, _, _| st.bg(theme::row_hover()))
                        .on_drop(move |d: &Dragged, _, cx| {
                            drop.update(cx, |this, cx| this.dropped(d.key, &Row::Shelf(kind), cx));
                        })
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            ui::text(&faces, format!("{kind:?}"))
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::muted()),
                        )
                        .child(ui::fine(&faces, count.to_string()))
                        .child(div().flex_1())
                        .child(chevron(entry.item().is_expanded())),
                )
            }
            Some(Row::More) => {
                let rest = self
                    .sessions
                    .all
                    .iter()
                    .filter(|s| s.meta.flags.settled.is_some() && self.listed_now(s))
                    .count()
                    .saturating_sub(self.settled_shown.max(SETTLED));
                let desk = desk.clone();
                item.pl(px(36.))
                    .child(
                        ui::text(&faces, format!("Show {} more", rest.min(SETTLED_MORE)))
                            .text_size(px(crate::style::LABEL))
                            .text_color(theme::muted()),
                    )
                    .on_click(move |_, _, cx| {
                        desk.update(cx, |d, cx| {
                            d.settled_shown = d.settled_shown.max(SETTLED) + SETTLED_MORE;
                            cx.notify();
                        });
                    })
            }
            None => item,
        }
    }

    /// Whether `s` is on a shelf, where its row is slim.
    fn on_shelf(s: &Session) -> bool {
        s.meta.flags.settled.is_some()
            || s.meta
                .flags
                .snoozed
                .is_some_and(|t| t > store::secs(SystemTime::now()))
    }

    /// A project's folder: its icon, its name; folded, how many sessions
    /// it hides and a dot for the one that most needs you; on hover, a new
    /// session in it; and the chevron at its end.
    fn project_row(
        &self,
        faces: &crate::fonts::Faces,
        item: ListItem,
        folder: &Path,
        entry: &TreeEntry,
        desk: &Entity<Self>,
    ) -> ListItem {
        let group = SharedString::from(format!("prow-{}", folder.display()));
        let open = entry.item().is_expanded();
        let hidden: Vec<&Session> = if open {
            Vec::new()
        } else {
            self.in_project(folder)
        };
        // Needs you, then failed, then done, then working.
        let dot = hidden
            .iter()
            .filter_map(|s| lists::status(s))
            .min_by_key(|st| match st {
                lists::Status::Approval | lists::Status::SignIn | lists::Status::Plan => 0,
                lists::Status::Failed | lists::Status::Limited => 1,
                lists::Status::Done | lists::Status::Woke => 2,
                lists::Status::Working => 3,
            });
        let name = if matches!(&self.renaming, Some(Renaming::Project(f)) if f == folder) {
            self.rename_field().into_any_element()
        } else {
            ui::one_line(
                ui::text(faces, self.project_name(folder))
                    .text_size(px(crate::style::BODY))
                    .weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme::text().opacity(0.9)),
            )
            .flex_1()
            .into_any_element()
        };
        let new = folder.to_path_buf();
        let (desk, drop, onto) = (
            desk.clone(),
            desk.clone(),
            Row::Project(folder.to_path_buf()),
        );
        item.child(
            div()
                .id(group.clone())
                .relative()
                .group(group.clone())
                .drag_over::<Dragged>(|st, _, _, _| st.bg(theme::row_hover()))
                .on_drop(move |d: &Dragged, _, cx| {
                    drop.update(cx, |this, cx| this.dropped(d.key, &onto, cx));
                })
                .flex()
                .items_center()
                .gap(px(10.))
                .child(crate::desk::projects::project_icon(
                    folder,
                    self.icon_of(folder),
                    16.,
                ))
                .child(name)
                .when(self.prefs(folder).pinned, |d| {
                    d.child(ui::icon("pin", 12.).flex_none().text_color(theme::faint()))
                })
                // At rest, at its end: folded, the dot for the one that
                // most needs you and how many it hides; open, the chevron.
                // On hover, over them on the row's own ground, a new session
                // in it and the chevron. Hover shows and hides them, never
                // lays them out: GPUI reads the pointer again between laying
                // out a frame and painting it, and a child laid out as gone
                // but painted as there panics.
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(8.))
                        .group_hover(group.clone(), gpui::Styled::invisible)
                        .children(dot.map(|st| ui::dot(st.color())))
                        .when(!hidden.is_empty(), |d| {
                            d.child(ui::fine(faces, hidden.len().to_string()))
                        })
                        .when(
                            hidden.is_empty() && !entry.item().children.is_empty(),
                            |d| d.child(chevron(open)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .right_0()
                        .h_full()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .pl(px(4.))
                        .bg(theme::rail().blend(theme::row_hover()))
                        .invisible()
                        .group_hover(group.clone(), gpui::Styled::visible)
                        .child(
                            Button::new(SharedString::from(format!("new-in-{}", folder.display())))
                                .ghost()
                                .xsmall()
                                .child(ui::icon("square-pen", 14.))
                                .accessibility_label(format!(
                                    "New session in {}",
                                    self.project_name(folder)
                                ))
                                .tooltip(format!("New session in {}", self.project_name(folder)))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    desk.update(cx, |d, cx| d.new_in(new.clone(), window, cx));
                                }),
                        )
                        .when(!entry.item().children.is_empty(), |d| {
                            d.child(chevron(open))
                        }),
                ),
        )
    }

    /// A session: in its folder, in under the project's name; on top or on
    /// a shelf, slim. Its agent's logo, its title, heavier while it
    /// wants you; at its end where it stands or when you last used it, and
    /// on hover Snooze and Settle (or Wake, Un-settle).
    fn session_row(
        &self,
        faces: &crate::fonts::Faces,
        item: ListItem,
        s: &Session,
        in_folder: bool,
        desk: &Entity<Self>,
    ) -> ListItem {
        let key = s.key;
        let open = self.current == Some(key);
        let group = SharedString::from(format!("srow-{key}"));
        let status = lists::status(s);
        let now = SystemTime::now();
        let title = s.shown_title().to_owned();
        // Not begun: marked as a draft, as t3code marks one, not as a
        // session with an agent's answer and a time.
        let draft = s.state == State::Draft && s.items.is_empty();
        // It recedes while it works or waits, and once read and finished.
        let wants = status.is_some_and(|st| st != lists::Status::Working);
        let name = if self.renaming == Some(Renaming::Session { key, here: false }) {
            self.rename_field().into_any_element()
        } else {
            ui::one_line(
                ui::text(faces, title.clone())
                    .text_size(px(crate::style::BODY))
                    .weight(if wants {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    }),
            )
            .flex_1()
            .when(status == Some(lists::Status::Working), |d| d.opacity(0.7))
            .into_any_element()
        };
        let number = self
            .side_order
            .iter()
            .position(|k| *k == key)
            .filter(|n| self.numbers_shown && *n < 9);
        let end = match (number, status) {
            (Some(n), _) => ui::fine(faces, crate::keys::written(&[crate::keys::numbered(n + 1)])),
            (None, Some(st)) => ui::text(faces, st.word())
                .text_size(px(crate::style::CAPTION))
                .text_color(st.color()),
            (None, None) if draft => ui::fine(faces, "Draft"),
            (None, None) => ui::fine(faces, compact(lists::used(s), now)),
        };
        // On hover: put it away a while, or done; back, on a shelf.
        let snoozed = s.meta.flags.snoozed.is_some_and(|t| t > store::secs(now));
        let settled = s.meta.flags.settled.is_some();
        let act = |id: &str, icon: &str, tip: &str| {
            Button::new(SharedString::from(format!("{id}-{key}")))
                .ghost()
                .xsmall()
                .child(ui::icon(icon, 14.))
                .accessibility_label(tip.to_owned())
                .tooltip(tip.to_owned())
        };
        let pinned = s.meta.flags.pinned.is_some();
        let unpin = desk.clone();
        let on_hover = div().flex().items_center().gap(px(2.)).when(pinned, |d| {
            d.child(act("unpin", "pin", "Unpin").on_click(move |_, _, cx| {
                cx.stop_propagation();
                unpin.update(cx, |d, cx| d.unpin(vec![key], cx));
            }))
        });
        let on_hover = if snoozed || settled {
            let desk = desk.clone();
            on_hover.child(
                act("back", "undo-2", if snoozed { "Wake" } else { "Un-settle" }).on_click(
                    move |_, _, cx| {
                        cx.stop_propagation();
                        desk.update(cx, |d, cx| {
                            if snoozed {
                                d.wake(key, cx);
                            } else {
                                d.toggle_settled(key, cx);
                            }
                        });
                    },
                ),
            )
        } else {
            let (snooze, settle) = (desk.clone(), desk.clone());
            on_hover
                .child(
                    act("snooze", "clock", "Snooze")
                        .dropdown_menu(move |menu, _, cx| snooze_items(menu, &snooze, &[key], cx)),
                )
                .child(
                    act(
                        "settle",
                        "check",
                        "Settle · drag across rows to settle them too",
                    )
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        settle.update(cx, |d, cx| d.sweep_from(key, cx));
                    })
                    .on_click(|_, _, cx| cx.stop_propagation()),
                )
        };
        // On hover, a card: its whole title, its agent, where it works, its
        // branch, and the last thing the agent said.
        let mut card = format!(
            "{}\n{} · {}",
            s.meta.title,
            self.name_of(&s.meta.agent),
            Desk::shown(&s.meta.folder)
        );
        if let Some(b) = &s.branch {
            card.push_str(" · ");
            card.push_str(b);
        }
        if let Some(said) = s.items.iter().rev().find_map(|i| match i {
            crate::session::Item::Agent(t) => Some(gist(t)).filter(|g| !g.is_empty()),
            _ => None,
        }) {
            card.push_str("\n\n");
            card.push_str(&said);
        }
        let dragged = Dragged {
            key,
            title: SharedString::from(ui::fit(&title, 40)),
        };
        let (click, drop, files) = (desk.clone(), desk.clone(), desk.clone());
        let (over, up, up_out) = (desk.clone(), desk.clone(), desk.clone());
        item.when(in_folder, |d| d.pl(px(36.)))
            .when(
                self.chosen.contains(&key) || self.sweep.contains(&key),
                |d| d.bg(theme::row_chosen()),
            )
            .child(
                div()
                    .id(group.clone())
                    .relative()
                    .group(group.clone())
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .tooltip(ui::tip(card))
                    // Its agent's logo, faint until the row is hovered.
                    .child(
                        div()
                            .flex_none()
                            .opacity(if open { 1. } else { 0.5 })
                            .group_hover(group.clone(), |st| st.opacity(1.))
                            .child(if draft {
                                ui::icon("square-pen", 16.)
                                    .text_color(theme::accent())
                                    .into_any_element()
                            } else {
                                ui::mark(faces, &self.mark_of(&s.meta.agent), 16.)
                            }),
                    )
                    .when(pinned, |d| {
                        d.child(ui::icon("pin", 12.).flex_none().text_color(theme::faint()))
                    })
                    .child(name)
                    .when(!s.meta.tags.is_empty(), |d| {
                        d.child(
                            ui::text(faces, tagged("", &s.meta.tags).trim().to_owned())
                                .flex_none()
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::muted()),
                        )
                    })
                    .children(s.git.pr.as_ref().map(|pr| {
                        ui::text(faces, format!("#{}", pr.number))
                            .flex_none()
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::muted())
                    }))
                    .child(div().flex_none().child(end))
                    // On hover the buttons cover where it stands, on the
                    // row's own ground.
                    .child(
                        div()
                            .absolute()
                            .right_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .pl(px(4.))
                            .bg(theme::rail().blend(if open {
                                theme::row_open()
                            } else {
                                theme::row_hover()
                            }))
                            .invisible()
                            .group_hover(group, gpui::Styled::visible)
                            .child(on_hover),
                    )
                    // Settle held down and dragged over it settles it too;
                    // let go anywhere, all it passed are settled.
                    .on_mouse_move(move |e: &gpui::MouseMoveEvent, _, cx| {
                        if e.pressed_button == Some(gpui::MouseButton::Left) {
                            over.update(cx, |d, cx| d.sweep_over(key, cx));
                        }
                    })
                    .on_mouse_up(gpui::MouseButton::Left, move |_, _, cx| {
                        up.update(cx, Desk::sweep_end);
                    })
                    .on_mouse_up_out(gpui::MouseButton::Left, move |_, _, cx| {
                        up_out.update(cx, Desk::sweep_end);
                    })
                    // Dragged onto another row, it takes that row's place
                    // in the lists.
                    .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
                    .drag_over::<Dragged>(|st, _, _, _| st.bg(theme::row_hover()))
                    .on_drop(move |d: &Dragged, _, cx| {
                        drop.update(cx, |this, cx| this.dropped(d.key, &Row::Session(key), cx));
                    })
                    // Files dropped on it go with what you write next there.
                    .drag_over::<gpui::ExternalPaths>(|st, _, _, _| st.bg(theme::row_hover()))
                    .on_drop(move |p: &gpui::ExternalPaths, window, cx| {
                        files.update(cx, |this, cx| {
                            this.show(key, window, cx);
                            this.drop_files(p.paths(), window, cx);
                        });
                    }),
            )
            .on_click(move |e, window, cx| {
                click.update(cx, |this, cx| this.side_click(key, e, window, cx));
            })
    }

    /// The session `moved` dropped on the row `on`, as t3code's sections
    /// take it: onto a pinned one, pinned there; onto Settled or a settled
    /// one, settled; onto a folder or a session in one, back among the
    /// active (unpinned, un-settled, woken). Nothing is snoozed by a drop,
    /// which needs a time.
    pub(crate) fn dropped(&mut self, moved: u64, on: &Row, cx: &mut Context<Self>) {
        let flags = |key: u64| self.sessions.at(key).map(|s| s.meta.flags.clone());
        match on {
            Row::Session(key) if flags(*key).is_some_and(|f| f.pinned.is_some()) => {
                if let Some(s) = self.sessions.at_mut(moved) {
                    s.meta.flags.settled = None;
                    s.meta.flags.snoozed = None;
                }
                self.move_session(moved, *key, cx);
            }
            Row::Shelf(Shelf::Settled) => self.undoable(lists::Undone::Settle, &[moved], cx),
            Row::Session(key) if flags(*key).is_some_and(|f| f.settled.is_some()) => {
                self.undoable(lists::Undone::Settle, &[moved], cx);
            }
            Row::Session(_) | Row::Project(_) => self.mark(moved, cx, |f| {
                f.pinned = None;
                f.settled = None;
                f.snoozed = None;
            }),
            Row::Shelf(Shelf::Snoozed) | Row::More => {}
        }
    }

    /// Keys in the list, past those the tree takes (↑ ↓ → ←, ⇧F10):
    /// Return or Space opens the session chosen, F2 renames it or its
    /// project, Home and End go to the first and last row. Whether one
    /// was taken.
    pub(crate) fn side_key(
        &mut self,
        k: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if k.modifiers.modified() {
            return false;
        }
        let (row, len) = {
            let tree = self.side.read(cx);
            // The rows shown: at most every row the shape holds.
            let len = (0..self.side_shape.len())
                .take_while(|ix| tree.entry(*ix).is_some())
                .count();
            let row = tree.selected_entry().and_then(|e| Row::of(&e.item().id));
            (row, len)
        };
        match (k.key.as_str(), row) {
            ("enter" | "space", Some(Row::Session(key))) => self.show(key, window, cx),
            ("f2", Some(Row::Session(key))) => {
                self.begin_rename(Renaming::Session { key, here: false }, window, cx);
            }
            ("f2", Some(Row::Project(folder))) => {
                self.begin_rename(Renaming::Project(folder), window, cx);
            }
            ("home" | "end", _) if len > 0 => {
                let ix = if k.key == "home" { 0 } else { len - 1 };
                self.side.update(cx, |tree, cx| {
                    tree.set_selected_index(Some(ix), cx);
                    tree.scroll_to_item(ix, gpui::ScrollStrategy::Nearest);
                });
            }
            _ => return false,
        }
        true
    }

    /// ⌘ held a moment shows each of the first nine sessions' number, to
    /// go to it with ⌘ and that digit; let go, their times again.
    pub(crate) fn command_held(&mut self, held: bool, cx: &mut Context<Self>) {
        if !held {
            if self.numbers_shown {
                self.numbers_shown = false;
                cx.notify();
            }
            self.command_down = None;
            return;
        }
        if self.command_down.is_some() {
            return;
        }
        let at = std::time::Instant::now();
        self.command_down = Some(at);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.command_down == Some(at) {
                    this.numbers_shown = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A session's row clicked: with ⌘ chosen among others (or no longer),
    /// with ⇧ the run from the last chosen to it; else opened.
    fn side_click(
        &mut self,
        key: u64,
        e: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let m = e.modifiers();
        if m.platform {
            if !self.chosen.remove(&key) {
                self.chosen.insert(key);
            }
        } else if m.shift
            && let Some(&from) = self.chosen.last().or(self.current.as_ref())
        {
            let at = |k: u64| self.side_order.iter().position(|o| *o == k);
            if let (Some(a), Some(b)) = (at(from), at(key)) {
                let run = self.side_order[a.min(b)..=a.max(b)].to_vec();
                self.chosen.extend(run);
            }
        } else {
            self.chosen.clear();
            self.show(key, window, cx);
        }
        cx.notify();
    }

    /// The menu of the row `id`, on a right click or Shift+F10: a
    /// session's, or with others chosen theirs together; a project's. What
    /// it says is read from the desk first, then built with the menu's own
    /// context, which its submenus need.
    pub(crate) fn side_menu(
        desk: &Entity<Self>,
        id: &str,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let plan = {
            let d = desk.read(cx);
            match Row::of(id) {
                Some(Row::Session(key)) if d.chosen.len() > 1 && d.chosen.contains(&key) => {
                    Plan::Chosen(d.chosen.iter().copied().collect())
                }
                Some(Row::Session(key)) => match d.sessions.at(key) {
                    Some(s) => Plan::Session(d.facts(s)),
                    None => return menu,
                },
                Some(Row::Project(folder)) => {
                    let pinned = d.prefs(&folder).pinned;
                    Plan::Project(folder, pinned)
                }
                _ => return menu,
            }
        };
        match plan {
            Plan::Session(f) => session_menu(menu, &f, desk, window, cx),
            Plan::Chosen(keys) => chosen_menu(menu, &keys, desk, window, cx),
            Plan::Project(folder, pinned) => project_menu(menu, &folder, pinned, desk),
        }
    }

    /// What a session's menu says of it.
    fn facts(&self, s: &Session) -> Facts {
        let now = store::secs(SystemTime::now());
        let busy = matches!(s.state, State::Working | State::Opening);
        Facts {
            key: s.key,
            pinned: s.meta.flags.pinned.is_some(),
            settled: s.meta.flags.settled.is_some(),
            snoozed: s.meta.flags.snoozed.is_some_and(|t| t > now),
            unread: s.meta.flags.unread,
            kept: s.meta.flags.kept,
            busy,
            fork: self.can_fork(s) && !busy,
            others: if s.meta.id.is_some() {
                self.others(&s.meta.agent)
            } else {
                Vec::new()
            },
            new_on: s
                .meta
                .worktree
                .is_some()
                .then(|| s.branch.clone().unwrap_or_else(|| "its worktree".into())),
            folder: s.meta.folder.display().to_string(),
            branch: s.branch.clone(),
            id: s.meta.id.as_ref().map(|id| id.0.to_string()),
            link: (s.meta.id.as_ref())
                .map(|id| crate::desk::links::Link::to_session(&s.meta.agent, &id.0)),
        }
    }
}

/// What a row's menu is of.
enum Plan {
    Session(Facts),
    Chosen(Vec<u64>),
    Project(PathBuf, bool),
}

/// What a session's menu says of it, read from the desk before it is built.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a fact of its own about the session, not a state among several"
)]
struct Facts {
    key: u64,
    pinned: bool,
    settled: bool,
    snoozed: bool,
    unread: bool,
    kept: bool,
    busy: bool,
    fork: bool,
    /// The other agents it could go on with, by id and name.
    others: Vec<(String, String)>,
    /// The branch a new session on its worktree would work on.
    new_on: Option<String>,
    folder: String,
    branch: Option<String>,
    id: Option<String>,
    /// The `ghost://` link that opens it.
    link: Option<String>,
}

/// An item of a menu: its label, its icon, and what it does.
fn item(
    label: impl Into<gpui::SharedString>,
    icon: &str,
    f: impl Fn(&mut Window, &mut App) + 'static,
) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .icon(ui::menu_glyph(icon))
        .on_click(move |_, window, cx| f(window, cx))
}

/// An item that copies `text`.
fn copy_item(label: &str, text: String) -> PopupMenuItem {
    item(label.to_owned(), "copy", move |_, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
    })
}

/// What can be done with a session, as t3code's menu has it.
fn session_menu(
    menu: PopupMenu,
    f: &Facts,
    desk: &Entity<Desk>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let key = f.key;
    let on = |g: fn(&mut Desk, u64, &mut Window, &mut Context<Desk>)| {
        let desk = desk.clone();
        move |window: &mut Window, cx: &mut App| desk.update(cx, |d, cx| g(d, key, window, cx))
    };
    let mut menu = menu
        .item(item(
            "Rename",
            "pencil",
            on(|d, key, window, cx| {
                d.begin_rename(Renaming::Session { key, here: false }, window, cx);
            }),
        ))
        .when(f.id.is_some(), |m| {
            m.item(item(
                "Regenerate title",
                "retry",
                on(|d, key, _, cx| d.retitle(key, cx)),
            ))
        })
        .item(item(
            if f.pinned { "Unpin" } else { "Pin" },
            "pin",
            on(|d, key, _, cx| d.toggle_pin(key, cx)),
        ))
        .item(item(
            if f.settled { "Un-settle" } else { "Settle" },
            "check",
            on(|d, key, _, cx| d.toggle_settled(key, cx)),
        ));
    menu = if f.snoozed {
        menu.item(item("Wake", "clock", on(|d, key, _, cx| d.wake(key, cx))))
    } else {
        let desk = desk.clone();
        menu.submenu_with_icon(
            Some(ui::menu_glyph("clock")),
            "Snooze",
            window,
            cx,
            move |menu, _, cx| snooze_items(menu, &desk, &[key], cx),
        )
    };
    if !f.unread {
        menu = menu.item(item(
            "Mark unread",
            "bell",
            on(|d, key, _, cx| d.mark_unread(key, cx)),
        ));
    }
    menu = menu
        .item(item(
            "Find in session",
            "search",
            on(|d, key, window, cx| {
                d.show(key, window, cx);
                if !d.find.open {
                    d.toggle_find(window, cx);
                }
            }),
        ))
        .item(item(
            "Filter by project",
            "folder",
            on(|d, key, _, cx| {
                if let Some(p) = d.sessions.at(key).map(|s| s.meta.project.clone()) {
                    d.filter = vec![d.listed_under(&p)];
                    cx.notify();
                }
            }),
        ))
        .item(item(
            "Project settings",
            "settings",
            on(|d, key, window, cx| {
                if let Some(p) = d.sessions.at(key).map(|s| s.meta.project.clone()) {
                    d.project_settings(&p, window, cx);
                }
            }),
        ));
    if let Some(branch) = &f.new_on {
        menu = menu.item(item(
            format!("New session on {}", ui::fit(branch, 22)),
            "square-pen",
            on(Desk::new_in_worktree),
        ));
    }
    if f.fork {
        menu = menu.item(item(
            "Branch off a copy",
            "branch",
            on(|d, key, _, cx| d.fork(key, cx)),
        ));
    }
    if !f.others.is_empty() {
        let (others, hand) = (f.others.clone(), desk.clone());
        menu = menu.submenu_with_icon(
            Some(ui::menu_glyph("corner-up-right")),
            "Continue with",
            window,
            cx,
            move |mut menu, _, _| {
                for (id, name) in &others {
                    let (desk, id) = (hand.clone(), id.clone());
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                        move |_, window, cx| {
                            desk.update(cx, |d, cx| d.hand_off(key, &id, window, cx));
                        },
                    ));
                }
                menu
            },
        );
    }
    let (copy_desk, folder, branch, id, link) = (
        desk.clone(),
        f.folder.clone(),
        f.branch.clone(),
        f.id.clone(),
        f.link.clone(),
    );
    menu = menu.separator().submenu_with_icon(
        Some(ui::menu_glyph("copy")),
        "Copy",
        window,
        cx,
        move |menu, _, _| {
            let desk = copy_desk.clone();
            let menu = menu
                .item(item("The conversation", "copy", move |_, cx| {
                    desk.update(cx, |d, cx| d.copy_thread(key, cx));
                }))
                .item(copy_item("Path", folder.clone()));
            let menu = match &branch {
                Some(b) => menu.item(copy_item("Branch", b.clone())),
                None => menu,
            };
            let menu = match &link {
                Some(l) => menu.item(copy_item("Link", l.clone())),
                None => menu,
            };
            match &id {
                Some(id) => menu.item(copy_item("Session ID", id.clone())),
                None => menu,
            }
        },
    );
    menu = menu.separator().item(item(
        if f.kept {
            "Let it settle on its own"
        } else {
            "Never settle on its own"
        },
        "circle-dot",
        on(|d, key, _, cx| d.mark(key, cx, |f| f.kept = !f.kept)),
    ));
    if !f.busy {
        menu = menu
            .item(item(
                "Archive",
                "archive",
                on(|d, key, _, cx| d.archive(key, cx)),
            ))
            .item(item(
                "Delete",
                "trash",
                on(|d, key, window, cx| d.ask_delete(vec![key], window, cx)),
            ));
    }
    menu
}

/// What can be done with the sessions chosen, all at once.
fn chosen_menu(
    menu: PopupMenu,
    keys: &[u64],
    desk: &Entity<Desk>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let n = keys.len();
    let each = |label: &str, icon: &str, f: fn(&mut Desk, Vec<u64>, &mut Context<Desk>)| {
        let (desk, keys) = (desk.clone(), keys.to_vec());
        item(format!("{label} ({n})"), icon, move |_, cx| {
            desk.update(cx, |d, cx| f(d, keys.clone(), cx));
        })
    };
    let (snooze, gone, keys) = (desk.clone(), desk.clone(), keys.to_vec());
    menu.item(each("Unpin", "pin", Desk::unpin))
        .item(each("Settle", "check", |d, keys, cx| {
            d.undoable(lists::Undone::Settle, &keys, cx);
        }))
        .item(each("Mark unread", "bell", |d, keys, cx| {
            for key in keys {
                d.mark_unread(key, cx);
            }
            d.chosen.clear();
        }))
        .submenu_with_icon(
            Some(ui::menu_glyph("clock")),
            format!("Snooze ({n})"),
            window,
            cx,
            move |menu, _, cx| snooze_items(menu, &snooze, &keys, cx),
        )
        .separator()
        .item(item(format!("Delete ({n})"), "trash", move |window, cx| {
            gone.update(cx, |d, cx| {
                let keys = std::mem::take(&mut d.chosen).into_iter().collect();
                d.ask_delete(keys, window, cx);
            });
        }))
}

/// What can be done with a project, as t3code's menu has it: begin a
/// session in it, copy its path, pin it, and its settings.
fn project_menu(menu: PopupMenu, folder: &Path, pinned: bool, desk: &Entity<Desk>) -> PopupMenu {
    let on = |f: fn(&mut Desk, &Path, &mut Window, &mut Context<Desk>)| {
        let (desk, folder) = (desk.clone(), folder.to_path_buf());
        move |window: &mut Window, cx: &mut App| desk.update(cx, |d, cx| f(d, &folder, window, cx))
    };
    menu.item(item(
        "New session",
        "square-pen",
        on(|d, f, window, cx| d.new_in(f.to_path_buf(), window, cx)),
    ))
    .item(copy_item("Copy path", folder.display().to_string()))
    .item(item(
        if pinned {
            "Unpin project"
        } else {
            "Pin project"
        },
        "pin",
        on(|d, f, _, cx| d.toggle_project_pin(f, cx)),
    ))
    .item(item(
        "Project settings",
        "settings",
        on(Desk::project_settings),
    ))
}

/// The ways to snooze the sessions `keys`, as menu items.
fn snooze_items(menu: PopupMenu, desk: &Entity<Desk>, keys: &[u64], cx: &App) -> PopupMenu {
    let presets = desk.read(cx).snoozes();
    let menu = presets.into_iter().fold(menu, |menu, (label, later)| {
        let (desk, keys) = (desk.clone(), keys.to_vec());
        menu.item(item(&label, "clock", move |_, cx| {
            desk.update(cx, |d, cx| d.snooze(&keys, later, cx));
        }))
    });
    let (desk, keys) = (desk.clone(), keys.to_vec());
    menu.separator()
        .item(item("Custom…", "clock", move |window, cx| {
            desk.update(cx, |d, cx| d.snooze_page(keys.clone(), window, cx));
        }))
}

/// A folder's chevron at its end: down when open, turned when folded.
fn chevron(open: bool) -> gpui::Svg {
    ui::icon(
        if open {
            "chevron-down"
        } else {
            "chevron-right"
        },
        12.,
    )
    .flex_none()
    .text_color(theme::faint())
}

/// The input in place of a row's name, while it is renamed.
impl Desk {
    pub(crate) fn rename_field(&self) -> gpui::Div {
        div()
            .flex_1()
            .min_w(px(0.))
            .child(Input::new(&self.rename).xsmall())
    }
}

/// The desk's tree for the sidebar, its folders' opening and folding kept
/// by the desk.
pub(crate) fn side_tree(window: &mut Window, cx: &mut Context<Desk>) -> Entity<TreeState> {
    let _ = window;
    let tree = cx.new(|cx| TreeState::new(cx));
    cx.subscribe(
        &tree,
        |this, _, e: &gpui::component::tree::TreeEvent, cx| match e {
            gpui::component::tree::TreeEvent::Expanded(id) => this.side_folded(id, true, cx),
            gpui::component::tree::TreeEvent::Collapsed(id) => this.side_folded(id, false, cx),
        },
    )
    .detach();
    tree
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_found_by_its_tags() {
        let now = SystemTime::now();
        let title = tagged("Fix the build", &["ci".into(), "urgent".into()]);
        assert_eq!(title, "Fix the build #ci #urgent");
        let found = |typed: &str| {
            rank::session(
                &rank::Query::new(typed),
                &title,
                None,
                None,
                "p",
                now,
                now,
                false,
            )
            .is_some()
        };
        assert!(found("#urgent"));
        assert!(!found("#later"));
    }

    #[test]
    fn each_row_reads_back_from_its_id() {
        for row in [
            Row::Project(PathBuf::from("/a/b c")),
            Row::Session(42),
            Row::Shelf(Shelf::Snoozed),
            Row::Shelf(Shelf::Settled),
            Row::More,
        ] {
            assert_eq!(Row::of(&row.id()), Some(row));
        }
    }

    #[test]
    fn a_time_is_said_in_its_largest_whole_unit() {
        let now = SystemTime::now();
        let ago = |s: u64| compact(now - std::time::Duration::from_secs(s), now);
        assert_eq!(ago(30), "now");
        assert_eq!(ago(300), "5m");
        assert_eq!(ago(3 * 3600), "3h");
        assert_eq!(ago(2 * 86_400), "2d");
    }
}
