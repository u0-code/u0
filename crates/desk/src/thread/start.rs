//! A session not begun, or none on screen: no greeting, as Claude has
//! none, only a card of the next steps that apply. A folder to work in
//! when there is no project; an agent to sign in to, or one to add; a new
//! session where none is open; the sessions that wait on you, and the
//! three last touched to pick up.

use gpui::component::list::ListItem;
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use crate::fonts::Weighted as _;
use crate::{
    desk::Desk,
    fonts::Faces,
    hub::Auth,
    session::{Session, State},
    theme, ui,
};

/// How many sessions to pick up are offered.
const RECENT: usize = 3;

impl Desk {
    /// The next steps, as one bordered card in the middle of the page, at
    /// most `width` wide; `fresh`: for a session on screen not begun yet.
    pub(crate) fn start(
        &self,
        faces: &Faces,
        width: f32,
        fresh: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let row =
            |id: SharedString, icon: &'static str, says: String, aside: Option<&'static str>| {
                ListItem::new(id)
                    .h(px(34.))
                    .px(px(12.))
                    .rounded(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(ui::icon(icon, 16.).text_color(theme::muted()))
                            .child(ui::one_line(ui::text(faces, says).text_size(px(14.))).w_auto())
                            .children(aside.map(|a| {
                                ui::text(faces, a)
                                    .flex_none()
                                    .text_size(px(14.))
                                    .text_color(theme::faint())
                            })),
                    )
            };
        let mut steps: Vec<ListItem> = Vec::new();
        // Settings that did not read, as Ghost started: read again once
        // the file is mended or moved away.
        if self.settings_refused.is_some() {
            steps.push(
                row(
                    "step-settings".into(),
                    "alert",
                    "Your settings did not read: read them again".into(),
                    None,
                )
                .on_click(cx.listener(|this, _, _, cx| this.read_settings_again(cx))),
            );
        }
        if self.projects().is_empty() {
            steps.push(
                row(
                    "step-folder".into(),
                    "folder",
                    "Choose a folder to work in".into(),
                    None,
                )
                .on_click(cx.listener(|_, _, window, cx| Self::add_project(window, cx))),
            );
        } else if !fresh {
            let named = self
                .chosen_project()
                .map(|p| self.project_name(&p))
                .unwrap_or_default();
            steps.push(
                row(
                    "step-new".into(),
                    "plus",
                    format!("New session in {named}"),
                    None,
                )
                .on_click(cx.listener(|this, _, window, cx| this.new_default(window, cx))),
            );
        }
        for slot in self
            .slots
            .iter()
            .filter(|x| matches!(x.auth, Auth::Needed(_)))
        {
            let id = slot.entry.id.clone();
            steps.push(
                row(
                    SharedString::from(format!("step-sign-in-{id}")),
                    "user",
                    format!("Sign in to {}", self.name_of(&id)),
                    None,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.sign_in_agent(&id, cx))),
            );
        }
        if self.slots.is_empty() {
            steps.push(
                row(
                    "step-agent".into(),
                    "plug",
                    "Add an agent from the ACP registry".into(),
                    None,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_tab(crate::pages::Tab::Agents, window, cx);
                })),
            );
        }
        for s in self.to_pick_up() {
            let key = s.key;
            steps.push(
                row(
                    SharedString::from(format!("step-pick-{key}")),
                    "reply",
                    format!("Pick up {}", ui::fit(&s.meta.title, 40)),
                    asks(s),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.show(key, window, cx))),
            );
        }
        // A session not begun asks, as t3code's does, what to build, in a
        // project picked from its name.
        let headline = self.session().filter(|_| fresh).map(|s| {
            let name = self.project_name(&s.meta.project);
            div()
                .flex()
                .flex_wrap()
                .items_baseline()
                .justify_center()
                .child(ui::text(faces, "What should we build in").text_size(px(26.)))
                .child(
                    div()
                        .id("start-project")
                        .ml(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .child(
                            ui::text(faces, name)
                                .text_size(px(26.))
                                .weight(gpui::FontWeight::MEDIUM),
                        )
                        .child(ui::icon("chevron-down", 14.).text_color(theme::faint()))
                        .on_click(cx.listener(|this, _, window, cx| {
                            if !this.palette.open {
                                this.toggle_palette(window, cx);
                            }
                            this.new_in_page(window, cx);
                        })),
                )
                .child(ui::text(faces, "?").text_size(px(26.)))
        });
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(28.))
            .mt(px(if headline.is_some() { 120. } else { 160. }))
            .children(headline)
            .when(!steps.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(width.min(420.)))
                        .p(px(4.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme::border2())
                        .children(steps),
                )
            })
    }

    /// The sessions that wait on you, then the others most lately touched.
    fn to_pick_up(&self) -> Vec<&Session> {
        let shown = self.sessions.all.iter().filter(|s| {
            self.listed_now(s)
                && Some(s.key) != self.current
                && s.meta.flags.settled.is_none()
                && !(s.state == State::Draft && s.items.is_empty())
        });
        let (mut needs, mut rest): (Vec<&Session>, Vec<&Session>) =
            shown.partition(|s| asks(s).is_some());
        rest.sort_by_key(|s| std::cmp::Reverse(s.meta.updated));
        rest.truncate(RECENT);
        needs.extend(rest);
        needs
    }
}

/// What a session waits on you for, if it does.
fn asks(s: &Session) -> Option<&'static str> {
    if s.asks() {
        Some("asks you")
    } else {
        match s.state {
            State::SignIn => Some("needs a sign-in"),
            State::Broken(_) => Some("stopped"),
            _ => None,
        }
    }
}
