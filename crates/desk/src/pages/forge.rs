//! The PRs and Issues pages drawn, as their boards set them: the page's
//! title, its search and filters, and the list in sections (pull requests
//! by your part in them, issues by project); an issue read beside the
//! list.

use gpui::component::{
    Sizable as _,
    input::{Input, Textarea},
    menu::ContextMenuExt as _,
    scroll::ScrollableElement as _,
    tab::{Tab as KitTab, TabBar},
};
use gpui::{AnyElement, Context, Div, Hsla, SharedString, div, prelude::*, px};
use u0ui::{
    components::menu::MenuItem,
    prelude::*,
    t3::{
        review::{
            ChecksStatus, IssueRow, IssueState, Label, Priority, PullRequestRow, PullRequestState,
            ReviewDecision,
        },
        shell::{ListFilter, ListToolbar, WorkspacePageHeader},
    },
};

mod issue;
mod menus;

use crate::{
    desk::{
        Desk, Place,
        forge::{Sort, Tab},
    },
    fonts::Faces,
    github::{Act, Detail, Item, Kind},
    markdown, theme,
    ui::{self, Weight},
    when,
};

/// A state's colour: open green, merged violet-blue, closed coral, a
/// draft muted.
fn tint(item: &Item) -> gpui::Hsla {
    match (item.state.as_str(), item.draft) {
        (_, true) => theme::muted(),
        ("open", _) => theme::ok(),
        ("merged", _) => theme::link(),
        _ => theme::coral(),
    }
}

fn when_of(text: &str) -> String {
    when::parse(text).map_or_else(String::new, |t| when::ago(t, std::time::SystemTime::now()))
}

/// A state as the filter says it, in sentence case as its neighbours.
pub fn state_word(state: &str) -> &'static str {
    match state {
        "open" => "Open",
        "closed" => "Closed",
        "merged" => "Merged",
        _ => "All",
    }
}

/// A pull request's state, as u0ui draws it.
pub fn pr_state(item: &Item) -> PullRequestState {
    match (item.state.as_str(), item.draft) {
        (_, true) => PullRequestState::Draft,
        ("merged", _) => PullRequestState::Merged,
        ("closed", _) => PullRequestState::Closed,
        _ => PullRequestState::Open,
    }
}

/// How its checks stand, from those passed, failed and still going: none
/// where it has none.
pub fn checks_of((passed, failed, going): (usize, usize, usize)) -> Option<ChecksStatus> {
    match () {
        () if failed > 0 => Some(ChecksStatus::Failed),
        () if going > 0 => Some(ChecksStatus::Running),
        () if passed > 0 => Some(ChecksStatus::Passed),
        () => None,
    }
}

/// A review's word, as the forge says it (`approved`, `changes
/// requested`, `review required`), as u0ui draws it.
pub fn decision(review: &str) -> Option<ReviewDecision> {
    match review {
        "approved" => Some(ReviewDecision::Approved),
        "changes requested" => Some(ReviewDecision::Changes),
        "review required" | "awaiting" | "pending" => Some(ReviewDecision::Awaiting),
        _ => None,
    }
}

/// Its labels in their colours (`#rrggbb`), the muted icon colour for one
/// the forge gives none.
pub fn labels_of(item: &Item) -> Vec<Label> {
    let coloured = |name: &String| {
        (item.label_colors.iter())
            .find(|(n, _)| n == name)
            .and_then(|(_, c)| u32::from_str_radix(c.trim_start_matches('#'), 16).ok())
            .map_or_else(
                || theme::role(ColorRole::IconSecondary),
                |c| Hsla::from(gpui::rgb(c)),
            )
    };
    (item.labels.iter())
        .map(|name| Label::new(name.clone(), coloured(name)))
        .collect()
}

/// An issue's state as u0ui draws it, from the forge's (`open`, `done`)
/// or Linear's (`progress`, `backlog`, `cancelled`) word.
pub fn issue_state(item: &Item) -> IssueState {
    match item.issue_state.as_deref() {
        Some("progress") => IssueState::Progress,
        Some("done") => IssueState::Done,
        Some("cancelled") => IssueState::Cancelled,
        Some("backlog") => IssueState::Backlog,
        _ if item.state == "closed" => IssueState::Done,
        _ => IssueState::Open,
    }
}

/// Linear's priority (1 urgent to 4 low; none 0).
pub fn priority_of(item: &Item) -> Option<Priority> {
    match item.priority? {
        1 => Some(Priority::Urgent),
        2 => Some(Priority::High),
        3 => Some(Priority::Medium),
        4 => Some(Priority::Low),
        _ => None,
    }
}

/// How the list is cut into sections: pull requests waiting on your
/// review, yours, and the rest; issues by project.
pub fn pr_section(item: &Item) -> usize {
    match () {
        () if item.viewer_requested && item.state == "open" => 0,
        () if item.viewer_authored => 1,
        () => 2,
    }
}

/// A filter, set to `value` where it has one.
fn valued(filter: ListFilter, value: Option<SharedString>) -> ListFilter {
    match value {
        Some(v) => filter.value(v),
        None => filter,
    }
}

/// The sections' names, in order.
const PR_SECTIONS: [&str; 3] = ["Needs your review", "Yours", "Watching"];

