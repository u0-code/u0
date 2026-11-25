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
    Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    list::ListItem,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    tree::{TreeEntry, TreeItem, TreeState},
};
use gpui::{App, Context, Entity, SharedString, Window, div, prelude::*, px};

use super::{Dragged, gist};
use crate::{
    desk::{Desk, Later, Renaming, lists, rank},
    fonts::Weighted as _,
    session::{Session, State},
    store, theme, ui,
};

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
    /// Give the sidebar's tree its rows, where they changed; the row open
    /// is the session on screen.
    pub(crate) fn sync_side(&mut self, cx: &mut Context<Self>) {
        let (items, order) = self.side_items(cx);
        let mut drawn = String::new();
        shape(&items, &mut drawn);
        let current = self.current;
        if drawn != self.side_shape || current != self.side_current {
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
                &s.meta.title,
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
                s.meta.project == folder
                    && self.listed_now(s)
                    && s.meta.flags.settled.is_none()
                    && s.meta.flags.pinned.is_none()
            })
            .collect();
        here.sort_by_key(|s| std::cmp::Reverse(lists::used(s)));
        here
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
        let mut projects: Vec<(PathBuf, SystemTime)> = self
            .projects()
            .into_iter()
            .filter(|p| self.filter.is_empty() || self.filter.contains(p))
            .map(|p| {
                let last = self
                    .sessions
                    .all
                    .iter()
                    .filter(|s| s.meta.project == p)
                    .map(lists::used)
                    .max()
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (p, last)
            })
            .collect();
        projects.sort_by(|(a, at), (b, bt)| {
            (
                !self.prefs(a).pinned,
                std::cmp::Reverse(*at),
                self.project_name(a),
            )
                .cmp(&(
                    !self.prefs(b).pinned,
                    std::cmp::Reverse(*bt),
                    self.project_name(b),
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

    /// A row of the tree, drawn from what the desk knows now.
    pub(crate) fn side_row(&self, entry: &TreeEntry, desk: &Entity<Self>, cx: &App) -> ListItem {
        let faces = crate::fonts::faces(cx).clone();
        let id = entry.item().id.clone();
        let item = ListItem::new(SharedString::from(format!("row-{id}")))
            .h(px(36.))
            .px(px(10.))
            .rounded(px(8.));
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
                item.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            ui::text(&faces, format!("{kind:?}"))
                                .text_size(px(12.5))
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
                            .text_size(px(13.))
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
                lists::Status::Failed => 1,
                lists::Status::Done => 2,
                lists::Status::Working => 3,
            });
        let name = if matches!(&self.renaming, Some(Renaming::Project(f)) if f == folder) {
            self.rename_field().into_any_element()
        } else {
            ui::one_line(
                ui::text(faces, self.project_name(folder))
                    .text_size(px(14.))
                    .weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme::text().opacity(0.9)),
            )
            .flex_1()
            .into_any_element()
        };
        let new = folder.to_path_buf();
        let desk = desk.clone();
        item.child(
            div()
                .id(group.clone())
                .group(group.clone())
                .flex()
                .items_center()
                .gap(px(10.))
                .child(crate::desk::projects::project_icon(
                    folder,
                    self.held.get(folder).and_then(|h| h.icon.clone()),
                    16.,
                ))
                .child(name)
                .when(self.prefs(folder).pinned, |d| {
                    d.child(ui::icon("pin", 12.).flex_none().text_color(theme::faint()))
                })
                .when(!hidden.is_empty(), |d| {
                    d.child(
                        ui::fine(faces, hidden.len().to_string())
                            .group_hover(group.clone(), gpui::Styled::invisible),
                    )
                })
                .children(dot.map(|st| ui::dot(st.color())))
                .child(
                    div()
                        .invisible()
                        .group_hover(group.clone(), gpui::Styled::visible)
                        .child(
                            Button::new(SharedString::from(format!("new-in-{}", folder.display())))
                                .ghost()
                                .xsmall()
                                .child(ui::icon("square-pen", 14.))
                                .tooltip(format!("New session in {}", self.project_name(folder)))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    desk.update(cx, |d, cx| d.new_in(new.clone(), window, cx));
                                }),
                        ),
                )
                .when(!entry.item().children.is_empty(), |d| {
                    d.child(chevron(open))
                }),
        )
    }

    /// A session: in its folder, in under the project's name; on top or on
    /// a shelf, slim, with its project's icon. Its title, heavier while it
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
        let title = if s.state == State::Draft && s.items.is_empty() {
            "New session".to_owned()
        } else {
            s.meta.title.clone()
        };
        // It recedes while it works or waits, and once read and finished.
        let wants = matches!(
            status,
            Some(
                lists::Status::Approval
                    | lists::Status::SignIn
                    | lists::Status::Failed
                    | lists::Status::Plan
                    | lists::Status::Done
            )
        );
        let name = if self.renaming == Some(Renaming::Session { key, here: false }) {
            self.rename_field().into_any_element()
        } else {
            ui::one_line(
                ui::text(faces, title.clone())
                    .text_size(px(14.))
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
                .text_size(px(12.))
                .text_color(st.color()),
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
                .tooltip(tip.to_owned())
        };
        let on_hover = div().flex().items_center().gap(px(2.));
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
                        .dropdown_menu(move |menu, _, _| snooze_items(menu, &snooze, &[key])),
                )
                .child(act("settle", "check", "Settle").on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    settle.update(cx, |d, cx| d.toggle_settled(key, cx));
                }))
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
        let pinned = s.meta.flags.pinned.is_some();
        let (click, drop) = (desk.clone(), desk.clone());
        item.when(in_folder, |d| d.pl(px(36.)))
            .when(self.chosen.contains(&key), |d| d.bg(theme::row_chosen()))
            .child(
                div()
                    .id(group.clone())
                    .relative()
                    .group(group.clone())
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .tooltip(ui::tip(card))
                    .when(!in_folder, |d| {
                        d.child(
                            div()
                                .opacity(0.4)
                                .group_hover(group.clone(), |st| st.opacity(1.))
                                .child(crate::desk::projects::project_icon(
                                    &s.meta.project,
                                    self.held.get(&s.meta.project).and_then(|h| h.icon.clone()),
                                    16.,
                                )),
                        )
                    })
                    .when(pinned, |d| {
                        d.child(ui::icon("pin", 12.).flex_none().text_color(theme::faint()))
                    })
                    .child(name)
                    .children(s.git.pr.as_ref().map(|pr| {
                        ui::text(faces, format!("#{}", pr.number))
                            .flex_none()
                            .text_size(px(11.))
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
                    // Pinned, it is dragged into the order you want.
                    .when(pinned, |d| {
                        d.on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
                            .drag_over::<Dragged>(|st, _, _, _| st.bg(theme::row_hover()))
                            .on_drop(move |d: &Dragged, _, cx| {
                                drop.update(cx, |this, cx| this.move_session(d.key, key, cx));
                            })
                    }),
            )
            .on_click(move |e, window, cx| {
                click.update(cx, |this, cx| this.side_click(key, e, window, cx));
            })
    }

    /// Keys in the list, past those the tree takes (↑ ↓ → ←): Return
    /// opens the session chosen, F2 renames it or its project, Home and
    /// End go to the first and last row. Whether one was taken.
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
            ("enter", Some(Row::Session(key))) => self.show(key, window, cx),
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

    /// The menu of the row `id`, on a right click: a session's, or with
    /// others chosen theirs together; a project's.
    pub(crate) fn side_menu(&self, id: &str, menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        match Row::of(id) {
            Some(Row::Session(key)) if self.chosen.len() > 1 && self.chosen.contains(&key) => {
                self.chosen_menu(menu, desk)
            }
            Some(Row::Session(key)) => match self.sessions.at(key) {
                Some(s) => self.session_menu(menu, s, desk),
                None => menu,
            },
            Some(Row::Project(folder)) => self.project_menu(menu, &folder, desk),
            _ => menu,
        }
    }

    /// What can be done with a session, as t3code's menu has it.
    fn session_menu(&self, menu: PopupMenu, s: &Session, desk: &Entity<Self>) -> PopupMenu {
        let key = s.key;
        let busy = matches!(s.state, State::Working | State::Opening);
        let now = store::secs(SystemTime::now());
        let item =
            |label: String, icon: &str, f: fn(&mut Desk, u64, &mut Window, &mut Context<Desk>)| {
                let desk = desk.clone();
                PopupMenuItem::new(label)
                    .icon(Icon::empty().path(format!("icons/{icon}.svg")))
                    .on_click(move |_, window, cx| desk.update(cx, |d, cx| f(d, key, window, cx)))
            };
        let copy = |label: &str, text: String| {
            PopupMenuItem::new(label.to_owned())
                .icon(Icon::empty().path("icons/copy.svg"))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                })
        };
        let mut menu = menu
            .item(item("Rename".into(), "pencil", |d, key, window, cx| {
                d.begin_rename(Renaming::Session { key, here: false }, window, cx);
            }))
            .item(item(
                if s.meta.flags.pinned.is_some() {
                    "Unpin"
                } else {
                    "Pin"
                }
                .into(),
                "pin",
                |d, key, _, cx| d.toggle_pin(key, cx),
            ))
            .item(item(
                if s.meta.flags.settled.is_some() {
                    "Un-settle"
                } else {
                    "Settle"
                }
                .into(),
                "check",
                |d, key, _, cx| d.toggle_settled(key, cx),
            ));
        menu = if s.meta.flags.snoozed.is_some_and(|t| t > now) {
            menu.item(item("Wake".into(), "clock", |d, key, _, cx| {
                d.wake(key, cx);
            }))
        } else {
            snooze_items(menu.separator(), desk, &[key]).separator()
        };
        if !s.meta.flags.unread {
            menu = menu.item(item("Mark unread".into(), "bell", |d, key, _, cx| {
                d.mark_unread(key, cx);
            }));
        }
        menu = menu.item(item(
            "Filter by project".into(),
            "folder",
            |d, key, _, cx| {
                if let Some(p) = d.sessions.at(key).map(|s| s.meta.project.clone()) {
                    d.filter = vec![p];
                    cx.notify();
                }
            },
        ));
        if s.meta.worktree.is_some() {
            let on = s.branch.clone().unwrap_or_else(|| "its worktree".into());
            menu = menu.item(item(
                format!("New session on {}", ui::fit(&on, 22)),
                "square-pen",
                Desk::new_in_worktree,
            ));
        }
        if self.can_fork(s) && !busy {
            menu = menu.item(item(
                "Branch off a copy".into(),
                "branch",
                |d, key, _, cx| {
                    d.fork(key, cx);
                },
            ));
        }
        menu = menu
            .separator()
            .item(item(
                "Copy the conversation".into(),
                "copy",
                |d, key, _, cx| {
                    d.copy_thread(key, cx);
                },
            ))
            .item(copy("Copy path", s.meta.folder.display().to_string()));
        if let Some(b) = &s.branch {
            menu = menu.item(copy("Copy branch", b.clone()));
        }
        if let Some(id) = &s.meta.id {
            menu = menu.item(copy("Copy session ID", id.0.to_string()));
        }
        menu = menu.separator().item(item(
            if s.meta.flags.kept {
                "Let it settle on its own"
            } else {
                "Never settle on its own"
            }
            .into(),
            "pin",
            |d, key, _, cx| d.mark(key, cx, |f| f.kept = !f.kept),
        ));
        if !busy {
            menu = menu
                .item(item("Archive".into(), "archive", |d, key, _, cx| {
                    d.archive(key, cx);
                }))
                .item(item("Delete".into(), "trash", |d, key, window, cx| {
                    d.ask_delete(vec![key], window, cx);
                }));
        }
        menu
    }

    /// What can be done with the sessions chosen, all at once.
    fn chosen_menu(&self, menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        let n = self.chosen.len();
        let keys: Vec<u64> = self.chosen.iter().copied().collect();
        let each = |label: &str, icon: &str, f: fn(&mut store::Flags)| {
            let desk = desk.clone();
            PopupMenuItem::new(format!("{label} ({n})"))
                .icon(Icon::empty().path(format!("icons/{icon}.svg")))
                .on_click(move |_, _, cx| desk.update(cx, |d, cx| d.for_chosen(cx, f)))
        };
        let gone = desk.clone();
        snooze_items(
            menu.item(each("Unpin", "pin", |f| f.pinned = None))
                .item(each("Settle", "check", |f| {
                    f.settled = Some(store::secs(SystemTime::now()));
                }))
                .item(each("Mark unread", "bell", |f| f.unread = true))
                .separator(),
            desk,
            &keys,
        )
        .separator()
        .item(
            PopupMenuItem::new(format!("Delete ({n})"))
                .icon(Icon::empty().path("icons/trash.svg"))
                .on_click(move |_, window, cx| {
                    gone.update(cx, |d, cx| {
                        let keys = std::mem::take(&mut d.chosen).into_iter().collect();
                        d.ask_delete(keys, window, cx);
                    });
                }),
        )
    }

    /// What can be done with a project: begin a session in it, copy its
    /// path, pin it, name it, find it, the agent and way of working its
    /// new sessions take, put it away.
    fn project_menu(&self, menu: PopupMenu, folder: &Path, desk: &Entity<Self>) -> PopupMenu {
        let prefs = self.prefs(folder);
        let item = |label: String, icon: &str, f: Box<ForProject>| {
            let (desk, folder) = (desk.clone(), folder.to_path_buf());
            PopupMenuItem::new(label)
                .icon(Icon::empty().path(format!("icons/{icon}.svg")))
                .on_click(move |_, window, cx| {
                    desk.update(cx, |d, cx| f(d, &folder, window, cx));
                })
        };
        let path = folder.display().to_string();
        let mut menu = menu
            .item(item(
                "New session".into(),
                "square-pen",
                Box::new(|d, f, window, cx| d.new_in(f.to_path_buf(), window, cx)),
            ))
            .item(
                PopupMenuItem::new("Copy path")
                    .icon(Icon::empty().path("icons/copy.svg"))
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
                    }),
            )
            .item(item(
                if prefs.pinned {
                    "Unpin project"
                } else {
                    "Pin project"
                }
                .into(),
                "pin",
                Box::new(|d, f, _, cx| d.toggle_project_pin(f, cx)),
            ))
            .item(item(
                "Rename".into(),
                "pencil",
                Box::new(|d, f, window, cx| {
                    d.begin_rename(Renaming::Project(f.to_path_buf()), window, cx);
                }),
            ))
            .item(item(
                "Show in Finder".into(),
                "folder",
                Box::new(|_, f, _, cx| cx.open_with_system(f)),
            ));
        if self.held.get(folder).is_some_and(|h| h.repo) {
            let on = prefs.worktree.unwrap_or(self.settings.worktree);
            menu = menu.item(
                item(
                    "New sessions in their own worktree".into(),
                    "branch",
                    Box::new(move |d, f, _, cx| d.set_project_worktree(f, !on, cx)),
                )
                .checked(on),
            );
        }
        menu = menu.separator().label("New sessions begin with");
        let agents = std::iter::once((None, "The one on screen".to_owned())).chain(
            self.slots
                .iter()
                .map(|s| (Some(s.entry.id.clone()), self.name_of(&s.entry.id))),
        );
        for (agent, name) in agents {
            let on = prefs.agent == agent;
            menu = menu.item(
                item(
                    name,
                    "plug",
                    Box::new(move |d, f, _, cx| d.set_project_agent(f, agent.clone(), cx)),
                )
                .checked(on),
            );
        }
        menu = menu.separator().item(item(
            "Archive".into(),
            "archive",
            Box::new(|d, f, _, cx| d.archive_project(f, cx)),
        ));
        if self.can_forget_project(folder) {
            menu = menu.item(item(
                "Remove from the list".into(),
                "x",
                Box::new(|d, f, _, cx| d.forget_project(f, cx)),
            ));
        }
        menu
    }
}

/// What an item of a project's menu does to it.
type ForProject = dyn Fn(&mut Desk, &Path, &mut Window, &mut Context<Desk>);

/// The ways to snooze the sessions `keys`, as menu items.
fn snooze_items(menu: PopupMenu, desk: &Entity<Desk>, keys: &[u64]) -> PopupMenu {
    [
        ("Snooze for an hour", Later::In(60)),
        ("Snooze until tomorrow", Later::Tomorrow),
        ("Snooze until next week", Later::NextWeek),
    ]
    .into_iter()
    .fold(menu, |menu, (label, later)| {
        let (desk, keys) = (desk.clone(), keys.to_vec());
        menu.item(
            PopupMenuItem::new(label)
                .icon(Icon::empty().path("icons/clock.svg"))
                .on_click(move |_, _, cx| {
                    desk.update(cx, |d, cx| {
                        for key in &keys {
                            d.snooze(*key, later, cx);
                        }
                        d.chosen.clear();
                    });
                }),
        )
    })
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
