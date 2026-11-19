//! Attention: everything any ghost the desk follows is waiting on you for,
//! on every daemon, newest first, in one list in the middle.
//!
//! Each daemon is asked, every minute and whenever the list is opened,
//! what each character it runs is waiting on whoever runs her
//! (`attention.list`): work waiting on an answer, work that ended blocked
//! or given up, what work done for you told her, and her reports to you.
//! Each item says who and where, what she needs, and can be answered in
//! place when it is a question, opened (her page, or the session it
//! belongs to), and marked seen. Which items you have seen is kept in the
//! desk's settings, by a key made of the daemon, the character and the
//! item's own id; the count in the sidebar is what you have not.
//!
//! A key is kept only while its item can still be listed again: by a
//! daemon the desk follows, for a character it runs, among what she lists
//! now. Her list holds work still waiting on an answer and what happened
//! in the last few days, and an item's id is never another's, so an item
//! that has left the list, or whose character or daemon has, never comes
//! back to be seen again.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use ghost_link::protocol::{Attention, AttentionKind};
use gpui::{AnyElement, Context, Div, SharedString, Stateful, div, prelude::*, px};

use crate::{
    answer::{Ask, Whom},
    daemons::{self, Host},
    desk::{Desk, Page},
    fonts::Faces,
    theme, thread, ui,
};

/// The key an item is kept seen by: its daemon, its character, its id.
pub fn key(host: &Host, ghost: &str, id: &str) -> String {
    format!("{}/{ghost}/{id}", host.key())
}

/// What one character is waiting on you for, with who she is and where.
pub struct Source<'a> {
    pub host: &'a Host,
    pub ghost: &'a str,
    pub name: String,
    /// Whether she is the desk's own ghost, whose work is in your sessions.
    pub solver: bool,
    pub items: &'a [Attention],
}

/// One item of the list.
pub struct Item<'a> {
    pub host: &'a Host,
    pub ghost: &'a str,
    pub name: String,
    pub solver: bool,
    pub it: &'a Attention,
    pub key: String,
    pub at: Option<DateTime<Utc>>,
}

/// Every item of every source, newest first; an item whose time cannot be
/// read goes last.
pub fn gather<'a>(sources: Vec<Source<'a>>) -> Vec<Item<'a>> {
    let mut items: Vec<Item<'a>> = sources
        .into_iter()
        .flat_map(|s| {
            s.items.iter().map(move |it| Item {
                host: s.host,
                ghost: s.ghost,
                name: s.name.clone(),
                solver: s.solver,
                it,
                key: key(s.host, s.ghost, &it.id),
                at: it.at.parse().ok(),
            })
        })
        .collect();
    items.sort_by(|a, b| (b.at, b.it.seq).cmp(&(a.at, a.it.seq)));
    items
}

/// How many of `items` have not been seen.
pub fn unseen(items: &[Item<'_>], seen: &BTreeSet<String>) -> usize {
    items.iter().filter(|i| !seen.contains(&i.key)).count()
}

/// Forget what was seen of the character `ghost` on `host` that she no
/// longer lists: an item's id is never another item's, so one that is gone
/// is gone for good. Says whether anything was forgotten.
pub fn forget_gone(
    seen: &mut BTreeSet<String>,
    host: &Host,
    ghost: &str,
    listed: &[Attention],
) -> bool {
    let prefix = key(host, ghost, "");
    let before = seen.len();
    seen.retain(|k| {
        k.strip_prefix(&prefix)
            .is_none_or(|id| listed.iter().any(|it| it.id == id))
    });
    seen.len() != before
}

/// Forget what was seen on every daemon not among `followed`: a
/// connection that was removed lists nothing any more. Says whether
/// anything was forgotten.
pub fn forget_unfollowed(seen: &mut BTreeSet<String>, followed: &[&Host]) -> bool {
    let before = seen.len();
    seen.retain(|k| owner(k, followed).is_some());
    seen.len() != before
}

/// Forget what was seen of every character the daemon on `host`, one of
/// `followed`, no longer runs, now that it has said who it runs:
/// `running`, by id. Says whether anything was forgotten.
pub fn forget_unrun(
    seen: &mut BTreeSet<String>,
    followed: &[&Host],
    host: &Host,
    running: &[&str],
) -> bool {
    let before = seen.len();
    seen.retain(|k| match owner(k, followed) {
        Some((of, rest)) if of == host => running.contains(&rest.split('/').next().unwrap_or("")),
        _ => true,
    });
    seen.len() != before
}

/// The daemon among `followed` the key `k` is of, with what follows it
/// there, `<ghost>/<id>`. A connection's name may hold a `/`, so where
/// more than one fits, the longest is the one.
fn owner<'k, 'h>(k: &'k str, followed: &[&'h Host]) -> Option<(&'h Host, &'k str)> {
    followed
        .iter()
        .filter_map(|host| {
            let rest = k.strip_prefix(host.key().as_str())?.strip_prefix('/')?;
            Some((*host, rest))
        })
        .min_by_key(|(_, rest)| rest.len())
}

