//! The column down the right, as the Carbon design draws it (`RightColumnStates`).
//! For a thread: `RightPanelTabs` for its diff, details, files, pull request,
//! devices, terminals, preview and side chats; a changed file opens here whole.
//! For an agent's page: what it is and how u0 runs it.

mod diff;
mod files;
mod output;
#[cfg(target_os = "macos")]
mod page;
mod preview;
mod repo;
mod review;
pub(crate) mod rules;
mod side;
mod sims;
mod tabs;

pub use files::Viewer;
#[cfg(target_os = "macos")]
pub use page::{Web, Went, answers};
pub use sims::Sims;
pub use tabs::{Column, Surface};

use std::rc::Rc;

use agent_client_protocol::v1::schema::{
    SessionConfigOptionCategory, ToolCallContent, ToolCallStatus,
};
use gpui::component::InteractiveElementExt as _;
use gpui::component::{
    Sizable as _,
    button::Button as KitButton,
    input::Input,
    list::ListItem,
    menu::PopupMenu,
    popover::Popover,
    resizable::{HandleEdge, resize_handle},
    scroll::ScrollableElement as _,
};
use gpui::{Context, Div, Entity, SharedString, div, prelude::*, px};
use u0ui::{
    components::{Button, ButtonSize, ButtonVariant},
    icons::{CarbonIcon, Icon},
    t3::review::{
        ContextMeter, IssueState, PullRequestState, ThreadDetailsSection, thread::meter_percent,
    },
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
    u0::panels::{LinkedItem, LinkedItemsSection, LinkedItemsTexts},
};

use crate::{
    desk::{Desk, Page, TOPBAR, draggable},
    fonts::Faces,
    session::{Item, Session, State},
    settings::Detail,
    theme,
    thread::{self, drawn},
    ui::{self, Weight},
    usage::Use,
    when,
};

/// The right column's edge, dragged.
#[derive(Clone)]
pub struct RightEdge;

impl gpui::Render for RightEdge {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// `.rp`: `background: var(--layer-01); border-left: 1px solid var(--border-subtle-01); overflow: hidden`.
fn frame(top: impl IntoElement) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_none()
        .h_full()
        .overflow_hidden()
        .bg(theme::role(ColorRole::Layer01))
        .border_l_1()
        .border_color(theme::role(ColorRole::BorderSubtle01))
        .child(top)
}

fn top() -> Div {
    draggable()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .h(px(TOPBAR))
        .px(px(10.))
        .border_b_1()
        .border_color(theme::role(ColorRole::BorderSubtle01))
}

/// Which pane of the split the column is for (`SplitSideChat`): a 38px
/// band on `background`, its dot in `border-interactive`, "Inspecting" in
/// 12px `text-secondary` and the title semibold in `text-primary`.
fn inspecting(faces: &Faces, title: &str) -> Div {
    let said = crate::tr_with("Inspecting %{title}", &[("title", &title)]).to_string();
    let bold = said.find(title).map(|at| {
        (
            at..at + title.len(),
            gpui::HighlightStyle {
                color: Some(theme::role(ColorRole::TextPrimary)),
                font_weight: Some(gpui::FontWeight::SEMIBOLD),
                ..Default::default()
            },
        )
    });
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(TOPBAR))
        .pl(px(16.))
        .pr(px(8.))
        .bg(theme::role(ColorRole::Background))
        .border_b_1()
        .border_color(theme::role(ColorRole::BorderSubtle01))
        .child(
            div()
                .size(px(8.))
                .flex_none()
                .rounded_full()
                .bg(theme::role(ColorRole::BorderInteractive)),
        )
        .child(
            div()
                .font_family(faces.ui.clone())
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextSecondary))
                .child(gpui::StyledText::new(said).with_highlights(bold)),
        )
}

/// A section: its head and what is under it, ruled off below.
fn section(faces: &Faces, head: impl Into<SharedString>, note: Option<Div>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .px(px(16.))
        .py(px(14.))
        .border_b_1()
        .border_color(theme::role(ColorRole::BorderSubtle01))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::label(faces, &head.into()).flex_1())
                .children(note),
        )
}

/// Small print that keeps its width in a row.
fn fine(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::fine(faces, text).flex_none()
}

fn fine_mono(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::fine_mono(faces, text).flex_none()
}

/// A row in a section: 28 high.
fn frow() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(28.))
        .text_size(px(crate::style::LABEL))
}

