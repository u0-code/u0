//! The settings that are Ghost's own, a tab each: how it tells you things
//! and what it does while you are away; how the thread reads; what you
//! write while a turn goes on; how new sessions begin; the keys; and what
//! Ghost is. Each row says what happens, and its control says the choice.

use gpui::{Context, Div, IntoElement, div, prelude::*, px};

use super::{card, fine};
use crate::{
    desk::Desk,
    fonts::{Faces, Weighted as _},
    keys,
    settings::{Notify, Width},
    theme,
    ui::{self, Pick, Weight},
};

impl Desk {
    /// The page of the tab `tab`, when it is one of Ghost's own.
    pub(super) fn own_tab(
        &self,
        faces: &Faces,
        tab: super::Tab,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        use super::Tab;
        Some(match tab {
            Tab::General => page(faces, "General", "How Ghost tells you things, and what it does while you are away.")
                .child(group(faces, "While you are away", vec![
                    row(faces, "When a session ends a turn or needs you", "A notification from macOS, only while Ghost is not in front.",
                        Self::pick(faces, "notify", [
                            ("Off", Notify::Off), ("Notify", Notify::Banner), ("Notify and sound", Notify::BannerAndSound),
                        ], self.settings.notify, |s, v| s.notify = v, cx)),
                    row(faces, "Keep the Mac awake while an agent works", "The display may sleep; a turn going on is not cut short by the Mac sleeping.",
                        ui::switch("awake", self.settings.awake).on_click(cx.listener(|this, _, _, cx| {
                            this.settings.awake = !this.settings.awake;
                            this.save_settings();
                            this.stay_awake();
                            cx.notify();
                        }))),
                ]))
                .child(group(faces, "Files", vec![
                    row(faces, "Files open in", "A file the agent named, an edit, or keybindings.json.",
                        self.editor_pick(faces, cx)),
                ]))
                .child(group(faces, "Done for now", vec![
                    row(faces, "A session is done after it is left for", "It moves to Done in the sidebar; anything said to it brings it back.",
                        Self::pick(faces, "settle", [
                            ("3 days", Some(3)), ("7 days", Some(7)), ("14 days", Some(14)), ("30 days", Some(30)), ("Never", None),
                        ], self.settings.settle_days, |s, v| s.settle_days = v, cx)),
                ])),
            Tab::Appearance => page(faces, "Appearance", "How the thread and what is beside it read.")
                .child(group(faces, "Reading", vec![
                    row(faces, "Reading width", "How wide the thread and the composer are.",
                        Self::pick(faces, "width", [
                            ("Comfortable", Width::Comfortable), ("Wide", Width::Wide), ("Full", Width::Full),
                        ], self.settings.width, |s, v| s.width = v, cx)),
                    row(faces, "A diff's long lines", "Wrapped, they show whole; kept to a line, they are cut at the edge.",
                        Self::pick(faces, "diff-wrap", [("Wrap", true), ("Cut", false)],
                            self.settings.diff_wrap, |s, v| s.diff_wrap = v, cx)),
                    row(faces, "Changes only in spacing", "Left out, a diff shows what changed in the words alone.",
                        Self::pick(faces, "spacing", [("Shown", false), ("Left out", true)],
                            self.settings.no_spacing, |s, v| s.no_spacing = v, cx)),
                    row(faces, "A file's diff opens", "Side by side widens the column beside the thread.",
                        Self::pick(faces, "diff", [("Stacked", false), ("Side by side", true)],
                            self.settings.side_by_side, |s, v| s.side_by_side = v, cx)),
                ])),
            Tab::Chat => page(faces, "Chat", "What you write, and what the agent did.")
                .child(group(faces, "While a turn goes on", vec![
                    row(faces, "What you send waits, or steers", "Steer stops the turn and says it now; waiting says it once the turn ends.",
                        Self::pick(faces, "steer", [("Wait its turn", false), ("Steer", true)],
                            self.settings.steer, |s, v| s.steer = v, cx)),
                ]))
                .child(group(faces, "Sending", vec![
                    row(faces, "What sends what you write", "With ⌘Return, Return breaks the line and ⇧⌘Return sends and begins another beside it.",
                        Self::pick(faces, "send", [("Return", false), ("⌘Return", true)],
                            self.settings.cmd_sends, |s, v| s.cmd_sends = v, cx)),
                ]))
                .child(group(faces, "The agent's work", vec![
                    row(faces, "Its calls and thinking in the thread", "Folded keeps a finished turn's work in one line, a click away; hidden shows only what was said.",
                        Self::pick(faces, "calls", [
                            ("Shown", crate::settings::Calls::Shown), ("Folded", crate::settings::Calls::Folded), ("Hidden", crate::settings::Calls::Hidden),
                        ], self.settings.calls, |s, v| s.calls = v, cx)),
                ])),
            Tab::NewSessions => page(faces, "New sessions", "How a session begins in a repository.")
                .child(group(faces, "In a repository", vec![
                    row(faces, "A new session works", "A project can choose otherwise from its menu.",
                        Self::pick(faces, "where", [("In the folder", false), ("In a new worktree", true)],
                            self.settings.worktree, |s, v| s.worktree = v, cx)),
                    row(faces, "Worktree branches begin with", "Then the session's title, as in ghost/retry-s3-uploads.",
                        div()
                            .w(px(160.))
                            .h(px(30.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .bg(theme::well())
                            .border_1()
                            .border_color(theme::border2())
                            .font_family(faces.mono.clone())
                            .text_size(px(12.5))
                            .child(self.prefix_input.clone())),
                ])),
            Tab::Keyboard => self.keyboard(faces, cx),
            Tab::About => self.about_ghost(faces, cx),
            Tab::Agents | Tab::Sessions => return None,
        })
    }

    /// A choice among `choices`, the one equal to `now` lit; picking one
    /// sets it with `set` and keeps the settings.
    fn pick<T: Copy + PartialEq + 'static, const N: usize>(
        faces: &Faces,
        id: &str,
        choices: [(&'static str, T); N],
        now: T,
        set: fn(&mut crate::settings::Settings, T),
        cx: &mut Context<Self>,
    ) -> Div {
        let choices = choices
            .into_iter()
            .map(|(label, value)| {
                let pick: Pick = Box::new(cx.listener(move |this, _, _, cx| {
                    set(&mut this.settings, value);
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                }));
                (label, value == now, pick)
            })
            .collect();
        ui::segmented(faces, id, choices)
    }

    /// The editors here to open files in, and macOS's own choice.
    fn editor_pick(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut choices: Vec<(&'static str, bool, Pick)> = vec![(
            "First found",
            self.settings.editor.is_none(),
            Box::new(cx.listener(|this, _, _, cx| {
                this.settings.editor = None;
                this.save_settings();
                this.settings_took(cx);
                cx.notify();
            })),
        )];
        for (command, name) in crate::desk::EDITORS {
            if crate::registry::which(command).is_some() {
                choices.push((
                    name,
                    self.settings.editor.as_deref() == Some(command),
                    Box::new(cx.listener(move |this, _, _, cx| {
                        this.settings.editor = Some(command.to_owned());
                        this.save_settings();
                        this.settings_took(cx);
                        cx.notify();
                    })),
                ));
            }
        }
        ui::segmented(faces, "editor", choices)
    }

    /// The keys as they are now, yours over the window's, and the file
    /// yours are kept in.
    fn keyboard(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut list = card().overflow_hidden();
        for (n, (name, does, _)) in keys::ACTIONS.into_iter().enumerate() {
            let mut keys = div().flex().gap(px(4.));
            for k in keys::keys_of(cx, name)
                .unwrap_or_default()
                .split(' ')
                .filter(|k| !k.is_empty())
            {
                keys = keys.child(
                    div()
                        .px(px(6.))
                        .rounded(px(5.))
                        .border_1()
                        .border_color(theme::tag_line())
                        .bg(theme::panel())
                        .font_family(faces.mono.clone())
                        .text_size(px(11.5))
                        .line_height(px(20.))
                        .text_color(theme::text2())
                        .child(k.to_owned()),
                );
            }
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.))
                    .px(px(16.))
                    .when(n > 0, |d| d.border_t_1().border_color(theme::border()))
                    .child(ui::text(faces, does).flex_1().text_size(px(13.)))
                    .child(keys),
            );
        }
        let path = self.home.join("keybindings.json");
        let open = ui::button(
            faces,
            "open-keys",
            "Open keybindings.json",
            Weight::Secondary,
        )
        .on_click(move |_, _, _| {
            if !path.exists() {
                let _ = crate::whole::write(&path, b"[]\n");
            }
            crate::desk::open_in_editor(&path);
        });
        page(faces, "Keyboard", "The keys as they are now, yours over Ghost's.")
            .child(list)
            .child(fine(
                faces,
                "A rule is { \"key\": \"cmd-shift-n\", \"action\": \"new_session\" }. Ghost reads the file as it starts.",
            ))
            .child(div().flex().child(open))
    }

    /// What Ghost is, and where it keeps what it keeps.
    fn about_ghost(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let home = self.home.clone();
        let show =
            ui::button(faces, "show-home", "Show", Weight::Quiet).on_click(move |_, _, _| {
                let _ = std::process::Command::new("open").arg(&home).spawn();
            });
        let registry = ui::button(faces, "open-registry", "Open", Weight::Quiet).on_click(
            cx.listener(|_, _, _, cx| cx.open_url("https://agentclientprotocol.com/registry")),
        );
        page(
            faces,
            &format!("Ghost {}", env!("CARGO_PKG_VERSION")),
            "Agent Client Protocol v1 · macOS",
        )
        .child(
            card()
                .overflow_hidden()
                .child(row(
                    faces,
                    "Where Ghost keeps its files",
                    &Desk::shown(&self.home),
                    show,
                ))
                .child(row(
                    faces,
                    "The agents come from",
                    "The ACP registry, read again every six hours.",
                    registry,
                )),
        )
    }
}

