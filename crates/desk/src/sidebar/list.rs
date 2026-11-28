//! The Carbon sidebar thread and project list (`AppSidebar` and
//! `SidebarStates` boards): `Pinned`, `Needs you` (oldest wait first),
//! `ServerHeader` + `ProjectFolderHeader` groups, `Other Macs`, `Snoozed`
//! and `Settled` shelves, keyboard navigation, and drag-and-drop.

use std::f32::consts::FRAC_PI_2;
use std::time::SystemTime;

use gpui::component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    kbd::Kbd,
    menu::{ContextMenuExt as _, DropdownMenu as _},
    tree::{TreeItem, TreeState},
};
use gpui::{
    AnyElement, BoxShadow, Context, Div, ElementId, Entity, Focusable as _, SharedString, Stateful,
    Window, div, point, prelude::*, px, radians, rgb,
};
use u0ui::{
    icons::{CarbonIcon, Icon as CarbonGlyph},
    t3::{
        Spin, Status as T3Status, fmt_elapsed,
        review::PullRequestState,
        shell::{
            FolderCounts, IdentitySize, MachineKind, ProjectFavicon, PullRequestMiniBadge,
            SidebarShelfHeader,
        },
    },
    tokens::ColorRole,
    u0::shell::{ServerHeader, ServerState},
};

use super::{Dragged, gist, menus::snooze_items};
use crate::{
    desk::{Desk, Place, Renaming, lists},
    fonts::{Faces, Weighted as _},
    hub::{Host, Up},
    session::{Item, Session},
    settings::{Icon as ProjectIcon, Sort},
    store, theme, ui, when,
};

const SETTLED: usize = 10;
const SETTLED_MORE: usize = 25;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Server(Host),
    Project(Place),
    Session(u64),
    Shelf(Shelf),
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Pinned,
    NeedsYou,
    Snoozed,
    Settled,
}

impl Row {
    pub(super) fn id(&self) -> SharedString {
        match self {
            Self::Server(h) => format!("m:{}", h.0).into(),
            Self::Project(p) => format!("p:{}:{}", p.host.0, p.folder.display()).into(),
            Self::Session(key) => format!("s:{key}").into(),
            Self::Shelf(Shelf::Pinned) => "h:pinned".into(),
            Self::Shelf(Shelf::NeedsYou) => "h:needs".into(),
            Self::Shelf(Shelf::Snoozed) => "h:snoozed".into(),
            Self::Shelf(Shelf::Settled) => "h:settled".into(),
            Self::More => "more".into(),
        }
    }

    pub fn of(id: &str) -> Option<Self> {
        Some(match id.split_once(':') {
            Some(("m", n)) => Self::Server(Host(n.parse().ok()?)),
            Some(("p", place)) => {
                let (n, path) = place.split_once(':')?;
                Self::Project(Place::new(Host(n.parse().ok()?), path))
            }
            Some(("s", key)) => Self::Session(key.parse().ok()?),
            Some(("h", "pinned")) => Self::Shelf(Shelf::Pinned),
            Some(("h", "needs")) => Self::Shelf(Shelf::NeedsYou),
            Some(("h", "snoozed")) => Self::Shelf(Shelf::Snoozed),
            Some(("h", "settled")) => Self::Shelf(Shelf::Settled),
            _ if id == "more" => Self::More,
            _ => return None,
        })
    }
}

fn shelf_default_open(shelf: Shelf) -> bool {
    matches!(shelf, Shelf::Pinned | Shelf::NeedsYou)
}

fn shelf_key(shelf: Shelf) -> String {
    if shelf_default_open(shelf) {
        format!("shelf-folded-{shelf:?}")
    } else {
        format!("shelf-{shelf:?}")
    }
}

fn shelf_is_open(expanded: &std::collections::HashSet<String>, shelf: Shelf) -> bool {
    let toggled = expanded.contains(&shelf_key(shelf));
    if shelf_default_open(shelf) {
        !toggled
    } else {
        toggled
    }
}

fn folded_key(host: Host) -> String {
    format!("server-folded-{}", host.0)
}

