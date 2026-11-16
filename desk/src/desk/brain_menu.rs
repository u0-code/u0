//! The brain menu: one menu of the brains the deployment names, opened
//! from the composer, the foot of the sidebar or the environment down the
//! right, and choosing the brain the solver thinks with from it.

use gpui::{AnyElement, Context, Corner, SharedString, anchored, deferred, div, prelude::*, px};

use super::Desk;
use crate::{fonts::Faces, solver::Provider, theme, ui};

/// Whether the brain picker is open. Adding and editing a brain is done in
/// the sheet (`sheet.rs`), never inside the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Menu {
    Closed,
    Brains,
}

/// The control the brain menu opened from. It is one menu, anchored to
/// whichever of the three places to change the brain was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    /// The chip under the line you type into.
    Composer,
    /// The solver's row at the foot of the sidebar.
    Account,
    /// The Brain row of the environment, down the right.
    Aside,
}

impl Desk {
    /// Open the brain menu from one of the places it opens from, or close
    /// it if it is open there already.
    pub(crate) fn open_menu(&mut self, menu: Menu, at: At, cx: &mut Context<Self>) {
        let same = self.menu == menu && self.menu_at == at;
        self.menu = if same { Menu::Closed } else { menu };
        self.menu_at = at;
        if self.menu != Menu::Closed {
            self.providers = self.deployment.providers();
            self.brain = self.deployment.brain();
        }
        cx.notify();
    }

    /// Think with another brain from now on: written into the deployment,
    /// and the daemon restarted if the desk runs it.
    pub(crate) fn use_brain(&mut self, name: &str, cx: &mut Context<Self>) {
        self.menu = Menu::Closed;
        if let Err(e) = self.deployment.choose(name) {
            self.notice = Some(format!("{e:#}"));
            cx.notify();
            return;
        }
        self.brain = Some(name.to_owned());
        self.restart(cx);
    }

    pub(crate) fn current_provider(&self) -> Option<Provider> {
        let brain = self.brain.as_deref()?;
        self.providers.iter().find(|p| p.name == brain).cloned()
    }

    pub(crate) fn menu_open_at(&self, at: At) -> bool {
        self.menu != Menu::Closed && self.menu_at == at
    }

    /// The brain menu, if it is open from `at`, anchored by `corner` to
    /// where the caller puts it.
    pub(crate) fn menu_over(
        &self,
        at: At,
        corner: Corner,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.menu_open_at(at).then(|| {
            deferred(
                anchored()
                    .anchor(corner)
                    .snap_to_window_with_margin(px(8.))
                    .child(self.brain_menu(faces, cx)),
            )
            .into_any_element()
        })
    }

    /// The brain picker: one row per brain the deployment names, with its
    /// mark, its name and model, an amber dot when this machine cannot run
    /// it (hover it for why), and a check on the one in use. A row's pencil
    /// opens it in the sheet; adding a brain is done there too.
    fn brain_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let mut menu = div()
            .id("brain-menu")
            .occlude()
            .flex()
            .flex_col()
            .w(px(260.))
            .max_h(px(420.))
            .overflow_y_scroll()
            .p(px(4.))
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border2())
            .rounded(px(10.))
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menu = Menu::Closed;
                cx.notify();
            }));
        for p in &self.providers {
            let on = self.brain.as_deref() == Some(p.name.as_str());
            let group = SharedString::from(format!("brain-{}", p.name));
            let (name, edit) = (p.name.clone(), p.name.clone());
            let dot = p.blocker.clone().map(|why| {
                div()
                    .id(SharedString::from(format!("brain-why-{}", p.name)))
                    .flex_none()
                    .p(px(3.))
                    .child(ui::dot(theme::accent()))
                    .tooltip(ui::tip(why))
            });
            let pencil = div()
                .id(SharedString::from(format!("brain-edit-{}", p.name)))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(20.))
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::panel2()))
                .child(
                    ui::icon("pencil", 12.)
                        .text_color(gpui::transparent_black())
                        .group_hover(group.clone(), |s| s.text_color(theme::muted())),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.menu = Menu::Closed;
                    this.open_sheet(Some(edit.clone()), window, cx);
                }));
            menu = menu.child(
                div()
                    .id(group.clone())
                    .group(group)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(30.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(ui::mark(faces, p.mark(), 16.))
                    .child(
                        ui::text(faces, p.title())
                            .text_size(px(13.))
                            .flex_none()
                            .max_w(px(110.))
                            .overflow_hidden()
                            .text_ellipsis(),
                    )
                    .child(ui::one_line(
                        ui::mono(faces, p.model.clone())
                            .text_size(px(11.))
                            .text_color(theme::faint()),
                    ))
                    .children(dot)
                    .child(pencil)
                    .child(div().flex_none().w(px(14.)).when(on, |d| {
                        d.child(ui::icon("check", 13.).text_color(theme::accent()))
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.use_brain(&name, cx))),
            );
        }
        if !self.providers.is_empty() {
            menu = menu.child(div().h(px(1.)).mx(px(4.)).my(px(4.)).bg(theme::border()));
        }
        menu.child(
            div()
                .id("add-brain")
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(30.))
                .px(px(8.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(ui::icon("plus", 16.).text_color(theme::muted()))
                .child(
                    ui::text(faces, "Add brain…")
                        .text_size(px(13.))
                        .text_color(theme::text2()),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu = Menu::Closed;
                    this.open_sheet(None, window, cx);
                })),
        )
    }
}