/// What an item is, in a word or two, and its colour.
fn kind(it: &Attention) -> (&'static str, gpui::Hsla) {
    match it.kind {
        AttentionKind::Question => ("Asks you", theme::purple()),
        AttentionKind::Ended if it.outcome.as_deref() == Some("gave_up") => {
            ("Gave up", theme::coral())
        }
        AttentionKind::Ended => ("Blocked", theme::coral()),
        AttentionKind::Told => ("Told you", theme::blue()),
        AttentionKind::Report => ("Report", theme::green()),
        AttentionKind::Other => ("Other", theme::muted()),
    }
}

impl Desk {
    /// What every character the desk follows is waiting on you for.
    fn sources(&self) -> Vec<Source<'_>> {
        let seal = self.solver.card.as_ref().map(|c| c.seal.as_str());
        let mut sources = Vec::new();
        for d in &self.daemons {
            for (ghost, items) in &d.attention {
                let listed = d.cast.ghosts.iter().find(|g| g.id == *ghost);
                sources.push(Source {
                    host: &d.host,
                    ghost,
                    name: listed.map_or_else(|| ghost.clone(), |g| g.card.name.clone()),
                    solver: listed.is_some_and(|g| daemons::is_solver(d, g, seal)),
                    items,
                });
            }
        }
        sources
    }

    /// How many items in Attention you have not seen.
    pub(crate) fn attention_count(&self) -> usize {
        unseen(&gather(self.sources()), &self.settings.seen)
    }

    /// The questions in Attention, for their reply boxes.
    pub(crate) fn attention_asks(&self) -> Vec<Ask<'_>> {
        gather(self.sources())
            .into_iter()
            .filter(|i| i.it.kind == AttentionKind::Question)
            .filter_map(|i| {
                Some(Ask {
                    whom: Whom::Listed {
                        host: i.host.clone(),
                        ghost: i.ghost.to_owned(),
                    },
                    work: i.it.work.as_deref()?,
                    question: Some(&i.it.text),
                    kind: i.it.ask,
                    options: &i.it.options,
                    open: true,
                })
            })
            .collect()
    }

    /// Show Attention in the middle, and ask every daemon afresh.
    pub(crate) fn open_attention(&mut self, cx: &mut Context<Self>) {
        self.page = Page::Attention;
        for d in &self.daemons {
            d.attend();
        }
        cx.notify();
    }

    fn mark_seen(&mut self, keys: Vec<String>, cx: &mut Context<Self>) {
        self.settings.seen.extend(keys);
        self.settings.save();
        cx.notify();
    }

    /// Go to what an item is about: the session its work was taken on in,
    /// for the desk's own ghost; her page for anyone else, at the place it
    /// was said or the work was taken on in. Its work's card is opened.
    fn open_item(
        &mut self,
        host: &Host,
        ghost: &str,
        solver: bool,
        it: Attention,
        cx: &mut Context<Self>,
    ) {
        self.page = Page::Thread;
        if let Some(work) = &it.work {
            self.expanded.insert(format!("task:{work}"));
        }
        if solver {
            let place = it
                .work
                .as_deref()
                .and_then(|w| thread::task_place(&self.solver.journal, w))
                .or(it.place.clone());
            let session = place
                .and_then(|p| p.split_once(':').map(|(_, id)| id.to_owned()))
                .filter(|id| self.sessions.has(id));
            match session {
                Some(id) => self.choose(&id, cx),
                None => self.close_character(cx),
            }
            cx.notify();
            return;
        }
        self.open_character(host, ghost, cx);
        if let Some(open) = &mut self.open {
            open.place = it.place;
            open.focus = it.work;
        }
        cx.notify();
    }

    /// The list, in the middle.
    pub(crate) fn attention_view(&self, faces: &Faces, cx: &mut Context<Self>) -> AnyElement {
        let items = gather(self.sources());
        let unseen_keys: Vec<String> = items
            .iter()
            .filter(|i| !self.settings.seen.contains(&i.key))
            .map(|i| i.key.clone())
            .collect();
        let head = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                ui::text(
                    faces,
                    "What every ghost the desk follows is waiting on you for, newest first.",
                )
                .text_size(px(13.))
                .text_color(theme::muted())
                .flex_1()
                .min_w(px(0.)),
            )
            .when(!unseen_keys.is_empty(), |d| {
                d.child(
                    small(faces, "attention-all-seen", "Mark all seen").on_click(
                        cx.listener(move |this, _, _, cx| this.mark_seen(unseen_keys.clone(), cx)),
                    ),
                )
            });
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .w_full()
            .max_w(px(thread::COLUMN))
            .mx_auto()
            .child(head)
            .children(self.notice_line(faces, cx));
        if items.is_empty() {
            column = column.child(
                ui::text(faces, "Nothing is waiting on you.")
                    .text_size(px(14.))
                    .text_color(theme::muted())
                    .py(px(24.))
                    .text_center(),
            );
        }
        for item in &items {
            column = column.child(self.attention_item(faces, item, cx));
        }
        div()
            .id("attention")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(32.))
            .pt(px(24.))
            .pb(px(24.))
            .child(column)
            .into_any_element()
    }

    /// One item: what and from whom, what she needs, and what can be done.
    fn attention_item(&self, faces: &Faces, item: &Item<'_>, cx: &mut Context<Self>) -> Div {
        let it = item.it;
        let seen = self.settings.seen.contains(&item.key);
        let (word, color) = kind(it);
        let ago = item.at.map_or_else(String::new, |at| {
            format!(" · {} ago", thread::span((Utc::now() - at).num_seconds()))
        });
        let open = {
            let (host, ghost, solver, it) = (
                item.host.clone(),
                item.ghost.to_owned(),
                item.solver,
                it.clone(),
            );
            small(faces, format!("{}:open", item.key), "Open").on_click(cx.listener(
                move |this, _, _, cx| {
                    this.open_item(&host, &ghost, solver, it.clone(), cx);
                },
            ))
        };
        let head = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::dot(if seen { theme::faint() } else { color }).size(px(7.)))
            .child(
                ui::mono(faces, word)
                    .text_size(px(11.5))
                    .text_color(color)
                    .flex_none(),
            )
            .child(ui::one_line(
                ui::mono(faces, format!("{} · {}{ago}", item.name, item.host.label()))
                    .text_size(px(11.5))
                    .text_color(theme::muted()),
            ))
            .child(open)
            .when(!seen, |d| {
                let key = item.key.clone();
                d.child(small(faces, format!("{}:seen", item.key), "Seen").on_click(
                    cx.listener(move |this, _, _, cx| this.mark_seen(vec![key.clone()], cx)),
                ))
            });
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(10.))
            .border_1()
            .border_color(if seen {
                theme::border()
            } else {
                theme::border2()
            })
            .when(!seen, |d| d.bg(theme::panel()))
            .child(head)
            .children(it.brief.as_ref().map(|b| {
                ui::one_line(
                    ui::text(faces, b.clone())
                        .text_size(px(12.5))
                        .text_color(theme::muted()),
                )
            }));
        if it.kind == AttentionKind::Question {
            if let Some(work) = it.work.as_deref() {
                let ask = Ask {
                    whom: Whom::Listed {
                        host: item.host.clone(),
                        ghost: item.ghost.to_owned(),
                    },
                    work,
                    question: Some(&it.text),
                    kind: it.ask,
                    options: &it.options,
                    open: true,
                };
                body = body.child(self.ask_controls(faces, &ask, cx));
            }
        } else {
            body = body.child(
                ui::text(faces, it.text.clone())
                    .text_size(px(13.5))
                    .line_height(px(20.))
                    .text_color(if seen { theme::text2() } else { theme::text() }),
            );
        }
        for file in &it.files {
            body = body.child(
                ui::mono(faces, file.clone())
                    .text_size(px(11.5))
                    .text_color(theme::muted()),
            );
        }
        body
    }
}

