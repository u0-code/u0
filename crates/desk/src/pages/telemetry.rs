//! Settings › Telemetry, and the sheet that asks as u0 first opens: how
//! much the server the settings are of tells the people who make u0
//! (`telemetry`), on gpui-kit's slider, a step a level; the batch that
//! would go next, shown exactly; and a new id for this install.

use gpui::component::{
    WindowExt as _,
    slider::{SliderEvent, SliderState},
};
use gpui::{AnyElement, Context, Entity, WeakEntity, Window, div, prelude::*, px};
use u0ui::{
    components::slider::Slider,
    icons::{CarbonIcon, Icon as CarbonGlyph},
    tokens::ColorRole,
    u0::settings::{Payload, TelemetryPreview, TelemetryPreviewTexts},
};

use super::srow::{Group, group, item, whole};
use crate::{
    desk::Desk,
    fonts::{Faces, Weighted as _},
    hub::Host,
    proto::Call,
    telemetry::{Ask, Level},
    theme, ui,
};

/// The level a slider stands at.
fn level_of(slider: &SliderState) -> Level {
    let at = slider.value().start().round().clamp(0., 3.) as u8;
    Level::try_from(at).unwrap_or(Level::Usage)
}

/// The slider of the level, standing at `level`: moved on the page, the
/// level of the server the settings are of is changed once one was
/// chosen; on the sheet, only as it is confirmed.
pub fn slider(level: Level, window: &mut Window, cx: &mut Context<Desk>) -> Entity<SliderState> {
    let state = cx.new(|_| {
        SliderState::new()
            .min(0.)
            .max(3.)
            .step(1.)
            .default_value(f32::from(u8::from(level)))
    });
    cx.subscribe_in(
        &state,
        window,
        |this, state, e: &SliderEvent, window, cx| {
            // What the level sends is said under it, on the sheet too.
            window.refresh();
            if let SliderEvent::Release(_) = e {
                let level = level_of(state.read(cx));
                let on = this.pages.on;
                if this.settings_on(on).telemetry.is_some() {
                    this.settings_on_mut(on).telemetry = Some(level);
                    this.save_settings_on(on);
                }
                this.pages.sent = Default::default();
                cx.notify();
            }
        },
    )
    .detach();
    state
}

/// A batch as the consent sheet's preview draws it: the JSON the server
/// answered, field for field and in its order; what does not read as
/// JSON, as the words it is.
fn payload_of(batch: &str) -> Payload {
    fn of(v: &serde_json::Value) -> Payload {
        use serde_json::Value;
        match v {
            Value::Null => Payload::Null,
            Value::Bool(b) => Payload::Bool(*b),
            Value::Number(n) => Payload::Number(n.as_f64().unwrap_or_default()),
            Value::String(s) => Payload::String(s.clone().into()),
            Value::Array(a) => Payload::Array(a.iter().map(of).collect()),
            Value::Object(o) => {
                Payload::Object(o.iter().map(|(k, v)| (k.clone().into(), of(v))).collect())
            }
        }
    }
    serde_json::from_str(batch)
        .map_or_else(|_| Payload::String(batch.to_owned().into()), |v| of(&v))
}