/// A tab's page: its title and what it is for, then its groups.
fn page(faces: &Faces, title: &str, lede: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(22.))
        .px(px(40.))
        .py(px(28.))
        .max_w(px(760.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    ui::text(faces, title.to_owned())
                        .text_size(px(20.))
                        .line_height(px(28.))
                        .weight(gpui::FontWeight::SEMIBOLD),
                )
                .child(
                    ui::text(faces, lede.to_owned())
                        .text_size(px(13.5))
                        .text_color(theme::text2()),
                ),
        )
}

/// Rows under a caption, in one frame.
fn group(faces: &Faces, caption: &str, rows: Vec<Div>) -> Div {
    let mut frame = card().overflow_hidden();
    for (n, r) in rows.into_iter().enumerate() {
        frame = frame.child(r.when(n > 0, |d| d.border_t_1().border_color(theme::border())));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(ui::label(faces, caption))
        .child(frame)
}

/// A setting: what happens, said plainly, and its control.
fn row(faces: &Faces, name: &str, about: &str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(16.))
        .py(px(12.))
        .min_h(px(56.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(2.))
                .child(ui::text(faces, name.to_owned()).text_size(px(13.5)))
                .child(
                    ui::text(faces, about.to_owned())
                        .text_size(px(12.5))
                        .line_height(px(18.))
                        .text_color(theme::faint()),
                ),
        )
        .child(control)
}
