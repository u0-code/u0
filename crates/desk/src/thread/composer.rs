//! Carbon `ThreadComposer` (`ThreadComposer.dc.html` and `ComposerStates.dc.html`):
//! notices (`ComposerBannerStack`), changed-files bar, `/` and `@` suggestion
//! menu (`ComposerCommandMenu`), queued follow-ups (`.tc-later`), live
//! dictation bar (`.tc-dict`), attached/mentioned pills (`.tc-chips`), the
//! square `field-01` message box with 32px context/attachment chips, ghost
//! suggestion (`Tab` to accept), and the 40px Carbon footer (`+`, model,
//! effort, mode, context meter, dictation and `ComposerPrimaryActions`).

use gpui::component::{
    Selectable as _, Sizable as _,
    input::{Enter, Escape, IndentInline, Input as KitInput, MoveDown, MoveUp, Textarea},
    menu::PopupMenu,
    popover::Popover,
    progress::ProgressCircle,
    speech::{SpeechButton, SpeechIndicator, SpeechStatus},
};
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};
use u0ui::{
    components::{
        button::{Button as CarbonButton, ButtonSize, ButtonVariant},
        kbd::Kbd,
    },
    icons::{CarbonIcon, Icon as CarbonGlyph},
    t3::composer::{
        AttachmentImage, AttachmentImageState, AttachmentTexts, BannerKind, CommandMenuItem,
        CommandMenuSection, ComposerAttachmentStrip, ComposerBanner, ComposerBannerStack,
        ComposerCommandMenu, ComposerPrimaryActions, ComposerState, ContextChip, ContextChipKind,
        PrimaryActionTexts, ReadOnlyComposerBar,
    },
    tokens::ColorRole,
    u0::composer::{ChipKind, ComposerChip},
};

use crate::{
    desk::{Desk, Popup, mention},
    fonts::Faces,
    keys::{EditQueued, SendAside, Stash},
    session::{Context as TextKind, Session, State},
    theme, ui,
};