impl Desk {
    /// The top bar over a page of them: the repositories read and when,
    /// or, over a pull request read, the way back to the list and to it on
    /// its host.
    pub(crate) fn forge_topbar(&self, left: Div, right: Div, cx: &mut Context<Self>) -> (Div, Div) {
        let f = &self.forge;
        if f.kind == Kind::Pr && f.chosen.is_some() {
            return self.pr_topbar(left, right, cx);
        }
        let mut names: Vec<String> = match &f.every {
            Some(every) => every.iter().map(|(p, _)| self.forge_name(p)).collect(),
            None => f.folder.iter().map(|p| self.forge_name(p)).collect(),
        };
        if let Some(when) = f.listed_at {
            let ago = when::ago(when, std::time::SystemTime::now());
            names.push(if ago == "now" {
                crate::tr("synced just now").to_string()
            } else {
                crate::tr_with("synced %{when} ago", &[("when", &ago)]).to_string()
            });
        }
        let line = div()
            .type_style(TypeStyle::Label01)
            .text_color(theme::role(ColorRole::TextSecondary))
            .child(ui::one_line(div().child(names.join(" · "))));
        (left.child(line), right)
    }

    pub(crate) fn forge_page(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        if f.kind == Kind::Pr && f.chosen.is_some() {
            return self.pr_page(faces, cx);
        }
        let list = div()
            .id("forge-page")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scrollbar()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .px(px(40.))
                    .pt(px(12.))
                    .pb(px(32.))
                    .child(self.forge_head())
                    .child(self.forge_toolbar(cx))
                    .child(self.forge_list(faces, cx)),
            );
        let side = (f.kind == Kind::Issue && (f.chosen.is_some() || f.creating)).then(|| {
            div()
                .w(px(400.))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .border_l_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .bg(theme::role(ColorRole::Layer01))
                .child(if f.creating {
                    self.forge_form(faces, &crate::tr("New issue"), crate::tr("Create"), cx)
                } else {
                    self.issue_aside(faces, cx)
                })
        });
        div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .child(list)
            .children(side)
    }

    /// The page's title and what it lists.
    fn forge_head(&self) -> WorkspacePageHeader {
        match self.forge.kind {
            Kind::Pr => WorkspacePageHeader::new("forge-head", crate::tr("Pull requests"))
                .description(crate::tr(
                    "Across GitHub, GitLab, Bitbucket and Azure DevOps, and from the other machines in this window.",
                )),
            Kind::Issue => WorkspacePageHeader::new("forge-head", crate::tr("Issues"))
                .description(crate::tr(
                    "Linear and GitHub issues for your projects. Start a thread from any of them.",
                )),
        }
    }

    /// The host the page's project keeps them on, by what was listed.
    fn forge_host(&self) -> Option<crate::forge::Host> {
        let url = (self.forge.rows.items().first()).map(|i| i.url.as_str());
        url.and_then(crate::forge::Host::of_url)
    }

    /// The search, the filters (its state, project, your part, a label;
    /// for issues where they are kept), the order and the views saved, and
    /// what can be done: read again, a new issue.
    fn forge_toolbar(&self, cx: &mut Context<Self>) -> ListToolbar {
        let f = &self.forge;
        let desk = cx.entity();
        let pick = |go: fn(&mut Desk, usize, &mut gpui::Window, &mut Context<Desk>)| {
            let desk = desk.clone();
            move |n: usize, window: &mut gpui::Window, cx: &mut gpui::App| {
                desk.update(cx, |d, cx| go(d, n, window, cx));
            }
        };
        let radio = |name: &str, options: Vec<SharedString>, at: usize| {
            MenuItem::radio_group(crate::tr(name), options).selected_item(Some(at))
        };
        // Its state.
        let states: &[&'static str] = match f.kind {
            Kind::Pr => &["open", "closed", "merged", "all"],
            Kind::Issue => &["open", "closed", "all"],
        };
        let at = states.iter().position(|s| *s == f.state).unwrap_or(0);
        let state = ListFilter::new(crate::tr("State"))
            .value(crate::tr(state_word(f.state)))
            .menu([radio(
                "State",
                states.iter().map(|s| crate::tr(state_word(s))).collect(),
                at,
            )
            .on_select_item(pick(|d, n, _, cx| {
                let states: &[&'static str] = match d.forge.kind {
                    Kind::Pr => &["open", "closed", "merged", "all"],
                    Kind::Issue => &["open", "closed", "all"],
                };
                d.forge.state = states[n];
                d.list_forge(cx);
            }))]);
        // Your part in them: a pull request opened by you, an issue
        // assigned to you.
        let (part_name, mine, part) = match f.kind {
            Kind::Pr => ("Author", "You", 1),
            Kind::Issue => ("Assignee", "Me", 2),
        };
        let yours = valued(
            ListFilter::new(crate::tr(part_name)),
            (f.part == part).then(|| crate::tr(mine)),
        )
        .menu([radio(
            part_name,
            vec![crate::tr("Anyone"), crate::tr(mine)],
            usize::from(f.part == part),
        )
        .on_select_item(pick(|d, n, _, cx| {
            let part = if d.forge.kind == Kind::Pr { 1 } else { 2 };
            d.forge.part = if n == 0 { 0 } else { part };
            d.list_forge(cx);
        }))]);
        let toolbar = ListToolbar::new("forge-bar")
            .search_state(f.search.clone())
            .search_width(if f.kind == Kind::Pr { 240. } else { 160. })
            .placeholder(crate::tr(match f.kind {
                Kind::Pr => "Search pull requests",
                Kind::Issue => "Search",
            }))
            .filter(state)
            .filter(self.forge_repository_filter(cx))
            .filter(yours);
        let toolbar = match f.kind {
            Kind::Pr => toolbar.filter(self.forge_label_filter(cx)),
            Kind::Issue => toolbar.filter(self.forge_tracker_filter(cx)),
        };
        self.forge_views_of(toolbar, cx)
            .when(f.kind == Kind::Issue && f.binding, |t| {
                t.action(div().w(px(200.)).child(Input::new(&f.team).small()))
            })
            .when(f.kind == Kind::Issue && f.every.is_none(), |t| {
                t.action(
                    Button::new("new-issue", crate::tr("New issue"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Md)
                        .icon(CarbonIcon::Add)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.edit_forge(true, window, cx);
                        })),
                )
            })
            .action(
                Button::new("forge-refresh", crate::tr("Read again"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Md)
                    .icon(CarbonIcon::Renew)
                    .icon_only(true)
                    .on_click(cx.listener(|this, _, _, cx| this.list_forge(cx))),
            )
    }

    /// Which project's: every one's, or one repository's.
    fn forge_repository_filter(&self, cx: &mut Context<Self>) -> ListFilter {
        let f = &self.forge;
        let places = self.forge_places();
        let at = match (&f.every, &f.folder) {
            (Some(_), _) | (_, None) => 0,
            (None, Some(p)) => places.iter().position(|q| q == p).map_or(0, |n| n + 1),
        };
        let names: Vec<SharedString> = std::iter::once(crate::tr("All projects"))
            .chain(places.iter().map(|p| self.forge_name(p).into()))
            .collect();
        let desk = cx.entity();
        let value = (at > 0).then(|| self.forge_name(&places[at - 1]).into());
        valued(ListFilter::new(crate::tr("Repository")), value).menu([MenuItem::radio_group(
            crate::tr("Repository"),
            names,
        )
        .selected_item(Some(at))
        .on_select_item(move |n, window, cx| {
            let place = places.get(n.wrapping_sub(1)).cloned();
            desk.update(cx, |d, cx| d.list_projects(n == 0, place, window, cx));
        })])
    }

    /// The label they carry: one of those of the pull requests listed.
    fn forge_label_filter(&self, cx: &mut Context<Self>) -> ListFilter {
        let f = &self.forge;
        let mut labels: Vec<String> = (f.rows.items().iter())
            .flat_map(|i| i.labels.iter().cloned())
            .chain(f.label.clone())
            .collect();
        labels.sort();
        labels.dedup();
        let at = (f.label.as_ref()).and_then(|l| labels.iter().position(|x| x == l));
        let names: Vec<SharedString> = std::iter::once(crate::tr("Any"))
            .chain(labels.iter().map(|l| SharedString::from(l.clone())))
            .collect();
        let desk = cx.entity();
        valued(
            ListFilter::new(crate::tr("Label")),
            f.label.clone().map(Into::into),
        )
        .menu([MenuItem::radio_group(crate::tr("Label"), names)
            .selected_item(Some(at.map_or(0, |n| n + 1)))
            .on_select_item(move |n, _, cx| {
                let label = labels.get(n.wrapping_sub(1)).cloned();
                desk.update(cx, |d, cx| {
                    d.forge.label = label;
                    d.list_forge(cx);
                });
            })])
    }

    /// Where the project's issues are kept: its host's, or a Linear
    /// team's, bound by its key.
    fn forge_tracker_filter(&self, cx: &mut Context<Self>) -> ListFilter {
        let f = &self.forge;
        let team = (f.folder.as_ref()).and_then(|p| self.prefs(p).linear);
        let host = (self
            .forge_host()
            .filter(|h| *h != crate::forge::Host::Linear))
        .map_or_else(|| crate::tr("Its host"), |h| h.name().into());
        let linear = team.as_ref().map_or_else(
            || crate::tr("A Linear team…"),
            |t| crate::tr_with("Linear · %{team}", &[("team", t)]),
        );
        let value = if team.is_some() {
            linear.clone()
        } else {
            host.clone()
        };
        let desk = cx.entity();
        valued(
            ListFilter::new(crate::tr("Tracker")),
            f.every.is_none().then_some(value),
        )
        .menu([MenuItem::radio_group(crate::tr("Tracker"), [host, linear])
            .selected_item(Some(usize::from(team.is_some())))
            .on_select_item(move |n, window, cx| {
                desk.update(cx, |d, cx| {
                    if n == 0 {
                        d.forge.binding = false;
                        (d.forge.team).update(cx, |i, cx| i.set_value("", window, cx));
                        d.bind_linear(cx);
                    } else {
                        d.forge.binding = true;
                        let team = d.forge.team.clone();
                        team.update(cx, |i, cx| {
                            window.focus(&gpui::Focusable::focus_handle(&*i, cx), cx);
                        });
                        cx.notify();
                    }
                });
            })])
    }

    /// The orders it can be shown in, and the views saved: each chosen to
    /// show it, the one shown saved, or a saved one removed.
    fn forge_views_of(&self, toolbar: ListToolbar, cx: &mut Context<Self>) -> ListToolbar {
        let f = &self.forge;
        let sorts = Sort::of(f.kind);
        let views: Vec<_> = (self.settings.views.iter())
            .filter(|v| v.kind == f.kind)
            .cloned()
            .collect();
        let removes: Vec<String> = (views.iter())
            .map(|v| crate::tr_with("Remove %{name}", &[("name", &v.name)]).to_string())
            .collect();
        let names: Vec<SharedString> = (sorts.iter().map(|s| crate::tr(s.word())))
            .chain(views.iter().map(|v| v.name.clone().into()))
            .chain(removes.iter().cloned().map(SharedString::from))
            .collect();
        let save = crate::tr("Save this view");
        let desk = cx.entity();
        toolbar
            .views(names)
            .view(crate::tr(f.sort.word()))
            .labels([crate::tr("Saved views"), save.clone(), crate::tr("Clear")])
            .on_view_change(move |name, window, cx| {
                desk.update(cx, |d, cx| {
                    if let Some(s) = sorts.iter().find(|s| crate::tr(s.word()) == name) {
                        d.forge.sort = *s;
                        cx.notify();
                    } else if let Some(v) = views.iter().find(|v| v.name == name.as_ref()) {
                        d.show_view(v, window, cx);
                    } else if let Some(at) = removes.iter().position(|r| r == name.as_ref()) {
                        d.remove_view(&views[at].name, cx);
                    } else if name == save {
                        d.save_view(cx);
                    }
                });
            })
    }

    /// The list in its sections, or why there is none.
    fn forge_list(&self, faces: &Faces, cx: &mut Context<Self>) -> AnyElement {
        let f = &self.forge;
        let project = match &f.folder {
            _ if f.every.is_some() => crate::tr("All projects").to_string(),
            Some(p) => self.forge_name(p),
            None => crate::tr("No project").to_string(),
        };
        let state = match &f.rows.list {
            None => {
                use u0ui::components::spinner::{Spinner, SpinnerSize};
                return div()
                    .pt(px(32.))
                    .child(
                        Spinner::new("forge-reading")
                            .size(SpinnerSize::Inline)
                            .description(crate::tr("Reading…")),
                    )
                    .into_any_element();
            }
            Some(Err(why)) => Err(why.clone()),
            Some(Ok(items)) if items.is_empty() => Ok(()),
            Some(Ok(_)) => return self.forge_sections(faces, cx).into_any_element(),
        };
        if let Some(full) = self.forge_state(&project, state.clone(), cx) {
            return full.into_any_element();
        }
        div()
            .pt(px(32.))
            .child(match state {
                Err(e) => ui::trouble(
                    "forge-unlisted",
                    crate::tr_with("Couldn't list them: %{e}", &[("e", &e)]),
                )
                .into_any_element(),
                Ok(()) => ui::fine(faces, crate::tr("None here.")).into_any_element(),
            })
            .into_any_element()
    }

    /// Each section: its name and how many, then its rows in the order
    /// chosen.
    fn forge_sections(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        let rows = &f.rows;
        let place_of = |row: usize| {
            rows.kin(row)
                .0
                .map(|(p, _)| p.clone())
                .or_else(|| f.folder.clone())
                .unwrap_or_default()
        };
        let all: Vec<(usize, &Item)> = rows.items().iter().enumerate().collect();
        // Pull requests by your part in them; issues by project.
        let sections: Vec<(String, Vec<usize>)> = match f.kind {
            Kind::Pr => PR_SECTIONS
                .iter()
                .enumerate()
                .map(|(n, name)| {
                    let of = all.iter().filter(|(_, i)| pr_section(i) == n);
                    (crate::tr(name).to_string(), of.map(|(r, _)| *r).collect())
                })
                .collect(),
            Kind::Issue if rows.groups.is_empty() => {
                let name = f.folder.as_ref().map(|p| self.forge_name(p));
                vec![(name.unwrap_or_default(), (0..all.len()).collect())]
            }
            Kind::Issue => {
                let mut start = 0;
                (rows.groups.iter())
                    .map(|(_, name, len)| {
                        let r = (start..start + len).collect();
                        start += len;
                        (name.clone(), r)
                    })
                    .collect()
            }
        };
        let mut page = div().flex().flex_col();
        for (name, members) in sections.into_iter().filter(|(_, m)| !m.is_empty()) {
            let items: Vec<&Item> = members.iter().map(|r| all[*r].1).collect();
            let ordered = f.sort.sorted(items);
            let mut section = div().flex().flex_col().mt(px(32.)).child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .pb(px(8.))
                    .border_b_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
                    .type_style(TypeStyle::HeadingCompact01)
                    .text_color(theme::role(ColorRole::TextPrimary))
                    .child(name)
                    .child(
                        div()
                            .type_style(TypeStyle::BodyCompact01)
                            .text_color(theme::role(ColorRole::TextHelper))
                            .child(ordered.len().to_string()),
                    ),
            );
            for item in ordered {
                // Where it is in the list, for its project.
                let row = (all.iter())
                    .position(|(_, i)| std::ptr::eq(*i, item))
                    .unwrap_or_default();
                let place = place_of(row);
                section = section.child(match f.kind {
                    Kind::Pr => self.pr_row(item, place, row, cx),
                    Kind::Issue => self.issue_row(faces, item, place, row, cx),
                });
            }
            page = page.child(section);
        }
        page
    }

    /// A pull request's row, opened to read it; its menu on a right click.
    fn pr_row(&self, item: &Item, place: Place, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let desk = cx.entity();
        let n = item.number;
        let at = place.clone();
        let mut r = PullRequestRow::new(
            SharedString::from(format!("pr-{row}")),
            u32::try_from(n).unwrap_or(u32::MAX),
            item.title.clone(),
        )
        .state(pr_state(item))
        .repo(self.forge_name(&place))
        .author(item.author.clone())
        .branches(item.branch.clone(), item.base.clone())
        .labels(labels_of(item))
        .comments(item.comments)
        .time(when_of(&item.updated))
        .divided(false)
        .show_avatar(false)
        .on_click({
            let desk = desk.clone();
            move |_, cx| {
                desk.update(cx, |d, cx| {
                    d.forge.folder = Some(at.clone());
                    d.forge.tab = Tab::Summary;
                    d.read_forge(n, cx);
                });
            }
        });
        if let Some(c) = checks_of(item.checks) {
            r = r.checks(c);
        }
        if let Some(rv) = decision(&item.review) {
            r = r.review(rv);
        }
        if item.added + item.removed > 0 {
            r = r.stat(item.added, item.removed);
        }
        Self::row_menu(r, item, place, row, desk)
    }

    /// An issue's row, read beside the list; the threads on it, and how the
    /// latest stands.
    fn issue_row(
        &self,
        _: &Faces,
        item: &Item,
        place: Place,
        row: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let desk = cx.entity();
        let n = item.number;
        let at = place.clone();
        let threads = self.issue_threads(&place, n);
        let chosen = self.forge.chosen == Some(n) && self.forge.folder.as_ref() == Some(&place);
        let key = (item.key.clone()).unwrap_or_else(|| format!("#{n}"));
        let mut r = IssueRow::new(
            SharedString::from(format!("issue-{row}")),
            key,
            item.title.clone(),
        )
        .state(issue_state(item))
        .labels(labels_of(item))
        .time(when_of(&item.updated))
        .linked_threads(threads.len())
        .selected(chosen)
        .divided(false)
        .dash_when_unprioritized(true)
        .show_assignee_avatar(false)
        .on_click({
            let desk = desk.clone();
            move |_, cx| {
                desk.update(cx, |d, cx| {
                    d.forge.folder = Some(at.clone());
                    d.read_forge(n, cx);
                });
            }
        });
        if let Some(p) = priority_of(item) {
            r = r.priority(p);
        }
        if let Some(who) = &item.assignee {
            r = r.assignee(who.clone());
        }
        if let Some(pill) = threads.first().and_then(|s| self.thread_pill(s)) {
            r = r.trailing(pill);
        }
        Self::row_menu(r, item, place, row, desk)
    }

    /// The sessions about the issue `n` of `place`, the latest first.
    pub(crate) fn issue_threads(&self, place: &Place, n: u64) -> Vec<&crate::session::Session> {
        let mut found: Vec<_> = (self.sessions.all.iter())
            .filter(|s| s.host == place.host && place.folder.starts_with(&s.meta.project))
            .filter(|s| (s.meta.links.iter()).any(|l| l.kind == Kind::Issue && l.number == n))
            .collect();
        found.sort_by_key(|s| std::cmp::Reverse(s.meta.updated));
        found
    }

    /// How a thread stands, as a pill: its agent and its state; none for
    /// one with nothing going on.
    pub(crate) fn thread_pill(&self, s: &crate::session::Session) -> Option<u0ui::t3::StatusPill> {
        use crate::desk::lists::{self, Status as S};
        use u0ui::t3::Status;
        let status = lists::status(s)?;
        let pill = match status {
            S::Working => Status::Working,
            S::Approval | S::SignIn | S::Plan => Status::NeedsYou,
            S::Failed | S::Limited => Status::Failed,
            S::Done | S::Woke => Status::Done,
        };
        let label = format!(
            "{} · {}",
            self.name_of(&s.meta.agent),
            crate::tr(status.word())
        );
        Some(u0ui::t3::StatusPill::new(pill).label(label))
    }

    /// A row with what can be done with it on a right click.
    fn row_menu(
        row: impl IntoElement,
        item: &Item,
        place: Place,
        n: usize,
        desk: gpui::Entity<Self>,
    ) -> AnyElement {
        let item = item.clone();
        div()
            .id(SharedString::from(format!("forge-row-{n}")))
            .child(row)
            .context_menu(move |menu, _, cx| {
                desk.read(cx).forge_menu(&item, place.clone(), menu, &desk)
            })
            .into_any_element()
    }

    /// Full-body empty or error state on the PRs / Issues page (`EmptyStates`).
    fn forge_state(
        &self,
        project: &str,
        list: Result<(), String>,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        use u0ui::components::alert::{Alert, AlertVariant};
        use u0ui::components::empty::{
            Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle,
        };
        use u0ui::prelude::*;
        let f = &self.forge;
        let wrap = |center: bool| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .p(px(24.))
                .gap(px(16.))
                .when(center, |d| d.items_start().justify_center())
        };
        match list {
            Err(why) if why == "no remote" => Some(
                wrap(true).child(
                    Empty::new()
                        .child(
                            EmptyHeader::new()
                                .child(EmptyTitle::new(crate::tr_with(
                                    "%{project} has no host",
                                    &[("project", &project)],
                                )))
                                .child(EmptyDescription::new(crate::tr(
                                    "Add a remote with git remote add origin, then read again.",
                                ))),
                        )
                        .child(
                            EmptyContent::new()
                                .child(
                                    Button::new("forge-no-host-again", crate::tr("Read again"))
                                        .variant(ButtonVariant::Secondary)
                                        .size(ButtonSize::Sm)
                                        .icon(CarbonIcon::Renew)
                                        .on_click(cx.listener(|this, _, _, cx| this.list_forge(cx))),
                                )
                                .child(
                                    Button::new("forge-no-host-all", crate::tr("All projects"))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.list_projects(true, None, window, cx);
                                        })),
                                ),
                        ),
                ),
            ),
            Err(why) if why.starts_with("no token:") => {
                let host_name = why
                    .strip_prefix("no token:")
                    .and_then(|s| s.split(':').next())
                    .map_or("host", str::trim);
                Some(
                    wrap(false)
                        .child(
                            Alert::new("forge-no-token")
                                .variant(AlertVariant::Info)
                                .title(crate::tr("Only what is public"))
                                .description(crate::tr_with(
                                    "%{project} is private; add a %{host} token (kept in the Keychain).",
                                    &[("host", &host_name), ("project", &project)],
                                )),
                        )
                        .child(
                            Button::new(
                                "forge-give-token",
                                crate::tr("Give a token in Source control › Hosts"),
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_tab(crate::pages::Tab::SourceControl, window, cx);
                            })),
                        ),
                )
            }
            Err(why) if why.contains("Linear team") => {
                let place = f.folder.clone();
                Some(
                    wrap(true).child(
                        Empty::new()
                            .child(
                                EmptyHeader::new()
                                    .child(EmptyTitle::new(crate::tr_with(
                                        "%{project}’s issues come through Linear",
                                        &[("project", &project)],
                                    )))
                                    .child(EmptyDescription::new(crate::tr_with(
                                        "Bind %{project} to a Linear team key, such as ENG, in its project settings.",
                                        &[("project", &project)],
                                    ))),
                            )
                            .child(
                                EmptyContent::new().child(
                                    Button::new(
                                        "forge-choose-linear",
                                        crate::tr("Choose a Linear team"),
                                    )
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.pages.project.clone_from(&place);
                                        this.open_tab(crate::pages::Tab::Projects, window, cx);
                                    })),
                                ),
                            ),
                    ),
                )
            }
            Err(why)
                if !self.on_path.contains(&"gh")
                    || (why.contains("gh") && why.contains("No such file")) =>
            {
                Some(
                    wrap(false)
                        .child(
                            ui::alert("forge-no-gh", AlertVariant::Warning)
                                .title(crate::tr(
                                    "GitHub is reached with gh, and gh is not on this Mac",
                                ))
                                .words(crate::tr(
                                    "brew install gh, then gh auth login, or add a token in Source control › Hosts.",
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(8.))
                                .child(
                                    Button::new(
                                        "forge-copy-brew-gh",
                                        crate::tr("Copy brew install gh"),
                                    )
                                    .variant(ButtonVariant::Secondary)
                                    .size(ButtonSize::Sm)
                                    .icon(CarbonIcon::Copy)
                                    .on_click(|_, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            "brew install gh".into(),
                                        ));
                                    }),
                                )
                                .child(
                                    Button::new("forge-no-gh-again", crate::tr("Read again"))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .icon(CarbonIcon::Renew)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.list_forge(cx)),
                                        ),
                                ),
                        ),
                )
            }
            Ok(()) if f.every.is_none() => {
                let filtered = f.part != 0
                    || f.state != "open"
                    || !f.search.read(cx).value().trim().is_empty();
                let desc = if f.kind == Kind::Issue && f.part == 1 && f.state == "open" {
                    crate::tr_with(
                        "No open issue in %{project} is assigned to you or was opened by you.",
                        &[("project", &project)],
                    )
                } else {
                    crate::tr("Nothing matched these filters.")
                };
                let mut actions = EmptyContent::new();
                if filtered {
                    actions = actions.child(
                        Button::new("forge-show-all", crate::tr("Show All"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.forge.part = 0;
                                this.forge.state = "open";
                                this.forge
                                    .search
                                    .update(cx, |i, cx| i.set_value("", window, cx));
                                this.list_forge(cx);
                            })),
                    );
                }
                if f.kind == Kind::Issue {
                    actions = actions.child(
                        Button::new("forge-empty-new-issue", crate::tr("New issue"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Add)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.edit_forge(true, window, cx);
                            })),
                    );
                }
                Some(
                    wrap(true).child(
                        Empty::new()
                            .child(
                                EmptyHeader::new()
                                    .child(EmptyTitle::new(crate::tr("None here.")))
                                    .child(EmptyDescription::new(desc)),
                            )
                            .when(filtered || f.kind == Kind::Issue, |e| e.child(actions)),
                    ),
                )
            }
            _ => None,
        }
    }
}