/// A small quiet action.
fn small(faces: &Faces, id: impl Into<SharedString>, text: &'static str) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .h(px(22.))
        .px(px(7.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(
            ui::text(faces, text)
                .text_size(px(12.5))
                .text_color(theme::muted()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, kind: AttentionKind, at: &str, seq: i64) -> Attention {
        Attention {
            id: id.to_owned(),
            kind,
            at: at.to_owned(),
            seq,
            work: None,
            brief: None,
            text: "t".to_owned(),
            ask: None,
            options: Vec::new(),
            outcome: None,
            files: Vec::new(),
            place: None,
        }
    }

    #[test]
    fn every_ghosts_items_are_one_list_newest_first_and_unseen_ones_are_counted() {
        let (here, hk) = (Host::Local, Host::Remote("hk".into()));
        let solver = [
            item(
                "asked-12",
                AttentionKind::Question,
                "2026-11-16T10:00:00Z",
                12,
            ),
            item("told-9", AttentionKind::Told, "2026-11-15T09:00:00Z", 9),
        ];
        let weiwei = [
            item(
                "report-40",
                AttentionKind::Report,
                "2026-11-16T12:00:00Z",
                40,
            ),
            item("x-1", AttentionKind::Other, "not a time", 1),
        ];
        let sources = vec![
            Source {
                host: &here,
                ghost: "solver",
                name: "Ghost".into(),
                solver: true,
                items: &solver,
            },
            Source {
                host: &hk,
                ghost: "weiwei",
                name: "薇薇".into(),
                solver: false,
                items: &weiwei,
            },
        ];
        let items = gather(sources);
        let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "group:remote:hk/weiwei/report-40",
                "group:machine/solver/asked-12",
                "group:machine/solver/told-9",
                "group:remote:hk/weiwei/x-1",
            ]
        );
        let seen: BTreeSet<String> = ["group:machine/solver/asked-12".to_owned()].into();
        assert_eq!(unseen(&items, &seen), 3);
    }

    #[test]
    fn what_was_seen_is_forgotten_once_its_ghost_no_longer_lists_it_and_not_before() {
        let (here, hk) = (Host::Local, Host::Remote("hk".into()));
        let mut seen: BTreeSet<String> = [
            key(&here, "solver", "asked-12"),
            key(&here, "solver", "told-9"),
            key(&hk, "weiwei", "report-40"),
        ]
        .into();
        let still = [item(
            "asked-12",
            AttentionKind::Question,
            "2026-11-16T10:00:00Z",
            12,
        )];
        assert!(forget_gone(&mut seen, &here, "solver", &still));
        assert_eq!(
            seen,
            [
                key(&here, "solver", "asked-12"),
                key(&hk, "weiwei", "report-40"),
            ]
            .into()
        );
        // Another character's, or another daemon's, are not hers to forget.
        assert!(!forget_gone(&mut seen, &hk, "lin", &[]));
        assert!(!forget_gone(&mut seen, &here, "solver", &still));
    }

    #[test]
    fn what_was_seen_goes_with_a_daemon_no_longer_followed_or_a_character_no_longer_run() {
        let (here, hk, hkg, sg) = (
            Host::Local,
            Host::Remote("hk".into()),
            // A name that begins with another's.
            Host::Remote("hk/g".into()),
            Host::Remote("sg".into()),
        );
        let followed = [&here, &hk, &hkg];
        let mut seen: BTreeSet<String> = [
            key(&here, "solver", "asked-12"),
            key(&hk, "weiwei", "report-40"),
            key(&hk, "lin", "told-3"),
            key(&hkg, "weiwei", "report-41"),
            key(&sg, "weiwei", "report-5"),
        ]
        .into();
        // The connection sg was removed.
        assert!(forget_unfollowed(&mut seen, &followed));
        assert!(!seen.contains(&key(&sg, "weiwei", "report-5")));
        assert!(!forget_unfollowed(&mut seen, &followed));
        // hk no longer runs lin; what is hk/g's is not hk's to forget.
        assert!(forget_unrun(&mut seen, &followed, &hk, &["weiwei"]));
        assert_eq!(
            seen,
            [
                key(&here, "solver", "asked-12"),
                key(&hk, "weiwei", "report-40"),
                key(&hkg, "weiwei", "report-41"),
            ]
            .into()
        );
    }
}
