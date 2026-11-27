//! The box you write in, as Claude's: a + for what goes with it, the
//! line, dictation and Send; over it what waits on you, the changes made
//! and what is queued; under it where the session works and the agent,
//! its model, its mode and how full its context is.

use gpui::component::{
    Selectable as _, Sizable as _,
    attachment::{
        Attachment, AttachmentContent, AttachmentDescription, AttachmentGroup, AttachmentMedia,
        AttachmentTitle,
    },
    button::{Button, ButtonVariants as _},
    input::{Enter, Escape, IndentInline, MoveDown, MoveUp, Textarea},
    list::ListItem,
    menu::PopupMenu,
    popover::Popover,
    progress::ProgressCircle,
    speech::{SpeechButton, SpeechIndicator},
};
use gpui::{Context, Div, Entity, SharedString, Window, div, prelude::*, px};

use crate::{
    desk::{Desk, Popup, mention},
    fonts::Faces,
    keys::{EditQueued, SendAside, Stash},
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
        // The session's, or with none on screen, the one not begun yet.
        let unbegun = self.unbegun();
        let Some(s) = self.session().or(unbegun.as_ref()) else {
            return div();
        };
        if let Some(shut) = self.shut(faces, s, column, cx) {
            return shut;
        }
        let name = self.name_of(&s.meta.agent);
        // What you might say next stands in the empty line, Tab to take.
        let suggested = self.suggestion(s.key, cx);
        let says = match suggested {
            Some(next) => SharedString::from(next.to_owned()),
            None if s.items.is_empty() => crate::tr("Describe a task or ask a question"),
            None => crate::tr("Reply"),
        };
        // Said again only when it changes: setting it draws the line again.
        self.composer.input.update(cx, |i, cx| {
            if *i.presentation().placeholder() != says {
                i.set_placeholder(says, window, cx);
            }
        });
        let focused =
            gpui::Focusable::focus_handle(self.composer.input.read(cx), cx).is_focused(window);
        // Dictation, heard on this Mac: gpui-kit's speech button, its bars
        // moving with your voice; again, finished.
        let dictate =
            (self.speech.as_ref()).map(|s| SpeechButton::new(&s.state).with_size(px(28.)));
        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .pb(px(16.))
            // Not begun: a press anywhere in it begins it, before what was
            // pressed does its part.
            .when(self.current.is_none(), |d| {
                d.capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                    this.begin_unbegun(window, cx);
                }))
            })
            .children(self.prompt_link_banner(column, cx))
            .children(self.side_question(faces, s, column, cx))
            .children(self.children_asking(faces, s, column, cx))
            .children(self.sign_in_banner(faces, s, &name, column, cx))
            .children(self.offers_menu(faces, column, cx))
            .children(Self::goal_row(faces, s, column, cx))
            .children(Self::changes_row(faces, s, column, cx))
            .children(self.queued_list(faces, s, column, cx))
            .children(self.held_banner(column, cx))
            .children(Self::branch_banner(s, column))
            .children(self.also_run_row(faces, s, column, cx))
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
                    // A session dragged from the sidebar goes with it.
                    .on_drop(
                        cx.listener(|this, d: &crate::sidebar::Dragged, window, cx| {
                            this.drop_session(d.key, window, cx);
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
                    .children(self.replying_row(faces, cx))
                    .children(self.attached_row(cx))
                    .children(self.chip_preview_row(faces, cx))
                    .children(self.note_row(cx))
                    .children(self.dictation_row(faces, cx))
                    // The +, the line and the mic on one middle: a line as
                    // tall as the buttons, the text in its middle, the same
                    // room above and below; a longer message grows upward,
                    // the buttons at its foot.
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(4.))
                            .px(px(7.))
                            .py(px(LINE_PAD))
                            .child(self.add_menu(s, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .min_h(px(28.))
                                    .flex()
                                    .flex_col()
                                    .justify_center()
                                    .px(px(4.))
                                    .child(self.composer_line(cx)),
                            )
                            .children(dictate)
                            .children(self.send_button(s, window, cx)),
                    )
                    .children(self.over_limit(faces, cx)),
            )
            .child(self.chips(faces, s, column, cx))
    }

    /// While you dictate, what is heard as it comes, with the way to
    /// finish (Done) or let it go (Esc); while the writer cleans it up,
    /// the way to go on without it.
    fn dictation_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        let d = self.dictation.as_ref()?;
        let speech = &self.speech.as_ref()?.state;
        let listening =
            speech.read(cx).status() == gpui::component::speech::SpeechStatus::Recording;
        let (said, end) = match (&d.cleaning, listening) {
            (Some(_), _) => (crate::tr("Cleaning up what you said…").into(), "Skip"),
            (None, true) if d.heard.is_empty() => (crate::tr("Listening…").into(), "Done"),
            (None, true) => (d.heard.clone(), "Done"),
            (None, false) if d.heard.is_empty() => (
                crate::tr("Waiting for speech recognition…").into(),
                "Cancel",
            ),
            (None, false) => (d.heard.clone(), "Cancel"),
        };
        let cleaning = d.cleaning.is_some();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .pt(px(8.))
                .child(SpeechIndicator::new(speech).with_size(px(14.)))
                .child(
                    ui::text(faces, said)
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(if d.heard.is_empty() {
                            theme::muted()
                        } else {
                            theme::text()
                        }),
                )
                .child(
                    Button::new("dictation-end")
                        .small()
                        .ghost()
                        .label(crate::tr(end))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if end == "Done" {
                                this.finish_dictation(cx);
                            } else {
                                this.cancel_dictation(window, cx);
                            }
                        })),
                )
                .when(!cleaning, |row| {
                    row.child(ui::fine(faces, crate::tr("Esc")).text_color(theme::muted()))
                }),
        )
    }

    /// The line itself: gpui-kit's text area, with the composer's own keys
    /// over it. ⌘S puts what is typed aside (or takes it back), ⇧⌘Return
    /// sends it the other way, or with nothing typed steers in what was
    /// queued first; ⌥↑ at its start takes back the last queued to edit;
    /// a new line in a list begins its next item; ↑ and ↓ on its first
    /// and last lines go back through what was sent; Escape lets go of a
    /// send waiting for its pictures; an image pasted goes with what you
    /// say.
    fn composer_line(&self, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity().downgrade();
        div()
            .key_context(mention::CONTEXT)
            .on_action(cx.listener(|this, _: &Stash, window, cx| this.stash_typed(window, cx)))
            .on_action(cx.listener(|this, _: &SendAside, window, cx| {
                let queued = this.session().is_some_and(|s| {
                    matches!(s.state, State::Working | State::Opening) && !s.meta.queued.is_empty()
                });
                if this.typed(cx).is_empty() && queued {
                    this.steer(0, cx);
                } else {
                    this.send_typed(true, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &EditQueued, window, cx| {
                let last = this.session().map_or(0, |s| s.meta.queued.len());
                if this.composer.input.read(cx).cursor() == 0 && last > 0 {
                    this.unqueue(last - 1, window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .capture_action(cx.listener(|this, enter: &Enter, window, cx| {
                let breaks = !enter.secondary && (enter.shift || !this.return_sends(cx));
                if breaks && this.continue_list(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if let Some(key) = this.current
                    && let Some(next) = this.suggestion(key, cx).map(str::to_owned)
                {
                    this.composer.suggested = None;
                    this.put_line(&next, window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.dictation.is_some() {
                    this.cancel_dictation(window, cx);
                    cx.stop_propagation();
                } else if this.composer.editing_queued.take().is_some() {
                    this.put_line("", window, cx);
                    cx.notify();
                    cx.stop_propagation();
                } else if this.composer.previewing_chip.take().is_some()
                    || this.composer.held.take().is_some()
                {
                    cx.notify();
                    cx.stop_propagation();
                } else if this.current.and_then(|k| this.suggestion(k, cx)).is_some() {
                    this.composer.suggested = None;
                    cx.notify();
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
                // Small: 2 above and below the text, not Medium's 8, so a
                // line fits inside the buttons' 28 and sits on their middle.
                Textarea::new(&self.composer.input)
                    .small()
                    .appearance(false)
                    // ⇧⌘V puts what is pasted in the line, whatever it is.
                    .on_paste(move |item, window, cx| {
                        !window.modifiers().shift
                            && desk
                                .update(cx, |this, cx| this.paste(item, window, cx))
                                .unwrap_or(false)
                    }),
            )
    }

    /// What the writer thinks you will say next in the session `key`,
    /// while nothing is typed there.
    fn suggestion<'a>(&'a self, key: u64, cx: &gpui::App) -> Option<&'a str> {
        let (k, next) = self.composer.suggested.as_ref()?;
        (*k == key && self.composer.input.read(cx).value().is_empty()).then_some(next.as_str())
    }

    /// In place of the line, for a session you do not write in: one
    /// archived, until it is restored; a subagent's, which answers to its
    /// parent, not to you.
    fn shut(&self, faces: &Faces, s: &Session, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        let (icon, says, button) = if s.meta.flags.archived {
            let key = s.key;
            (
                "archive",
                crate::tr("Archived. Restore it to write in it again."),
                ui::button("restore", crate::tr("Restore"), ui::Weight::Secondary)
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
                crate::tr_with(
                    "A subagent of “%{parent}”: it answers to it.",
                    &[("parent", &parent)],
                ),
                ui::button("to-parent", crate::tr("Back to it"), ui::Weight::Secondary)
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
                            .text_size(px(crate::style::LABEL))
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
        if draft && s.git_dir().is_some() {
            let on = s.wants_worktree;
            row = row.child(
                ui::opener("worktree")
                    .h(px(28.))
                    .selected(on)
                    .child(ui::icon("branch", 14.).text_color(theme::faint()))
                    .child(chip_text(faces, crate::tr("Worktree")))
                    .tooltip(crate::tr(if on {
                        "A worktree of its own on a new branch: your folder is left as it is"
                    } else {
                        "In this folder, on its branch"
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_worktree(!on, cx))),
            );
        }
        if draft && s.git_dir().is_some() && s.wants_worktree {
            row = row.child(self.base_chip(faces, s, cx));
            if !self.composer.also_run.is_empty() {
                let total = self.composer.also_run.len() + 1;
                let base = match &s.begin {
                    Some(crate::session::Begin::Branch(b)) => b.clone(),
                    Some(crate::session::Begin::Pr(n, _)) => format!("#{n}"),
                    None => s.branch.clone().unwrap_or_else(|| "HEAD".into()),
                };
                row = row.child(
                    ui::text(
                        faces,
                        crate::tr_with(
                            "%{n} worktrees from %{branch}, one per agent",
                            &[("n", &total), ("branch", &base)],
                        ),
                    )
                    .px(px(6.))
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::faint()),
                );
            }
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
        if draft {
            let bg_on = self.composer.in_background;
            row = row.child(
                ui::opener("start-bg")
                    .h(px(28.))
                    .selected(bg_on)
                    .child(chip_text(faces, crate::tr("Start in the background")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.composer.in_background = !this.composer.in_background;
                        cx.notify();
                    })),
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
                    .tooltip(ui::tip(crate::tr_with(
                        "%{share}% of the context used",
                        &[("share", &(share * 100.).round())],
                    ))),
            );
        }
        row
    }

    /// Where the session works: on a new one, a menu of the projects (on
    /// each server the window holds) and a folder of your choosing; after,
    /// its folder's name, and its server's where there are more.
    fn project_chip(
        &self,
        faces: &Faces,
        s: &Session,
        draft: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let named = self.project_on(&crate::desk::Place::project_of(s));
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
        over_line(self.menu(
            Popup::StartIn,
            says(true),
            |d, menu, desk, _| d.work_in_menu(menu, desk),
            cx,
        ))
        .into_any_element()
    }

    /// The projects a new session can work in, under their server's name
    /// where there are more, and a folder of this Mac's to choose.
    fn work_in_menu(&self, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        let here = self.session().map(crate::desk::Place::folder_of);
        // Each has its icon, so the one chosen is ticked at the end.
        menu = menu.check_side(gpui::component::Side::Right);
        let mut server = None;
        for p in self.projects() {
            if server != Some(p.host)
                && let Some(name) = self.server_chip(p.host)
            {
                menu = menu.label(name.to_owned());
            }
            server = Some(p.host);
            let on = here.as_ref() == Some(&p);
            let name = ui::fit(&self.project_name(&p), 28);
            menu = menu.item(
                Self::menu_item(desk, name, move |this, _, cx| {
                    if let Some(key) = this.current {
                        this.work_in(key, &p, cx);
                    }
                })
                .icon(ui::menu_glyph("folder"))
                .checked(on),
            );
        }
        menu.separator().item(
            Self::menu_item(desk, crate::tr("Choose a folder…"), |this, _, cx| {
                this.choose_folder(cx);
            })
            .icon(ui::menu_glyph("plus")),
        )
    }

    /// The + at the box's left: what can go with what you write.
    fn add_menu(&self, s: &Session, cx: &mut Context<Self>) -> Popover {
        let button = ui::icon_button("add", "plus", crate::tr("Add files, a window, a command"))
            .size(px(28.))
            .text_color(theme::muted());
        let able = s.state == State::Draft
            && self
                .slot(s.host, &s.meta.agent)
                .and_then(crate::hub::Slot::caps)
                .is_some_and(|k| k.session_capabilities.additional_directories.is_some());
        let desk = cx.entity();
        over_line(
            Popover::new("composer-add")
                .open(self.popup == Popup::Add)
                .trigger(button.selected(self.popup == Popup::Add))
                .appearance(false)
                .on_open_change({
                    let desk = desk.clone();
                    move |open, window, cx| {
                        desk.update(cx, |d, cx| {
                            if *open {
                                d.popup = Popup::Add;
                            } else if d.popup == Popup::Add {
                                d.close_popup(window, cx);
                            }
                            cx.notify();
                        });
                    }
                })
                .content(move |_, window, cx| {
                    let desk = desk.clone();
                    let wins = crate::macos::windows();
                    PopupMenu::build(window, cx, move |mut menu, window, cx| {
                        let row =
                            |icon: &str,
                             says: &'static str,
                             f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
                                Self::menu_item(&desk, crate::tr(says), f)
                                    .icon(ui::menu_glyph(icon))
                            };
                        menu = menu.item(row("file", "Add files or photos", |_, window, cx| {
                            Self::attach_files(window, cx);
                        }));
                        if wins.is_empty() {
                            menu = menu.item(row("camera", "Add a window", |this, _, cx| {
                                this.capture(None, cx);
                            }));
                        } else {
                            let sub_desk = desk.clone();
                            menu = menu.submenu_with_icon(
                                Some(ui::menu_glyph("camera")),
                                crate::tr("Add a window"),
                                window,
                                cx,
                                move |mut sub, _, _| {
                                    for w in wins.iter().take(12) {
                                        let id = w.id;
                                        let label = if w.title.trim().is_empty() {
                                            w.app.clone()
                                        } else {
                                            format!("{} — {}", ui::fit(&w.title, 36), w.app)
                                        };
                                        sub = sub.item(Self::menu_item(
                                            &sub_desk,
                                            label,
                                            move |this, _, cx| this.capture(Some(id), cx),
                                        ));
                                    }
                                    sub.separator().item(Self::menu_item(
                                        &sub_desk,
                                        crate::tr("Choose a window…"),
                                        |this, _, cx| this.capture(None, cx),
                                    ))
                                },
                            );
                        }
                        menu.item(row("terminal", "Slash commands", |this, window, cx| {
                            this.put_line("/", window, cx);
                            let line = this.composer.input.clone();
                            window.focus(&gpui::Focusable::focus_handle(line.read(cx), cx), cx);
                        }))
                        .when(able, |m| {
                            m.item(row("folder", "Add folder", |this, _, cx| {
                                this.add_folder(cx);
                            }))
                        })
                        .min_w(px(230.))
                    })
                }),
        )
        .offset(px(4. + LINE_PAD + 1.))
    }

    /// Send, once something is written; while a turn goes on, as it will
    /// be sent: queued or steering it, the other with ⌘ held (a click
    /// with ⌘ sends it so); Stop while a turn goes on and nothing is
    /// written; Force stop when asked to stop and still going a while
    /// after. Nothing while what is written is too long to send.
    fn send_button(&self, s: &Session, window: &Window, cx: &mut Context<Self>) -> Option<Button> {
        let working = matches!(s.state, State::Working | State::Opening);
        let typed = self.typed(cx);
        let key = s.key;
        let force = working
            && self
                .stopping
                .is_some_and(|(k, t)| k == key && t.elapsed() > std::time::Duration::from_secs(8));
        let round = |b: Button| b.size(px(28.)).rounded(px(14.));
        let other = if self.return_sends(cx) {
            "⌘Return"
        } else {
            "⇧⌘Return"
        };
        Some(if force {
            ui::button("force-stop", crate::tr("Force stop"), ui::Weight::Secondary)
                .danger()
                .tooltip(crate::tr(
                    "It would not stop: its process is ended, and started again when wanted",
                ))
                .on_click(cx.listener(move |this, _, _, cx| this.force_stop(key, cx)))
        } else if self.composer.editing_queued.is_some() && !typed.is_empty() {
            ui::button(
                "save-queued",
                crate::tr("Save to queue"),
                ui::Weight::Primary,
            )
            .on_click(cx.listener(|this, _, window, cx| this.send_typed(false, window, cx)))
        } else if s.state == State::Draft
            && s.items.is_empty()
            && !self.composer.also_run.is_empty()
            && !typed.is_empty()
            && typed.chars().count() <= mention::LIMIT
        {
            let total = self.composer.also_run.len() + 1;
            ui::button(
                "run-many",
                crate::tr_with("Run %{n} agents", &[("n", &total)]),
                ui::Weight::Primary,
            )
            .on_click(cx.listener(|this, _, window, cx| this.send_typed(false, window, cx)))
        } else if working && typed.is_empty() {
            round(ui::icon_button(
                "stop",
                "square",
                crate::keys::hint(cx, &crate::tr("Stop the turn"), "stop"),
            ))
            .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        } else if typed.is_empty() || typed.chars().count() > mention::LIMIT {
            return None;
        } else {
            let (icon, tip) = if !working {
                (
                    "arrow-up",
                    crate::tr_with(
                        "Send · %{keys} to go on in the background",
                        &[("keys", &other)],
                    ),
                )
            } else if self.settings.steer != window.modifiers().platform {
                (
                    "corner-up-right",
                    crate::tr_with(
                        "Steer: stop the turn and say it now · ⌘-click or %{keys} to queue it",
                        &[("keys", &other)],
                    ),
                )
            } else {
                (
                    "list-plus",
                    crate::tr_with(
                        "Queue it for when this turn ends · ⌘-click or %{keys} to steer",
                        &[("keys", &other)],
                    ),
                )
            };
            round(ui::icon_button("send", icon, tip).primary()).on_click(cx.listener(
                move |this, e: &gpui::ClickEvent, window, cx| {
                    this.send_typed(working && e.modifiers().platform, window, cx);
                },
            ))
        })
    }

    /// Under the line, once what is written is longer than a message may
    /// be: by how much, and a button to attach it as a file instead.
    fn over_limit(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        let count = self.typed(cx).chars().count();
        (count > mention::LIMIT).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(14.))
                .pb(px(8.))
                .child(
                    ui::text(
                        faces,
                        crate::tr_with(
                            "%{count} characters: %{over} over the %{limit} a message may hold. Cut it, or send it in parts.",
                            &[
                                ("count", &count),
                                ("over", &(count - mention::LIMIT)),
                                ("limit", &mention::LIMIT),
                            ],
                        ),
                    )
                    .flex_1()
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::coral()),
                )
                .child(
                    ui::button(
                        "attach-over-limit",
                        crate::tr("Attach as file"),
                        ui::Weight::Secondary,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.attach_typed_as_file(window, cx);
                    })),
                )
        })
    }

    /// Safety notice when a `u0://` link pre-filled a prompt into the draft.
    fn prompt_link_banner(&self, column: f32, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.composer.from_link {
            return None;
        }
        let desk = cx.entity().downgrade();
        Some(
            gpui::component::alert::Alert::info(
                "prompt-link-banner",
                crate::tr("Review it before sending — nothing runs until you send it."),
            )
            .title(crate::tr("Opened from a prompt link"))
            .w(px(column))
            .mb(px(8.))
            .on_close(move |_, _, cx| {
                let _ = desk.update(cx, |this, cx| {
                    this.composer.from_link = false;
                    cx.notify();
                });
            }),
        )
    }

    /// Multi-model fanout strip ("Also run on") above a draft's composer.
    fn also_run_row(
        &self,
        faces: &Faces,
        s: &Session,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !(s.state == State::Draft && s.items.is_empty()) || self.composer.also_run.is_empty() {
            return None;
        }
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .w(px(column))
            .mb(px(8.))
            .child(
                ui::text(faces, crate::tr("Also run on"))
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::faint()),
            );
        for (n, (agent, model)) in self.composer.also_run.iter().enumerate() {
            let label = self.also_label(s, agent, model.as_deref());
            let (a, m) = (agent.clone(), model.clone());
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(2.))
                    .border_1()
                    .border_color(theme::tag_line())
                    .bg(theme::panel())
                    .child(ui::mark(faces, &self.mark_of(agent), 13.))
                    .child(
                        ui::text(faces, label)
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::text2()),
                    )
                    .child(
                        ui::icon_button(
                            SharedString::from(format!("also-run-rm-{n}")),
                            "x",
                            crate::tr("Not this one"),
                        )
                        .size(px(18.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_also_run(&a, m.clone(), cx);
                        })),
                    ),
            );
        }
        row = row.child(
            ui::button("also-add-model", crate::tr("Add model"), ui::Weight::Quiet)
                .on_click(cx.listener(|this, _, window, cx| this.open_model(window, cx))),
        );
        Some(row)
    }

    /// Preview card for an attached text/excerpt chip when clicked.
    fn chip_preview_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        let n = self.composer.previewing_chip?;
        let Some(mention::Attached::Text(chip)) = self.composer.attached.get(n) else {
            return None;
        };
        let preview: String = chip.text.lines().take(8).collect::<Vec<_>>().join("\n");
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .mx(px(10.))
                .mt(px(8.))
                .p(px(10.))
                .bg(theme::well())
                .border_1()
                .border_color(theme::border())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            ui::text(faces, crate::tr(chip.kind.name()))
                                .flex_1()
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::muted()),
                        )
                        .child(
                            ui::button("chip-note-btn", crate::tr("Edit"), ui::Weight::Quiet)
                                .h(px(22.))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.composer.previewing_chip = None;
                                    this.begin_note(n, window, cx);
                                })),
                        )
                        .child(
                            ui::icon_button("chip-preview-close", "x", crate::tr("Close"))
                                .size(px(20.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.composer.previewing_chip = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    ui::mono(faces, preview)
                        .text_size(px(crate::style::CAPTION))
                        .text_color(theme::text2()),
                ),
        )
    }

    /// Over the box, while what you sent waits for its pictures to be
    /// read: that it goes once they are, and the way to keep it back.
    fn held_banner(&self, column: f32, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.composer.held?;
        let n = self.composer.reading;
        let desk = cx.entity().downgrade();
        Some(
            gpui::component::alert::Alert::info(
                "held",
                crate::tr("Edit, or press Escape, to keep it back"),
            )
            .title(if n == 1 {
                crate::tr("Sending once the picture is ready")
            } else {
                crate::tr_with("Sending once %{n} pictures are ready", &[("n", &n)])
            })
            .w(px(column))
            .mb(px(8.))
            .on_close(move |_, _, cx| {
                let _ = desk.update(cx, |this, cx| {
                    this.composer.held = None;
                    cx.notify();
                });
            }),
        )
    }

    /// Over the box, where the folder's branch changed since you last said
    /// something in the session: what goes next goes on on another branch.
    fn branch_banner(s: &Session, column: f32) -> Option<impl IntoElement> {
        let (was, now) = (s.ran_on.as_ref()?, s.branch.as_ref()?);
        (was != now).then(|| {
            gpui::component::alert::Alert::warning(
                SharedString::from(format!("branch-{}", s.key)),
                crate::tr_with(
                    "It last ran on %{was}; what you send now goes on on %{now}.",
                    &[("was", was), ("now", now)],
                ),
            )
            .title(crate::tr("The branch changed"))
            .w(px(column))
            .mb(px(8.))
        })
    }

    /// Over the box, while the session has a goal (`/goal`): what done
    /// means, whether it is paused, and the agent's own commands to pause,
    /// take up or clear it.
    fn goal_row(faces: &Faces, s: &Session, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        let goal = s.goal.as_ref()?;
        let command = |id: &'static str, label: &'static str, said: &'static str| {
            ui::button(id, crate::tr(label), ui::Weight::Quiet)
                .on_click(cx.listener(move |this, _, _, cx| this.say(said, cx)))
        };
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
                .child(ui::icon("circle-dot", 14.).text_color(if goal.paused {
                    theme::faint()
                } else {
                    theme::accent()
                }))
                .child(
                    chip_text(
                        faces,
                        crate::tr(if goal.paused { "Goal, paused" } else { "Goal" }),
                    )
                    .flex_none(),
                )
                .child(
                    ui::one_line(
                        chip_text(faces, goal.objective.clone()).text_color(theme::text()),
                    )
                    .flex_1()
                    .min_w(px(0.)),
                )
                .child(if goal.paused {
                    command("goal-resume", "Resume", "/goal resume")
                } else {
                    command("goal-pause", "Pause", "/goal pause")
                })
                .child(command("goal-clear", "Clear", "/goal clear")),
        )
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
                .child(ui::counts(faces, added, gone, crate::style::CAPTION))
                .child(div().flex_1())
                .child(
                    ui::button(
                        "commit-changes",
                        crate::tr("Commit changes"),
                        ui::Weight::Secondary,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_surface(crate::desk::Surface::Diff, cx);
                        if !this.review.committing {
                            this.toggle_commit(window, cx);
                        }
                    })),
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
        let Some(crate::hub::Auth::Needed(why)) = self.slot(s.host, &s.meta.agent).map(|x| &x.auth)
        else {
            return None;
        };
        let (host, a, b) = (s.host, s.meta.agent.clone(), s.meta.agent.clone());
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
                            ui::text(
                                faces,
                                crate::tr_with("%{name} needs you to sign in", &[("name", &name)]),
                            )
                            .text_size(px(crate::style::LABEL)),
                        )
                        .child(ui::one_line(
                            ui::text(faces, why.clone())
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::faint()),
                        )),
                )
                .child(
                    ui::button("sign-in-now", crate::tr("Sign in"), ui::Weight::Primary).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.sign_in_agent(host, &a, cx);
                        }),
                    ),
                )
                .child(
                    ui::button("signed-in", crate::tr("I've signed in"), ui::Weight::Quiet)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.recheck_auth(host, &b, cx);
                        })),
                ),
        )
    }

    /// What is offered for the word being typed: the agent's commands
    /// after `/`, after `@` files, sessions and pull requests. ↑ and ↓
    /// choose one, Return or Tab takes it.
    fn offers_menu(&self, faces: &Faces, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        let typed = self.typed(cx);
        let agent_name = self
            .session()
            .map(|s| self.name_of(&s.meta.agent))
            .unwrap_or_default();
        self.offers(&typed)
            .filter(|(_, o)| !o.is_empty())
            .map(|(_, list)| {
                use crate::desk::mention::Offer;
                let mut menu = div().flex().flex_col().p(px(6.)).w(px(column));
                let chosen = self.composer.offer.min(list.len() - 1);
                let mut last_group = String::new();
                for (n, offer) in list.into_iter().enumerate() {
                    let group = match &offer {
                        Offer::Command { .. } => {
                            crate::tr_with("%{name}'s commands", &[("name", &agent_name)])
                                .to_string()
                        }
                        Offer::Own { .. } => crate::tr("u0's").to_string(),
                        Offer::Model { .. } => crate::tr("Models").to_string(),
                        Offer::File(_) => crate::tr("Files").to_string(),
                        Offer::Session { .. } => crate::tr("Threads").to_string(),
                        Offer::Pr { .. } => crate::tr("Pull requests").to_string(),
                        Offer::Issue { .. } => crate::tr("Issues").to_string(),
                    };
                    if group != last_group {
                        last_group.clone_from(&group);
                        menu = menu.child(
                            ui::text(faces, group)
                                .px(px(8.))
                                .pt(px(if n == 0 { 2. } else { 8. }))
                                .pb(px(2.))
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::faint()),
                        );
                    }
                    let (icon, name, about) = match offer {
                        Offer::Command { name, about } => ("terminal", format!("/{name}"), about),
                        Offer::Own { name, about } => {
                            ("u0", format!("/{name}"), crate::tr(about).to_string())
                        }
                        Offer::Model { label, .. } => (
                            "sliders",
                            format!("@{label}"),
                            crate::tr("switch to this model").into(),
                        ),
                        Offer::File(f) => ("file", f, String::new()),
                        Offer::Session { title, .. } => ("list", title, crate::tr("thread").into()),
                        Offer::Pr { number, title } => (
                            "pull-request",
                            format!("#{number} {title}"),
                            crate::tr("pull request").into(),
                        ),
                        Offer::Issue { number, title } => (
                            "circle-dot",
                            format!("#{number} {title}"),
                            crate::tr("issue").into(),
                        ),
                    };
                    menu = menu.child(
                        ListItem::new(SharedString::from(format!("offer-{n}")))
                            .selected(n == chosen)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(ui::icon(icon, 14.).text_color(theme::faint()))
                                    .child(
                                        ui::one_line(
                                            ui::mono(faces, name)
                                                .text_size(px(crate::style::CAPTION))
                                                .text_color(theme::text()),
                                        )
                                        .w_auto()
                                        .max_w(px(360.)),
                                    )
                                    .child(
                                        ui::one_line(
                                            ui::text(faces, about)
                                                .text_size(px(crate::style::CAPTION))
                                                .text_color(theme::faint()),
                                        )
                                        .flex_1(),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.take_offer(n, window, cx);
                            })),
                    );
                }
                div().mb(px(8.)).child(ui::menu("offers").child(menu))
            })
    }

    /// What is attached, as gpui-kit's attachments: an image with its
    /// picture, a file with its name and size, a long paste with its
    /// length; each can be taken out.
    fn attached_row(&self, cx: &mut Context<Self>) -> Option<AttachmentGroup> {
        use crate::desk::mention::Attached;
        (!self.composer.attached.is_empty()).then(|| {
            let mut row = AttachmentGroup::new("attached").px(px(10.)).pt(px(10.));
            for (n, a) in self.composer.attached.iter().enumerate() {
                let (media, title, about) = match a {
                    Attached::Image { mime, bytes } => (
                        AttachmentMedia::new().src(gpui::ImageSource::Image(std::sync::Arc::new(
                            gpui::Image::from_bytes(
                                gpui::ImageFormat::from_mime_type(mime)
                                    .unwrap_or(gpui::ImageFormat::Png),
                                bytes.clone(),
                            ),
                        ))),
                        crate::tr_with("Image %{n}", &[("n", &(n + 1))]).to_string(),
                        crate::tidy::shown(bytes.len() as u64),
                    ),
                    Attached::File(path) => (
                        AttachmentMedia::new().child(ui::icon("file", 16.)),
                        path.file_name().map_or_else(
                            || path.display().to_string(),
                            |f| f.to_string_lossy().into_owned(),
                        ),
                        std::fs::metadata(path)
                            .map(|m| crate::tidy::shown(m.len()))
                            .unwrap_or_default(),
                    ),
                    Attached::Text(chip) => (
                        AttachmentMedia::new().child(ui::icon(
                            match chip.kind {
                                crate::session::Context::Paste => "list",
                                crate::session::Context::Quote => "reply",
                                crate::session::Context::Terminal => "terminal",
                                crate::session::Context::Review => "square-pen",
                            },
                            16.,
                        )),
                        if chip.note.is_empty() {
                            crate::tr(chip.kind.name()).to_string()
                        } else {
                            format!(
                                "{}: {}",
                                crate::tr(chip.kind.name()),
                                ui::fit(&chip.note, 24)
                            )
                        },
                        ui::fit(chip.text.lines().next().unwrap_or_default(), 32),
                    ),
                };
                row = row.child(
                    Attachment::new()
                        .id(SharedString::from(format!("attached-{n}")))
                        .small()
                        .media(media)
                        .content(
                            AttachmentContent::new()
                                .title(AttachmentTitle::new(title))
                                .description(AttachmentDescription::new(about)),
                        )
                        .when(matches!(a, Attached::Text(_)), |c| {
                            c.tooltip(crate::tr("Click to write a note on it"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_chip_preview(n, cx);
                                }))
                        })
                        .on_remove(cx.listener(move |this, _, _, cx| this.detach(n, cx))),
                );
            }
            row
        })
    }

    /// Under the chips, while one is being noted: the field for your note
    /// on it, going back to where it was quoted from, and copying the
    /// chips to paste in another draft.
    fn note_row(&self, cx: &mut Context<Self>) -> Option<Div> {
        use crate::desk::mention::{Attached, Chip};
        let n = self.composer.noting?;
        let Some(Attached::Text(chip)) = self.composer.attached.get(n) else {
            return None;
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .pt(px(8.))
                .child(
                    div()
                        .flex_1()
                        .child(gpui::component::input::Input::new(&self.composer.note).small()),
                )
                .when(matches!(chip, Chip { from: Some(_), .. }), |d| {
                    d.child(
                        ui::button("chip-back", crate::tr("Show where"), ui::Weight::Quiet)
                            .h(px(24.))
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.go_back(n, window, cx)),
                            ),
                    )
                })
                .child(
                    ui::button("chips-copy", crate::tr("Copy chips"), ui::Weight::Quiet)
                        .h(px(24.))
                        .on_click(cx.listener(|this, _, _, cx| this.copy_chips(cx))),
                ),
        )
    }

    /// Over the line, where a quote of a message of a thread goes with
    /// it: "Replying to" its first words, which go back to it.
    fn replying_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<gpui::Stateful<Div>> {
        use crate::desk::mention::{Attached, Chip};
        let (n, text) = (self.composer.attached.iter().enumerate()).find_map(|(n, a)| match a {
            Attached::Text(Chip {
                kind: crate::session::Context::Quote,
                from: Some(_),
                text,
                ..
            }) => Some((n, text.clone())),
            _ => None,
        })?;
        Some(
            div()
                .id("replying")
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .pt(px(8.))
                .cursor_pointer()
                .child(ui::icon("reply", 12.).text_color(theme::faint()))
                .child(ui::one_line(ui::fine(
                    faces,
                    crate::tr_with(
                        "Replying to “%{said}”",
                        &[(
                            "said",
                            &ui::fit(text.lines().next().unwrap_or_default(), 60),
                        )],
                    ),
                )))
                .on_click(cx.listener(move |this, _, window, cx| this.go_back(n, window, cx))),
        )
    }

    /// What was said while the turn goes on, waiting its turn: each can
    /// be said now (the turn is stopped first), edited in place, or removed.
    fn queued_list(
        &self,
        faces: &Faces,
        s: &Session,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
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
                        crate::tr("1 message queued")
                    } else {
                        crate::tr_with("%{count} messages queued", &[("count", &count)])
                    },
                ));
            for (n, text) in s.meta.queued.iter().enumerate() {
                let editing = self.composer.editing_queued == Some(n);
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .when(editing, |d| d.px(px(6.)).py(px(2.)).bg(theme::role(u0ui::tokens::ColorRole::StateCurrentBackground)))
                        .child(ui::icon("clock", 12.).text_color(theme::faint()))
                        .child(
                            ui::one_line(
                                ui::text(faces, text.clone())
                                    .text_size(px(crate::style::CAPTION))
                                    .text_color(theme::text2()),
                            )
                            .flex_1(),
                        )
                        .when(editing, |d| {
                            d.child(
                                ui::text(faces, crate::tr("Editing in the composer"))
                                    .text_size(px(crate::style::CAPTION))
                                    .text_color(theme::link()),
                            )
                        })
                        .when(n > 0 && !editing, |d| {
                            d.child(
                                ui::icon_button(
                                    SharedString::from(format!("queue-up-{n}")),
                                    "arrow-up",
                                    crate::tr("Earlier"),
                                )
                                .size(px(22.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.queue_earlier(n, cx)),
                                ),
                            )
                        })
                        .when(!editing, |d| {
                            d.child(
                                ui::button(
                                    SharedString::from(format!("steer-{n}")),
                                    crate::tr("Send now"),
                                    ui::Weight::Quiet,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| this.steer(n, cx))),
                            )
                            .child(
                                ui::icon_button(
                                    SharedString::from(format!("unqueue-{n}")),
                                    "pencil",
                                    crate::tr("Edit"),
                                )
                                .size(px(22.))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.unqueue(n, window, cx);
                                    },
                                )),
                            )
                        })
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("queue-rm-{n}")),
                                "x",
                                crate::tr("Remove from queue"),
                            )
                            .size(px(22.))
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_queued(n, cx))),
                        ),
                );
            }
            list
        })
    }

    /// What a subagent of this session asks before it goes on, answered
    /// here as in its own thread.
    fn children_asking(
        &self,
        faces: &Faces,
        s: &Session,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        (self.sessions.all.iter())
            .filter(|c| c.parent == Some(s.key))
            .filter_map(|c| {
                let asked = c.waiting()?;
                let key = format!("child-{}", c.key);
                Some(
                    div()
                        .w(px(column))
                        .mb(px(8.))
                        .child(
                            ui::text(
                                faces,
                                crate::tr_with(
                                    "Its subagent “%{title}” asks",
                                    &[("title", &ui::fit(&c.meta.title, 40))],
                                ),
                            )
                            .pb(px(4.))
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::faint()),
                        )
                        .child(self.asked(faces, c, &key, asked, cx)),
                )
            })
            .collect()
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
                        crate::tr_with(
                            "A side question of “%{title}”: it does not go in it.",
                            &[("title", &title)],
                        ),
                    )
                    .flex_1()
                    .text_size(px(crate::style::LABEL))
                    .text_color(theme::text2()),
                )
                .when(answered, |d| {
                    d.child(
                        ui::button(
                            "answer-back",
                            crate::tr("Add the answer to it"),
                            ui::Weight::Secondary,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.answer_back(key, window, cx);
                            },
                        )),
                    )
                })
                .child(
                    ui::button("beside-back", crate::tr("Back to it"), ui::Weight::Quiet).on_click(
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

/// Room above and below the line in the box: with the line's 28 and the
/// border, the box is Claude's 44 high.
const LINE_PAD: f32 = 7.;

/// A menu of the composer's at its left.
pub(super) fn over_line(menu: Popover) -> Popover {
    over_box(menu, gpui::Anchor::BottomLeft)
}

/// What a chip under the box says.
fn chip_text(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .flex_none()
        .text_size(px(crate::style::LABEL))
        .text_color(theme::text2())
}
