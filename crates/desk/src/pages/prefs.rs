//! The settings that are Ghost's own, a tab each: how it tells you things
//! and what it does while you are away; how the thread reads; what you
//! write while a turn goes on; how new sessions begin; the keys; and what
//! Ghost is. Each row says what happens, and its control says the choice.

use gpui::component::button::ButtonGroup;
use gpui::component::{Sizable as _, input::Input, kbd::Kbd, switch::Switch};
use gpui::{Context, Div, IntoElement, SharedString, div, prelude::*, px};

use super::card;
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
        q: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        use super::Tab;
        let (lede, groups) = match tab {
            Tab::General => (
                "How Ghost tells you things, and what it does while you are away.",
                vec![
                    group(
                        faces,
                        "While you are away",
                        vec![
                            row(
                                faces,
                                q,
                                "When a session ends a turn or needs you",
                                "A notification from macOS, only while Ghost is not in front.",
                                Self::pick(
                                    "notify",
                                    [
                                        ("Off", Notify::Off),
                                        ("Notify", Notify::Banner),
                                        ("Notify and sound", Notify::BannerAndSound),
                                    ],
                                    self.settings.notify,
                                    |s, v| s.notify = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Keep the Mac awake while an agent works",
                                "The display may sleep; a turn going on is not cut short by the Mac sleeping.",
                                Switch::new("awake")
                                    .small()
                                    .checked(self.settings.awake)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.awake = !this.settings.awake;
                                        this.save_settings();
                                        this.stay_awake();
                                        cx.notify();
                                    })),
                            ),
                        ],
                    ),
                    group(
                        faces,
                        "Files",
                        vec![row(
                            faces,
                            q,
                            "Files open in",
                            "A file the agent named, an edit, or keybindings.json.",
                            self.editor_pick(cx),
                        )],
                    ),
                    group(
                        faces,
                        "Done for now",
                        vec![row(
                            faces,
                            q,
                            "A session is done after it is left for",
                            "It moves to Done in the sidebar; anything said to it brings it back.",
                            Self::pick(
                                "settle",
                                [
                                    ("3 days", Some(3)),
                                    ("7 days", Some(7)),
                                    ("14 days", Some(14)),
                                    ("30 days", Some(30)),
                                    ("Never", None),
                                ],
                                self.settings.settle_days,
                                |s, v| s.settle_days = v,
                                cx,
                            ),
                        )],
                    ),
                ],
            ),
            Tab::Appearance => (
                "How the thread and what is beside it read.",
                vec![group(
                    faces,
                    "Reading",
                    vec![
                        row(
                            faces,
                            q,
                            "Look",
                            "Light and dark keep the same amber for what is going on now.",
                            Self::pick(
                                "look",
                                [
                                    ("Dark", crate::settings::Look::Dark),
                                    ("Light", crate::settings::Look::Light),
                                    ("As the system", crate::settings::Look::System),
                                ],
                                self.settings.look,
                                |s, v| s.look = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "Contrast",
                            "More draws the greys of what matters less, and the edges, stronger.",
                            Self::pick(
                                "contrast",
                                [("As designed", false), ("More", true)],
                                self.settings.contrast,
                                |s, v| s.contrast = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "Code font",
                            "What code, commands and numbers are set in.",
                            Self::pick(
                                "mono",
                                [
                                    ("JetBrains Mono", crate::settings::CodeFont::JetBrains),
                                    ("SF Mono", crate::settings::CodeFont::SfMono),
                                    ("Menlo", crate::settings::CodeFont::Menlo),
                                ],
                                self.settings.mono,
                                |s, v| s.mono = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "Reading width",
                            "How wide the thread and the composer are.",
                            Self::pick(
                                "width",
                                [
                                    ("Comfortable", Width::Comfortable),
                                    ("Wide", Width::Wide),
                                    ("Full", Width::Full),
                                ],
                                self.settings.width,
                                |s, v| s.width = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "What a change added and took away",
                            "Blue and orange tell them apart without green and red.",
                            Self::pick(
                                "diff-colours",
                                [("Green and red", false), ("Blue and orange", true)],
                                self.settings.blue_orange,
                                |s, v| s.blue_orange = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "Times",
                            "When a message was said, and what Ghost says of times.",
                            Self::pick(
                                "clock",
                                [("24-hour", false), ("12-hour", true)],
                                self.settings.twelve,
                                |s, v| s.twelve = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "A diff's long lines",
                            "Wrapped, they show whole; kept to a line, they are cut at the edge.",
                            Self::pick(
                                "diff-wrap",
                                [("Wrap", true), ("Cut", false)],
                                self.settings.diff_wrap,
                                |s, v| s.diff_wrap = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "Changes only in spacing",
                            "Left out, a diff shows what changed in the words alone.",
                            Self::pick(
                                "spacing",
                                [("Shown", false), ("Left out", true)],
                                self.settings.no_spacing,
                                |s, v| s.no_spacing = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "A file's diff opens",
                            "Side by side widens the column beside the thread.",
                            Self::pick(
                                "diff",
                                [("Stacked", false), ("Side by side", true)],
                                self.settings.side_by_side,
                                |s, v| s.side_by_side = v,
                                cx,
                            ),
                        ),
                    ],
                )],
            ),
            Tab::Chat => (
                "What you write, and what the agent did.",
                vec![
                    group(
                        faces,
                        "While a turn goes on",
                        vec![row(
                            faces,
                            q,
                            "What you send waits, or steers",
                            "Steer stops the turn and says it now; waiting says it once the turn ends.",
                            Self::pick(
                                "steer",
                                [("Wait its turn", false), ("Steer", true)],
                                self.settings.steer,
                                |s, v| s.steer = v,
                                cx,
                            ),
                        )],
                    ),
                    group(
                        faces,
                        "Sending",
                        vec![row(
                            faces,
                            q,
                            "What sends what you write",
                            "With ⌘Return, Return breaks the line and ⇧⌘Return sends and begins another beside it.",
                            Self::pick(
                                "send",
                                [("Return", false), ("⌘Return", true)],
                                self.settings.cmd_sends,
                                |s, v| s.cmd_sends = v,
                                cx,
                            ),
                        )],
                    ),
                    group(
                        faces,
                        "The agent's work",
                        vec![row(
                            faces,
                            q,
                            "Transcript view (⌃O)",
                            "Normal folds each run of calls into a line; thinking shows its reasoning too; verbose shows every call and step.",
                            Self::pick(
                                "view",
                                [
                                    ("Normal", crate::settings::View::Normal),
                                    ("Thinking", crate::settings::View::Thinking),
                                    ("Verbose", crate::settings::View::Verbose),
                                ],
                                self.settings.view,
                                |s, v| s.view = v,
                                cx,
                            ),
                        )],
                    ),
                ],
            ),
            Tab::NewSessions => (
                "How a session begins in a repository.",
                vec![group(
                    faces,
                    "In a repository",
                    vec![
                        row(
                            faces,
                            q,
                            "A new session works",
                            "A project can choose otherwise from its menu.",
                            Self::pick(
                                "where",
                                [("In the folder", false), ("In a new worktree", true)],
                                self.settings.worktree,
                                |s, v| s.worktree = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "Worktree branches begin with",
                            "Then the session's title, as in ghost/retry-s3-uploads.",
                            div()
                                .w(px(160.))
                                .font_family(faces.mono.clone())
                                .child(Input::new(&self.pages.prefix).small()),
                        ),
                    ],
                )],
            ),
            Tab::Keyboard => return self.keyboard(faces, q, cx),
            Tab::Usage => return q.is_empty().then(|| self.usage(faces, cx)),
            Tab::About => return self.about_ghost(faces, q, cx),
            Tab::Agents | Tab::Sessions | Tab::Projects => return None,
        };
        let groups: Vec<Div> = groups.into_iter().flatten().collect();
        (!groups.is_empty()).then(|| page(faces, tab.title(), lede).children(groups))
    }

    /// A choice among `choices`, the one equal to `now` lit; picking one
    /// sets it with `set` and keeps the settings.
    fn pick<T: Copy + PartialEq + 'static, const N: usize>(
        id: &str,
        choices: [(&'static str, T); N],
        now: T,
        set: fn(&mut crate::settings::Settings, T),
        cx: &mut Context<Self>,
    ) -> ButtonGroup {
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
        ui::segmented(id, choices)
    }

    /// The editors here to open files in, and macOS's own choice.
    fn editor_pick(&self, cx: &mut Context<Self>) -> ButtonGroup {
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
            if self.on_path.contains(&command) {
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
        ui::segmented("editor", choices)
    }

    /// The keys as they are now, yours over the window's, and the file
    /// yours are kept in.
    fn keyboard(&self, faces: &Faces, q: &str, cx: &mut Context<Self>) -> Option<Div> {
        let mut list = card().overflow_hidden();
        let mut n = 0;
        for (name, does, _) in keys::ACTIONS {
            let bound = keys::keys_of(cx, name);
            if !said(q, &[does, &keys::written(&bound)]) {
                continue;
            }
            n += 1;
            let keys = div()
                .flex()
                .gap(px(4.))
                .children(bound.into_iter().map(Kbd::new));
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.))
                    .px(px(16.))
                    .when(n > 1, |d| d.border_t_1().border_color(theme::border()))
                    .child(ui::text(faces, does).flex_1().text_size(px(13.)))
                    // A click records new keys for it; the arrow gives it
                    // back Ghost's.
                    .child(
                        div()
                            .id(SharedString::from(format!("record-{name}")))
                            .cursor_pointer()
                            .rounded(px(6.))
                            .px(px(4.))
                            .hover(|st| st.bg(theme::hover()))
                            .child(if self.pages.recording == Some(name) {
                                ui::text(faces, "Press the keys…  Escape to keep these")
                                    .text_size(px(12.))
                                    .text_color(theme::link())
                                    .into_any_element()
                            } else {
                                keys.into_any_element()
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pages.recording = Some(name);
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("unbind-{name}")),
                            "retry",
                            "Back to Ghost's keys",
                        )
                        .size(px(22.))
                        .text_color(theme::faint())
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.set_keys(name, None);
                        })),
                    ),
            );
        }
        let path = self.home.join("keybindings.json");
        let open = ui::button("open-keys", "Open keybindings.json", Weight::Secondary).on_click(
            cx.listener(move |this, _, _, _| {
                this.edit(path.clone(), Some(b"[]\n"));
            }),
        );
        if n == 0 {
            return None;
        }
        let page = page(
            faces,
            super::Tab::Keyboard.title(),
            "The keys as they are now, yours over Ghost's.",
        )
        .child(list);
        if !q.is_empty() {
            return Some(page);
        }
        Some(page
            .child(ui::fine(
                faces,
                "A rule is { \"key\": \"cmd-shift-n\", \"action\": \"new_session\" }. Ghost reads the file as it starts.",
            ))
            .child(div().flex().child(open)))
    }

    /// What the sessions spent, as their agents said it (the context each
    /// holds, and its cost where told), and the agents running: their
    /// process and the memory it holds, each can be let go.
    fn usage(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let mut spent = card().overflow_hidden();
        let mut totals: Vec<(String, f64)> = Vec::new();
        let mut any = false;
        for (n, s) in self
            .sessions
            .all
            .iter()
            .filter(|s| s.usage.is_some() && s.parent.is_none())
            .enumerate()
        {
            let u = s.usage.as_ref().expect("filtered");
            any = true;
            let share = if u.size > 0 {
                u.used as f64 / u.size as f64 * 100.
            } else {
                0.
            };
            let cost = u.cost.as_ref().map(|c| {
                match totals.iter_mut().find(|(cur, _)| *cur == c.currency) {
                    Some((_, t)) => *t += c.amount,
                    None => totals.push((c.currency.clone(), c.amount)),
                }
                format!("{:.2} {}", c.amount, c.currency)
            });
            spent = spent.child(
                setting(
                    faces,
                    &ui::fit(&s.meta.title, 48),
                    &format!(
                        "{} · {}k of {}k in context ({share:.0}%)",
                        self.name_of(&s.meta.agent),
                        u.used / 1000,
                        u.size / 1000
                    ),
                    ui::text(faces, cost.unwrap_or_default())
                        .text_size(px(12.5))
                        .text_color(theme::text2()),
                )
                .when(n > 0, |d| d.border_t_1().border_color(theme::border())),
            );
        }
        if !any {
            spent = spent.child(div().px(px(16.)).py(px(14.)).child(ui::fine(
                faces,
                "No agent has said what a session spent yet.",
            )));
        }
        let total = totals
            .iter()
            .map(|(cur, t)| format!("{t:.2} {cur}"))
            .collect::<Vec<_>>()
            .join(" · ");
        let mut running = card().overflow_hidden();
        for (n, (agent, pid, kb)) in self.weights.iter().enumerate() {
            let agent_id = agent.clone();
            running = running.child(
                setting(
                    faces,
                    &self.name_of(agent),
                    &format!("process {pid} · {} MB", kb / 1024),
                    ui::button(
                        SharedString::from(format!("let-go-{n}")),
                        "Let it go",
                        Weight::Quiet,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.let_go(&agent_id);
                        this.weigh_agents();
                        cx.notify();
                    })),
                )
                .when(n > 0, |d| d.border_t_1().border_color(theme::border())),
            );
        }
        if self.weights.is_empty() {
            running = running.child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .child(ui::fine(faces, "No agent is running.")),
            );
        }
        page(
            faces,
            super::Tab::Usage.title(),
            "What the sessions hold and spent, as their agents say, and what runs.",
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .child(ui::label(faces, "Sessions").flex_1())
                        .when(!total.is_empty(), |d| {
                            d.child(ui::fine(faces, format!("in all {total}")))
                        }),
                )
                .child(spent),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(ui::label(faces, "Agents running"))
                .child(running),
        )
    }

    /// What Ghost is, and where it keeps what it keeps.
    fn about_ghost(&self, faces: &Faces, q: &str, cx: &mut Context<Self>) -> Option<Div> {
        let home = self.home.clone();
        let show = ui::button("show-home", "Show", Weight::Quiet)
            .on_click(move |_, _, cx| cx.open_with_system(&home));
        let registry = ui::button("open-registry", "Open", Weight::Quiet).on_click(
            cx.listener(|_, _, _, cx| cx.open_url("https://agentclientprotocol.com/registry")),
        );
        let rows: Vec<Div> = [
            row(
                faces,
                q,
                "Where Ghost keeps its files",
                &Desk::shown(&self.home),
                show,
            ),
            row(
                faces,
                q,
                "The agents come from",
                "The ACP registry, read again every six hours.",
                registry,
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!rows.is_empty()).then(|| {
            page(
                faces,
                &format!("Ghost {}", env!("CARGO_PKG_VERSION")),
                "Agent Client Protocol v1 · macOS",
            )
            .child(group_of(rows))
        })
    }
}