fn shape(items: &[TreeItem], out: &mut String) {
    for item in items {
        out.push_str(&item.id);
        out.push(if item.is_expanded() { '+' } else { '-' });
        shape(&item.children, out);
        out.push('\n');
    }
}

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
    pub(crate) fn sync_side(&mut self, window: &Window, cx: &mut Context<Self>) {
        let (items, order) = self.side_items();
        let mut drawn = String::new();
        shape(&items, &mut drawn);
        let current = self.current;
        let away = {
            let tree = self.side.read(cx);
            let open = current.and_then(|k| tree.index_of(&Row::Session(k).id()));
            !tree.focus_handle(cx).contains_focused(window, cx) && tree.selected_index() != open
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

    pub(crate) fn side_folded(&mut self, id: &str, open: bool, cx: &mut Context<Self>) {
        match Row::of(id) {
            Some(Row::Project(p)) if open => {
                self.closed.remove(&p);
            }
            Some(Row::Project(p)) => {
                self.closed.insert(p);
            }
            Some(Row::Shelf(kind)) => {
                let key = shelf_key(kind);
                if open == shelf_default_open(kind) {
                    self.expanded.remove(&key);
                } else {
                    self.expanded.insert(key);
                }
            }
            Some(Row::Server(host)) if open => {
                self.expanded.remove(&folded_key(host));
            }
            Some(Row::Server(host)) => {
                self.expanded.insert(folded_key(host));
            }
            _ => return,
        }
        self.side_current = None;
        cx.notify();
    }

    fn toggle_shelf(&mut self, kind: Shelf, cx: &mut Context<Self>) {
        let now_open = shelf_is_open(&self.expanded, kind);
        self.side_folded(&Row::Shelf(kind).id(), !now_open, cx);
    }

    fn toggle_project_open(&mut self, folder: &Place, cx: &mut Context<Self>) {
        let now_open = !self.closed.contains(folder);
        self.side_folded(&Row::Project(folder.clone()).id(), !now_open, cx);
    }

    fn toggle_server_open(&mut self, host: Host, cx: &mut Context<Self>) {
        let now_open = !self.expanded.contains(&folded_key(host));
        self.side_folded(&Row::Server(host).id(), !now_open, cx);
    }

    fn matches_tags(&self, s: &Session) -> bool {
        self.tag_filter.is_empty()
            || self
                .tag_filter
                .iter()
                .all(|wanted| s.meta.tags.iter().any(|t| t == wanted))
    }

    pub(crate) fn in_project(&self, folder: &Place) -> Vec<&Session> {
        let mut here: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                self.listed_under(&Place::project_of(s)) == *folder
                    && self.listed_now(s)
                    && s.meta.flags.settled.is_none()
                    && s.meta.flags.pinned.is_none()
            })
            .collect();
        here.sort_by(|a, b| self.before(a, b));
        here
    }

    fn before(&self, a: &Session, b: &Session) -> std::cmp::Ordering {
        match self.settings.sort {
            Sort::Name => a
                .meta
                .title
                .to_lowercase()
                .cmp(&b.meta.title.to_lowercase()),
            _ => self.sorted_at(b).cmp(&self.sorted_at(a)),
        }
    }

    fn sorted_at(&self, s: &Session) -> SystemTime {
        match self.settings.sort {
            Sort::Used | Sort::Name => lists::used(s),
            Sort::Updated => s.meta.updated,
            Sort::Begun => s.meta.begun,
        }
    }

    fn ordered_projects(&self) -> Vec<Place> {
        let mut folders: Vec<Place> = Vec::new();
        for p in self.projects() {
            let p = self.listed_under(&p);
            if !folders.contains(&p) {
                folders.push(p);
            }
        }
        let mut projects: Vec<(Place, SystemTime)> = folders
            .into_iter()
            .filter(|p| self.filter.is_empty() || self.filter.contains(p))
            .map(|p| {
                let last = self
                    .sessions
                    .all
                    .iter()
                    .filter(|s| self.listed_under(&Place::project_of(s)) == p)
                    .map(|s| self.sorted_at(s))
                    .max()
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (p, last)
            })
            .collect();
        let named = self.settings.sort == Sort::Name;
        projects.sort_by(|(a, at), (b, bt)| {
            let when = |t: SystemTime| std::cmp::Reverse((!named).then_some(t));
            (
                a.host,
                !self.prefs(a).pinned,
                when(*at),
                self.project_name(a).to_lowercase(),
            )
                .cmp(&(
                    b.host,
                    !self.prefs(b).pinned,
                    when(*bt),
                    self.project_name(b).to_lowercase(),
                ))
        });
        projects.into_iter().map(|(p, _)| p).collect()
    }

    fn side_items(&self) -> (Vec<TreeItem>, Vec<u64>) {
        let secs = store::secs(SystemTime::now());
        let shown = |s: &&Session| {
            (self.filter.is_empty() || self.filter.contains(&Place::project_of(s)))
                && self.matches_tags(s)
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
        if !pinned.is_empty() {
            let open = shelf_is_open(&self.expanded, Shelf::Pinned);
            if open {
                order.extend(pinned.iter().map(|s| s.key));
            }
            items.push(
                TreeItem::new(Row::Shelf(Shelf::Pinned).id(), "Pinned")
                    .expanded(open)
                    .children(pinned.into_iter().map(leaf)),
            );
        }

        let mut needs: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                s.meta.flags.pinned.is_none()
                    && s.meta.flags.settled.is_none()
                    && self.listed_now(s)
                    && lists::needs_you_why(s).is_some()
            })
            .filter(shown)
            .collect();
        needs.sort_by_key(|s| lists::used(s));
        if !needs.is_empty() {
            let open = shelf_is_open(&self.expanded, Shelf::NeedsYou);
            if open {
                order.extend(needs.iter().map(|s| s.key));
            }
            items.push(
                TreeItem::new(Row::Shelf(Shelf::NeedsYou).id(), "Needs you")
                    .expanded(open)
                    .children(needs.into_iter().map(leaf)),
            );
        }

        let projects = self.ordered_projects();
        let grouped = self.machines.len() > 1;
        for m in &self.machines {
            let server_open = !self.expanded.contains(&folded_key(m.host));
            let mut under = Vec::new();
            for folder in projects.iter().filter(|p| p.host == m.host) {
                let sessions: Vec<&Session> = self
                    .in_project(folder)
                    .into_iter()
                    .filter(|s| lists::needs_you_why(s).is_none())
                    .filter(shown)
                    .collect();
                let open = !self.closed.contains(folder);
                if open && (server_open || !grouped) {
                    order.extend(sessions.iter().map(|s| s.key));
                }
                under.push(
                    TreeItem::new(Row::Project(folder.clone()).id(), self.project_name(folder))
                        .expanded(open)
                        .children(sessions.into_iter().map(leaf)),
                );
            }
            if grouped {
                items.push(
                    TreeItem::new(Row::Server(m.host).id(), m.name.clone())
                        .expanded(server_open)
                        .children(under),
                );
            } else {
                items.extend(under);
            }
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
            let open = shelf_is_open(&self.expanded, shelf);
            let most = match shelf {
                Shelf::Settled => self.settled_shown.max(SETTLED),
                _ => usize::MAX,
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

    pub(crate) fn side_list(
        &self,
        faces: &Faces,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let focus = self.side.read(cx).focus_handle(cx);
        let selected_id = focus
            .contains_focused(window, cx)
            .then(|| {
                self.side
                    .read(cx)
                    .selected_entry()
                    .map(|e| e.item().id.clone())
            })
            .flatten();
        let content = self.render_grouped_list(faces, selected_id.as_deref(), cx);

        div()
            .id("sb-list")
            .track_focus(&focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if this.side_key(&e.keystroke, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .children(content)
    }

    fn render_grouped_list(
        &self,
        faces: &Faces,
        selected_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let desk = cx.entity();
        let now = SystemTime::now();
        let secs = store::secs(now);
        let shown_project =
            |s: &&Session| self.filter.is_empty() || self.filter.contains(&Place::project_of(s));
        let mut groups: Vec<AnyElement> = Vec::new();
        let push_group = |groups: &mut Vec<AnyElement>, tight: bool, group: Div| {
            let mt = if groups.is_empty() {
                0.
            } else if tight {
                4.
            } else {
                20.
            };
            groups.push(group.mt(px(mt)).flex_none().into_any_element());
        };

        let mut pinned: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| s.meta.flags.pinned.is_some() && s.meta.flags.settled.is_none())
            .filter(|s| self.listed_now(s) && self.matches_tags(s))
            .filter(shown_project)
            .collect();
        pinned.sort_by_key(|s| s.meta.flags.pinned);
        if !pinned.is_empty() {
            let open = shelf_is_open(&self.expanded, Shelf::Pinned);
            let d = desk.clone();
            let mut grp = div().flex().flex_col().child(
                SidebarShelfHeader::new("sb-shelf-pinned", crate::tr("Pinned"))
                    .count(pinned.len())
                    .expanded(open)
                    .on_toggle(move |_, cx| {
                        d.update(cx, |this, cx| this.toggle_shelf(Shelf::Pinned, cx));
                    }),
            );
            if open {
                for s in pinned {
                    let kbd = selected_id == Some(Row::Session(s.key).id().as_ref());
                    grp = grp.child(self.thread_item(faces, s, None, kbd, &desk, cx));
                }
            }
            push_group(&mut groups, false, grp);
        }

        let mut needs: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                s.meta.flags.pinned.is_none()
                    && s.meta.flags.settled.is_none()
                    && self.listed_now(s)
                    && self.matches_tags(s)
                    && lists::needs_you_why(s).is_some()
            })
            .filter(shown_project)
            .collect();
        needs.sort_by_key(|s| lists::used(s));
        if !needs.is_empty() {
            let open = shelf_is_open(&self.expanded, Shelf::NeedsYou);
            let mut grp = div().flex().flex_col().child(self.needs_you_header(
                faces,
                needs.len(),
                open,
                &desk,
            ));
            if open {
                for s in needs {
                    let why = lists::needs_you_why(s).map(crate::tr).unwrap_or_default();
                    let meta = format!("{} · {why}", self.project_on(&Place::project_of(s)));
                    let kbd = selected_id == Some(Row::Session(s.key).id().as_ref());
                    grp = grp.child(self.thread_item(faces, s, Some(meta), kbd, &desk, cx));
                }
            }
            push_group(&mut groups, false, grp);
        }

        let projects = self.ordered_projects();
        let multi_machine = self.machines.len() > 1;
        for m in &self.machines {
            let host_projects: Vec<&Place> = projects.iter().filter(|p| p.host == m.host).collect();
            if host_projects.is_empty() {
                continue;
            }
            let server_open = !self.expanded.contains(&folded_key(m.host));
            if multi_machine {
                let server_head =
                    self.render_server_header(m, host_projects.len(), server_open, &desk);
                push_group(&mut groups, false, server_head);
            }
            if multi_machine && !server_open {
                continue;
            }
            for folder in host_projects {
                let all_active = self.in_project(folder);
                let counts = folder_counts(&all_active);
                let unfiltered_len = all_active
                    .iter()
                    .filter(|s| lists::needs_you_why(s).is_none())
                    .count();
                let visible: Vec<&Session> = all_active
                    .into_iter()
                    .filter(|s| lists::needs_you_why(s).is_none() && self.matches_tags(s))
                    .collect();
                let hidden_by_tag = unfiltered_len.saturating_sub(visible.len());
                let open = !self.closed.contains(folder);
                let mut grp = div()
                    .flex()
                    .flex_col()
                    .child(self.project_header(faces, folder, counts, open, &desk, cx));
                if open {
                    for s in visible {
                        let kbd = selected_id == Some(Row::Session(s.key).id().as_ref());
                        grp = grp.child(self.thread_item(faces, s, None, kbd, &desk, cx));
                    }
                    if !self.tag_filter.is_empty() && hidden_by_tag > 0 {
                        let tags_str = self
                            .tag_filter
                            .iter()
                            .map(|t| format!("#{t}"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let clear_desk = desk.clone();
                        grp = grp.child(
                            div()
                                .id(SharedString::from(format!(
                                    "hidden-tags-{}-{}",
                                    folder.host.0,
                                    folder.folder.display()
                                )))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .h(px(32.))
                                .pl(px(36.))
                                .pr(px(12.))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    clear_desk.update(cx, |d, cx| d.set_tag_filter(Vec::new(), cx));
                                })
                                .child(
                                    ui::text(
                                        faces,
                                        format!(
                                            "{} · {}",
                                            crate::tr_with(
                                                "%{n} more hidden by %{tags}",
                                                &[("n", &hidden_by_tag), ("tags", &tags_str)],
                                            ),
                                            crate::tr("Clear")
                                        ),
                                    )
                                    .text_size(px(crate::style::CAPTION))
                                    .text_color(theme::text2()),
                                ),
                        );
                    }
                }
                push_group(&mut groups, false, grp);
            }
        }

        if self.machines.len() > 1 {
            let mut mach = div().flex().flex_col();
            for m in self.machines.iter().skip(1) {
                mach = mach.child(self.other_mac_row(faces, m, &desk));
            }
            push_group(&mut groups, false, mach);
        }

        let mut snoozed: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| s.parent.is_none() && s.beside.is_none() && !s.meta.flags.archived)
            .filter(|s| s.meta.flags.snoozed.is_some_and(|t| t > secs) && self.matches_tags(s))
            .filter(shown_project)
            .collect();
        snoozed.sort_by_key(|s| s.meta.flags.snoozed);
        let mut settled: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                s.meta.flags.settled.is_some() && self.listed_now(s) && self.matches_tags(s)
            })
            .filter(shown_project)
            .collect();
        settled.sort_by_key(|s| std::cmp::Reverse(s.meta.flags.settled));

        let mut prev_shelf = false;
        for (shelf, list) in [(Shelf::Snoozed, snoozed), (Shelf::Settled, settled)] {
            if list.is_empty() {
                continue;
            }
            let open = shelf_is_open(&self.expanded, shelf);
            let total = list.len();
            let most = match shelf {
                Shelf::Settled => self.settled_shown.max(SETTLED),
                _ => usize::MAX,
            };
            let label = crate::tr(match shelf {
                Shelf::Snoozed => "Snoozed",
                _ => "Settled",
            });
            let toggle_desk = desk.clone();
            let drop_desk = desk.clone();
            let mut grp = div()
                .id(SharedString::from(format!("shelf-wrap-{shelf:?}")))
                .flex()
                .flex_col()
                .drag_over::<Dragged>(|st, _, _, _| {
                    st.bg(theme::role(ColorRole::LayerHover01))
                        .shadow(drop_frame())
                })
                .on_drop(move |d: &Dragged, _, cx| {
                    drop_desk.update(cx, |this, cx| this.dropped(d.key, &Row::Shelf(shelf), cx));
                })
                .child(
                    SidebarShelfHeader::new(
                        SharedString::from(format!("sb-shelf-{shelf:?}")),
                        label,
                    )
                    .count(total)
                    .expanded(open)
                    .on_toggle(move |_, cx| {
                        toggle_desk.update(cx, |this, cx| this.toggle_shelf(shelf, cx));
                    }),
                );
            if open {
                for s in list.iter().take(most) {
                    let meta = (shelf == Shelf::Snoozed)
                        .then(|| {
                            s.meta
                                .flags
                                .snoozed
                                .map(|t| when::said_at(store::time(t), now, self.settings.twelve))
                        })
                        .flatten();
                    let kbd = selected_id == Some(Row::Session(s.key).id().as_ref());
                    grp = grp.child(self.thread_item(faces, s, meta, kbd, &desk, cx));
                }
                if total > most {
                    let rest = (total - most).min(SETTLED_MORE);
                    let more_desk = desk.clone();
                    grp = grp.child(
                        div()
                            .id("sb-settled-more")
                            .flex()
                            .items_center()
                            .h(px(32.))
                            .pl(px(36.))
                            .pr(px(12.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::role(ColorRole::LayerHover01)))
                            .on_click(move |_, _, cx| {
                                more_desk.update(cx, |d, cx| {
                                    d.settled_shown = d.settled_shown.max(SETTLED) + SETTLED_MORE;
                                    cx.notify();
                                });
                            })
                            .child(
                                ui::text(faces, crate::tr_with("Show %{n} more", &[("n", &rest)]))
                                    .text_size(px(crate::style::CAPTION))
                                    .text_color(theme::text2()),
                            ),
                    );
                }
            }
            push_group(&mut groups, prev_shelf, div().child(grp));
            prev_shelf = true;
        }

        groups
    }

    fn needs_you_header(
        &self,
        faces: &Faces,
        count: usize,
        open: bool,
        desk: &Entity<Self>,
    ) -> Stateful<Div> {
        let _ = self;
        let desk = desk.clone();
        let (fill, text_color) = count_colors(T3Status::NeedsYou);
        div()
            .id("sb-needs-head")
            .flex()
            .items_center()
            .h(px(32.))
            .px(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(theme::role(ColorRole::LayerHover01)))
            .on_click(move |_, _, cx| {
                desk.update(cx, |this, cx| this.toggle_shelf(Shelf::NeedsYou, cx));
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(4.))
                    .child(
                        CarbonGlyph::carbon(CarbonIcon::ChevronDown)
                            .size_length(px(14.))
                            .color(theme::text())
                            .rotate(radians(if open { 0. } else { -FRAC_PI_2 })),
                    )
                    .child(
                        ui::text(faces, crate::tr("Needs you"))
                            .text_size(px(crate::style::CAPTION))
                            .weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::text()),
                    )
                    .child(
                        ui::text(faces, count.to_string())
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(18.))
                            .min_w(px(18.))
                            .px(px(5.))
                            .text_center()
                            .rounded(px(16.))
                            .bg(fill)
                            .text_color(text_color),
                    ),
            )
    }

    fn render_server_header(
        &self,
        m: &crate::hub::Machine,
        project_count: usize,
        open: bool,
        desk: &Entity<Self>,
    ) -> Div {
        let _ = self;
        let (state, label, why) = match &m.up {
            Up::On => (ServerState::Online, crate::tr("Online"), None),
            Up::Reaching => (ServerState::Reaching, crate::tr("Reaching"), None),
            Up::Lost(w) => (ServerState::Offline, crate::tr("Offline"), Some(w.clone())),
            Up::Refused(w) => (
                ServerState::TurnedAway,
                crate::tr("Turned away"),
                Some(w.clone()),
            ),
        };
        let host = m.host;
        let (toggle_desk, reach_desk, menu_desk) = (desk.clone(), desk.clone(), desk.clone());
        let id_str = Row::Server(host).id();
        let mut header = ServerHeader::new(
            SharedString::from(format!("sb-server-{}", host.0)),
            m.name.to_uppercase(),
        )
        .kind(if host == Host::HERE {
            MachineKind::Laptop
        } else {
            MachineKind::Server
        })
        .state(state)
        .state_label(label)
        .count(project_count)
        .expanded(open)
        .reach_label(crate::tr("Reach it again"))
        .on_toggle(move |_, cx| {
            toggle_desk.update(cx, |d, cx| d.toggle_server_open(host, cx));
        })
        .on_reach(move |_, cx| {
            reach_desk.update(cx, |d, cx| d.reach_again(host, cx));
        });
        if host == Host::HERE {
            header = header.version(env!("CARGO_PKG_VERSION"));
        }
        if let Some(reason) = why {
            header = header.reason(reason);
        }
        div().child(div().child(header).context_menu(move |menu, window, cx| {
            Desk::side_menu(&menu_desk, &id_str, menu, window, cx)
        }))
    }

    fn other_mac_row(&self, faces: &Faces, m: &crate::hub::Machine, desk: &Entity<Self>) -> Div {
        let _ = self;
        let (dot, word, show_reach) = match &m.up {
            Up::On => (
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(theme::role(ColorRole::SupportSuccess)),
                crate::tr("Here"),
                false,
            ),
            Up::Reaching => (
                div()
                    .size(px(8.))
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(theme::role(ColorRole::SupportWarning)),
                crate::tr("Reaching"),
                false,
            ),
            Up::Lost(_) => (
                div()
                    .size(px(8.))
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(theme::muted()),
                crate::tr("Offline"),
                false,
            ),
            Up::Refused(_) => (
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(theme::role(ColorRole::SupportError)),
                crate::tr("Turned away"),
                true,
            ),
        };
        let host = m.host;
        let (menu_desk, reach_desk, toggle_desk) = (desk.clone(), desk.clone(), desk.clone());
        let id_str = Row::Server(host).id();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(SharedString::from(format!("sb-mrow-{}", host.0)))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(28.))
                    .px(px(12.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::role(ColorRole::LayerHover01)))
                    .on_click(move |_, _, cx| {
                        toggle_desk.update(cx, |d, cx| d.toggle_server_open(host, cx));
                    })
                    .child(
                        CarbonGlyph::carbon(CarbonIcon::ChevronRight)
                            .size_length(px(16.))
                            .color(theme::muted()),
                    )
                    .child(ui::icon("server", 16.).text_color(theme::muted()))
                    .child(
                        ui::text(faces, m.name.to_uppercase())
                            .flex_1()
                            .min_w(px(96.))
                            .truncate()
                            .text_size(px(crate::style::CAPTION))
                            .weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::text2()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(6.))
                            .child(dot)
                            .child(
                                ui::text(faces, word)
                                    .text_size(px(crate::style::CAPTION))
                                    .text_color(theme::text2()),
                            ),
                    )
                    .context_menu(move |menu, window, cx| {
                        Desk::side_menu(&menu_desk, &id_str, menu, window, cx)
                    }),
            )
            .when(show_reach, |d| {
                let reach_label = crate::tr_with("Reach %{name} again", &[("name", &m.name)]);
                d.child(
                    div()
                        .id(SharedString::from(format!("sb-mreach-{}", host.0)))
                        .flex()
                        .items_center()
                        .h(px(24.))
                        .pl(px(60.))
                        .pr(px(12.))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            reach_desk.update(cx, |d, cx| d.reach_again(host, cx));
                        })
                        .child(
                            ui::text(faces, reach_label)
                                .truncate()
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::link()),
                        ),
                )
            })
    }

    fn project_header(
        &self,
        faces: &Faces,
        folder: &Place,
        counts: FolderCounts,
        open: bool,
        desk: &Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Div {
        let _ = cx;
        let project_name = self.project_name(folder);
        let group = SharedString::from(format!(
            "prow-{}-{}",
            folder.host.0,
            folder.folder.display()
        ));
        let row_id = Row::Project(folder.clone()).id();
        let avatar: AnyElement = match self.icon_of(folder) {
            Some(ProjectIcon::Picture(path)) => ProjectFavicon::new(project_name.clone())
                .size(IdentitySize::Sm)
                .src(path)
                .into_any_element(),
            Some(icon @ ProjectIcon::Mark { .. }) => {
                crate::desk::projects::project_icon(Some(icon), 20.)
            }
            None => ProjectFavicon::new(project_name.clone())
                .size(IdentitySize::Sm)
                .into_any_element(),
        };
        let name_el: AnyElement = if matches!(&self.renaming, Some(Renaming::Project(f)) if f == folder)
        {
            self.rename_field().into_any_element()
        } else {
            ui::text(faces, project_name.clone())
                .flex_shrink(1.)
                .min_w(px(40.))
                .truncate()
                .text_size(px(crate::style::BODY))
                .weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .into_any_element()
        };

        let count_pills =
            div()
                .flex()
                .flex_none()
                .gap(px(2.))
                .children(counts.visible().into_iter().map(|(st, n)| {
                    let (bg, fg) = count_colors(st);
                    ui::text(faces, n.to_string())
                        .flex()
                        .justify_center()
                        .min_w(px(18.))
                        .px(px(4.))
                        .rounded(px(16.))
                        .bg(bg)
                        .text_size(px(crate::style::CAPTION))
                        .line_height(px(18.))
                        .text_color(fg)
                }));

        let (toggle_desk, add_desk, drop_desk, menu_desk) =
            (desk.clone(), desk.clone(), desk.clone(), desk.clone());
        let (toggle_folder, add_folder, onto) =
            (folder.clone(), folder.clone(), Row::Project(folder.clone()));
        let new_tip = crate::tr_with("New thread in %{name}", &[("name", &project_name)]);

        div().child(
            div()
                .id(group.clone())
                .group(group.clone())
                .relative()
                .flex()
                .items_center()
                .gap(px(4.))
                .h(px(32.))
                .pr(px(8.))
                .hover(|s| s.bg(theme::role(ColorRole::LayerHover01)))
                .drag_over::<Dragged>(|st, _, _, _| {
                    st.bg(theme::role(ColorRole::LayerHover01))
                        .shadow(drop_frame())
                })
                .on_drop(move |d: &Dragged, _, cx| {
                    drop_desk.update(cx, |this, cx| this.dropped(d.key, &onto, cx));
                })
                .child(
                    div()
                        .id((ElementId::from(group.clone()), "main"))
                        .flex()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            toggle_desk
                                .update(cx, |d, cx| d.toggle_project_open(&toggle_folder, cx));
                        })
                        .child(
                            CarbonGlyph::carbon(CarbonIcon::ChevronDown)
                                .size_length(px(16.))
                                .color(theme::muted())
                                .rotate(radians(if open { 0. } else { -FRAC_PI_2 }))
                                .flex_none(),
                        )
                        .child(avatar)
                        .child(name_el),
                )
                .child(count_pills)
                .when(self.prefs(folder).pinned, |d| {
                    d.child(
                        CarbonGlyph::carbon(CarbonIcon::PinFilled)
                            .size_length(px(14.))
                            .color(theme::muted())
                            .flex_none(),
                    )
                })
                .child(
                    div()
                        .flex_none()
                        .invisible()
                        .group_hover(group, gpui::Styled::visible)
                        .child(
                            Button::new(SharedString::from(format!(
                                "new-in-{}-{}",
                                folder.host.0,
                                folder.folder.display()
                            )))
                            .ghost()
                            .xsmall()
                            .child(
                                CarbonGlyph::carbon(CarbonIcon::Add)
                                    .size_length(px(16.))
                                    .color(theme::muted()),
                            )
                            .accessibility_label(new_tip.clone())
                            .tooltip(new_tip)
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                add_desk
                                    .update(cx, |d, cx| d.new_in(add_folder.clone(), window, cx));
                            }),
                        ),
                )
                .context_menu(move |menu, window, cx| {
                    Desk::side_menu(&menu_desk, &row_id, menu, window, cx)
                }),
        )
    }

    fn thread_item(
        &self,
        faces: &Faces,
        s: &Session,
        meta_override: Option<String>,
        kbd_selected: bool,
        desk: &Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Div {
        let _ = cx;
        let key = s.key;
        let row_id = Row::Session(key).id();
        if self.renaming == Some(Renaming::Session { key, here: false }) {
            return div()
                .flex()
                .items_center()
                .h(px(32.))
                .pl(px(36.))
                .pr(px(8.))
                .child(self.rename_field());
        }

        let active = self.current == Some(key);
        let selected = self.chosen.contains(&key) || self.sweep.contains(&key);
        let highlighted = active || selected;
        let unread = s.meta.flags.unread;
        let now = SystemTime::now();

        let status_glyph = glyph(Glyph::of(s), key);

        let meta_color = if highlighted {
            theme::text2()
        } else {
            theme::faint()
        };
        // The agent is its logo at the head of the line; its name is in the tip.
        let meta_text = meta_override.or_else(|| self.thread_meta_line(s));

        let title_str = s.shown_title().to_owned();
        let title_color = if highlighted || unread {
            theme::text()
        } else {
            theme::text2()
        };
        let title_el = ui::text(faces, title_str.clone())
            .min_w(px(0.))
            .truncate()
            .text_size(px(crate::style::BODY))
            .line_height(px(crate::style::prose_line()))
            .weight(if unread {
                gpui::FontWeight::SEMIBOLD
            } else {
                gpui::FontWeight::NORMAL
            })
            .text_color(title_color);

        let jump = self
            .side_order
            .iter()
            .position(|k| *k == key)
            .filter(|n| self.numbers_shown && *n < 9);
        let working_elapsed = (lists::status(s) == Some(lists::Status::Working))
            .then(|| {
                now.duration_since(s.meta.updated)
                    .ok()
                    .map(|d| fmt_elapsed(d.as_secs()))
            })
            .flatten();
        let right_text: AnyElement = if let Some(n) = jump {
            Kbd::new(crate::keys::numbered(n + 1)).into_any_element()
        } else {
            let t = working_elapsed.unwrap_or_else(|| compact(lists::used(s), now));
            ui::text(faces, t)
                .text_size(px(crate::style::CAPTION))
                .text_color(meta_color)
                .into_any_element()
        };

        let pr_badge = s.git.pr.as_ref().map(|pr| {
            let state = match pr.state.as_str() {
                "merged" => PullRequestState::Merged,
                "closed" => PullRequestState::Closed,
                _ if pr.draft => PullRequestState::Draft,
                _ => PullRequestState::Open,
            };
            PullRequestMiniBadge::new(u32::try_from(pr.number).unwrap_or(0), state)
        });

        let pinned = s.meta.flags.pinned.is_some();
        let snoozed = s.meta.flags.snoozed.is_some_and(|t| t > store::secs(now));
        let settled = s.meta.flags.settled.is_some();
        let on_hover = self.thread_hover_actions(key, pinned, snoozed, settled, desk);

        let mut tip = format!(
            "{}\n{} · {}",
            s.meta.title,
            self.name_of(&s.meta.agent),
            Desk::shown(&s.meta.folder)
        );
        if let Some(b) = &s.branch {
            tip.push_str(" · ");
            tip.push_str(b);
        }
        if s.elsewhere {
            tip.push('\n');
            tip.push_str(&crate::tr(
                "Open in another process too, such as a terminal",
            ));
        }
        if let Some(said) = s.items.iter().rev().find_map(|i| match i {
            Item::Agent(t) => Some(gist(t)).filter(|g| !g.is_empty()),
            _ => None,
        }) {
            tip.push_str("\n\n");
            tip.push_str(&said);
        }

        let group = SharedString::from(format!("srow-{key}"));
        let dragged = Dragged {
            key,
            title: SharedString::from(ui::fit(&title_str, 40)),
        };
        let (click, drop, files, menu_desk) =
            (desk.clone(), desk.clone(), desk.clone(), desk.clone());
        let (over, up, up_out) = (desk.clone(), desk.clone(), desk.clone());

        div().child(
            div()
                .id(group.clone())
                .group(group.clone())
                .relative()
                .flex()
                .w_full()
                .items_start()
                .gap(px(8.))
                .min_h(px(36.))
                .px(px(12.))
                .py(px(2.))
                .cursor_pointer()
                .when(highlighted, |d| {
                    d.bg(theme::role(ColorRole::LayerSelected01)).child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(3.))
                            .bg(theme::role(ColorRole::BorderInteractive)),
                    )
                })
                .when(!highlighted, |d| {
                    d.hover(|st| st.bg(theme::role(ColorRole::LayerHover01)))
                })
                .when(kbd_selected, |d| {
                    d.child(
                        div()
                            .absolute()
                            .inset_0()
                            .border_2()
                            .border_color(theme::role(ColorRole::Focus)),
                    )
                })
                .tooltip(ui::tip(tip))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .w(px(16.))
                        .h(px(20.))
                        .child(status_glyph),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(2.))
                        .child(title_el)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .min_w(px(0.))
                                .min_h(px(crate::style::caption_line()))
                                .child(ui::mark(faces, &self.mark_of(&s.meta.agent), 12.))
                                .children(meta_text.filter(|m| !m.is_empty()).map(|m| {
                                    ui::text(faces, m)
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_size(px(crate::style::CAPTION))
                                        .line_height(px(crate::style::caption_line()))
                                        .text_color(meta_color)
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(6.))
                        .h(px(20.))
                        .children(pr_badge)
                        .when(pinned, |d| {
                            d.child(
                                CarbonGlyph::carbon(CarbonIcon::PinFilled)
                                    .size_length(px(12.))
                                    .color(theme::muted()),
                            )
                        })
                        .when(s.elsewhere, |d| {
                            d.child(
                                CarbonGlyph::carbon(CarbonIcon::Terminal)
                                    .size_length(px(12.))
                                    .color(theme::muted()),
                            )
                        })
                        .child(right_text),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(8.))
                        .top(px(2.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .pl(px(4.))
                        .bg(if highlighted {
                            theme::role(ColorRole::LayerSelected01)
                        } else {
                            theme::role(ColorRole::LayerHover01)
                        })
                        .invisible()
                        .group_hover(group, gpui::Styled::visible)
                        .child(on_hover),
                )
                .on_click(move |e, window, cx| {
                    click.update(cx, |this, cx| this.side_click(key, e, window, cx));
                })
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
                .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
                .drag_over::<Dragged>(|st, _, _, _| {
                    st.bg(theme::role(ColorRole::LayerHover01))
                        .shadow(drop_frame())
                })
                .on_drop(move |d: &Dragged, _, cx| {
                    drop.update(cx, |this, cx| this.dropped(d.key, &Row::Session(key), cx));
                })
                .drag_over::<gpui::ExternalPaths>(|st, _, _, _| {
                    st.bg(theme::role(ColorRole::LayerHover01))
                })
                .on_drop(move |p: &gpui::ExternalPaths, window, cx| {
                    files.update(cx, |this, cx| {
                        this.show(key, window, cx);
                        this.drop_files(p.paths(), window, cx);
                    });
                })
                .context_menu(move |menu, window, cx| {
                    Desk::side_menu(&menu_desk, &row_id, menu, window, cx)
                }),
        )
    }

    fn thread_meta_line(&self, s: &Session) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(server) = self.server_chip(s.host).filter(|_| s.host != Host::HERE) {
            parts.push(server.to_owned());
        }
        if matches!(
            lists::status(s),
            Some(lists::Status::Failed | lists::Status::Limited)
        ) {
            parts.push(crate::tr(lists::status(s)?.word()).to_string());
        } else if let Some(pr) = &s.git.pr {
            parts.push(format!("PR #{} {}", pr.number, pr.state));
        }
        if !s.meta.tags.is_empty() {
            parts.push(
                s.meta
                    .tags
                    .iter()
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        Some(parts.join(" · "))
    }

    fn thread_hover_actions(
        &self,
        key: u64,
        pinned: bool,
        snoozed: bool,
        settled: bool,
        desk: &Entity<Self>,
    ) -> Div {
        let _ = self;
        let act = |id: &str, icon: &str, tip: &str| {
            Button::new(SharedString::from(format!("{id}-{key}")))
                .ghost()
                .xsmall()
                .child(ui::icon(icon, 14.))
                .accessibility_label(crate::tr(tip))
                .tooltip(crate::tr(tip))
        };
        let unpin = desk.clone();
        let row = div().flex().items_center().gap(px(2.)).when(pinned, |d| {
            d.child(act("unpin", "pin", "Unpin").on_click(move |_, _, cx| {
                cx.stop_propagation();
                unpin.update(cx, |d, cx| d.unpin(vec![key], cx));
            }))
        });
        if snoozed || settled {
            let desk = desk.clone();
            row.child(
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
            row.child(
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
        }
    }

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
            Row::Shelf(Shelf::Pinned) => {
                if self
                    .sessions
                    .at(moved)
                    .is_some_and(|s| s.meta.flags.pinned.is_none())
                {
                    self.toggle_pin(moved, cx);
                }
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
            Row::Server(_) | Row::Shelf(Shelf::Snoozed | Shelf::NeedsYou) | Row::More => {}
        }
    }

    pub(crate) fn side_key(
        &mut self,
        k: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if k.modifiers.modified() {
            return false;
        }
        let (ix, row, is_folder, is_open, len) = {
            let tree = self.side.read(cx);
            let len = (0..usize::MAX)
                .take_while(|i| tree.entry(*i).is_some())
                .count();
            let ix = tree.selected_index();
            let entry = tree.selected_entry();
            let row = entry.and_then(|e| Row::of(&e.item().id));
            let is_folder = entry.is_some_and(gpui::base::TreeEntry::is_folder);
            let is_open = entry.is_some_and(gpui::base::TreeEntry::is_expanded);
            (ix, row, is_folder, is_open, len)
        };
        match k.key.as_str() {
            "up" if len > 0 => {
                let next = ix.unwrap_or(0).checked_sub(1).unwrap_or(len - 1);
                self.side
                    .update(cx, |tree, cx| tree.set_selected_index(Some(next), cx));
            }
            "down" if len > 0 => {
                let next = match ix {
                    Some(i) if i + 1 < len => i + 1,
                    _ => 0,
                };
                self.side
                    .update(cx, |tree, cx| tree.set_selected_index(Some(next), cx));
            }
            "left" if is_folder && is_open => {
                if let Some(r) = row {
                    self.side_folded(&r.id(), false, cx);
                }
            }
            "right" if is_folder && !is_open => {
                if let Some(r) = row {
                    self.side_folded(&r.id(), true, cx);
                }
            }
            "enter" | "space" => match row {
                Some(Row::Session(key)) => self.show(key, window, cx),
                Some(r) if is_folder => self.side_folded(&r.id(), !is_open, cx),
                _ => return false,
            },
            "f2" => match row {
                Some(Row::Session(key)) => {
                    self.begin_rename(Renaming::Session { key, here: false }, window, cx);
                }
                Some(Row::Project(folder)) => {
                    self.begin_rename(Renaming::Project(folder), window, cx);
                }
                _ => return false,
            },
            "home" | "end" if len > 0 => {
                let to = if k.key == "home" { 0 } else { len - 1 };
                self.side
                    .update(cx, |tree, cx| tree.set_selected_index(Some(to), cx));
            }
            _ => return false,
        }
        cx.notify();
        true
    }

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

    pub(super) fn side_click(
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

    pub(crate) fn rename_field(&self) -> gpui::Div {
        div()
            .flex_1()
            .min_w(px(0.))
            .h(px(28.))
            .px(px(6.))
            .flex()
            .items_center()
            .bg(theme::role(ColorRole::Field01))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderStrong01))
            .shadow(vec![BoxShadow {
                color: theme::role(ColorRole::Focus),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(2.),
                inset: true,
            }])
            .child(Input::new(&self.rename).appearance(false).xsmall())
    }
}

