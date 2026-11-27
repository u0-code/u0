//! Settings › Telemetry, and the sheet that asks as u0 first opens: how
//! much the server the settings are of tells the people who make u0
//! (`telemetry`), on gpui-kit's slider, a step a level; the batch that
//! would go next, shown exactly; and a new id for this install.

use gpui::component::{
    WindowExt as _,
    setting::SettingGroup,
    slider::{Slider, SliderEvent, SliderState},
};
use gpui::{AnyElement, Context, Entity, WeakEntity, Window, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::{
    desk::Desk,
    fonts::Faces,
    hub::Host,
    proto::Call,
    telemetry::{Ask, Level},
    theme,
    ui::{self, Weight},
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
                this.pages.sent = None;
                cx.notify();
            }
        },
    )
    .detach();
    state
}

/// The slider, with each level's name under its step and what the level
/// sends under that.
fn levels(state: &Entity<SliderState>, faces: &Faces, cx: &gpui::App) -> AnyElement {
    let at = level_of(state.read(cx));
    let by = crate::policy::locked("telemetry");
    let names = Level::ALL.map(|l| {
        let shown = ui::fine(faces, crate::tr(l.name())).text_color(if l == at {
            theme::text()
        } else {
            theme::faint()
        });
        div().w(px(80.)).flex().justify_center().child(shown)
    });
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .w(px(320.))
        .child(
            div().px(px(40.)).child(
                Slider::new(state)
                    .accessibility_label(crate::tr("Telemetry"))
                    .disabled(by.is_some()),
            ),
        )
        .child(div().flex().children(names))
        .child(ui::fine(faces, crate::tr(at.about())))
        .children(by.map(|by| ui::fine(faces, crate::tr_with("Managed by %{by}", &[("by", &by)]))))
        .into_any_element()
}

impl Desk {
    /// The Telemetry tab, of the server the settings are of.
    pub(super) fn telemetry_tab(&self, w: &WeakEntity<Desk>) -> Vec<SettingGroup> {
        let mut items = vec![
            item(
                w,
                crate::tr("What u0 tells"),
                crate::tr(
                    "Each level sends what the one before it does, and more; none sends what you or an agent wrote, a path, a name of yours or an error's words. README.md lists every field.",
                ),
                |d, faces, cx| levels(&d.pages.level, faces, cx),
            ),
            item(
                w,
                crate::tr("Show what is sent"),
                crate::tr("The next day's batch at this level, exactly as it goes."),
                |_, _, cx| {
                    ui::button("telemetry-show", crate::tr("Show"), Weight::Secondary)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let level = level_of(this.pages.level.read(cx));
                            this.call(this.pages.on, Call::Telemetry(Ask::Show(level)));
                        }))
                        .into_any_element()
                },
            ),
        ];
        if let Some(sent) = self.pages.sent.clone() {
            items.push(whole(w, &["telemetry"], move |_, faces, _| {
                ui::fine_mono(faces, sent.clone()).into_any_element()
            }));
        }
        items.push(item(
            w,
            crate::tr("Reset install id"),
            crate::tr("From the next batch on, this install is sent under a new random id, unlinked from the old."),
            |_, _, cx| {
                ui::button("telemetry-forget", crate::tr("Reset"), Weight::Quiet)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.call(this.pages.on, Call::Telemetry(Ask::Forget));
                        this.pages.sent = None;
                        cx.notify();
                    }))
                    .into_any_element()
            },
        ));
        vec![group(crate::tr("Telemetry"), items)]
    }

    /// Ask, as u0 first opens, how much it may tell: Carbon `ConsentSheet`
    /// (`Dialogs` board), where refusing ("Keep telemetry off") is never
    /// harder than agreeing, and nothing above Off goes until confirmed.
    pub(crate) fn ask_telemetry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.telemetry.is_some() {
            return;
        }
        let desk = cx.entity();
        let shown = cx.new(|cx| {
            crate::desk::Over::new(
                desk.clone(),
                |_, _, cx| {
                    use u0ui::ghost::settings::{ConsentSheet, ConsentTexts};
                    let desk = cx.entity();
                    let texts = ConsentTexts {
                        title: std::rc::Rc::new(|a| {
                            crate::tr_with("Welcome to %{app}", &[("app", &a)])
                        }),
                        intro: std::rc::Rc::new(|_| {
                            crate::tr(
                                "Choose what you share to help improve u0. Nothing is sent until you continue, and you can change it any time in Settings › Telemetry.",
                            )
                        }),
                        slider: crate::tr("Telemetry"),
                        levels: Level::ALL.map(|l| crate::tr(l.name())),
                        describe: Level::ALL.map(|l| crate::tr(l.about())),
                        share_nothing: crate::tr("Keep telemetry off"),
                        proceed: crate::tr("Continue"),
                    };
                    ConsentSheet::new("first-run-telemetry")
                        .app_name("u0")
                        .default_level(1)
                        .texts(texts)
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