fn empty(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .text_size(px(crate::style::CAPTION))
        .line_height(px(18.))
        .text_color(theme::role(ColorRole::TextSecondary))
}

/// One `.dt` definition row (`96px` key in `text-secondary`, value in `text-primary`).
fn dt_row(faces: &Faces, key: impl Into<SharedString>, value: impl IntoElement) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .type_style(TypeStyle::Label01)
        .child(
            ui::text(faces, key)
                .w(px(96.))
                .flex_none()
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextSecondary)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextPrimary))
                .child(value),
        )
}

impl Desk {
    pub(crate) fn aside(
        &self,
        faces: &Faces,
        cell: thread::drawer::Grid,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(s) = self.session() else {
            return frame(top());
        };
        if self.panes.whole && self.surface() == Some(Surface::Diff) {
            return self.review_workspace(faces, s, cx);
        }
        let inspecting = self
            .split
            .is_some()
            .then(|| inspecting(faces, s.shown_title()));
        let head = div()
            .flex()
            .flex_col()
            .children(inspecting)
            .child(self.strip(faces, s, cx));
        let scrolled = |body: Div| {
            div()
                .id("aside")
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .child(body)
                .overflow_y_scrollbar()
        };
        let body = match self.surface() {
            None => self.no_tabs(faces, cx).into_any_element(),
            Some(Surface::Details) => scrolled(self.details(faces, s, cx)).into_any_element(),
            Some(Surface::Diff) => self.diff_tab(faces, s, cx).into_any_element(),
            Some(Surface::Files) => self.files_tab(faces, s, cx).into_any_element(),
            Some(Surface::File(path)) => self.file_tab(faces, s, &path, cx).into_any_element(),
            Some(Surface::Terminal) => self.terminal_view(faces, cell, cx).into_any_element(),
            Some(Surface::PullRequest) => {
                scrolled(self.pull_request(faces, s, cx).unwrap_or_else(|| {
                    section(faces, crate::tr("Pull request"), None)
                        .border_b_0()
                        .child(empty(
                            faces,
                            crate::tr(
                                "None for this branch yet: commit, push and open one from the Diff tab.",
                            ),
                        ))
                }))
                .into_any_element()
            }
            Some(Surface::Side(key)) => self.side_chat(faces, key, cx).into_any_element(),
            Some(Surface::Devices) => scrolled(self.sims_tab(faces, cx)).into_any_element(),
            Some(Surface::Preview) => self.preview_tab(cx).into_any_element(),
            Some(Surface::Output(id)) => self.output_tab(faces, s, &id, cx).into_any_element(),
        };
        let split_device =
            self.viewer.beside_device && matches!(self.surface(), Some(Surface::File(_)));
        let main_col = frame(head).child(body);
        let container = if split_device {
            div()
                .relative()
                .flex()
                .flex_none()
                .h_full()
                .overflow_hidden()
                .child(main_col.flex_1().min_w(px(0.)))
                .child(self.device_split_pane(faces, cx))
        } else {
            main_col
        };
        container.child(
            resize_handle::<RightEdge, RightEdge>("right-edge", gpui::Axis::Horizontal)
                .inside(HandleEdge::Leading)
                .on_drag(RightEdge, |edge, _, _, cx| cx.new(|_| edge.clone()))
                .on_double_click(cx.listener(|this, _, _, cx| {
                    this.settings.right = crate::settings::RIGHT;
                    this.save_settings();
                    cx.notify();
                })),
        )
    }