/// The slider across the page, and under it each level and what it sends,
/// the one chosen marked; a policy's lock said.
fn levels(state: &Entity<SliderState>, faces: &Faces, cx: &gpui::App) -> AnyElement {
    let at = level_of(state.read(cx));
    let by = crate::policy::locked("telemetry");
    let names = Level::ALL.map(|l| {
        ui::fine(faces, crate::tr(l.name()))
            .weight(if l == at {
                gpui::FontWeight::SEMIBOLD
            } else {
                gpui::FontWeight::NORMAL
            })
            .text_color(if l == at {
                theme::text()
            } else {
                theme::text2()
            })
    });
    let rows = Level::ALL.map(|l| {
        let chosen = l == at;
        let name = format!("{} · {}", u8::from(l), crate::tr(l.name()));
        div()
            .flex()
            .gap(px(16.))
            .py(px(8.))
            .px(px(12.))
            .border_l(px(3.))
            .border_color(if chosen {
                theme::role(ColorRole::BorderInteractive)
            } else {
                gpui::transparent_black()
            })
            .when(chosen, |d| d.bg(theme::role(ColorRole::LayerSelected01)))
            .child(
                div().w(px(112.)).flex_none().child(
                    ui::text(faces, name)
                        .text_size(px(crate::style::BODY))
                        .weight(if chosen {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        }),
                ),
            )
            .child(
                ui::fine(faces, crate::tr(l.about()))
                    .flex_1()
                    .min_w(px(0.))
                    .text_color(theme::text2()),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .w_full()
        .child(
            Slider::new("telemetry-level", state)
                .label(crate::tr("Telemetry level"))
                .layer(u0ui::theme::LayerLevel::Two)
                .hide_input(true)
                .disabled(by.is_some()),
        )
        .child(div().flex().justify_between().children(names))
        .child(div().flex().flex_col().children(rows))
        .child(ui::fine(
            faces,
            crate::tr("Each level sends what the one before it does, and more; never what you write, a path, a name of yours or a host."),
        ))
        .children(by.map(|by| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    CarbonGlyph::carbon(CarbonIcon::Locked)
                        .size_length(px(14.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
                .child(ui::fine(
                    faces,
                    crate::tr_with("Managed by %{by}", &[("by", &by)]),
                ))
        }))
        .into_any_element()
}

impl Desk {
    /// The Telemetry tab, of the server the settings are of.
    pub(super) fn telemetry_tab(&self, w: &WeakEntity<Desk>) -> Vec<Group> {
        let tells = vec![whole(w, &["telemetry", "level"], |d, faces, cx| {
            levels(&d.pages.level, faces, cx)
        })];
        let mut install = vec![item(
            w,
            crate::tr("Show what is sent"),
            crate::tr("The next day's batch at this level, exactly as it goes."),
            |_, _, cx| {
                let desk = cx.entity();
                super::srow::button("telemetry-show", crate::tr("Show"), ui::Weight::Secondary)
                    .on_click(move |_, _, cx| {
                        desk.update(cx, |this, cx| {
                            let level = level_of(this.pages.level.read(cx));
                            this.call(this.pages.on, Call::Telemetry(Ask::Show(level)));
                        });
                    })
                    .into_any_element()
            },
        )];
        if self.pages.sent.iter().any(Option::is_some) {
            install.push(whole(w, &["telemetry", "json"], |d, _, cx| {
                let at = level_of(d.pages.level.read(cx));
                let sent = d.pages.sent[usize::from(u8::from(at))].clone();
                TelemetryPreview::new(
                    "telemetry-sent",
                    u32::from(u8::from(at)),
                    payload_of(&sent.unwrap_or_default()),
                )
                .texts(TelemetryPreviewTexts {
                    at_level: std::rc::Rc::new(|n| {
                        crate::tr_with("What is sent at level %{n}", &[("n", &n.to_string())])
                    }),
                    copy: crate::tr("Copy JSON"),
                    copied: crate::tr("Copied"),
                    ..TelemetryPreviewTexts::default()
                })
                .into_any_element()
            }));
        }
        install.push(item(
            w,
            crate::tr("Reset install id"),
            crate::tr("Later batches go under a new random id, unlinked from the old."),
            |d, faces, cx| {
                let id = d.pages.install_id.clone();
                let desk = cx.entity();
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .children(id.map(|s| ui::fine_mono(faces, s)))
                    .child(
                        super::srow::button(
                            "telemetry-forget",
                            crate::tr("Reset"),
                            ui::Weight::Secondary,
                        )
                        .on_click(move |_, _, cx| {
                            desk.update(cx, |this, cx| {
                                this.call(this.pages.on, Call::Telemetry(Ask::Forget));
                                this.pages.sent = Default::default();
                                this.pages.install_id = None;
                                cx.notify();
                            });
                        }),
                    )
                    .into_any_element()
            },
        ));
        vec![
            group(crate::tr("What u0 tells"), tells).description(crate::tr(
                "Counts of how u0 is used, never what you or your agents write.",
            )),
            group(crate::tr("This install"), install),
        ]
    }

    /// Ask, as u0 first opens, how much it may tell: Carbon `ConsentSheet`
    /// (`Dialogs` board), where refusing ("Keep telemetry off") is never
    /// harder than agreeing, and nothing above Off goes until confirmed.
    pub(crate) fn ask_telemetry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.telemetry.is_some()
            || self.settings_refused.is_some()
            || crate::policy::current().is_err()
        {
            return;
        }
        // What each level sends, as the server would send it, to show on
        // the sheet: never a sample.
        for level in Level::ALL {
            self.call(Host::HERE, Call::Telemetry(Ask::Show(level)));
        }
        let desk = cx.entity();
        let shown = cx.new(|cx| {
            crate::desk::Over::new(
                desk.clone(),
                |d, _, cx| {
                    use u0ui::u0::settings::{ConsentActions, ConsentSheet, ConsentTexts};
                    let sent = d.pages.sent.clone();
                    let reading = crate::tr("Reading…");
                    let desk = cx.entity();
                    let texts = ConsentTexts {
                        title: std::rc::Rc::new(|a| {
                            crate::tr_with("Welcome to %{app}", &[("app", &a)])
                        }),
                        intro: std::rc::Rc::new(|_| {
                            crate::tr(
                                "Nothing is sent until you continue. Change it later in Settings › Telemetry.",
                            )
                        }),
                        slider: crate::tr("Telemetry"),
                        levels: Level::ALL.map(|l| crate::tr(l.name())),
                        describe: Level::ALL.map(|l| crate::tr(l.about())),
                        share_nothing: crate::tr("Keep telemetry off"),
                        proceed: std::rc::Rc::new(|level| {
                            crate::tr_with("Continue with %{level}", &[("level", &level)])
                        }),
                    };
                    ConsentSheet::new("first-run-telemetry")
                        .app_name("u0")
                        .default_level(1)
                        .actions(ConsentActions::Dialog)
                        .texts(texts)
                        .payload(move |lvl| {
                            sent.get(lvl).cloned().flatten().map_or_else(
                                || Payload::String(reading.clone()),
                                |b| payload_of(&b),
                            )
                        })
                        .on_continue(move |lvl, window, cx| {
                            window.close_dialog(cx);
                            let chosen = Level::try_from(u8::try_from(lvl).unwrap_or(0))
                                .unwrap_or(Level::Off);
                            desk.update(cx, |d, cx| {
                                d.settings.telemetry = Some(chosen);
                                d.save_settings_on(Host::HERE);
                                cx.notify();
                            });
                        })
                        .into_any_element()
                },
                cx,
            )
        });
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .w(px(512.))
                .p_0()
                .rounded_none()
                .border_0()
                .close_button(false)
                .overlay_closable(false)
                .child(shown.clone())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_shows_the_batch_the_server_answered_field_for_field() {
        let shown = payload_of(
            r#"{"level":1,"version":"0.2.0","agents":{"codex":"0.9.4"},"listening":true}"#,
        );
        assert_eq!(
            shown,
            Payload::Object(vec![
                ("level".into(), Payload::Number(1.)),
                ("version".into(), Payload::String("0.2.0".into())),
                (
                    "agents".into(),
                    Payload::Object(vec![("codex".into(), Payload::String("0.9.4".into()))])
                ),
                ("listening".into(), Payload::Bool(true)),
            ])
        );
        let refused = "telemetry.json does not read, so nothing is sent.";
        assert_eq!(payload_of(refused), Payload::String(refused.into()));
    }
}