fn count_colors(st: T3Status) -> (gpui::Hsla, gpui::Hsla) {
    match st {
        T3Status::NeedsYou => (
            theme::role(ColorRole::SupportWarning),
            rgb(0x0016_1616).into(),
        ),
        T3Status::Failed => (
            theme::role(ColorRole::SupportError),
            if theme::is_light() {
                gpui::white()
            } else {
                rgb(0x0016_1616).into()
            },
        ),
        T3Status::Working => (
            theme::role(ColorRole::TagBackgroundBlue),
            theme::role(ColorRole::TagColorBlue),
        ),
        _ => (theme::role(ColorRole::LayerAccent01), theme::text2()),
    }
}

fn folder_counts(sessions: &[&Session]) -> FolderCounts {
    let mut c = FolderCounts::default();
    for s in sessions {
        match lists::status(s) {
            Some(lists::Status::Approval | lists::Status::SignIn | lists::Status::Plan) => {
                c.needs_you += 1;
            }
            Some(lists::Status::Failed | lists::Status::Limited) => c.failed += 1,
            Some(lists::Status::Working) => c.working += 1,
            Some(lists::Status::Done | lists::Status::Woke) => c.done += 1,
            None => {}
        }
    }
    c
}

fn drop_frame() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: theme::role(ColorRole::BorderInteractive),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(2.),
        inset: true,
    }]
}

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

