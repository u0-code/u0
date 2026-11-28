//! Settings › Keybindings (`SetKeybindings.dc.html`): every action in
//! `keys::ACTIONS` with its keys now, where they hold and whether they are
//! yours; found by what it does or by its keys, shown all or only yours,
//! those that clash or those with none; each changed in place (its keys
//! recorded, where they hold) and given back u0's keys, one or all.

use gpui::component::WindowExt as _;
use gpui::{AnyElement, Context, ElementId, SharedString, div, prelude::*, px};
use u0ui::{
    components::{
        Button, ButtonSize, ButtonVariant, Input, InputGroup, InputGroupAddon, InputGroupInput,
        Select,
        alert::{AlertAction, AlertVariant},
        form::FieldSize,
        list_box::ListItem,
    },
    icons::{CarbonIcon, Icon as CarbonGlyph},
    t3::workspace::{
        KeybindingRow, KeybindingTexts,
        settings::{KEYBINDING_COLUMNS, KEYBINDING_GAP, KEYBINDING_PADDING},
    },
    theme::LayerLevel,
    tokens::ColorRole,
    u0::primitives::{KeyRecorder, key_recorder::keystroke},
};

use super::srow::carbon_page_with_tail;
use crate::{
    desk::{Desk, banners::banner},
    fonts::{Faces, Weighted as _},
    keys::{self, Bound, Keys},
    theme, ui,
};

/// Which actions the table shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    All,
    /// Those whose keys you set.
    Yours,
    /// Those whose keys another has where they hold.
    Clashing,
    /// Those with no keys.
    Unbound,
}

impl Show {
    const ALL: [Self; 4] = [Self::All, Self::Yours, Self::Clashing, Self::Unbound];

    fn label(self) -> SharedString {
        crate::tr(match self {
            Self::All => "All",
            Self::Yours => "Customized",
            Self::Clashing => "With problems",
            Self::Unbound => "Unbound",
        })
    }

    fn shows(self, b: &Bound, all: &[Bound]) -> bool {
        match self {
            Self::All => true,
            Self::Yours => b.yours,
            Self::Clashing => keys::clash(all, b).is_some(),
            Self::Unbound => b.keys.is_empty(),
        }
    }
}

/// Whether `b` is found by `query`: by what it does, its name, or its
/// keys however they are typed ("⌘J", "⌘+J").
fn found(b: &Bound, query: &str) -> bool {
    let bare = |s: &str| s.replace(['+', ' '], "").to_lowercase();
    let q = query.trim().to_lowercase();
    q.is_empty()
        || crate::tr(b.does).to_lowercase().contains(&q)
        || b.name.contains(&q)
        || (!b.keys.is_empty() && bare(&b.keys).contains(&bare(&q)))
}