/// A tab's page: its title and what it is for, then its groups.
pub(super) fn page(faces: &Faces, title: &str, lede: &str) -> Div {
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

/// The rows left by the search under a caption, in one frame; none when
/// the search left none.
fn group(faces: &Faces, caption: &str, rows: Vec<Option<Div>>) -> Option<Div> {
    let rows: Vec<Div> = rows.into_iter().flatten().collect();
    (!rows.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(ui::label(faces, caption))
            .child(group_of(rows))
    })
}

/// Rows in one frame, a line between each.
pub(super) fn group_of(rows: Vec<Div>) -> Div {
    let mut frame = card().overflow_hidden();
    for (n, r) in rows.into_iter().enumerate() {
        frame = frame.child(r.when(n > 0, |d| d.border_t_1().border_color(theme::border())));
    }
    frame
}

/// Whether what is looked for, `q` (lower case), is in any of `words`;
/// nothing looked for is in all.
fn said(q: &str, words: &[&str]) -> bool {
    q.is_empty() || words.iter().any(|w| w.to_lowercase().contains(q))
}

/// A setting: what happens, said plainly, and its control; none when
/// what is looked for, `q`, is in neither.
fn row(faces: &Faces, q: &str, name: &str, about: &str, control: impl IntoElement) -> Option<Div> {
    said(q, &[name, about]).then(|| setting(faces, name, about, control))
}

/// A row: what it is, said plainly, and its control.
pub(super) fn setting(faces: &Faces, name: &str, about: &str, control: impl IntoElement) -> Div {
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