    /// The Details tab (`RightColumnStates` Column 2): collapsible Carbon
    /// `ThreadDetailsSection`s + `LinkedItemsSection` + `"Choose what shows here"`.
    fn details(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Div {
        let shown = |d: Detail| !self.settings.hidden_details.contains(&d);
        div()
            .flex()
            .flex_col()
            .px(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .when(shown(Detail::Context), |d| {
                d.child(self.context(faces, s, cx))
            })
            .when(shown(Detail::Checkpoints), |d| {
                d.children(self.checkpoints_section(faces, s))
            })
            .when(shown(Detail::Linked), |d| {
                d.child(Self::linked_items(s, cx))
            })
            .when(shown(Detail::Plan), |d| d.child(Self::plan(faces, s)))
            .when(shown(Detail::Background), |d| {
                d.children(self.background(faces, s, cx))
            })
            .when(shown(Detail::Servers), |d| {
                d.children(self.servers(faces, s, cx))
            })
            .when(shown(Detail::Terminals), |d| {
                d.child(Self::commands(faces, s, cx))
            })
            .when(shown(Detail::Lineage), |d| {
                d.child(self.lineage(faces, s, cx))
            })
            .child(
                div()
                    .flex()
                    .pt(px(8.))
                    .px(px(8.))
                    .child(self.details_chooser(faces, cx)),
            )
    }

    /// `Checkpoints` section: `Turn N · <prompt>`, `<k> files` / `no change`, `<clock>`,
    /// plus the explanatory caption.
    fn checkpoints_section(&self, faces: &Faces, s: &Session) -> Option<ThreadDetailsSection> {
        if s.turns.is_empty() {
            return None;
        }
        let prompts: Vec<(usize, &str)> = s
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, it)| match it {
                Item::You(text) => Some((idx, text.lines().next().unwrap_or_default().trim())),
                _ => None,
            })
            .collect();
        // `.cp { padding: 0 8px 8px }`.
        let mut rows = div().flex().flex_col().px(px(8.)).pb(px(8.));
        for (i, turn) in s.turns.iter().enumerate().rev() {
            let count = turn.diff.as_ref().map_or(0, Vec::len);
            let files = if count == 0 {
                crate::tr("no change")
            } else {
                crate::tr_with("%{n} files", &[("n", &count)])
            };
            let turn_label = match prompts.get(i).filter(|(_, p)| !p.is_empty()) {
                Some((_, p)) => crate::tr_with(
                    "Turn %{n} \u{00b7} %{prompt}",
                    &[("n", &(i + 1)), ("prompt", p)],
                ),
                None => crate::tr_with("Turn %{n}", &[("n", &(i + 1))]),
            };
            let clock = prompts.get(i).and_then(|(item_ix, _)| {
                s.times
                    .iter()
                    .find(|(ix, _)| ix == item_ix)
                    .map(|(_, at)| when::clock(*at, self.settings.twelve))
            });
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(28.))
                    .type_style(TypeStyle::Label01)
                    .child(
                        ui::one_line(
                            ui::text(faces, turn_label)
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextPrimary)),
                        )
                        .flex_1()
                        .min_w(px(0.)),
                    )
                    .child(
                        ui::text(faces, files)
                            .flex_none()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary)),
                    )
                    .children(clock.map(|t| {
                        ui::text(faces, t)
                            .flex_none()
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                    })),
            );
        }
        rows = rows.child(
            ui::text(
                faces,
                crate::tr(
                    "Before and after each turn, out of your index and branch. Undo is under each turn in the thread.",
                ),
            )
            .type_style(TypeStyle::Label01)
            .text_color(theme::role(ColorRole::TextSecondary)),
        );
        Some(
            ThreadDetailsSection::new("det-checkpoints", crate::tr(Detail::Checkpoints.head()))
                .count(s.turns.len())
                .child(rows),
        )
    }

    /// Linked pull requests and issues (`u0ui::u0::panels::LinkedItemsSection`).
    fn linked_items(s: &Session, cx: &mut Context<Self>) -> LinkedItemsSection {
        let items: Vec<LinkedItem> = s
            .meta
            .links
            .iter()
            .map(|link| match link.kind {
                crate::github::Kind::Pr => {
                    let state = s
                        .git
                        .pr
                        .as_ref()
                        .filter(|p| p.number == link.number)
                        .map_or(PullRequestState::Open, |p| {
                            if p.draft {
                                PullRequestState::Draft
                            } else if p.state == "merged" {
                                PullRequestState::Merged
                            } else if p.state == "closed" {
                                PullRequestState::Closed
                            } else {
                                PullRequestState::Open
                            }
                        });
                    LinkedItem::pull_request(format!("#{}", link.number), state, link.title.clone())
                }
                crate::github::Kind::Issue => LinkedItem::issue(
                    format!("#{}", link.number),
                    IssueState::Open,
                    link.title.clone(),
                ),
            })
            .collect();
        let desk_link = cx.entity();
        let desk_unlink = cx.entity();
        let (key, links) = (s.key, s.meta.links.clone());
        let texts = LinkedItemsTexts {
            title: crate::tr(Detail::Linked.head()),
            link: crate::tr("Link"),
            unlink: Rc::new(|id| crate::tr_with("Unlink %{id}", &[("id", &id)])),
            empty: crate::tr("No pull requests or issues linked to this thread."),
        };
        LinkedItemsSection::new("det-linked", items)
            .texts(texts)
            .on_link(move |window, cx| {
                desk_link.update(cx, |d, cx| {
                    d.open_page(Page::Forge(crate::github::Kind::Pr), window, cx);
                });
            })
            .on_unlink(move |id, _, cx| {
                let n = id.trim_start_matches('#').parse::<u64>().ok();
                if let Some(gone) = n.and_then(|num| links.iter().find(|l| l.number == num)) {
                    let gone = gone.clone();
                    desk_unlink.update(cx, |d, cx| {
                        d.link(key, gone);
                        cx.notify();
                    });
                }
            })
    }

    /// Lineage section (`ThreadDetailsSection`), shown with no count while
    /// the thread has no kin.
    fn lineage(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> ThreadDetailsSection {
        let section = ThreadDetailsSection::new("det-lineage", crate::tr(Detail::Lineage.head()))
            .default_open(false);
        let id = s.meta.id.as_ref().map(|i| i.0.to_string());
        let mut kin: Vec<(SharedString, &Session)> = Vec::new();
        if let Some(o) = self.origin_of(s).and_then(|k| self.sessions.at(k)) {
            kin.push((crate::tr("Branched off"), o));
        }
        for o in &self.sessions.all {
            let word = if o.parent == Some(s.key) {
                crate::tr("Subagent")
            } else if o.beside == Some(s.key) {
                crate::tr("Side chat")
            } else if o.meta.agent == s.meta.agent && id.is_some() && o.meta.from == id {
                crate::tr("Branch")
            } else {
                continue;
            };
            kin.push((word, o));
        }
        if kin.is_empty() {
            return section.child(empty(
                faces,
                crate::tr("Branches, subagents and side chats of this thread."),
            ));
        }
        let count = kin.len();
        let mut list = div().flex().flex_col();
        for (word, o) in kin {
            let (key, side) = (o.key, o.beside == Some(s.key));
            let at = s.key;
            list = list.child(
                frow()
                    .id(SharedString::from(format!("kin-{key}")))
                    .cursor_pointer()
                    .child(ui::mark(faces, &self.mark_of(&o.meta.agent), 14.))
                    .child(ui::one_line(ui::text(faces, o.meta.title.clone())).flex_1())
                    .child(fine(faces, word))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if side {
                            this.columns.entry(at).or_default().open(Surface::Side(key));
                            cx.notify();
                        } else {
                            this.show(key, window, cx);
                        }
                    })),
            );
        }
        section.count(count).child(list)
    }

    /// Footer button opening `Popup::Details` (`"Choose what shows here"` + `Settings` icon),
    /// as Carbon's ghost `sm` button with an icon: 32 high, 16 in at each side.
    fn details_chooser(&self, faces: &Faces, cx: &mut Context<Self>) -> Popover {
        let link = theme::role(ColorRole::LinkPrimary);
        let chooser = ui::opener("details-rows")
            .h(px(32.))
            .px(px(16.))
            .gap(px(8.))
            .child(
                ui::text(faces, crate::tr("Choose what shows here"))
                    .type_style(TypeStyle::BodyCompact01)
                    .text_color(link),
            )
            .child(
                Icon::carbon(CarbonIcon::Settings)
                    .size_length(px(16.))
                    .color(link),
            );
        self.menu(
            crate::desk::Popup::Details,
            chooser,
            |d, menu, desk, _| d.details_menu(menu, desk),
            cx,
        )
        .anchor(gpui::Anchor::BottomLeft)
    }

    /// Which of the details show, each ticked or not, and all again.
    fn details_menu(&self, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        for d in Detail::ALL {
            let on = !self.settings.hidden_details.contains(&d);
            menu = menu.item(
                Self::menu_item(desk, crate::tr(d.head()), move |this, _, cx| {
                    let hidden = &mut this.settings.hidden_details;
                    if on {
                        hidden.push(d);
                    } else {
                        hidden.retain(|h| *h != d);
                    }
                    this.save_settings();
                    cx.notify();
                })
                .checked(on),
            );
        }
        menu.separator().item(Self::menu_item(
            desk,
            crate::tr("Show them all"),
            |this, _, cx| {
                this.settings.hidden_details.clear();
                this.save_settings();
                cx.notify();
            },
        ))
    }

    /// Background subagents (`ThreadDetailsSection`).
    fn background(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Option<ThreadDetailsSection> {
        let family = s.parent.unwrap_or(s.key);
        let kids: Vec<&Session> = self
            .sessions
            .all
            .iter()
            .filter(|c| c.parent == Some(family))
            .collect();
        if kids.is_empty() {
            return None;
        }
        let live = |c: &Session| c.sub.working() || c.sub.needs_you;
        let going = kids.iter().filter(|c| live(c)).count();
        let over = kids.len() - going;
        let total = kids.len();
        let fold = format!("background-{family}");
        let open = self.expanded.contains(&fold);
        let row_of = |id: SharedString| {
            frow()
                .id(id)
                .px(px(6.))
                .mx(px(-6.))
                .hover(|st| st.bg(theme::role(ColorRole::LayerHover01)))
        };
        let title = |text: &str| {
            ui::one_line(ui::text(faces, text.to_owned()).text_size(px(crate::style::LABEL)))
                .flex_1()
                .min_w(px(0.))
        };
        let stop =
            |id: SharedString| ui::icon_button(id, "square", crate::tr("Stop it")).size(px(20.));
        let sub_row = |c: &Session, cx: &mut Context<Self>| {
            let key = c.key;
            row_of(SharedString::from(format!("bg-sub-{key}")))
                .cursor_pointer()
                .when(key == s.key, |d| d.bg(theme::role(ColorRole::LayerHover01)))
                .child(crate::thread::subagent::mark(c))
                .child(title(&c.meta.title))
                .children(c.sub.time().map(|t| fine(faces, t)))
                .when(c.sub.working() && c.sub.stoppable, |d| {
                    d.child(
                        stop(SharedString::from(format!("bg-sub-stop-{key}"))).on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.stop_sub(key, cx);
                            }),
                        ),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.show(key, window, cx);
                }))
        };
        let mut list = div().flex().flex_col();
        for c in kids.iter().filter(|c| c.sub.needs_you) {
            list = list.child(sub_row(c, cx));
        }
        for c in kids.iter().filter(|c| c.sub.working() && !c.sub.needs_you) {
            list = list.child(sub_row(c, cx));
        }
        if over > 0 {
            let toggle = fold.clone();
            list = list.child(
                row_of(SharedString::from(format!("bg-over-{family}")))
                    .cursor_pointer()
                    .child(
                        Icon::carbon(if open {
                            CarbonIcon::ChevronDown
                        } else {
                            CarbonIcon::ChevronRight
                        })
                        .size_length(px(12.))
                        .color(theme::role(ColorRole::IconSecondary)),
                    )
                    .child(fine(
                        faces,
                        crate::tr_with("%{n} finished", &[("n", &over)]),
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(&toggle) {
                            this.expanded.insert(toggle.clone());
                        }
                        cx.notify();
                    })),
            );
            if open {
                for c in kids.iter().filter(|c| !live(c)) {
                    list = list.child(sub_row(c, cx));
                }
            }
        }
        Some(
            ThreadDetailsSection::new("det-background", crate::tr(Detail::Background.head()))
                .count(if going > 0 { going } else { total })
                .default_open(going > 0)
                .child(list),
        )
    }

    /// The branch where the session works; a click lists the others, to
    /// check one out.
    pub(super) fn branch_chip(
        &self,
        faces: &Faces,
        s: &Session,
        branch: &str,
        cx: &mut Context<Self>,
    ) -> Popover {
        use u0ui::u0::foundations::truncate::{Truncate, TruncateMode};
        let link = theme::role(ColorRole::LinkPrimary);
        let chip = ui::opener("branch-chip")
            .h(px(28.))
            .px(px(8.))
            .gap(px(6.))
            .child(
                div()
                    .max_w(px(140.))
                    .min_w(px(0.))
                    .font_family(faces.mono.clone())
                    .type_style(TypeStyle::Code01)
                    .text_color(link)
                    .child(
                        Truncate::new("branch-chip-name", branch.to_owned())
                            .mode(TruncateMode::Middle),
                    ),
            )
            .child(
                Icon::carbon(CarbonIcon::ChevronDown)
                    .size_length(px(14.))
                    .color(link),
            );
        let (faces, key, branch) = (faces.clone(), s.key, branch.to_owned());
        self.popover(
            crate::desk::Popup::Branch,
            chip,
            move |d, _, cx| d.branch_menu(&faces, key, &branch, cx),
            cx,
        )
        .anchor(gpui::Anchor::TopLeft)
    }

    /// The branches to check out, and a line for a new one.
    fn branch_menu(&self, faces: &Faces, key: u64, branch: &str, cx: &mut Context<Self>) -> Div {
        let mut menu = div().flex().flex_col().w(px(228.)).child(
            ui::text(faces, crate::tr("Check out"))
                .px(px(8.))
                .pt(px(6.))
                .pb(px(2.))
                .text_size(px(crate::style::CAPTION))
                .text_color(theme::role(ColorRole::TextSecondary)),
        );
        let branches = self.sessions.at(key).map(|s| s.git.branches.as_slice());
        for (n, b) in branches.unwrap_or_default().iter().enumerate() {
            let (on, name) = (b == branch, b.clone());
            let copied = name.clone();
            menu = menu.child(
                ListItem::new(SharedString::from(format!("branch-{n}")))
                    .selected(on)
                    .child(ui::one_line(
                        ui::mono(faces, b.clone()).text_size(px(crate::style::CAPTION)),
                    ))
                    .suffix(move |_, _| {
                        let copied = copied.clone();
                        ui::icon_button(
                            SharedString::from(format!("branch-copy-{n}")),
                            "copy",
                            crate::tr("Copy its name"),
                        )
                        .size(px(20.))
                        .text_color(theme::role(ColorRole::IconSecondary))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
                        })
                    })
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.switch_branch(name.clone(), cx)),
                    ),
            );
        }
        menu.child(
            div()
                .mt(px(4.))
                .mx(px(4.))
                .child(Input::new(&self.menu_search).xsmall()),
        )
    }

    fn plan(faces: &Faces, s: &Session) -> ThreadDetailsSection {
        let Some(plan) = s.plan.as_ref().filter(|p| !p.entries.is_empty()) else {
            return ThreadDetailsSection::new("det-plan", crate::tr(Detail::Plan.head()))
                .default_open(false)
                .child(empty(faces, crate::tr("Shown when the agent makes one.")));
        };
        let steps = thread::plan_steps(plan);
        ThreadDetailsSection::new("det-plan", crate::tr(Detail::Plan.head()))
            .count(steps.len())
            .default_open(false)
            .child(drawn::plan(faces, &steps))
    }

    /// Dev servers in `Details` (`ThreadDetailsSection`).
    fn servers(
        &self,
        faces: &Faces,
        s: &Session,
        cx: &mut Context<Self>,
    ) -> Option<ThreadDetailsSection> {
        let found: Vec<(u64, String)> = self
            .drawer
            .terminals
            .iter()
            .filter(|t| t.session == s.key && t.ended.is_none())
            .filter_map(|t| crate::desk::runs::address(&t.text()).map(|a| (t.id, a)))
            .collect();
        if found.is_empty() {
            return None;
        }
        let count = found.len();
        let host = s.host;
        let mut list = div().flex().flex_col();
        for (n, (id, address)) in found.into_iter().enumerate() {
            let open = address.clone();
            list = list.child(
                frow()
                    .child(ui::dot(theme::role(ColorRole::SupportSuccess)))
                    .child(ui::one_line(fine_mono(faces, address)).flex_1())
                    .child(
                        ui::button(
                            SharedString::from(format!("serve-open-{n}")),
                            crate::tr("Open"),
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let to = this.pages_to();
                            this.open_address(host, open.clone(), to, cx);
                        })),
                    )
                    .child(
                        ui::button(
                            SharedString::from(format!("serve-stop-{n}")),
                            crate::tr("Stop"),
                            Weight::Quiet,
                        )
                        .h(px(22.))
                        .on_click(cx.listener(move |this, _, _, cx| this.close_terminal(id, cx))),
                    ),
            );
        }
        Some(
            ThreadDetailsSection::new("det-servers", crate::tr(Detail::Servers.head()))
                .count(count)
                .default_open(false)
                .child(list),
        )
    }

    fn commands(faces: &Faces, s: &Session, cx: &mut Context<Self>) -> ThreadDetailsSection {
        let commands: Vec<_> = s.commands().collect();
        if commands.is_empty() {
            return ThreadDetailsSection::new("det-terminals", crate::tr(Detail::Terminals.head()))
                .default_open(false)
                .child(empty(faces, crate::tr("Commands the agent runs, live.")));
        }
        let running = commands
            .iter()
            .filter(|t| {
                matches!(
                    t.call.status,
                    ToolCallStatus::Pending | ToolCallStatus::InProgress
                )
            })
            .count();
        let mut list = div().flex().flex_col();
        for t in commands.iter().rev().take(6) {
            let live = matches!(
                t.call.status,
                ToolCallStatus::Pending | ToolCallStatus::InProgress
            );
            let failed = t.call.status == ToolCallStatus::Failed;
            let dot = if live {
                ui::live_dot().into_any_element()
            } else {
                ui::dot(if failed {
                    theme::role(ColorRole::SupportError)
                } else {
                    theme::role(ColorRole::SupportSuccess)
                })
                .into_any_element()
            };
            let time = if live {
                fine(faces, t.running().map(when::took).unwrap_or_default())
                    .text_color(theme::role(ColorRole::LinkPrimary))
            } else if failed {
                fine(faces, "failed").text_color(theme::role(ColorRole::SupportError))
            } else {
                fine(faces, t.took_said().map(when::took).unwrap_or_default())
            };
            let text = t.command_line();
            let group = SharedString::from(format!("ran-{}", t.call.tool_call_id.0));
            let again = text.clone();
            list = list.child(
                frow()
                    .id(group.clone())
                    .group(group.clone())
                    .child(dot)
                    .child(div().flex_1().min_w(px(0.)).child(thread::command(
                        faces,
                        &text,
                        crate::style::CAPTION,
                    )))
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .child(time.when(!live, |d| {
                                d.group_hover(group.clone(), gpui::Styled::invisible)
                            }))
                            .when(!live, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .right(px(-4.))
                                        .invisible()
                                        .group_hover(group, gpui::Styled::visible)
                                        .child(
                                            ui::icon_button(
                                                SharedString::from(format!(
                                                    "rerun-{}",
                                                    t.call.tool_call_id.0
                                                )),
                                                "play",
                                                crate::tr("Run it in a terminal of yours"),
                                            )
                                            .size(px(22.))
                                            .on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.open_terminal(Some(&again), window, cx);
                                                }),
                                            ),
                                        ),
                                )
                            }),
                    ),
            );
            if live
                && let Some(out) = t.call.content.iter().find_map(|c| match c {
                    ToolCallContent::Content(x) => Some(crate::session::text_of(&x.content)),
                    _ => None,
                })
            {
                let tail: Vec<&str> = out.lines().rev().take(3).collect();
                let tail: Vec<&str> = tail.into_iter().rev().collect();
                list = list.child(
                    thread::well(faces)
                        .px(px(10.))
                        .py(px(8.))
                        .text_size(px(crate::style::CAPTION))
                        .line_height(px(17.))
                        .child(tail.join("\n")),
                );
            }
        }
        ThreadDetailsSection::new("det-terminals", crate::tr(Detail::Terminals.head()))
            .count(commands.len())
            .default_open(running > 0)
            .child(list)
    }

    /// `Context` section: `ContextMeter` + `.dt` definition list (`Agent`, `Mode`,
    /// `Folder`, `Worktree`, `Began`, `Used`).
    fn context(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> ThreadDetailsSection {
        let agent_short = self.name_of(&s.meta.agent);
        let model_opt = Self::option_name(s, &SessionConfigOptionCategory::Model);
        let effort_opt = Self::option_name(s, &SessionConfigOptionCategory::ThoughtLevel);
        let meter = s.usage.as_ref().filter(|u| u.size > 0).map(|u| {
            let pct = meter_percent(u.used, u.size);
            let name = model_opt.as_deref().unwrap_or(&agent_short);
            let label = crate::tr_with(
                "%{model} \u{00b7} %{pct}% of its window",
                &[("model", &name), ("pct", &pct)],
            );
            ContextMeter::new(u.used, u.size)
                .label(label)
                .tokens_label(crate::tr("tokens"))
        });

        let mut agent_desc = agent_short.clone();
        if let Some(m) = &model_opt {
            agent_desc = format!("{agent_desc} \u{00b7} {m}");
        }
        if let Some(e) = &effort_opt {
            agent_desc = format!("{agent_desc} \u{00b7} {e}");
        }

        // `.dt { gap: 6px 12px; padding: 4px 8px 8px }`.
        let mut dl = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .child(dt_row(
                faces,
                crate::tr("Agent"),
                ui::text(faces, agent_desc).type_style(TypeStyle::Label01),
            ));

        if let Some(mode) = Desk::mode_name(s) {
            let said = crate::tr_with(
                "%{mode}, as %{name} lists it",
                &[("mode", &mode), ("name", &agent_short)],
            );
            dl = dl.child(dt_row(
                faces,
                crate::tr("Mode"),
                ui::text(faces, said).type_style(TypeStyle::Label01),
            ));
        }

        dl = dl.child(dt_row(
            faces,
            crate::tr("Folder"),
            ui::mono(faces, Desk::shown(&s.meta.folder)).type_style(TypeStyle::Code01),
        ));
        for folder in &s.meta.also {
            dl = dl.child(dt_row(
                faces,
                crate::tr("And in"),
                ui::mono(faces, Desk::shown(folder)).type_style(TypeStyle::Code01),
            ));
        }

        if let Some(wt) = &s.meta.worktree {
            let removable = !matches!(s.state, State::Working | State::Opening);
            let wt_for_click = wt.clone();
            let desk = cx.entity();
            let base = s
                .git
                .compare_to
                .as_deref()
                .or_else(|| s.git.pr.as_ref().map(|p| p.base.as_str()))
                .filter(|b| !b.is_empty());
            let wt_text = match base {
                Some(b) => crate::tr_with(
                    "%{branch} from %{base}",
                    &[("branch", &wt.branch), ("base", &b)],
                ),
                None => wt.branch.clone().into(),
            };
            dl = dl.child(dt_row(
                faces,
                crate::tr("Worktree"),
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(ui::mono(faces, wt_text).type_style(TypeStyle::Code01))
                    .when(removable, |d| {
                        d.child(
                            Button::new("remove-worktree", crate::tr("Remove"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    let wt = wt_for_click.clone();
                                    desk.update(cx, |this, cx| {
                                        this.git_action(
                                            crate::repo::GitAction::RemoveWorktree(wt),
                                            cx,
                                        );
                                    });
                                }),
                        )
                    }),
            ));
        }

        let twelve = self.settings.twelve;
        let now = std::time::SystemTime::now();
        let day = when::start_of_day(now);
        let began = if s.meta.begun >= day {
            crate::tr_with(
                "Today %{time}",
                &[("time", &when::clock(s.meta.begun, twelve))],
            )
        } else {
            when::said_at(s.meta.begun, now, twelve).into()
        };
        let begun_text = match self.server_chip(s.host) {
            Some(srv) => crate::tr_with(
                "%{when} \u{00b7} on %{server}",
                &[("when", &began), ("server", &srv)],
            ),
            None => began,
        };
        dl = dl.child(dt_row(
            faces,
            crate::tr("Began"),
            ui::text(faces, begun_text).type_style(TypeStyle::Label01),
        ));

        if let Some(used_rows) = self.used.get(&s.key) {
            let (mut in_tok, mut out_tok, mut cost) = (0u64, 0u64, None::<(f64, String)>);
            for u in used_rows {
                match &u.what {
                    Use::Tokens(t) => {
                        in_tok += t.usage.input_tokens;
                        out_tok += t.usage.output_tokens;
                    }
                    Use::Cost { amount, currency } => {
                        cost = Some((*amount, currency.clone()));
                    }
                    Use::Limit(_) | Use::Turn | Use::LimitPaused => {}
                }
            }
            if in_tok > 0 || out_tok > 0 {
                let (in_k, out_k) = ((in_tok + 500) / 1000, (out_tok + 500) / 1000);
                let said = match cost {
                    Some((amt, cur)) if cur == "USD" || cur == "$" => crate::tr_with(
                        "%{in_k}k tokens in \u{00b7} %{out_k}k out \u{00b7} %{cost}, as %{name} reports it",
                        &[
                            ("in_k", &in_k),
                            ("out_k", &out_k),
                            ("cost", &format!("${amt:.2}")),
                            ("name", &agent_short),
                        ],
                    ),
                    _ => crate::tr_with(
                        "%{in_k}k tokens in \u{00b7} %{out_k}k out, as %{name} reports it",
                        &[("in_k", &in_k), ("out_k", &out_k), ("name", &agent_short)],
                    ),
                };
                dl = dl.child(dt_row(
                    faces,
                    crate::tr("Used"),
                    ui::text(faces, said).type_style(TypeStyle::Label01),
                ));
            }
        }

        ThreadDetailsSection::new("det-context", crate::tr(Detail::Context.head()))
            .children(meter)
            .child(dl)
    }
}

/// A button that does a git action with the commit message typed.
fn git_button(
    id: &'static str,
    label: &'static str,
    weight: Weight,
    cx: &mut Context<Desk>,
    make: fn(String) -> crate::repo::GitAction,
) -> KitButton {
    ui::button(id, crate::tr(label), weight).on_click(cx.listener(move |this, _, _, cx| {
        let message = this.commit_message(cx);
        this.review.committing = false;
        this.git_action(make(message), cx);
    }))
}
