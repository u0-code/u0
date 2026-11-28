//! Context menus for sidebar rows (sessions, multi-selected sessions,
//! projects, paired servers, and snooze presets).

use std::time::SystemTime;

use gpui::component::menu::{PopupMenu, PopupMenuItem};
use gpui::{App, Context, Entity, Window, prelude::*};

use super::list::Row;
use crate::{
    desk::{Desk, Place, Renaming, lists},
    hub::Host,
    session::{Session, State},
    store, ui,
};

enum Plan {
    Session(Facts),
    Chosen(Vec<u64>),
    Project(Place, bool),
    Server(Host),
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a fact of its own about the thread, not a state among several"
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
    others: Vec<(String, String)>,
    new_on: Option<String>,
    folder: String,
    branch: Option<String>,
    id: Option<String>,
    link: Option<String>,
}

impl Desk {
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
                Some(Row::Server(host)) => Plan::Server(host),
                _ => return menu,
            }
        };
        match plan {
            Plan::Session(f) => session_menu(menu, &f, desk, window, cx),
            Plan::Chosen(keys) => chosen_menu(menu, &keys, desk, window, cx),
            Plan::Project(folder, pinned) => project_menu(menu, &folder, pinned, desk),
            Plan::Server(host) => server_menu(menu, host, desk),
        }
    }

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
                self.others(s.host, &s.meta.agent)
            } else {
                Vec::new()
            },
            new_on: s.meta.worktree.is_some().then(|| {
                (s.branch.clone()).unwrap_or_else(|| crate::tr("its worktree").to_string())
            }),
            folder: s.meta.folder.display().to_string(),
            branch: s.branch.clone(),
            id: s.meta.id.as_ref().map(|id| id.0.to_string()),
            link: (s.meta.id.as_ref())
                .map(|id| crate::desk::links::Link::to_session(&s.meta.agent, &id.0)),
        }
    }
}

fn item(
    label: impl Into<gpui::SharedString>,
    icon: &str,
    f: impl Fn(&mut Window, &mut App) + 'static,
) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .icon(ui::menu_glyph(icon))
        .on_click(move |_, window, cx| f(window, cx))
}

fn copy_item(label: &str, text: String) -> PopupMenuItem {
    item(crate::tr(label), "copy", move |_, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
    })
}

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
            crate::tr("Rename"),
            "pencil",
            on(|d, key, window, cx| {
                d.begin_rename(Renaming::Session { key, here: false }, window, cx);
            }),
        ))
        .when(f.id.is_some(), |m| {
            m.item(item(
                crate::tr("Regenerate title"),
                "retry",
                on(|d, key, _, cx| d.retitle(key, cx)),
            ))
        })
        .item(item(
            crate::tr(if f.pinned { "Unpin" } else { "Pin" }),
            "pin",
            on(|d, key, _, cx| d.toggle_pin(key, cx)),
        ))
        .item(item(
            crate::tr(if f.settled { "Un-settle" } else { "Settle" }),
            "check",
            on(|d, key, _, cx| d.toggle_settled(key, cx)),
        ));
    menu = if f.snoozed {
        menu.item(item(
            crate::tr("Wake"),
            "clock",
            on(|d, key, _, cx| d.wake(key, cx)),
        ))
    } else {
        let desk = desk.clone();
        menu.submenu_with_icon(
            Some(ui::menu_glyph("clock")),
            crate::tr("Snooze"),
            window,
            cx,
            move |menu, _, cx| snooze_items(menu, &desk, &[key], cx),
        )
    };
    if !f.unread {
        menu = menu.item(item(
            crate::tr("Mark unread"),
            "bell",
            on(|d, key, _, cx| d.mark_unread(key, cx)),
        ));
    }
    menu = menu
        .item(item(
            crate::tr("Find in thread"),
            "search",
            on(|d, key, window, cx| {
                d.show(key, window, cx);
                if !d.find.open {
                    d.toggle_find(window, cx);
                }
            }),
        ))
        .item(item(
            crate::tr("Filter by project"),
            "folder",
            on(|d, key, _, cx| {
                if let Some(p) = d.sessions.at(key).map(Place::project_of) {
                    d.filter = vec![d.listed_under(&p)];
                    cx.notify();
                }
            }),
        ))
        .item(item(
            crate::tr("Project settings"),
            "settings",
            on(|d, key, window, cx| {
                if let Some(p) = d.sessions.at(key).map(Place::project_of) {
                    d.project_settings(&p, window, cx);
                }
            }),
        ));
    if let Some(branch) = &f.new_on {
        menu = menu.item(item(
            crate::tr_with(
                "New thread on %{branch}",
                &[("branch", &ui::fit(branch, 22))],
            ),
            "square-pen",
            on(Desk::new_in_worktree),
        ));
    }
    if f.fork {
        menu = menu.item(item(
            crate::tr("Branch off a copy"),
            "branch",
            on(|d, key, _, cx| d.fork(key, cx)),
        ));
    }
    if !f.others.is_empty() {
        let (others, hand) = (f.others.clone(), desk.clone());
        menu = menu.submenu_with_icon(
            Some(ui::menu_glyph("corner-up-right")),
            crate::tr("Continue with"),
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
        crate::tr("Copy"),
        window,
        cx,
        move |menu, _, _| {
            let desk = copy_desk.clone();
            let menu = menu
                .item(item(crate::tr("The conversation"), "copy", move |_, cx| {
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
                Some(id) => menu.item(copy_item("Thread ID", id.clone())),
                None => menu,
            }
        },
    );
    menu = menu.separator().item(item(
        crate::tr(if f.kept {
            "Let it settle on its own"
        } else {
            "Never settle on its own"
        }),
        "circle-dot",
        on(|d, key, _, cx| d.mark(key, cx, |f| f.kept = !f.kept)),
    ));
    if !f.busy {
        menu = menu
            .item(item(
                crate::tr("Archive"),
                "archive",
                on(|d, key, _, cx| d.archive(key, cx)),
            ))
            .item({
                let f = on(|d, key, window, cx| d.ask_delete(vec![key], window, cx));
                ui::danger_item(crate::tr("Delete"), "trash")
                    .on_click(move |_, window, cx| f(window, cx))
            });
    }
    menu
}

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
        item(crate::tr_with(label, &[("n", &n)]), icon, move |_, cx| {
            desk.update(cx, |d, cx| f(d, keys.clone(), cx));
        })
    };
    let (snooze, gone, keys) = (desk.clone(), desk.clone(), keys.to_vec());
    menu.item(each("Unpin (%{n})", "pin", Desk::unpin))
        .item(each("Settle (%{n})", "check", |d, keys, cx| {
            d.undoable(lists::Undone::Settle, &keys, cx);
        }))
        .item(each("Mark unread (%{n})", "bell", |d, keys, cx| {
            for key in keys {
                d.mark_unread(key, cx);
            }
            d.chosen.clear();
        }))
        .submenu_with_icon(
            Some(ui::menu_glyph("clock")),
            crate::tr_with("Snooze (%{n})", &[("n", &n)]),
            window,
            cx,
            move |menu, _, cx| snooze_items(menu, &snooze, &keys, cx),
        )
        .separator()
        .item(item(
            crate::tr_with("Delete (%{n})", &[("n", &n)]),
            "trash",
            move |window, cx| {
                gone.update(cx, |d, cx| {
                    let keys = std::mem::take(&mut d.chosen).into_iter().collect();
                    d.ask_delete(keys, window, cx);
                });
            },
        ))
}