impl Desk {
    /// The Keybindings page, found among the settings by `query`.
    pub(super) fn keybindings_tab(
        &self,
        query: &str,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let all = keys::bound(cx, &self.keys_yours);
        let typed = self.pages.keys_search.read(cx).value().to_string();
        let shown: Vec<&Bound> = all
            .iter()
            .filter(|b| self.pages.keys_show.shows(b, &all))
            .filter(|b| found(b, query) && found(b, &typed))
            .collect();
        let mut rows: Vec<AnyElement> = vec![table_head(faces)];
        for b in &shown {
            rows.push(Self::key_row(b, &all, cx));
            if self
                .pages
                .editing_keys
                .as_ref()
                .is_some_and(|(n, _)| *n == b.name)
            {
                rows.push(self.key_editor(b, &all, faces, cx));
            }
        }
        if shown.is_empty() {
            rows.push(
                ui::text(faces, crate::tr("No actions match"))
                    .px(px(16.))
                    .py(px(12.))
                    .text_size(px(crate::style::BODY))
                    .text_color(theme::text2())
                    .into_any_element(),
            );
        }
        let desk = cx.entity();
        let tail = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(Self::clash_alert(&all, cx))
            .child(self.keys_toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .bg(theme::role(ColorRole::Layer01))
                    .children(rows),
            )
            .child(
                div().flex().justify_end().child(
                    Button::new("keys-reset-all", crate::tr("Reset all keybindings…"))
                        .variant(ButtonVariant::DangerGhost)
                        .size(ButtonSize::Sm)
                        .disabled(self.keys_yours.is_empty())
                        .on_click(move |_, window, cx| {
                            let desk = desk.clone();
                            window.open_alert_dialog(cx, move |alert, _, _| {
                                let desk = desk.clone();
                                alert
                                    .title(crate::tr("Reset all keybindings?"))
                                    .description(crate::tr(
                                        "Every action gets u0's keys back; keybindings.json is emptied.",
                                    ))
                                    .ok_text(crate::tr("Reset all"))
                                    .show_cancel(true)
                                    .on_ok(move |_, _, cx| {
                                        desk.update(cx, |d, cx| {
                                            d.pages.editing_keys = None;
                                            d.forget_keys();
                                            cx.notify();
                                        });
                                        true
                                    })
                            });
                        }),
                ),
            );
        carbon_page_with_tail(
            "settings-keybindings",
            super::Tab::Keybindings.title(),
            crate::tr(
                "Every shortcut, where it works, and your own in keybindings.json. F6 and ⇧F6 move the keys between the parts of the window.",
            ),
            self.carbon_settings_banner(super::Tab::Keybindings, cx),
            Vec::new(),
            Some(tail.into_any_element()),
            "",
            faces,
        )
    }

    /// What clashes, said once above the table, with a way to see only
    /// those.
    fn clash_alert(all: &[Bound], cx: &mut Context<Self>) -> Option<AnyElement> {
        let clashing: Vec<&Bound> = all
            .iter()
            .filter(|b| keys::clash(all, b).is_some())
            .collect();
        let first = clashing.first()?;
        let other = keys::clash(all, first)?;
        let desk = cx.entity();
        let show = AlertAction::new(crate::tr("Show them")).on_click(move |_, cx| {
            desk.update(cx, |d, cx| {
                d.pages.keys_show = Show::Clashing;
                cx.notify();
            });
        });
        let n = clashing.len().to_string();
        Some(banner(
            "keys-clash",
            AlertVariant::Warning,
            crate::tr_with("%{n} shortcuts clash", &[("n", &n)]),
            crate::tr_with(
                "“%{one}” and “%{other}” have %{keys} in the same place, so only one of them runs. The last bound wins; change one of them.",
                &[
                    ("one", &crate::tr(first.does)),
                    ("other", &crate::tr(other.does)),
                    ("keys", &first.keys.replace('+', "")),
                ],
            ),
            None,
            Some(show),
        ))
    }