impl Desk {
    /// The one read: its head and what can be done with it, its tabs, and
    /// the line to say something.
    pub(super) fn forge_detail(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let f = &self.forge;
        let shell = div().flex().flex_col().flex_1().min_w(px(0.)).h_full();
        let d = match &f.detail {
            None if f.chosen.is_none() => {
                return shell.items_center().justify_center().child(ui::fine(
                    faces,
                    crate::tr(match f.kind {
                        Kind::Pr => "Pick a pull request to read it.",
                        Kind::Issue => "Pick an issue to read it.",
                    }),
                ));
            }
            None => {
                return shell
                    .p(px(16.))
                    .child(ui::fine(faces, crate::tr("Reading…")));
            }
            Some(Err(e)) => {
                return shell.p(px(16.)).child(ui::trouble(
                    "forge-unread",
                    crate::tr_with("Couldn't read it: %{e}", &[("e", e)]),
                ));
            }
            Some(Ok(d)) => d,
        };
        if f.editing {
            return self.forge_form(faces, &crate::tr("Edit"), crate::tr("Save"), cx);
        }
        let item = &d.item;
        let url = item.url.clone();
        let head = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(20.))
            .pt(px(16.))
            .pb(px(10.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(
                        ui::text(faces, format!("{} #{}", item.title, item.number))
                            .flex_1()
                            .text_size(px(crate::style::H2))
                            .text_color(theme::text()),
                    )
                    .child(
                        ui::button("forge-open", crate::tr("Open on its host"), Weight::Quiet)
                            .h(px(26.))
                            .on_click(cx.listener(move |this, _, _, cx| this.browse(&url, cx))),
                    )
                    .child(self.forge_actions(&d.item, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::fine(
                            faces,
                            if item.draft {
                                crate::tr("draft").to_string()
                            } else {
                                item.state.clone()
                            },
                        )
                        .text_color(tint(item)),
                    )
                    .child(ui::fine(
                        faces,
                        crate::tr_with("by %{author}", &[("author", &item.author)]),
                    ))
                    .when(!item.branch.is_empty(), |d| {
                        d.child(ui::fine_mono(
                            faces,
                            format!("{} → {}", item.branch, item.base),
                        ))
                    })
                    .when(d.conflicts, |d| {
                        d.child(
                            ui::fine(faces, crate::tr("conflicts with its base"))
                                .text_color(theme::coral()),
                        )
                    })
                    .child(ui::fine(faces, when_of(&item.updated))),
            );
        let tabs: &[(&str, Tab)] = match f.kind {
            Kind::Pr => &[
                ("Summary", Tab::Summary),
                ("Timeline", Tab::Timeline),
                ("Code", Tab::Code),
                ("Conversations", Tab::Threads),
                ("Guide", Tab::Guide),
            ],
            Kind::Issue => &[("Summary", Tab::Summary), ("Timeline", Tab::Timeline)],
        };
        let chosen = tabs.iter().position(|(_, t)| *t == f.tab).unwrap_or(0);
        let picks: Vec<Tab> = tabs.iter().map(|(_, t)| *t).collect();
        let strip = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(20.))
            .pb(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                TabBar::new("forge-tabs")
                    .segmented()
                    .small()
                    .selected_index(chosen)
                    .children(
                        tabs.iter()
                            .map(|(name, _)| KitTab::new().label(crate::tr(name))),
                    )
                    .on_click(cx.listener(move |this, n: &usize, _, cx| {
                        this.forge.tab = picks[*n];
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .when(f.tab == Tab::Code && !f.pr.files.is_empty(), |el| {
                let seen = (f.pr.files.iter()).filter(|file| self.viewed(file)).count();
                el.child(ui::fine(
                    faces,
                    crate::tr_with(
                        "%{seen} of %{total} viewed",
                        &[("seen", &seen), ("total", &f.pr.files.len())],
                    ),
                ))
            });
        let body: gpui::AnyElement = match f.tab {
            Tab::Code if f.kind == Kind::Pr => div()
                .flex_1()
                .min_h(px(0.))
                .child(self.pr_code(d, cx))
                .into_any_element(),
            Tab::Threads if f.kind == Kind::Pr => div()
                .id("forge-threads")
                .flex_1()
                .min_h(px(0.))
                .child(self.pr_threads(faces, d, cx))
                .overflow_y_scrollbar()
                .into_any_element(),
            Tab::Guide if f.kind == Kind::Pr => div()
                .id("forge-guide")
                .flex_1()
                .min_h(px(0.))
                .child(self.pr_guide(faces, d, cx))
                .overflow_y_scrollbar()
                .into_any_element(),
            Tab::Timeline => {
                let said: Vec<_> = d.said.iter().collect();
                let mut list = div().flex().flex_col().gap(px(14.)).px(px(20.)).py(px(14.));
                if said.is_empty() {
                    list = list.child(ui::fine(faces, crate::tr("Nothing said yet.")));
                }
                for (n, s) in said.into_iter().enumerate() {
                    list = list.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(ui::fine(
                                faces,
                                format!("{} {} · {}", s.who, s.how, when_of(&s.when)),
                            ))
                            .when(!s.body.trim().is_empty(), |d| {
                                d.child(markdown::render(
                                    faces,
                                    &format!("forge-said-{n}"),
                                    &s.body,
                                    cx,
                                ))
                            }),
                    );
                }
                div()
                    .id("forge-timeline")
                    .flex_1()
                    .min_h(px(0.))
                    .child(list)
                    .overflow_y_scrollbar()
                    .into_any_element()
            }
            _ => div()
                .id("forge-summary")
                .flex_1()
                .min_h(px(0.))
                .child(self.forge_summary(faces, d, cx))
                .overflow_y_scrollbar()
                .into_any_element(),
        };
        shell
            .child(head)
            .child(strip)
            .child(body)
            .child(self.forge_say(faces, cx))
    }

    /// What it is: its labels and people, its checks, its words.
    fn forge_summary(&self, faces: &Faces, d: &Detail, cx: &mut Context<Self>) -> Div {
        let row = |label: &str, value: String| {
            div()
                .flex()
                .gap(px(10.))
                .child(ui::fine(faces, crate::tr(label)).w(px(80.)).flex_none())
                .child(
                    ui::text(faces, value)
                        .text_size(px(crate::style::LABEL))
                        .text_color(theme::text2()),
                )
        };
        let none = |v: &[String]| {
            if v.is_empty() {
                crate::tr("none").to_string()
            } else {
                v.join(", ")
            }
        };
        let kind = self.forge.kind;
        let mut checks = div().flex().flex_col().gap(px(4.));
        for (n, c) in d.checks.iter().enumerate() {
            let url = c.url.clone();
            let failed = c.failed();
            let run = c.run().map(str::to_owned).filter(|_| failed);
            checks = checks.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::dot(match c.state.as_str() {
                        "success" | "neutral" | "skipped" => theme::ok(),
                        _ if failed => theme::coral(),
                        _ => theme::accent(),
                    }))
                    .child(
                        ui::one_line(
                            ui::text(faces, c.name.clone()).text_size(px(crate::style::LABEL)),
                        )
                        .flex_1(),
                    )
                    .child(ui::fine(faces, c.state.clone()))
                    .children(run.clone().map(|run| {
                        ui::button(
                            SharedString::from(format!("log-{n}")),
                            crate::tr("Log"),
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(cx.listener(move |this, _, _, cx| this.read_log(run.clone(), cx)))
                    }))
                    .children(run.map(|run| {
                        ui::button(
                            SharedString::from(format!("rerun-{n}")),
                            crate::tr("Run again"),
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.act_forge(Act::Rerun(run.clone()), cx);
                        }))
                    }))
                    .when(!url.is_empty(), |d| {
                        d.child(
                            ui::button(
                                SharedString::from(format!("check-{n}")),
                                crate::tr("Open"),
                                Weight::Quiet,
                            )
                            .h(px(22.))
                            .on_click(cx.listener(move |this, _, _, cx| this.browse(&url, cx))),
                        )
                    }),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(20.))
            .py(px(14.))
            .child(row("Labels", none(&d.item.labels)))
            .child(row("Assignees", none(&d.assignees)))
            .when(kind == Kind::Pr, |el| {
                let listed = self.forge.rows.of(self.forge.folder.as_ref());
                let (under, over) = super::pr::stack(&d.item, listed);
                let numbers = |ns: Vec<u64>| {
                    ns.iter()
                        .map(|n| format!("#{n}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                el.child(row("Reviewers", none(&d.reviewers)))
                    .when(under.is_some() || !over.is_empty(), |el| {
                        let mut said = Vec::new();
                        if let Some(n) = under {
                            said.push(crate::tr_with("on #%{under}", &[("under", &n)]).to_string());
                        }
                        if !over.is_empty() {
                            said.push(
                                crate::tr_with("%{numbers} on it", &[("numbers", &numbers(over))])
                                    .to_string(),
                            );
                        }
                        el.child(row("Stack", said.join("; ")))
                    })
                    .when(!self.forge.pr.files.is_empty(), |el| {
                        el.child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(ui::fine(faces, crate::tr("Changes")).w(px(80.)).flex_none())
                                .child(super::pr::breakdown(&self.forge.pr.files)),
                        )
                    })
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div().w(px(240.)).child(
                            Input::new(&self.forge.who)
                                .list(&self.forge.offered)
                                .small(),
                        ),
                    )
                    .child(
                        ui::button("add-label", crate::tr("Add label"), Weight::Quiet)
                            .h(px(24.))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_person_or_label(0, window, cx);
                            })),
                    )
                    .child(
                        ui::button(
                            "add-person",
                            crate::tr(if kind == Kind::Pr {
                                "Ask for review"
                            } else {
                                "Assign"
                            }),
                            Weight::Quiet,
                        )
                        .h(px(24.))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_person_or_label(1, window, cx);
                        })),
                    ),
            )
            .when(!d.checks.is_empty(), |el| {
                el.child(ui::label(faces, &crate::tr("Checks")))
                    .child(checks)
            })
            .children(self.forge.log.as_ref().map(|(run, log)| {
                let (text, bad) = match log {
                    Ok(t) => (t.clone(), false),
                    Err(e) => (e.clone(), true),
                };
                div()
                    .id("forge-log")
                    .max_h(px(320.))
                    .p(px(10.))
                    .rounded(px(8.))
                    .bg(theme::well())
                    .child(ui::fine(
                        faces,
                        crate::tr_with("Run %{run}, where it failed", &[("run", run)]),
                    ))
                    .child(
                        ui::fine_mono_said(faces, "forge-log-text", text)
                            .when(bad, |d| d.text_color(theme::coral())),
                    )
                    .overflow_y_scrollbar()
            }))
            .child(ui::label(faces, &crate::tr("Description")))
            .child(if d.body.trim().is_empty() {
                ui::fine(faces, crate::tr("No description.")).into_any_element()
            } else {
                markdown::render(faces, "forge-body", &d.body, cx).into_any_element()
            })
    }

    /// Add what is on the line as a label (`0`), or as a reviewer or
    /// assignee.
    pub(crate) fn add_person_or_label(
        &mut self,
        what: usize,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let named = self
            .forge
            .who
            .read(cx)
            .value()
            .trim()
            .trim_start_matches('@')
            .to_owned();
        if named.is_empty() {
            return;
        }
        self.forge
            .who
            .update(cx, |i, cx| i.set_value("", window, cx));
        let act = match (what, self.forge.kind) {
            (0, _) => Act::Label(named),
            (_, Kind::Pr) => Act::Reviewer(named),
            _ => Act::Assignee(named),
        };
        self.act_forge(act, cx);
    }

    /// The line to say something on the one read, and its buttons.
    fn forge_say(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let pr = self.forge.kind == Kind::Pr;
        let lines = self.forge.pr.lines.len();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(8.))
            .p(px(12.))
            .border_t_1()
            .border_color(theme::border())
            .child(Textarea::new(&self.forge.line).small())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(self.forge.doing.as_ref().map(|d| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(ui::live_dot())
                            .child(ui::fine(faces, format!("{d}…")))
                    }))
                    .child(div().flex_1())
                    .when(pr, |d| {
                        d.child(
                            ui::button(
                                "review-changes",
                                crate::tr("Request changes"),
                                Weight::Quiet,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.say_forge(Some("request-changes"), window, cx);
                                },
                            )),
                        )
                        .child(
                            ui::button("review-approve", crate::tr("Approve"), Weight::Secondary)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.say_forge(Some("approve"), window, cx);
                                })),
                        )
                    })
                    .child(
                        ui::button(
                            "say-comment",
                            if lines > 0 {
                                crate::tr_with(
                                    "Comment with %{lines} on lines",
                                    &[("lines", &lines)],
                                )
                            } else {
                                crate::tr("Comment")
                            },
                            Weight::Primary,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.say_forge(None, window, cx)),
                        ),
                    ),
            )
    }

    /// A title and words: editing the one read, or a new issue.
    fn forge_form(
        &self,
        faces: &Faces,
        head: &str,
        done: SharedString,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(10.))
            .p(px(20.))
            .child(ui::label(faces, head))
            .when(
                self.forge.creating && !self.forge.templates.is_empty(),
                |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.))
                            .child(ui::fine(faces, crate::tr("Begin from")))
                            .children(self.forge.templates.iter().enumerate().map(
                                |(n, (name, _))| {
                                    ui::button(
                                        SharedString::from(format!("template-{n}")),
                                        name.clone(),
                                        Weight::Secondary,
                                    )
                                    .h(px(24.))
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| this.use_template(n, window, cx),
                                    ))
                                },
                            )),
                    )
                },
            )
            .child(Input::new(&self.forge.title))
            .child(Textarea::new(&self.forge.body))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        ui::button("form-done", done, Weight::Primary)
                            .on_click(cx.listener(|this, _, _, cx| this.save_forge(cx))),
                    )
                    .child(
                        ui::button("form-cancel", crate::tr("Cancel"), Weight::Quiet).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.forge.editing = false;
                                this.forge.creating = false;
                                cx.notify();
                            }),
                        ),
                    ),
            )
    }
}