fn project_menu(menu: PopupMenu, folder: &Place, pinned: bool, desk: &Entity<Desk>) -> PopupMenu {
    let on = |f: fn(&mut Desk, &Place, &mut Window, &mut Context<Desk>)| {
        let (desk, folder) = (desk.clone(), folder.clone());
        move |window: &mut Window, cx: &mut App| desk.update(cx, |d, cx| f(d, &folder, window, cx))
    };
    menu.item(item(
        crate::tr("New thread"),
        "square-pen",
        on(|d, f, window, cx| d.new_in(f.clone(), window, cx)),
    ))
    .item(copy_item("Copy path", folder.folder.display().to_string()))
    .item(item(
        crate::tr(if pinned {
            "Unpin project"
        } else {
            "Pin project"
        }),
        "pin",
        on(|d, f, _, cx| d.toggle_project_pin(f, cx)),
    ))
    .item(item(
        crate::tr("Project settings"),
        "settings",
        on(Desk::project_settings),
    ))
}

fn server_menu(menu: PopupMenu, host: Host, desk: &Entity<Desk>) -> PopupMenu {
    let (again, settings) = (desk.clone(), desk.clone());
    menu.item(item(crate::tr("Reach it again"), "retry", move |_, cx| {
        again.update(cx, |d, cx| d.reach_again(host, cx));
    }))
    .item(item(
        crate::tr("Its settings"),
        "settings",
        move |window, cx| {
            settings.update(cx, |d, cx| d.settings_of(host, window, cx));
        },
    ))
}

pub(super) fn snooze_items(
    menu: PopupMenu,
    desk: &Entity<Desk>,
    keys: &[u64],
    cx: &App,
) -> PopupMenu {
    let presets = desk.read(cx).snoozes();
    let menu = presets.into_iter().fold(menu, |menu, (label, later)| {
        let (desk, keys) = (desk.clone(), keys.to_vec());
        menu.item(item(&label, "clock", move |_, cx| {
            desk.update(cx, |d, cx| d.snooze(&keys, later, cx));
        }))
    });
    let (desk, keys) = (desk.clone(), keys.to_vec());
    menu.separator()
        .item(item(crate::tr("Custom…"), "clock", move |window, cx| {
            desk.update(cx, |d, cx| d.snooze_page(keys.clone(), window, cx));
        }))
}