impl Desk {
    /// Carbon `ThreadComposer`: notices and menus above, `.tc` stack in the
    /// middle, and workspace/worktree/stash/later controls underneath.
    pub(crate) fn composer(
        &self,
        faces: &Faces,
        column: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let unbegun = self.unbegun();
        let Some(s) = self.session().or(unbegun.as_ref()) else {
            return div();
        };
        if let Some(shut) = self.shut(faces, s, column, cx) {
            return shut;
        }
        let name = self.name_of(&s.meta.agent);
        let working = matches!(s.state, State::Working | State::Opening);
        let waiting_approval = s.waiting().is_some();
        let suggested = self.suggestion(s.key, cx).map(SharedString::from);
        let offline = self
            .machine(s.host)
            .filter(|m| m.up != crate::hub::Up::On)
            .map(|m| m.name.clone());
        let placeholder = if let Some(server) = offline {
            crate::tr_with("Sent once %{name} is back", &[("name", &server)])
        } else if suggested.is_some() {
            SharedString::default()
        } else if waiting_approval {
            crate::tr("Reply, or answer the card above")
        } else if working {
            crate::tr("Waits its turn, or ⇧⌘↵ steers")
        } else if s.items.is_empty() {
            crate::tr("Ask for a change. @ to mention, / for commands")
        } else {
            crate::tr_with(
                "Reply to %{name}. @ to mention, / for commands",
                &[("name", &name)],
            )
        };
        self.composer.input.update(cx, |i, cx| {
            if *i.presentation().placeholder() != placeholder {
                i.set_placeholder(placeholder, window, cx);
            }
        });

        let focused =
            gpui::Focusable::focus_handle(self.composer.input.read(cx), cx).is_focused(window);
        let offers = self.offers_menu(s, &name, column, cx);
        let has_offers = offers.is_some();
        let banners = self.banners_above(s, &name, column, cx);

        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .pb(px(16.))
            .when(self.current.is_none(), |d| {
                d.capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                    this.begin_unbegun(window, cx);
                }))
            })
            .children(self.children_asking(faces, s, column, cx))
            .children(banners)
            .children(self.changes_row(faces, s, column, cx))
            .children(offers)
            .children(self.also_run_row(faces, s, column, cx))
            .child(self.tc_stack(faces, s, &name, column, focused, has_offers, suggested, cx))
            .child(self.workspace_strip(faces, s, column, cx))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the composer stack coordinates session, focus, offer guard and ghost suggestion"
    )]
    fn tc_stack(
        &self,
        faces: &Faces,
        s: &Session,
        agent_name: &str,
        column: f32,
        focused: bool,
        has_offers: bool,
        suggested: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Div {
        let ctx_strip = self.attached_row(cx);
        let has_ctx = ctx_strip.is_some();
        let pad_top = if has_ctx { 10. } else { 14. };
        let frame = theme::role(if focused {
            ColorRole::Focus
        } else {
            ColorRole::BorderSubtle01
        });
        let suggest_overlay = suggested.map(|next| {
            let accept_text = next.to_string();
            div()
                .id("composer-suggest")
                .absolute()
                .top(px(pad_top))
                .left(px(16.))
                .right(px(16.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_text()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.composer.suggested = None;
                    this.put_line(&accept_text, window, cx);
                    let fh = gpui::Focusable::focus_handle(this.composer.input.read(cx), cx);
                    window.focus(&fh, cx);
                }))
                .child(
                    ui::text(faces, next)
                        .min_w(px(0.))
                        .truncate()
                        .italic()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(theme::role(ColorRole::TextSecondary)),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(12.))
                        .text_color(theme::role(ColorRole::TextHelper))
                        .child(Kbd::new("Tab"))
                        .child(crate::tr("to accept")),
                )
        });

        let dictate: AnyElement = if let Some(sp) = &self.speech {
            SpeechButton::new(&sp.state)
                .with_size(px(32.))
                .into_any_element()
        } else {
            let desk_mic = cx.entity();
            CarbonButton::new(
                "composer-mic",
                crate::keys::hint(cx, &crate::tr("Dictate"), "dictate"),
            )
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Sm)
            .icon(CarbonIcon::Microphone)
            .icon_only(true)
            .on_click(move |_, window, cx| {
                desk_mic.update(cx, |this, cx| this.dictate(window, cx));
            })
            .into_any_element()
        };

        let foot_left = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .min_w(px(0.))
            .overflow_hidden()
            .child(self.add_menu(s, cx))
            .child(self.model_chip(faces, s, cx))
            .children(self.effort_chip(s, cx))
            .children(self.mode_chip(faces, s, cx));

        let foot_right = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .children(self.context_meter(faces, s, cx))
            .child(dictate)
            .child(self.primary_actions(s, has_offers, cx));

        let box_el = div()
            .id("composer")
            .flex()
            .flex_col()
            .w_full()
            .rounded(px(0.))
            .bg(theme::role(ColorRole::Field01))
            .border_1()
            .border_color(frame)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                    this.drop_files(paths.paths(), window, cx);
                }),
            )
            .on_drop(
                cx.listener(|this, d: &crate::sidebar::Dragged, window, cx| {
                    this.drop_session(d.key, window, cx);
                }),
            )
            .children(self.replying_row(faces, cx))
            .children(ctx_strip)
            .children(self.chip_preview_row(faces, cx))
            .children(self.note_row(faces, cx))
            .child(
                div()
                    .relative()
                    .pt(px(pad_top - 2.))
                    .pb(px(6.))
                    .px(px(12.))
                    .child(self.composer_line(cx))
                    .children(suggest_overlay),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .min_h(px(40.))
                    .px(px(8.))
                    .pb(px(8.))
                    .child(foot_left)
                    .child(foot_right),
            );

        div()
            .flex()
            .flex_col()
            .w(px(column))
            .gap(px(8.))
            .children(self.queued_list(s, agent_name, cx))
            .children(self.dictation_row(faces, cx))
            .children(self.mention_chips(faces, s, cx))
            .child(box_el)
    }

    /// `.tc-dict` (`ThreadComposer.dc.html`): Apple's on-device speech bar, or
    /// the Writer tidying punctuation and names.
    fn dictation_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        let d = self.dictation.as_ref()?;
        let speech = &self.speech.as_ref()?.state;
        let listening = speech.read(cx).status() == SpeechStatus::Recording;
        let cleaning = d.cleaning.is_some();
        let (word, detail, action_label, finish_on_click) = if cleaning {
            (
                crate::tr("Tidying"),
                crate::tr("The Writer fixes punctuation and names"),
                crate::tr("Skip"),
                false,
            )
        } else if listening {
            let text = if d.heard.is_empty() {
                crate::tr("Listening…")
            } else {
                SharedString::from(d.heard.clone())
            };
            (crate::tr("Listening"), text, crate::tr("Done"), true)
        } else {
            let text = if d.heard.is_empty() {
                crate::tr("Waiting for speech recognition…")
            } else {
                SharedString::from(d.heard.clone())
            };
            (crate::tr("Listening"), text, crate::tr("Cancel"), false)
        };
        let desk_act = cx.entity();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(36.))
                .pl(px(12.))
                .pr(px(4.))
                .bg(theme::role(ColorRole::Layer01))
                .border_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(if listening {
                    SpeechIndicator::new(speech)
                        .with_size(px(16.))
                        .into_any_element()
                } else {
                    CarbonGlyph::carbon(CarbonIcon::Microphone)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::IconPrimary))
                        .flex_none()
                        .into_any_element()
                })
                .child(
                    ui::text(faces, word)
                        .flex_none()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(
                    ui::text(faces, detail)
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(theme::role(ColorRole::TextSecondary)),
                )
                .child(
                    CarbonButton::new("dictation-end", action_label)
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            desk_act.update(cx, |this, cx| {
                                if finish_on_click {
                                    this.finish_dictation(cx);
                                } else {
                                    this.cancel_dictation(window, cx);
                                }
                            });
                        }),
                ),
        )
    }

    /// `.tc-chips` (`ThreadComposer.dc.html`): extra folders (`s.meta.also`)
    /// and `@` mentions currently named in the composer line.
    fn mention_chips(&self, faces: &Faces, s: &Session, cx: &mut Context<Self>) -> Option<Div> {
        use crate::desk::mention::Offer;
        let typed = self.composer.input.read(cx).value().to_string();
        let draft = s.is_draft();
        let key = s.key;
        let mut pills: Vec<AnyElement> = Vec::new();

        for (n, folder) in s.meta.also.iter().enumerate() {
            let label = Desk::shown(folder);
            let remove_label = crate::tr_with("Remove %{name}", &[("name", &label)]);
            pills.push(tc_pill(
                faces,
                SharedString::from(format!("tc-also-{n}")),
                CarbonIcon::Folder,
                label,
                draft.then(|| {
                    let desk = cx.entity();
                    (
                        remove_label,
                        Box::new(move |_: &mut Window, cx: &mut gpui::App| {
                            desk.update(cx, |this, cx| {
                                if let Some(sess) = this.sessions.at_mut(key)
                                    && n < sess.meta.also.len()
                                {
                                    sess.meta.also.remove(n);
                                }
                                cx.notify();
                            });
                        }) as Box<dyn Fn(&mut Window, &mut gpui::App)>,
                    )
                }),
            ));
        }

        for (n, m) in self.composer.mentions.iter().enumerate() {
            if !typed.contains(&m.token) {
                continue;
            }
            let (icon, label) = match &m.what {
                Offer::File(f) => (CarbonIcon::Document, f.clone()),
                Offer::Session { title, .. } => (CarbonIcon::Chat, ui::fit(title, 32)),
                Offer::Pr { number, title } => (
                    CarbonIcon::PullRequest,
                    format!("#{number} {}", ui::fit(title, 24)),
                ),
                Offer::Issue { number, title } => (
                    CarbonIcon::Ticket,
                    format!("#{number} {}", ui::fit(title, 24)),
                ),
                Offer::Model { label, .. } => (CarbonIcon::Bot, format!("@{label}")),
                Offer::Command { .. } | Offer::Own { .. } => continue,
            };
            let remove_label = crate::tr_with("Remove %{name}", &[("name", &label)]);
            let tok = m.token.clone();
            let desk = cx.entity();
            pills.push(tc_pill(
                faces,
                SharedString::from(format!("tc-mention-{n}")),
                icon,
                label,
                Some((
                    remove_label,
                    Box::new(move |window: &mut Window, cx: &mut gpui::App| {
                        desk.update(cx, |this, cx| {
                            let cur = this.composer.input.read(cx).value().to_string();
                            let cleaned = cur
                                .replace(&format!("{tok} "), "")
                                .replace(&tok, "")
                                .trim_start()
                                .to_owned();
                            this.put_line(&cleaned, window, cx);
                            if n < this.composer.mentions.len() {
                                this.composer.mentions.remove(n);
                            }
                            cx.notify();
                        });
                    }),
                )),
            ));
        }

        (!pills.is_empty()).then(|| {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(4.))
                .children(pills)
        })
    }

    /// The multi-line input itself, preserving all keyboard actions and paste
    /// handling (`Stash`, `SendAside`, `EditQueued`, list continuation, `Tab`
    /// suggestion accept, `Escape`, `↑`/`↓` prompt recall, image/long paste).
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
                } else if this.composer.editing_queued.is_some() {
                    this.drop_queue_edit(cx);
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
                Textarea::new(&self.composer.input)
                    .small()
                    .appearance(false)
                    .on_paste(move |item, window, cx| {
                        !window.modifiers().shift
                            && desk
                                .update(cx, |this, cx| this.paste(item, window, cx))
                                .unwrap_or(false)
                    }),
            )
    }

    fn suggestion<'a>(&'a self, key: u64, cx: &gpui::App) -> Option<&'a str> {
        let (k, next) = self.composer.suggested.as_ref()?;
        (*k == key && self.composer.input.read(cx).value().is_empty()).then_some(next.as_str())
    }

    /// Read-only composer bar (`ReadOnlyComposerBar`) for archived or subagent
    /// threads.
    fn shut(&self, faces: &Faces, s: &Session, column: f32, cx: &mut Context<Self>) -> Option<Div> {
        if let Some(m) = self
            .machine(s.host)
            .filter(|m| m.host != crate::hub::Host::HERE && m.scope == crate::access::Scope::Read)
        {
            let placeholder = crate::tr_with(
                "One suggestion at a time. You can write the next one after %{host} answers.",
                &[("host", &m.name)],
            );
            let note = crate::tr_with(
                "Read only: you watch the threads, diffs and files of %{host}'s projects, and change nothing. Permission cards, terminals and settings stay with %{host}.",
                &[("host", &m.name)],
            );
            return Some(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .items_center()
                    .pb(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .w(px(column))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .bg(theme::role(u0ui::tokens::ColorRole::Layer01))
                                    .child(
                                        div()
                                            .min_h(px(64.))
                                            .px(px(16.))
                                            .pt(px(12.))
                                            .pb(px(8.))
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(theme::role(
                                                u0ui::tokens::ColorRole::TextDisabled,
                                            ))
                                            .child(placeholder),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .h(px(36.))
                                            .px(px(16.))
                                            .text_size(px(12.))
                                            .text_color(theme::role(
                                                u0ui::tokens::ColorRole::TextDisabled,
                                            ))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.))
                                                    .child(crate::tr("Attach"))
                                                    .child(
                                                        u0ui::Icon::carbon(
                                                            u0ui::icons::CarbonIcon::Add,
                                                        )
                                                        .size_length(px(16.))
                                                        .color(theme::role(
                                                            u0ui::tokens::ColorRole::IconDisabled,
                                                        )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .child(crate::tr("Up to 4 files · 10 MB")),
                                            )
                                            .child(crate::tr("Send")),
                                    ),
                            )
                            .child(
                                ui::text(faces, note)
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(theme::role(u0ui::tokens::ColorRole::TextHelper)),
                            ),
                    ),
            );
        }
        let bar = if s.meta.flags.archived {
            let key = s.key;
            let desk = cx.entity();
            ReadOnlyComposerBar::new(crate::tr("Archived. Restore it to write in it again."))
                .action(
                    CarbonButton::new("restore", crate::tr("Restore"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk.update(cx, |this, cx| this.restore(key, cx));
                        }),
                )
        } else {
            let up = s.parent?;
            let parent = self
                .sessions
                .at(up)
                .map(|p| ui::fit(&p.meta.title, 40))
                .unwrap_or_default();
            let desk = cx.entity();
            ReadOnlyComposerBar::new(crate::tr_with(
                "A subagent of “%{parent}”: it answers to it.",
                &[("parent", &parent)],
            ))
            .action(
                CarbonButton::new("to-parent", crate::tr("Back to it"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, window, cx| {
                        desk.update(cx, |this, cx| this.show(up, window, cx));
                    }),
            )
        };
        Some(
            div()
                .flex()
                .flex_none()
                .justify_center()
                .pb(px(18.))
                .child(div().w(px(column)).child(bar)),
        )
    }

    /// Context window meter (`14px` ring + tabular `%`), or red character
    /// count when over `mention::LIMIT`.
    fn context_meter(&self, faces: &Faces, s: &Session, cx: &gpui::App) -> Option<AnyElement> {
        let chars = self.typed(cx).chars().count();
        if chars > mention::LIMIT {
            let text = format!("{}k / {}k", chars / 1000, mention::LIMIT / 1000);
            return Some(
                div()
                    .id("context-over")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .px(px(6.))
                    .child(
                        div()
                            .size(px(10.))
                            .rounded_full()
                            .bg(theme::role(ColorRole::SupportError)),
                    )
                    .child(
                        ui::mono(faces, text)
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::role(ColorRole::TextPrimary)),
                    )
                    .into_any_element(),
            );
        }
        let u = s.usage.as_ref().filter(|u| u.size > 0)?;
        let share = (u.used as f32 / u.size as f32).clamp(0., 1.);
        let pct = (share * 100.).round() as u32;
        Some(
            div()
                .id("context-used")
                .flex()
                .flex_none()
                .items_center()
                .gap(px(5.))
                .px(px(6.))
                .h(px(32.))
                .child(
                    ProgressCircle::new("context-ring")
                        .value(share * 100.)
                        .color(if share >= 0.9 {
                            theme::role(ColorRole::SupportError)
                        } else {
                            theme::role(ColorRole::IconSecondary)
                        })
                        .size(px(14.)),
                )
                .child(
                    ui::mono(faces, format!("{pct}%"))
                        .text_size(px(12.))
                        .text_color(theme::role(ColorRole::TextSecondary)),
                )
                .tooltip(ui::tip(crate::tr_with(
                    "%{share}% of the context used",
                    &[("share", &pct)],
                )))
                .into_any_element(),
        )
    }

    /// Primary Send / Steer / Queue / Stop controls (`ComposerPrimaryActions`).
    fn primary_actions(&self, s: &Session, has_offers: bool, cx: &mut Context<Self>) -> AnyElement {
        let working = matches!(s.state, State::Working | State::Opening);
        let typed = self.typed(cx);
        let empty = typed.is_empty();
        let over = typed.chars().count() > mention::LIMIT;
        let key = s.key;
        let force = working
            && self
                .stopping
                .is_some_and(|(k, t)| k == key && t.elapsed() > std::time::Duration::from_secs(8));
        if force {
            let desk = cx.entity();
            return div()
                .id("force-stop-wrap")
                .tooltip(ui::tip(crate::tr(
                    "It would not stop: its process is ended, and started again when wanted",
                )))
                .child(
                    CarbonButton::new("force-stop", crate::tr("Force stop"))
                        .variant(ButtonVariant::Danger)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk.update(cx, |this, cx| this.force_stop(key, cx));
                        }),
                )
                .into_any_element();
        }
        // Editing a queued message: Return saves it back in its place.
        // A new thread run on several agents: one button says how many.
        let special = if self.composer.editing_queued.is_some() && !empty {
            Some(("save-queued", crate::tr("Save to queue")))
        } else if s.is_draft() && !self.composer.draft.also_run.is_empty() && !empty && !over {
            let total = self.composer.draft.also_run.len() + 1;
            Some((
                "run-many",
                crate::tr_with("Run %{n} agents", &[("n", &total)]),
            ))
        } else {
            None
        };
        if let Some((id, label)) = special {
            let desk = cx.entity();
            return CarbonButton::new(id, label)
                .variant(ButtonVariant::Primary)
                .size(ButtonSize::Sm)
                .on_click(move |_, window, cx| {
                    desk.update(cx, |this, cx| this.send_typed(false, window, cx));
                })
                .into_any_element();
        }

        let other = if self.return_sends(cx) {
            "⌘Return"
        } else {
            "⇧⌘Return"
        };
        let texts = PrimaryActionTexts {
            send: crate::tr_with(
                "Send · %{keys} to go on in the background",
                &[("keys", &other)],
            ),
            steer: crate::tr("Steer"),
            steer_title: crate::tr("Interrupt the current step and send now"),
            queue: crate::tr("Queue"),
            queue_title: crate::tr("Send after this turn finishes"),
            stop: crate::tr("Stop"),
            stop_title: crate::keys::hint(cx, &crate::tr("Stop the turn"), "stop").into(),
            stop_short: crate::keys::hint(cx, &crate::tr("Stop the turn"), "stop").into(),
            ..PrimaryActionTexts::default()
        };
        let state = if working {
            ComposerState::Running
        } else {
            ComposerState::Idle
        };
        let disabled = empty || over || has_offers || self.dictation.is_some();
        let desk_send = cx.entity();
        let desk_queue = cx.entity();
        let desk_steer = cx.entity();
        let desk_stop = cx.entity();
        ComposerPrimaryActions::new("composer-primary", state)
            .texts(texts)
            .disabled(disabled)
            .empty(empty || over || has_offers || self.dictation.is_some())
            .on_send(move |window, cx| {
                desk_send.update(cx, |this, cx| this.send_typed(false, window, cx));
            })
            .on_queue(move |window, cx| {
                desk_queue.update(cx, |this, cx| {
                    let flip = this.settings.steer;
                    this.send_typed(flip, window, cx);
                });
            })
            .on_steer(move |window, cx| {
                desk_steer.update(cx, |this, cx| {
                    let flip = !this.settings.steer;
                    this.send_typed(flip, window, cx);
                });
            })
            .on_stop(move |_, cx| {
                desk_stop.update(cx, Self::stop);
            })
            .into_any_element()
    }

    /// Compact workspace strip below the composer box: project folder,
    /// worktree toggle / base branch, stashed prompts (`⌘S`) and scheduled
    /// prompts (`Say it later`).
    fn workspace_strip(
        &self,
        faces: &Faces,
        s: &Session,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let draft = s.is_draft();
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .w(px(column))
            .h(px(28.))
            .mt(px(6.))
            .child(self.project_chip(faces, s, draft, cx));
        if draft && s.git_dir().is_some() {
            let on = s.wants_worktree;
            row = row.child(
                ui::opener("worktree")
                    .h(px(26.))
                    .rounded(px(0.))
                    .selected(on)
                    .child(
                        CarbonGlyph::carbon(CarbonIcon::Branch)
                            .size_length(px(14.))
                            .color(theme::role(ColorRole::IconSecondary)),
                    )
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
            if !self.composer.draft.also_run.is_empty() {
                let total = self.composer.draft.also_run.len() + 1;
                let base = match &s.begin {
                    Some(crate::session::Begin::Branch(b)) => b.clone(),
                    Some(crate::session::Begin::Pr(n, _)) => format!("#{n}"),
                    None => s.branch.clone().unwrap_or_else(|| "HEAD".into()),
                };
                row = row.child(chip_text(
                    faces,
                    crate::tr_with(
                        "%{n} worktrees from %{branch}, one per agent",
                        &[("n", &total), ("branch", &base)],
                    ),
                ));
            }
        } else if let Some(branch) = &s.branch {
            row = row.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .child(
                        CarbonGlyph::carbon(CarbonIcon::Branch)
                            .size_length(px(14.))
                            .color(theme::role(ColorRole::IconSecondary)),
                    )
                    .child(chip_text(faces, ui::fit(branch, 28))),
            );
        }
        if draft {
            let bg_on = self.composer.draft.in_background;
            row = row.child(
                ui::opener("start-bg")
                    .h(px(26.))
                    .rounded(px(0.))
                    .selected(bg_on)
                    .child(chip_text(faces, crate::tr("Start in the background")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.composer.draft.in_background = !this.composer.draft.in_background;
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
        row
    }

    fn project_chip(
        &self,
        faces: &Faces,
        s: &Session,
        draft: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let named = self.project_on(&crate::desk::Place::project_of(s));
        let says = |chevron: bool| {
            ui::opener("project-chip")
                .h(px(26.))
                .rounded(px(0.))
                .child(
                    CarbonGlyph::carbon(CarbonIcon::Folder)
                        .size_length(px(14.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
                .child(chip_text(faces, ui::fit(&named, 28)))
                .when(chevron, |b| {
                    b.child(
                        CarbonGlyph::carbon(CarbonIcon::ChevronDown)
                            .size_length(px(12.))
                            .color(theme::role(ColorRole::IconSecondary)),
                    )
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

    fn work_in_menu(&self, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        let here = self.session().map(crate::desk::Place::folder_of);
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

    /// `+` attach trigger in the Carbon footer left group.
    fn add_menu(&self, s: &Session, cx: &mut Context<Self>) -> Popover {
        let button = ui::opener("add")
            .size(px(32.))
            .px(px(0.))
            .rounded(px(0.))
            .justify_center()
            .child(
                CarbonGlyph::carbon(CarbonIcon::Add)
                    .size_length(px(16.))
                    .color(theme::role(ColorRole::IconPrimary)),
            )
            .tooltip(crate::tr("Add files, a window, a command"));
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
        .offset(px(10.))
    }

    /// Multi-model fanout strip ("Also run on") above a draft's composer.
    fn also_run_row(
        &self,
        faces: &Faces,
        s: &Session,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !s.is_draft() || self.composer.draft.also_run.is_empty() {
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
        for (n, (agent, model)) in self.composer.draft.also_run.iter().enumerate() {
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
        let Some(mention::Attached::Text(chip)) = self.composer.draft.attached.get(n) else {
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

    /// Notices above the composer (`ComposerBannerStack` + `ComposerBanner`).
    fn banners_above(
        &self,
        s: &Session,
        agent_name: &str,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let mut banners: Vec<AnyElement> = Vec::new();

        if self.composer.draft.from_link {
            let desk = cx.entity();
            banners.push(
                ComposerBanner::new("banner-prompt-link", BannerKind::Info)
                    .title(crate::tr("Opened from a prompt link"))
                    .body(crate::tr(
                        "Review it before sending — nothing runs until you send it.",
                    ))
                    .on_dismiss(move |_, cx| {
                        desk.update(cx, |this, cx| {
                            this.composer.draft.from_link = false;
                            cx.notify();
                        });
                    })
                    .into_any_element(),
            );
        }

        if let Some(up) = s.beside {
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
            let mut banner = ComposerBanner::new("banner-beside", BannerKind::Info)
                .icon(CarbonIcon::Chat)
                .body(crate::tr_with(
                    "A side question of “%{title}”: it does not go in it.",
                    &[("title", &title)],
                ));
            if answered {
                let desk_add = cx.entity();
                banner = banner.action(
                    CarbonButton::new("answer-back", crate::tr("Add the answer to it"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            desk_add.update(cx, |this, cx| this.answer_back(key, window, cx));
                        }),
                );
            }
            let desk_back = cx.entity();
            banner = banner.action(
                CarbonButton::new("beside-back", crate::tr("Back to it"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, window, cx| {
                        desk_back.update(cx, |this, cx| this.show(up, window, cx));
                    }),
            );
            banners.push(banner.into_any_element());
        }

        // Taken over from a terminal that still has it open: both can
        // write in it, and neither sees what the other says.
        if s.elsewhere && s.parent.is_none() {
            let mut banner = ComposerBanner::new("banner-elsewhere", BannerKind::Warning)
                .icon(CarbonIcon::Terminal)
                .title(crate::tr_with(
                    "%{name} has this thread open elsewhere too",
                    &[("name", &agent_name)],
                ))
                .body(crate::tr("Another process, such as a terminal, runs it as well. What you send here goes on in u0, and that process does not see it: write in one place at a time, or branch off a copy to go on apart."));
            if self.can_fork(s) {
                let (desk, key) = (cx.entity(), s.key);
                banner = banner.action(
                    CarbonButton::new("elsewhere-fork", crate::tr("Branch off a copy"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk.update(cx, |this, cx| this.fork(key, cx));
                        }),
                );
            }
            banners.push(banner.into_any_element());
        }

        if let Some(crate::hub::Auth::Needed(why)) =
            self.slot(s.host, &s.meta.agent).map(|x| &x.auth)
        {
            let (host, a, b) = (s.host, s.meta.agent.clone(), s.meta.agent.clone());
            let desk_in = cx.entity();
            let desk_re = cx.entity();
            banners.push(
                ComposerBanner::new("banner-signin", BannerKind::Warning)
                    .title(crate::tr_with(
                        "%{name} needs you to sign in",
                        &[("name", &agent_name)],
                    ))
                    .body(why.clone())
                    .action(
                        CarbonButton::new("sign-in-now", crate::tr("Sign in"))
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_in.update(cx, |this, cx| this.sign_in_agent(host, &a, cx));
                            }),
                    )
                    .action(
                        CarbonButton::new("signed-in", crate::tr("I've signed in"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_re.update(cx, |this, cx| this.recheck_auth(host, &b, cx));
                            }),
                    )
                    .into_any_element(),
            );
        }

        if let Some(goal) = &s.goal {
            let (label, said) = if goal.paused {
                ("Resume", "/goal resume")
            } else {
                ("Pause", "/goal pause")
            };
            let desk_toggle = cx.entity();
            let desk_clear = cx.entity();
            banners.push(
                ComposerBanner::new("banner-goal", BannerKind::Info)
                    .icon(CarbonIcon::CheckmarkOutline)
                    .title(crate::tr(if goal.paused { "Goal, paused" } else { "Goal" }))
                    .body(goal.objective.clone())
                    .action(
                        CarbonButton::new("goal-toggle", crate::tr(label))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_toggle.update(cx, |this, cx| this.say(said, cx));
                            }),
                    )
                    .action(
                        CarbonButton::new("goal-clear", crate::tr("Clear"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_clear.update(cx, |this, cx| this.say("/goal clear", cx));
                            }),
                    )
                    .into_any_element(),
            );
        }

        if self.composer.held.is_some() {
            let n = self.composer.reading;
            let desk = cx.entity();
            banners.push(
                ComposerBanner::new("banner-held", BannerKind::Progress)
                    .title(if n == 1 {
                        crate::tr("Sending once the picture is ready")
                    } else {
                        crate::tr_with("Sending once %{n} pictures are ready", &[("n", &n)])
                    })
                    .body(crate::tr("Edit, or press Escape, to keep it back"))
                    .on_dismiss(move |_, cx| {
                        desk.update(cx, |this, cx| {
                            this.composer.held = None;
                            cx.notify();
                        });
                    })
                    .into_any_element(),
            );
        }

        if let (Some(was), Some(now)) = (&s.ran_on, &s.branch)
            && was != now
        {
            banners.push(
                ComposerBanner::new(
                    SharedString::from(format!("banner-branch-{}", s.key)),
                    BannerKind::Warning,
                )
                .title(crate::tr("The branch changed"))
                .body(crate::tr_with(
                    "It last ran on %{was}; what you send now goes on on %{now}.",
                    &[("was", was), ("now", now)],
                ))
                .into_any_element(),
            );
        }

        let count = self.typed(cx).chars().count();
        if count > mention::LIMIT {
            let desk_attach = cx.entity();
            banners.push(
                ComposerBanner::new("banner-over-limit", BannerKind::Error)
                    .title(crate::tr("This message is too long"))
                    .body(crate::tr_with(
                        "%{count} characters: %{over} over the %{limit} a message may hold. Cut it, or send it in parts.",
                        &[
                            ("count", &count),
                            ("over", &(count - mention::LIMIT)),
                            ("limit", &mention::LIMIT),
                        ],
                    ))
                    .action(
                        CarbonButton::new("attach-over-limit", crate::tr("Attach as file"))
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, window, cx| {
                                desk_attach.update(cx, |this, cx| {
                                    this.attach_typed_as_file(window, cx);
                                });
                            }),
                    )
                    .into_any_element(),
            );
        }

        (!banners.is_empty()).then(|| {
            div()
                .w(px(column))
                .mb(px(8.))
                .child(ComposerBannerStack::new("composer-banners").children(banners))
        })
    }

    /// Square Carbon changed-files bar above the composer; closed for
    /// its folder until the changes change (`close_changes`).
    fn changes_row(
        &self,
        faces: &Faces,
        s: &Session,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let files = s.total().filter(|f| !f.is_empty())?;
        let project = crate::desk::Place::project_of(s);
        let closed = (self.settings_on(s.host).prefs.get(&project.folder))
            .and_then(|p| p.closed.changes.get(&s.meta.folder));
        if closed.is_some_and(|m| s.total_mark() == Some(m.as_str())) {
            return None;
        }
        let key = s.key;
        let file_count = files.len();
        let (added, gone) = files
            .iter()
            .fold((0, 0), |(a, g), f| (a + f.added, g + f.gone));
        let count_text = if file_count == 1 {
            crate::tr("1 file changed")
        } else {
            crate::tr_with("%{n} files changed", &[("n", &file_count)])
        };
        let desk_review = cx.entity();
        let desk_commit = cx.entity();
        let desk_close = cx.entity();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .w(px(column))
                .min_h(px(40.))
                .mb(px(8.))
                .pl(px(16.))
                .pr(px(4.))
                .py(px(4.))
                .bg(theme::role(ColorRole::Layer01))
                .border_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(
                    ui::text(faces, count_text)
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(ui::counts(faces, added, gone, 12.))
                .children(s.branch.clone().map(|b| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            CarbonGlyph::carbon(CarbonIcon::Branch)
                                .size_length(px(13.))
                                .color(theme::role(ColorRole::IconSecondary)),
                        )
                        .child(chip_text(faces, ui::fit(&b, 24)))
                }))
                .child(div().flex_1())
                .child(
                    CarbonButton::new("review-changes", crate::tr("Review changes"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk_review.update(cx, |this, cx| {
                                this.open_surface(crate::desk::Surface::Diff, cx);
                            });
                        }),
                )
                .child(
                    CarbonButton::new("commit-changes", crate::tr("Commit changes"))
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            desk_commit.update(cx, |this, cx| {
                                this.open_surface(crate::desk::Surface::Diff, cx);
                                if !this.review.committing {
                                    this.toggle_commit(window, cx);
                                }
                            });
                        }),
                )
                .child(
                    CarbonButton::new("close-changes", crate::tr("Close"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .icon(CarbonIcon::Close)
                        .icon_only(true)
                        .on_click(move |_, _, cx| {
                            desk_close.update(cx, |this, cx| this.close_changes(key, cx));
                        }),
                ),
        )
    }

    /// Close the changes bar of the session `key` until its folder's
    /// changes change; the marks of folders no session here works in any
    /// more are let go.
    fn close_changes(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(s) = self.sessions.at(key) else {
            return;
        };
        let Some(mark) = s.total_mark().map(str::to_owned) else {
            return;
        };
        let (project, folder) = (crate::desk::Place::project_of(s), s.meta.folder.clone());
        let held: std::collections::HashSet<_> = (self.sessions.all.iter())
            .filter(|o| crate::desk::Place::project_of(o) == project)
            .map(|o| o.meta.folder.clone())
            .collect();
        self.set_prefs(&project, cx, |p| {
            p.closed.changes.retain(|f, _| held.contains(f));
            p.closed.changes.insert(folder, mark);
        });
    }

    /// `/` commands and `@` mentions rendered via `u0ui::t3::composer::ComposerCommandMenu`.
    fn offers_menu(
        &self,
        _s: &Session,
        agent_name: &str,
        column: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        use crate::desk::mention::Offer;
        let typed = self.typed(cx);
        let (_, list) = self.offers(&typed).filter(|(_, o)| !o.is_empty())?;
        let chosen = self.composer.offer.min(list.len() - 1);

        // Preserve flat `offers` index ordering across sections so `on_select`
        // maps 1-to-1 onto `take_offer(idx)`.
        let mut sections: Vec<CommandMenuSection> = Vec::new();
        for offer in list {
            let (group_label, icon, title, about) = match offer {
                Offer::Command { name, about } => (
                    crate::tr_with("%{agent}'s commands", &[("agent", &agent_name)]),
                    CarbonIcon::Terminal,
                    format!("/{name}"),
                    about,
                ),
                Offer::Own { name, about } => {
                    let glyph = match name {
                        "clear" => CarbonIcon::Add,
                        "usage-limits" => CarbonIcon::Time,
                        "model" => CarbonIcon::Bot,
                        "branch" => CarbonIcon::Fork,
                        _ => CarbonIcon::Copy,
                    };
                    (
                        crate::tr("u0's own"),
                        glyph,
                        format!("/{name}"),
                        crate::tr(about).to_string(),
                    )
                }
                Offer::Pr { number, title } => (
                    crate::tr("Pull requests"),
                    CarbonIcon::PullRequest,
                    format!("#{number} {title}"),
                    crate::tr("pull request").to_string(),
                ),
                Offer::Model { label, .. } => (
                    crate::tr("Models"),
                    CarbonIcon::Bot,
                    format!("@{label}"),
                    crate::tr("switch to this model").to_string(),
                ),
                Offer::Session { title, .. } => (
                    crate::tr("Threads"),
                    CarbonIcon::Chat,
                    title,
                    crate::tr("thread").to_string(),
                ),
                Offer::Issue { number, title } => (
                    crate::tr("Issues"),
                    CarbonIcon::Ticket,
                    format!("#{number} {title}"),
                    crate::tr("issue").to_string(),
                ),
                Offer::File(f) => (crate::tr("Files"), CarbonIcon::Document, f, String::new()),
            };
            let mut item = CommandMenuItem::new(title).icon(icon);
            if !about.is_empty() {
                item = item.description(about);
            }
            match sections.last_mut() {
                Some(tail_sec) if tail_sec.label == group_label => tail_sec.items.push(item),
                _ => sections.push(CommandMenuSection::new(group_label, vec![item])),
            }
        }

        let desk = cx.entity();
        Some(
            div().w(px(column)).mb(px(8.)).child(
                ComposerCommandMenu::new("composer-offers", sections)
                    .active_index(chosen)
                    .on_select(move |idx, window, cx| {
                        desk.update(cx, |this, cx| this.take_offer(idx, window, cx));
                    }),
            ),
        )
    }

    /// Context & attachment row inside `.t3-composer-box` (`ComposerAttachmentStrip`
    /// for 32px image thumbnails and uploading spinners, `ContextChip` for files
    /// and text excerpts).
    fn attached_row(&self, cx: &mut Context<Self>) -> Option<Div> {
        use crate::desk::mention::Attached;
        if self.composer.draft.attached.is_empty() && self.composer.reading == 0 {
            return None;
        }

        let mut images: Vec<(usize, AttachmentImage)> = Vec::new();
        let mut chips: Vec<AnyElement> = Vec::new();

        for (n, a) in self.composer.draft.attached.iter().enumerate() {
            match a {
                Attached::Image { mime, bytes } => {
                    let src =
                        gpui::ImageSource::Image(std::sync::Arc::new(gpui::Image::from_bytes(
                            gpui::ImageFormat::from_mime_type(mime)
                                .unwrap_or(gpui::ImageFormat::Png),
                            bytes.clone(),
                        )));
                    let name = crate::tr_with("Image %{n}", &[("n", &(n + 1))]);
                    images.push((n, AttachmentImage::new(name).source(src)));
                }
                Attached::File(path) => {
                    let title = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |f| f.to_string_lossy().into_owned(),
                    );
                    let kind = if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                    {
                        ContextChipKind::DocPdf
                    } else {
                        ContextChipKind::File
                    };
                    let size = std::fs::metadata(path)
                        .ok()
                        .map(|m| crate::tidy::shown(m.len()));
                    let desk = cx.entity();
                    let remove_label = crate::tr_with("Remove %{name}", &[("name", &title)]);
                    let mut chip =
                        ContextChip::new(SharedString::from(format!("att-file-{n}")), kind, title)
                            .in_composer(true)
                            .remove_label(remove_label)
                            .on_remove(move |_, cx| {
                                desk.update(cx, |this, cx| this.detach(n, cx));
                            });
                    if let Some(sz) = size {
                        chip = chip.meta(sz);
                    }
                    chips.push(chip.into_any_element());
                }
                Attached::Text(c) => {
                    let kind = match c.kind {
                        TextKind::Quote => ContextChipKind::Citation,
                        TextKind::Terminal => ContextChipKind::Terminal,
                        TextKind::Review => ContextChipKind::ReviewComment,
                        TextKind::Paste => ContextChipKind::File,
                    };
                    let first_line = ui::fit(c.text.lines().next().unwrap_or_default(), 28);
                    let title = if c.note.is_empty() {
                        format!("{} · {first_line}", crate::tr(c.kind.name()))
                    } else {
                        format!("{}: {}", crate::tr(c.kind.name()), ui::fit(&c.note, 24))
                    };
                    let remove_label = crate::tr_with("Remove %{name}", &[("name", &title)]);
                    let desk_click = cx.entity();
                    let desk_rm = cx.entity();
                    let chip =
                        ContextChip::new(SharedString::from(format!("att-text-{n}")), kind, title)
                            .in_composer(true)
                            .remove_label(remove_label)
                            .on_click(move |_, cx| {
                                desk_click.update(cx, |this, cx| this.toggle_chip_preview(n, cx));
                            })
                            .on_remove(move |_, cx| {
                                desk_rm.update(cx, |this, cx| this.detach(n, cx));
                            });
                    chips.push(chip.into_any_element());
                }
            }
        }

        let mut strip_images: Vec<AttachmentImage> =
            images.iter().map(|(_, img)| img.clone()).collect();
        for r in 0..self.composer.reading {
            strip_images.push(
                AttachmentImage::new(SharedString::from(format!("uploading-{r}")))
                    .state(AttachmentImageState::Uploading),
            );
        }

        let image_indices: Vec<(SharedString, usize)> =
            images.into_iter().map(|(ix, img)| (img.name, ix)).collect();
        let opened = image_indices.clone();
        let desk_img = cx.entity();
        let desk_open = cx.entity();

        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .pt(px(14.))
                .px(px(16.))
                .when(!strip_images.is_empty(), |row| {
                    row.child(
                        ComposerAttachmentStrip::new("composer-images", strip_images, Vec::new())
                            .in_composer(true)
                            .texts(AttachmentTexts {
                                remove: std::rc::Rc::new(|name| {
                                    crate::tr_with("Remove %{name}", &[("name", &name)])
                                }),
                                open: std::rc::Rc::new(|name| {
                                    crate::tr_with("Open %{name}", &[("name", &name)])
                                }),
                                ..AttachmentTexts::default()
                            })
                            .on_open(move |name, window, cx| {
                                if let Some(&(_, ix)) = opened.iter().find(|(n, _)| *n == name) {
                                    desk_open
                                        .update(cx, |this, cx| this.view_image(ix, window, cx));
                                }
                            })
                            .on_remove(move |name, _, cx| {
                                if let Some((_, ix)) =
                                    image_indices.iter().find(|(n, _)| *n == name)
                                {
                                    let ix = *ix;
                                    desk_img.update(cx, |this, cx| this.detach(ix, cx));
                                }
                            }),
                    )
                })
                .children(chips),
        )
    }

    /// The attached image `n` at full size, in a dialog that fits the
    /// window; a click outside it or Escape closes it.
    fn view_image(&self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        use gpui::component::WindowExt as _;
        use gpui::{ObjectFit, StyledImage as _};
        let Some(mention::Attached::Image { mime, bytes }) = self.composer.draft.attached.get(n)
        else {
            return;
        };
        let picture = std::sync::Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::from_mime_type(mime.as_str()).unwrap_or(gpui::ImageFormat::Png),
            bytes.clone(),
        ));
        let name = crate::tr_with("Image %{n}", &[("n", &(n + 1))]);
        window.open_dialog(cx, move |dialog, window, _| {
            let room = window.viewport_size();
            let (w, h) = (room.width * 0.8, room.height * 0.8);
            dialog
                .w(w)
                .p_0()
                .rounded_none()
                .border_0()
                .bg(theme::role(ColorRole::Layer01))
                .overlay_closable(true)
                .title(name.clone())
                .child(
                    div().flex().justify_center().p(px(16.)).child(
                        gpui::img(gpui::ImageSource::Image(picture.clone()))
                            .max_w(w - px(32.))
                            .max_h(h - px(96.))
                            .object_fit(ObjectFit::Contain),
                    ),
                )
        });
    }

    /// Expanded excerpt preview (`ComposerChip`) and note field when a text
    /// chip is clicked (`self.composer.noting`).
    fn note_row(&self, _faces: &Faces, cx: &mut Context<Self>) -> Option<Div> {
        use crate::desk::mention::{Attached, Chip};
        let n = self.composer.noting?;
        let Some(Attached::Text(chip)) = self.composer.draft.attached.get(n) else {
            return None;
        };
        let kind = match chip.kind {
            TextKind::Quote => ChipKind::Quote,
            TextKind::Terminal => ChipKind::TerminalOutput,
            TextKind::Review => ChipKind::ReviewComment,
            TextKind::Paste => ChipKind::Quote,
        };
        let has_origin = matches!(chip, Chip { from: Some(_), .. });
        let desk_rm = cx.entity();
        let desk_back = cx.entity();
        let desk_copy = cx.entity();
        let preview = ComposerChip::new("composer-chip-preview", kind, crate::tr(chip.kind.name()))
            .excerpt(chip.text.clone())
            .on_remove(move |_, cx| {
                desk_rm.update(cx, |this, cx| this.detach(n, cx));
            });

        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .px(px(16.))
                .pt(px(10.))
                .child(preview)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .h(px(32.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .bg(theme::role(ColorRole::Field02))
                                .border_b_1()
                                .border_color(theme::role(ColorRole::BorderStrong01))
                                .child(
                                    KitInput::new(&self.composer.note).appearance(false).small(),
                                ),
                        )
                        .when(has_origin, |d| {
                            d.child(
                                CarbonButton::new("chip-back", crate::tr("Show where"))
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(move |_, window, cx| {
                                        desk_back.update(cx, |this, cx| {
                                            this.go_back(n, window, cx);
                                        });
                                    }),
                            )
                        })
                        .child(
                            CarbonButton::new("chips-copy", crate::tr("Copy chips"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    desk_copy.update(cx, Self::copy_chips);
                                }),
                        ),
                ),
        )
    }

    fn replying_row(&self, faces: &Faces, cx: &mut Context<Self>) -> Option<gpui::Stateful<Div>> {
        use crate::desk::mention::{Attached, Chip};
        let (n, text) =
            (self.composer.draft.attached.iter().enumerate()).find_map(|(n, a)| match a {
                Attached::Text(Chip {
                    kind: TextKind::Quote,
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
                .px(px(16.))
                .pt(px(10.))
                .cursor_pointer()
                .child(
                    CarbonGlyph::carbon(CarbonIcon::Quotes)
                        .size_length(px(13.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
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

    /// What waits its turn while the agent works (the board's `.tc-later`,
    /// u0ui's `QueuedRunsControl`): each said in order as the turn ends,
    /// moved earlier, said now (the turn stopped for it), edited in the
    /// composer, or removed unsent.
    fn queued_list(
        &self,
        s: &Session,
        agent_name: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use u0ui::t3::composer::{QueuedRun, QueuedRunsControl, QueuedRunsTexts};
        if s.meta.queued.is_empty() {
            return None;
        }
        let count = s.meta.queued.len();
        let head = if count == 1 {
            crate::tr_with(
                "%{n} waits its turn · sent in order when %{name}'s turn ends",
                &[("n", &count), ("name", &agent_name)],
            )
        } else {
            crate::tr_with(
                "%{n} wait their turn · sent in order when %{name}'s turn ends",
                &[("n", &count), ("name", &agent_name)],
            )
        };
        // The one being edited, by what it said: wherever the queue moved it.
        let edited = (self.composer.editing_queued.as_ref())
            .filter(|(k, _)| *k == s.key)
            .and_then(|(_, was)| s.meta.queued.iter().position(|q| q == was));
        let items = (s.meta.queued.iter().enumerate())
            .map(|(n, text)| QueuedRun::new(text.clone()).editing(edited == Some(n)))
            .collect();
        let (earlier, now, edit, remove) = (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        Some(
            QueuedRunsControl::new("composer-later", items)
                .icon(CarbonIcon::Time)
                .title(head)
                .collapsible(false)
                .numbered(false)
                .single_line(true)
                .bordered(true)
                .texts(QueuedRunsTexts {
                    editing: crate::tr("Editing in the composer"),
                    send_now: crate::tr("Send now"),
                    edit: crate::tr("Edit"),
                    remove: crate::tr("Remove"),
                    earlier: crate::tr("Earlier"),
                    ..QueuedRunsTexts::default()
                })
                .on_earlier(move |n, _, cx| {
                    earlier.update(cx, |this, cx| this.queue_earlier(n, cx));
                })
                .on_send_now(move |n, _, cx| now.update(cx, |this, cx| this.steer(n, cx)))
                .on_edit(move |n, window, cx| {
                    edit.update(cx, |this, cx| this.unqueue(n, window, cx));
                })
                .on_remove(move |n, _, cx| remove.update(cx, |this, cx| this.remove_queued(n, cx)))
                .into_any_element(),
        )
    }

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
}

type RemoveHandler = Box<dyn Fn(&mut Window, &mut gpui::App)>;

/// One `.tc-chip` pill (`28px` height, `rounded_full`, `layer-accent-01`,
/// `16px` Carbon icon, `12px` mono label, `20px` circular `Close`).
fn tc_pill(
    faces: &Faces,
    id: SharedString,
    icon: CarbonIcon,
    label: String,
    on_remove: Option<(SharedString, RemoveHandler)>,
) -> AnyElement {
    let hover_bg = theme::role(ColorRole::LayerHover01);
    let rm_id = SharedString::from(format!("{id}-rm"));
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .pl(px(8.))
        .pr(px(if on_remove.is_some() { 4. } else { 8. }))
        .rounded_full()
        .bg(theme::role(ColorRole::LayerAccent01))
        .child(
            CarbonGlyph::carbon(icon)
                .size_length(px(16.))
                .color(theme::role(ColorRole::IconSecondary))
                .flex_none(),
        )
        .child(
            ui::mono(faces, label)
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(theme::role(ColorRole::TextPrimary)),
        )
        .when_some(on_remove, |pill, (rm_label, handler)| {
            pill.child(
                div()
                    .id(rm_id)
                    .role(gpui::Role::Button)
                    .tooltip(ui::tip(rm_label))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(20.))
                    .rounded_full()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, window, cx| handler(window, cx))
                    .child(
                        CarbonGlyph::carbon(CarbonIcon::Close)
                            .size_length(px(14.))
                            .color(theme::role(ColorRole::IconSecondary)),
                    ),
            )
        })
        .into_any_element()
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

fn chip_text(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .flex_none()
        .text_size(px(12.))
        .text_color(theme::role(ColorRole::TextSecondary))
}