/// What leads a thread in a list (the sidebar, ⌘K): where it stands,
/// else whether it is a draft, unread, said in, or new.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Glyph {
    Working,
    NeedsYou,
    Failed,
    Draft,
    Unread,
    Done,
    New,
}

impl Glyph {
    pub(crate) fn of(s: &Session) -> Self {
        match lists::status(s) {
            Some(lists::Status::Working) => Self::Working,
            Some(lists::Status::Approval | lists::Status::SignIn | lists::Status::Plan) => {
                Self::NeedsYou
            }
            Some(lists::Status::Failed | lists::Status::Limited) => Self::Failed,
            _ if s.is_draft() => Self::Draft,
            _ if s.meta.flags.unread => Self::Unread,
            _ if !s.items.is_empty() => Self::Done,
            _ => Self::New,
        }
    }
}

/// The glyph `g` drawn, 16px; `key` keeps a spinner turning across frames.
pub(crate) fn glyph(g: Glyph, key: u64) -> AnyElement {
    let carbon = |icon, color| {
        CarbonGlyph::carbon(icon)
            .size_length(px(16.))
            .color(color)
            .into_any_element()
    };
    match g {
        Glyph::Working => Spin::new(SharedString::from(format!("spin-{key}"))).into_any_element(),
        Glyph::NeedsYou => carbon(
            CarbonIcon::WarningFilled,
            theme::role(ColorRole::SupportWarning),
        ),
        Glyph::Failed => carbon(
            CarbonIcon::ErrorFilled,
            theme::role(ColorRole::SupportError),
        ),
        Glyph::Draft => carbon(CarbonIcon::Edit, theme::muted()),
        Glyph::Unread => div()
            .size(px(8.))
            .rounded_full()
            .bg(theme::role(ColorRole::Interactive))
            .into_any_element(),
        Glyph::Done => carbon(CarbonIcon::CheckmarkFilled, theme::muted()),
        Glyph::New => carbon(CarbonIcon::Chat, theme::muted()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::State;

    #[test]
    fn a_thread_leads_with_where_it_stands_before_anything_else() {
        let mut s = Session::new(1, "codex", "/p".into());
        assert_eq!(Glyph::of(&s), Glyph::Draft);
        s.state = State::Working;
        assert_eq!(Glyph::of(&s), Glyph::Working);
        s.state = State::Broken("exit 1".into());
        s.meta.flags.unread = true;
        assert_eq!(Glyph::of(&s), Glyph::Failed);
        s.state = State::Idle;
        s.items.push(Item::You("hi".into()));
        assert_eq!(Glyph::of(&s), Glyph::Unread);
        s.meta.flags.unread = false;
        assert_eq!(Glyph::of(&s), Glyph::Done);
    }

    #[test]
    fn each_row_reads_back_from_its_id() {
        for row in [
            Row::Server(Host(3)),
            Row::Project(Place::new(Host(2), "/a/b: c")),
            Row::Session(42),
            Row::Shelf(Shelf::Pinned),
            Row::Shelf(Shelf::NeedsYou),
            Row::Shelf(Shelf::Snoozed),
            Row::Shelf(Shelf::Settled),
            Row::More,
        ] {
            assert_eq!(Row::of(&row.id()), Some(row));
        }
    }

    #[test]
    fn shelf_defaults_keep_pinned_and_needs_you_open_and_others_folded() {
        let mut expanded = std::collections::HashSet::new();
        assert!(shelf_is_open(&expanded, Shelf::Pinned));
        assert!(shelf_is_open(&expanded, Shelf::NeedsYou));
        assert!(!shelf_is_open(&expanded, Shelf::Snoozed));
        assert!(!shelf_is_open(&expanded, Shelf::Settled));
        expanded.insert(shelf_key(Shelf::NeedsYou));
        expanded.insert(shelf_key(Shelf::Settled));
        assert!(!shelf_is_open(&expanded, Shelf::NeedsYou));
        assert!(shelf_is_open(&expanded, Shelf::Settled));
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
