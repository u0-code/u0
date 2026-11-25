//! The box you write in, as Claude's: a + for what goes with it, the
//! line, dictation and Send; over it what waits on you, the changes made
//! and what is queued; under it where the session works and the agent,
//! its model, its mode and how full its context is.

use gpui::component::{
    Selectable as _,
    button::{Button, ButtonVariants as _},
    input::{Enter, MoveDown, MoveUp, Textarea},
    list::ListItem,
    popover::Popover,
    progress::ProgressCircle,
    separator::Separator,
};
use gpui::{Context, Div, SharedString, Window, div, prelude::*, px};

use crate::{
    desk::{Desk, Popup, mention},
    fonts::Faces,
    keys::{SendAside, Stash},
    session::{Session, State},
    theme, ui,
};

impl Desk {
    /// The box you write in, what asks for you over it, and the chips
    /// under it; for a session you do not write in, why.
    pub(crate) fn composer(
        &self,
        faces: &Faces,
        column: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(s) = self.session() else {
            return div();
        };
        if let Some(shut) = self.shut(faces, s, column, cx) {
            return shut;
        }
        let name = self.name_of(&s.meta.agent);
        let says = if s.items.is_empty() {
            "Describe a task or ask a question"
        } else {
            "Reply"
        };
        // Said again only when it changes: setting it draws the line again.
        self.composer.input.update(cx, |i, cx| {
            if *i.presentation().placeholder() != says {
                i.set_placeholder(says, window, cx);
            }
        });
        let focused =
            gpui::Focusable::focus_handle(self.composer.input.read(cx), cx).is_focused(window);
        // macOS dictation, typed into the line as you speak.
        let input = self.composer.input.clone();
        let dictate = ui::icon_button("dictate", "mic", "Dictate")
            .size(px(28.))
            .text_color(theme::muted())
            .on_click(move |_, window, cx| {
                window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx), cx);
                crate::macos::dictate();
            });
        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .pb(px(16.))
            .children(self.side_question(faces, s, column, cx))
            .children(self.sign_in_banner(faces, s, &name, column, cx))
            .children(self.offers_menu(faces, column, cx))
            .children(Self::changes_row(faces, s, column, cx))
            .children(Self::queued_list(faces, s, column, cx))
            .child(
                div()
                    .id("composer")
                    .flex()
                    .flex_col()
                    .w(px(column))
                    .on_drop(
                        cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                            this.drop_files(paths.paths(), window, cx);
                        }),
                    )
                    .rounded(px(12.))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(if focused {
                        theme::ghost_line()
                    } else {
                        theme::border2()
                    })
                    .children(self.pictures_row(faces, cx))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(4.))
                            .min_h(px(44.))
                            .px(px(7.))
                            .py(px(4.))
                            .child(self.add_menu(faces, s, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .px(px(2.))
                                    .child(self.composer_line(cx)),
                            )
                            .child(dictate)
                            .children(self.send_button(s, cx)),
                    ),
            )
            .child(self.chips(faces, s, column, cx))
    }

    /// The line itself: gpui-kit's text area, with the composer's own keys
    /// over it. ⌘S puts what is typed aside, ⇧⌘Return sends it aside; a
    /// new line in a list begins its next item; ↑ and ↓ on its first and
    /// last lines go back through what was sent; an image pasted goes
    /// with what you say.
    fn composer_line(&self, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity().downgrade();
        div()
            .key_context(mention::CONTEXT)
            .on_action(cx.listener(|this, _: &Stash, window, cx| this.stash_typed(window, cx)))
            .on_action(cx.listener(|this, _: &SendAside, window, cx| {
                this.send_typed(true, window, cx);
            }))
            .capture_action(cx.listener(|this, enter: &Enter, window, cx| {
                let breaks = !enter.secondary && (enter.shift || this.settings.cmd_sends);
                if breaks && this.continue_list(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.recall(-1, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.recall(1, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(
                Textarea::new(&self.composer.input)
                    .appearance(false)
                    .on_paste(move |item, _, cx| {
                        let Some(image) = item.entries().iter().find_map(|e| match e {
                            gpui::ClipboardEntry::Image(i) => Some(i.clone()),
                            _ => None,
                        }) else {
                            return false;
                        };
                        desk.update(cx, |this, cx| this.paste_picture(&image, cx))
                            .is_ok()
                    }),
            )
    }

    /// In place of the line, for a session you do not write in: one
    /// archived, until it is restored; a subagent's, which answers to its
    /// parent, not to you.
    fn shut(&self, faces: &Faces, s: &Session, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        let (icon, says, button) = if s.meta.flags.archived {
            let key = s.key;
            (
                "archive",
                "Archived. Restore it to write in it again.".to_owned(),
                ui::button("restore", "Restore", ui::Weight::Secondary)
                    .on_click(cx.listener(move |this, _, _, cx| this.restore(key, cx))),
            )
        } else {
            let up = s.parent?;
            let parent = self
                .sessions
                .at(up)
                .map(|p| ui::fit(&p.meta.title, 40))
                .unwrap_or_default();
            (
                "user",
                format!("A subagent of “{parent}”: it answers to it."),
                ui::button("to-parent", "Back to it", ui::Weight::Secondary)
                    .on_click(cx.listener(move |this, _, window, cx| this.show(up, window, cx))),
            )
        };
        Some(
            div().flex().flex_none().justify_center().pb(px(18.)).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .w(px(column))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(theme::border2())
                    .child(ui::icon(icon, 16.).text_color(theme::faint()))
                    .child(
                        ui::text(faces, says)
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(theme::text2()),
                    )
                    .child(button),
            ),
        )
    }

    /// Under the box, as Claude sets them: where it works (its project,
    /// a worktree of its own, its branch) and what was put aside or is
    /// to be said later at the left; the agent and its model, its mode
    /// and how full its context is at the right.
    fn chips(&self, faces: &Faces, s: &Session, column: f32, cx: &mut Context<Self>) -> Div {
        let draft = s.state == State::Draft && s.items.is_empty();
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .w(px(column))
            .h(px(28.))
            .mt(px(8.))
            .child(self.project_chip(faces, s, draft, cx));
        if draft && s.repo {
            let on = s.wants_worktree;
            row = row.child(
                ui::opener("worktree")
                    .h(px(28.))
                    .selected(on)
                    .child(ui::icon("branch", 14.).text_color(theme::faint()))
                    .child(chip_text(faces, "Worktree"))
                    .tooltip(if on {
                        "A worktree of its own on a new branch: your folder is left as it is"
                    } else {
                        "In this folder, on its branch"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_worktree(!on, cx))),
            );
        }
        if draft && s.repo && s.wants_worktree {
            row = row.child(self.base_chip(faces, s, cx));
        } else if let Some(branch) = &s.branch {
            row = row.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .px(px(7.))
                    .child(ui::icon("branch", 14.).text_color(theme::faint()))
                    .child(chip_text(faces, ui::fit(branch, 24))),
            );
        }
        if !self.composer.stashed.is_empty() {
            row = row.child(self.stash_menu(faces, cx));
        }
        if s.meta.id.is_some() && s.parent.is_none() {
            row = row.child(self.later_menu(faces, s, cx));
        }
        row = row
            .child(div().flex_1())
            .child(self.model_chip(faces, s, cx))
            .children(self.mode_chip(faces, s, cx));
        if let Some(u) = s.usage.as_ref().filter(|u| u.size > 0) {
            let share = u.used as f32 / u.size as f32;
            row = row.child(
                div()
                    .id("context-used")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .child(
                        ProgressCircle::new("context-ring")
                            .value(share * 100.)
                            .color(theme::accent())
                            .size(px(16.)),
                    )
                    .tooltip(ui::tip(format!(
                        "{}% of the context used",
                        (share * 100.).round()
                    ))),
            );
        }
        row
    }

    /// Where the session works: on a new one, a menu of the projects and a
    /// folder of your choosing; after, its folder's name.
    fn project_chip(
        &self,
        faces: &Faces,
        s: &Session,
        draft: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let named = self.project_name(&s.meta.project);
        let says = |chevron: bool| {
            ui::opener("project-chip")
                .h(px(28.))
                .child(ui::icon("folder", 14.).text_color(theme::faint()))
                .child(chip_text(faces, ui::fit(&named, 28)))
                .when(chevron, |b| {
                    b.child(ui::icon("chevron-down", 11.).text_color(theme::faint()))
                })
        };
        if !draft {
            return says(false)
                .tooltip(Desk::shown(&s.meta.folder))
                .into_any_element();
        }
        let faces = faces.clone();
        over_line(self.menu(
            Popup::StartIn,
            says(true),
            move |d, _, cx| d.work_in_menu(&faces, cx),
            cx,
        ))
        .into_any_element()
    }

    /// The projects a new session can work in, and a folder to choose.
    fn work_in_menu(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let here = self.session().map(|s| s.meta.folder.clone());
        let mut menu = div().flex().flex_col().w(px(240.));
        for (n, p) in self.projects().into_iter().enumerate() {
            let on = here.as_ref() == Some(&p);
            menu = menu.child(
                ListItem::new(SharedString::from(format!("work-in-{n}")))
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(ui::icon("folder", 14.).text_color(theme::faint()))
                            .child(
                                ui::text(faces, ui::fit(&self.project_name(&p), 28))
                                    .text_size(px(13.)),
                            ),
                    )
                    .when(on, |d| {
                        d.suffix(|_, _| ui::icon("check", 13.).text_color(theme::text2()))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_popup(window, cx);
                        if let Some(key) = this.current {
                            this.work_in(key, &p, cx);
                        }
                    })),
            );
        }
        menu.child(Separator::horizontal().my(px(4.))).child(
            ListItem::new("work-in-other")
                .h(px(28.))
                .px(px(8.))
                .rounded(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon("plus", 14.).text_color(theme::faint()))
                        .child(ui::text(faces, "Choose a folder…").text_size(px(13.))),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_popup(window, cx);
                    this.choose_folder(cx);
                })),
        )
    }

    /// The + at the box's left: what can go with what you write.
    fn add_menu(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Popover {
        let button = ui::icon_button("add", "plus", "Add files, a window, a command")
            .size(px(28.))
            .text_color(theme::muted());
        let able = s.state == State::Draft
            && self
                .slot(&s.meta.agent)
                .and_then(crate::hub::Slot::caps)
                .is_some_and(|k| k.session_capabilities.additional_directories.is_some());
        let faces = faces.clone();
        over_line(self.menu(
            Popup::Add,
            button,
            move |_, _, cx| {
                let row = |id: &'static str, icon: &'static str, says: &'static str| {
                    ListItem::new(id)
                        .h(px(28.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(ui::icon(icon, 16.).text_color(theme::muted()))
                                .child(ui::text(&faces, says).text_size(px(13.))),
                        )
                };
                div()
                    .flex()
                    .flex_col()
                    .w(px(230.))
                    .child(
                        row("add-files", "file", "Add files or photos").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.close_popup(window, cx);
                                Self::attach_files(window, cx);
                            },
                        )),
                    )
                    .child(
                        row("add-window", "camera", "Add a window").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.close_popup(window, cx);
                                this.capture(cx);
                            },
                        )),
                    )
                    .child(
                        row("add-command", "terminal", "Slash commands").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.close_popup(window, cx);
                                this.put_line("/", window, cx);
                                let line = this.composer.input.clone();
                                window.focus(&gpui::Focusable::focus_handle(line.read(cx), cx), cx);
                            },
                        )),
                    )
                    .when(able, |m| {
                        m.child(
                            row("add-folder", "folder", "Add folder").on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.close_popup(window, cx);
                                    this.add_folder(cx);
                                },
                            )),
                        )
                    })
            },
            cx,
        ))
    }

    /// Send, once something is written; Stop while a turn goes on and
    /// nothing is; Force stop when asked to stop and still going a while
    /// after.
    fn send_button(&self, s: &Session, cx: &mut Context<Self>) -> Option<Button> {
        let working = matches!(s.state, State::Working | State::Opening);
        let empty = self.composer.input.read(cx).value().trim().is_empty();
        let key = s.key;
        let force = working
            && self
                .stopping
                .is_some_and(|(k, t)| k == key && t.elapsed() > std::time::Duration::from_secs(8));
        let round = |b: Button| b.size(px(28.)).rounded(px(14.));
        Some(if force {
            ui::button("force-stop", "Force stop", ui::Weight::Secondary)
                .danger()
                .tooltip("It would not stop: its process is ended, and started again when wanted")
                .on_click(cx.listener(move |this, _, _, cx| this.force_stop(key, cx)))
        } else if working && empty {
            round(ui::icon_button(
                "stop",
                "square",
                crate::keys::hint(cx, "Stop the turn", "stop"),
            ))
            .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        } else if empty {
            return None;
        } else {
            round(
                ui::icon_button(
                    "send",
                    "arrow-up",
                    if working {
                        "Queue it for when this turn ends · Return"
                    } else {
                        "Send · Return, or ⌘Return to go on in the background"
                    },
                )
                .primary(),
            )
            .on_click(cx.listener(|this, _, window, cx| this.send_typed(false, window, cx)))
        })
    }

    /// Over the box, while the session has changed files: where, how
    /// much, and the way to commit them.
    fn changes_row(faces: &Faces, s: &Session, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        let files = s.total.as_ref().filter(|f| !f.is_empty())?;
        let (added, gone) = files
            .iter()
            .fold((0, 0), |(a, g), f| (a + f.added, g + f.gone));
        let repo = s.meta.worktree.as_ref().map_or(&s.meta.folder, |w| &w.repo);
        let repo = repo.file_name().map_or_else(
            || repo.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .w(px(column))
                .mb(px(8.))
                .px(px(12.))
                .py(px(6.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme::border2())
                .child(ui::icon("folder", 14.).text_color(theme::faint()))
                .child(chip_text(faces, repo))
                .children(s.branch.clone().map(|b| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(ui::icon("branch", 13.).text_color(theme::faint()))
                        .child(chip_text(faces, ui::fit(&b, 24)))
                }))
                .child(ui::counts(faces, added, gone, 12.))
                .child(div().flex_1())
                .child(
                    ui::button("commit-changes", "Commit changes", ui::Weight::Secondary).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.show_aside(crate::desk::Aside::Session, cx);
                            if !this.review.committing {
                                this.toggle_commit(window, cx);
                            }
                        }),
                    ),
                ),
        )
    }

    /// An agent that wants a sign-in says so above the line, with its
    /// way to sign in, before anything said to it is refused.
    fn sign_in_banner(
        &self,
        faces: &Faces,
        s: &Session,
        name: &str,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let Some(crate::hub::Auth::Needed(why)) = self.slot(&s.meta.agent).map(|x| &x.auth) else {
            return None;
        };
        let (a, b) = (s.meta.agent.clone(), s.meta.agent.clone());
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .w(px(column))
                .mb(px(8.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(12.))
                .bg(theme::rail())
                .border_1()
                .border_color(theme::border2())
                .child(ui::icon("alert", 16.).text_color(theme::accent()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(
                            ui::text(faces, format!("{name} needs you to sign in"))
                                .text_size(px(13.)),
                        )
                        .child(ui::one_line(
                            ui::text(faces, ui::fit(why, 80))
                                .text_size(px(11.5))
                                .text_color(theme::faint()),
                        )),
                )
                .child(
                    ui::button("sign-in-now", "Sign in", ui::Weight::Primary).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.sign_in_agent(&a, cx);
                        }),
                    ),
                )
                .child(
                    ui::button("signed-in", "I've signed in", ui::Weight::Quiet).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.recheck_auth(&b, cx);
                        }),
                    ),
                ),
        )
    }

    /// What is offered for the word being typed: the agent's commands
    /// after `/`, after `@` files, sessions and pull requests. ↑ and ↓
    /// choose one, Return or Tab takes it.
    fn offers_menu(&self, faces: &Faces, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        let typed = self.typed(cx);
        self.offers(&typed)
            .filter(|(_, o)| !o.is_empty())
            .map(|(_, list)| {
                use crate::desk::mention::Offer;
                let mut menu = div().flex().flex_col().p(px(6.)).w(px(column));
                let chosen = self.composer.offer.min(list.len() - 1);
                for (n, offer) in list.into_iter().enumerate() {
                    let (icon, name, about) = match offer {
                        Offer::Command { name, about } => ("terminal", format!("/{name}"), about),
                        Offer::Own { name, about } => {
                            ("ghost", format!("/{name}"), about.to_owned())
                        }
                        Offer::Model { label, .. } => (
                            "sliders",
                            format!("@{label}"),
                            "switch to this model".into(),
                        ),
                        Offer::File(f) => ("file", f, String::new()),
                        Offer::Session { title, .. } => ("list", title, "session".into()),
                        Offer::Pr { number, title } => (
                            "branch",
                            format!("#{number} {title}"),
                            "pull request".into(),
                        ),
                    };
                    menu = menu.child(
                        ui::menu_row(SharedString::from(format!("offer-{n}")), n == chosen)
                            .child(ui::icon(icon, 14.).text_color(theme::faint()))
                            .child(
                                ui::one_line(
                                    ui::mono(faces, name)
                                        .text_size(px(12.5))
                                        .text_color(theme::text()),
                                )
                                .w_auto()
                                .max_w(px(360.)),
                            )
                            .child(
                                ui::one_line(
                                    ui::text(faces, about)
                                        .text_size(px(12.))
                                        .text_color(theme::faint()),
                                )
                                .flex_1(),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.take_offer(n, window, cx);
                            })),
                    );
                }
                div().mb(px(8.)).child(ui::menu("offers").child(menu))
            })
    }

    /// Images waiting to go with what is said.
    fn pictures_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        (!self.composer.pictures.is_empty()).then(|| {
            let mut row = div().flex().flex_wrap().gap(px(6.)).px(px(10.)).pt(px(10.));
            for (n, p) in self.composer.pictures.iter().enumerate() {
                row = row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(26.))
                        .pl(px(8.))
                        .pr(px(2.))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(theme::tag_line())
                        .child(ui::icon("file", 12.).text_color(theme::faint()))
                        .child(
                            ui::text(
                                faces,
                                format!("Image {} · {} KB", n + 1, p.bytes.len() / 1024),
                            )
                            .text_size(px(12.))
                            .text_color(theme::text2()),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("drop-pic-{n}")),
                                "x",
                                "Take it out",
                            )
                            .size(px(20.))
                            .on_click(cx.listener(move |this, _, _, cx| this.drop_picture(n, cx))),
                        ),
                );
            }
            row
        })
    }

    /// What was said while the turn goes on, waiting its turn: each can
    /// be said now (the turn is stopped first) or taken back to edit.
    fn queued_list(faces: &Faces, s: &Session, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        (!s.meta.queued.is_empty()).then(|| {
            let count = s.meta.queued.len();
            let mut list = div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .w(px(column))
                .mb(px(8.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme::border2())
                .child(chip_text(
                    faces,
                    if count == 1 {
                        "1 message queued".to_owned()
                    } else {
                        format!("{count} messages queued")
                    },
                ));
            for (n, text) in s.meta.queued.iter().enumerate() {
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon("clock", 12.).text_color(theme::faint()))
                        .child(
                            ui::one_line(
                                ui::text(faces, ui::fit(text, 70))
                                    .text_size(px(12.5))
                                    .text_color(theme::text2()),
                            )
                            .flex_1(),
                        )
                        .when(n > 0, |d| {
                            d.child(
                                ui::icon_button(
                                    SharedString::from(format!("queue-up-{n}")),
                                    "arrow-up",
                                    "Earlier",
                                )
                                .size(px(22.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.queue_earlier(n, cx)),
                                ),
                            )
                        })
                        .child(
                            ui::button(
                                SharedString::from(format!("steer-{n}")),
                                "Send now",
                                ui::Weight::Quiet,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.steer(n, cx))),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("unqueue-{n}")),
                                "pencil",
                                "Edit",
                            )
                            .size(px(22.))
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.unqueue(n, window, cx)),
                            ),
                        ),
                );
            }
            list
        })
    }

    /// A side question: what is asked here stays out of the session it
    /// was asked of; its answer can be taken back there.
    fn side_question(
        &self,
        faces: &Faces,
        s: &Session,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        s.beside.map(|up| {
            let title = self
                .sessions
                .at(up)
                .map(|p| ui::fit(&p.meta.title, 40))
                .unwrap_or_default();
            let key = s.key;
            let answered = s
                .items
                .iter()
                .any(|i| matches!(i, crate::session::Item::Agent(_)));
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .w(px(column))
                .mb(px(8.))
                .px(px(14.))
                .py(px(8.))
                .rounded(px(12.))
                .border_1()
                .border_color(theme::border2())
                .child(ui::icon("bulb", 15.).text_color(theme::faint()))
                .child(
                    ui::text(
                        faces,
                        format!("A side question of “{title}”: it does not go in it."),
                    )
                    .flex_1()
                    .text_size(px(13.))
                    .text_color(theme::text2()),
                )
                .when(answered, |d| {
                    d.child(
                        ui::button("answer-back", "Add the answer to it", ui::Weight::Secondary)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.answer_back(key, window, cx);
                            })),
                    )
                })
                .child(
                    ui::button("beside-back", "Back to it", ui::Weight::Quiet).on_click(
                        cx.listener(move |this, _, window, cx| this.show(up, window, cx)),
                    ),
                )
        })
    }
}

/// A menu of the composer, opened upward from its mark at `corner` over
/// the box.
pub(super) fn over_box(menu: Popover, corner: gpui::Anchor) -> Popover {
    menu.anchor(corner).offset(px(4.))
}

/// A menu of the composer's at its left.
pub(super) fn over_line(menu: Popover) -> Popover {
    over_box(menu, gpui::Anchor::BottomLeft)
}

/// What a chip under the box says.
fn chip_text(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .flex_none()
        .text_size(px(13.))
        .text_color(theme::text2())
}