    /// The search, which actions show, and the file.
    fn keys_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let desk = cx.entity();
        let path = self.home.join("keybindings.json");
        let shown = Show::ALL
            .iter()
            .position(|s| *s == self.pages.keys_show)
            .unwrap_or(0);
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div().flex_1().min_w(px(0.)).child(
                    InputGroup::new("keys-search")
                        .addon(
                            InputGroupAddon::new().child(
                                CarbonGlyph::carbon(CarbonIcon::Search).size_length(px(16.)),
                            ),
                        )
                        .input(
                            InputGroupInput::new(&self.pages.keys_search)
                                .placeholder(crate::tr("Search commands, or type keys like ⌘J")),
                        ),
                ),
            )
            .child(
                div().w(px(160.)).child(
                    Select::new("keys-show")
                        .size(FieldSize::Sm)
                        .items(
                            Show::ALL
                                .iter()
                                .enumerate()
                                .map(|(ix, s)| ListItem::labeled(ix.to_string(), s.label())),
                        )
                        .value(Some(shown.to_string()))
                        .on_value_change(move |v, _, cx| {
                            if let Some(show) = v
                                .and_then(|v| v.parse::<usize>().ok())
                                .and_then(|ix| Show::ALL.get(ix))
                            {
                                desk.update(cx, |d, cx| {
                                    d.pages.keys_show = *show;
                                    cx.notify();
                                });
                            }
                        }),
                ),
            )
            .child(
                Button::new("keys-open-file", crate::tr("Open keybindings.json"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(move |d, _, _, _| d.edit(path.clone(), Some(b"[]\n")))),
            )
    }

    /// One action's row: what it does, its keys, where they hold, what
    /// else has them, and its edit button.
    fn key_row(b: &Bound, all: &[Bound], cx: &mut Context<Self>) -> AnyElement {
        let (name, keys_now, when) = (b.name, b.keys.clone(), b.when.clone());
        let desk = cx.entity();
        let mut row = KeybindingRow::new(
            ElementId::from(SharedString::from(format!("key-{name}"))),
            crate::tr(b.does),
            b.keys.clone(),
        )
        .custom(b.yours)
        .texts(KeybindingTexts {
            custom: crate::tr("Custom"),
            edit: crate::tr("Edit keybinding"),
        })
        .on_edit(move |window, cx| {
            let (keys_now, when) = (keys_now.clone(), when.clone().unwrap_or_default());
            desk.update(cx, |d, cx| {
                d.pages.editing_keys = Some((name, keys_now.into()));
                d.pages
                    .keys_when
                    .update(cx, |i, cx| i.set_value(when, window, cx));
                cx.notify();
            });
        });
        if let Some(when) = &b.when {
            row = row.when_clause(when.clone());
        }
        if let Some(other) = keys::clash(all, b) {
            row = row.warning(crate::tr_with(
                "Also bound to “%{other}”",
                &[("other", &crate::tr(other.does))],
            ));
        }
        row.into_any_element()
    }

    /// The action being changed, under its row: its keys recorded, where
    /// they hold, and u0's keys back.
    fn key_editor(
        &self,
        b: &Bound,
        all: &[Bound],
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = b.name;
        let recorded = self
            .pages
            .editing_keys
            .as_ref()
            .map(|(_, k)| k.clone())
            .unwrap_or_default();
        let when_typed = self.pages.keys_when.read(cx).value().trim().to_owned();
        let when_reads =
            when_typed.is_empty() || gpui::KeyBindingContextPredicate::parse(&when_typed).is_ok();
        // Where it is to hold, as the keymap writes it, to be compared.
        let when = (!when_typed.is_empty()).then(|| {
            gpui::KeyBindingContextPredicate::parse(&when_typed)
                .map_or_else(|_| when_typed.clone(), |p| p.to_string())
        });
        let would = Bound {
            name,
            does: b.does,
            keys: recorded.to_string(),
            when: when.clone(),
            yours: true,
        };
        let clash = keys::clash(all, &would).map(|o| crate::tr(o.does));
        let (record, save, cancel, reset) = (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let off = keys::every_key_of(cx, name);
        let mut keys_field = KeyRecorder::new(SharedString::from(format!("key-record-{name}")))
            .label(crate::tr("Keys"))
            .value(recorded.clone())
            .texts([
                crate::tr("Press a shortcut…"),
                crate::tr("Not set"),
                crate::tr("Esc to cancel"),
                crate::tr("Click, then press the keys"),
                crate::tr("Clear shortcut"),
            ])
            .conflict_text(|other| {
                crate::tr_with("Already used by “%{other}”", &[("other", &other)])
            })
            .on_value_change(move |keys, _, cx| {
                record.update(cx, |d, cx| {
                    if let Some((_, k)) = &mut d.pages.editing_keys {
                        *k = keys;
                    }
                    cx.notify();
                });
            });
        if let Some(other) = clash {
            keys_field = keys_field.conflict(other);
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(20.))
            .py(px(16.))
            .bg(theme::role(ColorRole::Layer02))
            .border_l(px(3.))
            .border_color(theme::role(ColorRole::BorderInteractive))
            .child(
                ui::text(
                    faces,
                    crate::tr_with("Editing %{what}", &[("what", &crate::tr(b.does))]),
                )
                .text_size(px(crate::style::BODY))
                .weight(gpui::FontWeight::SEMIBOLD),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .child(div().w(px(240.)).child(keys_field))
                    .child(
                        div().w(px(240.)).child(
                            Input::new("key-when", &self.pages.keys_when)
                                .label(crate::tr("When"))
                                .placeholder(crate::tr("Everywhere"))
                                .helper_text(crate::tr("Terminal or Composer, with ! && ||"))
                                .invalid(!when_reads)
                                .invalid_text(crate::tr("This doesn't read as a when"))
                                .size(FieldSize::Sm)
                                .layer(LayerLevel::Two),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(4.))
                    .when(b.yours, |d| {
                        d.child(
                            Button::new("key-reset", crate::tr("Reset to default"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    reset.update(cx, |d, cx| {
                                        d.pages.editing_keys = None;
                                        d.set_keys(name, Keys::Defaults);
                                        cx.notify();
                                    });
                                }),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("key-cancel", crate::tr("Cancel"))
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                cancel.update(cx, |d, cx| {
                                    d.pages.editing_keys = None;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new("key-save", crate::tr("Save"))
                            .size(ButtonSize::Sm)
                            .disabled(!when_reads)
                            .on_click(move |_, _, cx| {
                                let keys = if recorded.is_empty() {
                                    Keys::Off(off.clone())
                                } else {
                                    Keys::Yours {
                                        key: keystroke(&recorded),
                                        when: when.clone(),
                                    }
                                };
                                save.update(cx, |d, cx| {
                                    d.pages.editing_keys = None;
                                    d.set_keys(name, keys);
                                    cx.notify();
                                });
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// The table's head, its columns as `KeybindingRow` lays them out.
fn table_head(faces: &Faces) -> AnyElement {
    let [keys, when, warn, edit] = KEYBINDING_COLUMNS;
    let cell = |text: SharedString| {
        ui::text(faces, text)
            .text_size(px(crate::style::BODY))
            .weight(gpui::FontWeight::SEMIBOLD)
    };
    div()
        .flex()
        .items_center()
        .gap(px(KEYBINDING_GAP))
        .h(px(36.))
        .pl(px(KEYBINDING_PADDING.0))
        .pr(px(KEYBINDING_PADDING.1))
        .bg(theme::role(ColorRole::LayerAccent01))
        .child(cell(crate::tr("Command")).flex_1())
        .child(cell(crate::tr("Keys")).w(px(keys)).flex_none())
        .child(
            cell(crate::tr("When"))
                .flex_shrink(1.)
                .flex_basis(px(when))
                .min_w_0(),
        )
        .child(div().w(px(warn + KEYBINDING_GAP + edit)).flex_none())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound(name: &'static str, keys: &str, yours: bool) -> Bound {
        Bound {
            name,
            does: name,
            keys: keys.into(),
            when: None,
            yours,
        }
    }

    #[test]
    fn an_action_is_found_by_what_it_does_or_its_keys_however_typed() {
        let b = bound("drawer", "⌘+J", false);
        assert!(found(&b, ""));
        assert!(found(&b, "draw"));
        assert!(found(&b, "⌘J"));
        assert!(found(&b, "⌘ + j"));
        assert!(!found(&b, "⌘K"));
        assert!(!found(&bound("issues", "", false), "⌘"));
    }

    #[test]
    fn the_filter_shows_yours_the_clashing_or_the_unbound() {
        let all = [
            bound("drawer", "⌘+J", false),
            bound("review", "⌘+J", true),
            bound("issues", "", false),
        ];
        let shown = |s: Show| -> Vec<&str> {
            all.iter()
                .filter(|b| s.shows(b, &all))
                .map(|b| b.name)
                .collect()
        };
        assert_eq!(shown(Show::All).len(), 3);
        assert_eq!(shown(Show::Yours), ["review"]);
        assert_eq!(shown(Show::Clashing), ["drawer", "review"]);
        assert_eq!(shown(Show::Unbound), ["issues"]);
    }
}
