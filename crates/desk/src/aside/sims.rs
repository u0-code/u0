//! The Devices tab (`DevicesTab` board): the iOS simulators and Android
//! emulators of the server the session is on. One chosen and running shows
//! its screen live, frame after frame; click to tap, hold ½ s to press, drag
//! to swipe, scroll with the wheel, type into it, rotate it, toggle Dark/Light,
//! or press Home (plus Back and Recents on Android). Where `AXe` is missing
//! for iOS simulators, the tab offers to install it with Homebrew.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gpui::component::input::InputState;
use gpui::{AppContext as _, Context, Div, Entity, SharedString, Window, div, prelude::*, px};
use u0ui::{
    components::{
        Button, ButtonSize, ButtonVariant, Empty, EmptyDescription, EmptyHeader, EmptyTitle,
        FieldSize, Input, Spinner, SpinnerSize,
        alert::{Alert, AlertVariant},
    },
    icons::{CarbonIcon, Icon},
    theme::LayerLevel,
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
};

use crate::{
    desk::Desk,
    devices::{AXE, AXE_FORMULA, Act, Device, Key, NO_SIMULATOR, Toward},
    fonts::Faces,
    hub::Host,
    proto::Call,
    theme, ui,
};

/// Portrait screen width in the Devices tab (`.screen { width: 232px }`).
const PORTRAIT_WIDE: f32 = 232.;

/// Landscape screen width in the Devices tab (`.screen--wide { width: 376px }`).
const LANDSCAPE_WIDE: f32 = 376.;

/// How far a press may move (in points of the window) and stay a touch,
/// not a swipe.
const STILL: f32 = 6.;

/// How long a touch is held to be a long press.
const HELD: Duration = Duration::from_millis(500);

/// How often the wheel scrolls a device, at most: each scroll is a swipe
/// that takes its time.
const WHEELED: Duration = Duration::from_millis(400);

/// The devices as the server last listed them (none until it has; whether
/// it was asked), and what is missing to have any; the one chosen, its screen
/// last read with its size in pixels, whether one is being read; what is to
/// be typed.
#[allow(
    clippy::struct_excessive_bools,
    reason = "device polling, stream-reading, and AXe-installation flags are independent"
)]
pub struct Sims {
    pub listed: Option<(Vec<Device>, Vec<String>)>,
    pub asked: bool,
    pub axe_installing: bool,
    pub axe_outcome: Option<Result<(), String>>,
    pub on: Option<String>,
    pub screen: Option<(String, Arc<gpui::Image>, (u32, u32))>,
    pub reading: bool,
    pub typed: Entity<InputState>,
    /// Where the screen was drawn last, for a click on it to be read against.
    pub origin: Arc<Mutex<gpui::Point<gpui::Pixels>>>,
    /// Where and when the screen was pressed, until it is let go.
    pub pressed: Option<(gpui::Point<gpui::Pixels>, Instant)>,
    /// When the wheel last scrolled it.
    pub wheeled: Option<Instant>,
}

impl Sims {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        Self {
            listed: None,
            asked: false,
            axe_installing: false,
            axe_outcome: None,
            on: None,
            screen: None,
            reading: false,
            typed: cx.new(|cx| InputState::new(window, cx)),
            origin: Arc::default(),
            pressed: None,
            wheeled: None,
        }
    }
}

/// Format `<machine> · X simulators, Y emulators` (or just `<machine>` when none).
fn machine_summary(machine: &str, found: &[Device]) -> String {
    let sims = found.iter().filter(|d| !d.android).count();
    let emus = found.iter().filter(|d| d.android).count();
    let mut parts = Vec::new();
    if sims > 0 {
        parts.push(
            crate::tr_with(
                if sims == 1 {
                    "%{n} simulator"
                } else {
                    "%{n} simulators"
                },
                &[("n", &sims)],
            )
            .to_string(),
        );
    }
    if emus > 0 {
        parts.push(
            crate::tr_with(
                if emus == 1 {
                    "%{n} emulator"
                } else {
                    "%{n} emulators"
                },
                &[("n", &emus)],
            )
            .to_string(),
        );
    }
    if parts.is_empty() {
        machine.to_owned()
    } else {
        format!("{machine} \u{00b7} {}", parts.join(", "))
    }
}

impl Desk {
    /// The server devices are read on: the session's.
    fn device_host(&self) -> Host {
        self.session().map_or(Host::HERE, |s| s.host)
    }

    /// The device chosen, as last listed.
    fn device(&self) -> Option<Device> {
        let (found, _) = self.sims.listed.as_ref()?;
        found
            .iter()
            .find(|d| Some(&d.id) == self.sims.on.as_ref())
            .cloned()
    }

    /// Do `act` to `device`, on its server.
    fn device_act(&self, device: Device, act: Act) {
        self.call(self.device_host(), Call::Device { device, act });
    }

    /// Each second, and as each frame comes: list the devices once the tab
    /// is first shown, and read the chosen one's screen while it is shown
    /// and that one runs.
    pub(crate) fn watch_device(&mut self) {
        let on_devices = self.surface() == Some(crate::desk::Surface::Devices);
        let beside_file = self.viewer.beside_device
            && matches!(self.surface(), Some(crate::desk::Surface::File(_)));
        if !on_devices && !beside_file {
            return;
        }
        if !self.sims.asked {
            self.sims.asked = true;
            self.call(self.device_host(), Call::Devices);
        }
        if let Some(device) = self.device().filter(|d| d.running)
            && !self.sims.reading
        {
            self.sims.reading = true;
            self.call(self.device_host(), Call::Screen { device });
        }
    }

    /// What the server says of its devices.
    pub(crate) fn devices_listed(&mut self, found: Vec<Device>, missing: Vec<String>) {
        if !found.iter().any(|d| Some(&d.id) == self.sims.on.as_ref()) {
            self.sims.on = found.iter().find(|d| d.running).map(|d| d.id.clone());
        }
        self.sims.listed = Some((found, missing));
    }

    pub(crate) fn axe_installed(&mut self, result: Result<(), String>) {
        self.sims.axe_installing = false;
        self.sims.axe_outcome = Some(result);
    }

    /// What the device `id` shows; the next frame asked for.
    pub(crate) fn device_screen(&mut self, id: String, picture: Result<Vec<u8>, String>) {
        self.sims.reading = false;
        let read = picture.and_then(|bytes| {
            let format = match imagesize::image_type(&bytes) {
                Ok(imagesize::ImageType::Jpeg) => gpui::ImageFormat::Jpeg,
                Ok(imagesize::ImageType::Png) => gpui::ImageFormat::Png,
                _ => return Err(crate::tr("it is neither a PNG nor a JPEG").to_string()),
            };
            let size = imagesize::blob_size(&bytes).map_err(|e| e.to_string())?;
            let size = (size.width as u32, size.height as u32);
            Ok((Arc::new(gpui::Image::from_bytes(format, bytes)), size))
        });
        match read {
            Ok((image, size)) => {
                self.sims.screen = Some((id, image, size));
                self.watch_device();
            }
            Err(why) => tracing::warn!("a device's screen could not be read: {why}"),
        }
    }

    /// The `Devices` tab (`DevicesTab` board).
    #[allow(
        clippy::too_many_lines,
        reason = "renders the device list, empty/missing/AXe states, and active device screen in one cohesive column"
    )]
    pub(super) fn sims_tab(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let Some((found, missing)) = &self.sims.listed else {
            return div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(32.))
                .p(px(12.))
                .child(
                    Spinner::new("devices-loading")
                        .size(SpinnerSize::Sm)
                        .layer(LayerLevel::One),
                )
                .child(
                    ui::text(faces, crate::tr("Looking for devices…"))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                );
        };

        let desk = cx.entity();
        let host = self.device_host();
        let machine = self
            .machine(host)
            .map_or_else(|| crate::tr("This Mac").to_string(), |m| m.name.clone());
        let summary = machine_summary(&machine, found);

        let d_ref = desk.clone();
        let top_row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(32.))
            .child(
                ui::one_line(
                    ui::text(faces, summary)
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                )
                .flex_1()
                .min_w(px(0.)),
            )
            .child(
                Button::new("devices-refresh", crate::tr("Look again"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .disabled(self.sims.axe_installing)
                    .on_click(move |_, _, cx| {
                        d_ref.update(cx, |this, cx| {
                            (this.sims.listed, this.sims.asked) = (None, false);
                            cx.notify();
                        });
                    }),
            );

        let outcome_alert = self.sims.axe_outcome.as_ref().map(|outcome| match outcome {
            Ok(()) => Alert::new("axe-installed-alert")
                .variant(AlertVariant::Success)
                .title(crate::tr("AXe is installed"))
                .description(div().max_w(px(300.)).child(crate::tr(
                    "iOS simulators are streamed and touched here now.",
                ))),
            Err(why) => Alert::new("axe-failed-alert")
                .variant(AlertVariant::Error)
                .title(crate::tr("AXe was not installed"))
                .description(div().max_w(px(300.)).child(why.clone())),
        });

        let rows = found.iter().map(|d| {
            let chosen = Some(&d.id) == self.sims.on.as_ref();
            let id = d.id.clone();
            let (start_dev, stop_dev) = (d.clone(), d.clone());
            let (d_pick, d_act) = (desk.clone(), desk.clone());
            let os = if d.os.is_empty() {
                if d.android { "Android" } else { "iOS" }
            } else {
                d.os.as_str()
            };
            let state = if d.running {
                crate::tr("Running")
            } else {
                crate::tr("Shut down")
            };
            let sub = format!("{os} \u{00b7} {state}");

            let act_btn = if d.running {
                Button::new(
                    SharedString::from(format!("stop-{}", d.id)),
                    crate::tr("Power off"),
                )
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let target = stop_dev.clone();
                    d_act.update(cx, |this, _| this.device_act(target, Act::Stop));
                })
            } else {
                Button::new(
                    SharedString::from(format!("start-{}", d.id)),
                    crate::tr("Start"),
                )
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let target = start_dev.clone();
                    d_act.update(cx, |this, _| this.device_act(target, Act::Start));
                })
            };

            div()
                .id(SharedString::from(format!("device-{}", d.id)))
                .relative()
                .flex()
                .items_center()
                .gap(px(10.))
                .min_h(px(48.))
                .pl(px(12.))
                .pr(px(8.))
                .py(px(6.))
                .bg(theme::role(if chosen {
                    ColorRole::LayerSelected01
                } else {
                    ColorRole::Layer02
                }))
                .hover(|st| {
                    st.bg(theme::role(if chosen {
                        ColorRole::LayerHover01
                    } else {
                        ColorRole::LayerHover02
                    }))
                })
                .when(chosen, |r| {
                    r.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .w(px(3.))
                            .bg(theme::role(ColorRole::BorderInteractive)),
                    )
                })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let next = id.clone();
                    d_pick.update(cx, |this, cx| {
                        this.sims.on = Some(next);
                        this.sims.screen = None;
                        this.watch_device();
                        cx.notify();
                    });
                })
                .child(
                    Icon::carbon(CarbonIcon::Mobile)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .flex_1()
                        .min_w(px(0.))
                        .child(ui::one_line(
                            ui::text(faces, d.name.clone())
                                .type_style(TypeStyle::BodyCompact01)
                                .text_color(theme::role(ColorRole::TextPrimary)),
                        ))
                        .child(ui::one_line(
                            ui::text(faces, sub)
                                .type_style(TypeStyle::Label01)
                                .text_color(theme::role(ColorRole::TextSecondary)),
                        )),
                )
                .child(act_btn)
        });

        let running_ios_no_sim = found
            .iter()
            .find(|d| !d.android && d.running)
            .filter(|_| missing.iter().any(|m| m == NO_SIMULATOR));
        let no_sim_alert = running_ios_no_sim.map(|d| {
            Alert::new("no-simulator-app-alert")
                .variant(AlertVariant::Warning)
                .title(crate::tr_with(
                    "%{name} runs, but can’t be touched",
                    &[("name", &d.name)],
                ))
                .description(div().max_w(px(300.)).child(crate::tr(
                    "Needs AXe (this Xcode has no Simulator.app). The screen shows as stills.",
                )))
        });

        let miss_rows = missing.iter().filter(|m| *m != NO_SIMULATOR).map(|m| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(
                    Icon::carbon(CarbonIcon::Information)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::IconSecondary)),
                )
                .child(
                    ui::text(faces, crate::tr_with("Not found: %{m}", &[("m", m)]))
                        .flex_1()
                        .min_w(px(0.))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                )
        });

        let axe_missing = missing.iter().any(|m| m == AXE);
        let cmd = format!("brew install {AXE_FORMULA}");
        let d_axe = desk.clone();

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .children(outcome_alert)
            .child(top_row)
            .children(rows);

        if found.is_empty() {
            body = body.child(
                Empty::new().child(
                    EmptyHeader::new()
                        .child(EmptyTitle::new(crate::tr("No simulator or emulator here.")))
                        .child(EmptyDescription::new(crate::tr_with(
                            "u0 looked for Xcode’s simulators and the Android SDK on %{machine}.",
                            &[("machine", &machine)],
                        ))),
                ),
            );
        }

        body = body.children(no_sim_alert).children(miss_rows);

        if self.sims.axe_installing {
            body = body
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_h(px(32.))
                        .child(
                            Spinner::new("axe-installing-spinner")
                                .size(SpinnerSize::Sm)
                                .layer(LayerLevel::One),
                        )
                        .child(
                            ui::text(faces, crate::tr("Installing AXe with Homebrew…"))
                                .type_style(TypeStyle::BodyCompact01)
                                .text_color(theme::role(ColorRole::TextPrimary)),
                        ),
                )
                .child(
                    ui::mono(faces, cmd)
                        .type_style(TypeStyle::Code01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                );
        } else if axe_missing {
            body = body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(12.))
                    .gap_y(px(4.))
                    .child(
                        Button::new("install-axe", crate::tr("Install AXe"))
                            .variant(ButtonVariant::Tertiary)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Download)
                            .on_click(move |_, _, cx| {
                                d_axe.update(cx, |this, cx| {
                                    this.sims.axe_installing = true;
                                    this.sims.axe_outcome = None;
                                    this.call(this.device_host(), Call::InstallAxe);
                                    this.sims.asked = true;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                ui::text(faces, crate::tr("Runs "))
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::TextSecondary)),
                            )
                            .child(
                                ui::mono(faces, cmd)
                                    .type_style(TypeStyle::Code01)
                                    .text_color(theme::role(ColorRole::TextSecondary)),
                            ),
                    ),
            );
            if self.sims.axe_outcome.is_none() && running_ios_no_sim.is_none() {
                body = body.child(
                    ui::text(
                        faces,
                        crate::tr("Installs AXe with Homebrew, only when you press it."),
                    )
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary)),
                );
            }
        }

        body.children(
            self.device()
                .filter(|d| d.running)
                .map(|d| self.device_screen_view(faces, d, cx)),
        )
    }

    /// The chosen running device's live screen, touch hint, toolbar, and
    /// `Type into it` field (`DevicesTab` Columns 1, 2, 7).
    fn device_screen_view(&self, faces: &Faces, device: Device, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity();
        let screen = self
            .sims
            .screen
            .as_ref()
            .filter(|(id, ..)| *id == device.id);

        let Some((_, image, (img_w, img_h))) = screen else {
            return div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(32.))
                .child(
                    Spinner::new("device-screen-loading")
                        .size(SpinnerSize::Sm)
                        .layer(LayerLevel::One),
                )
                .child(
                    ui::text(faces, crate::tr("Reading its screen…"))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                );
        };

        let (img_w, img_h) = (*img_w, *img_h);
        let wide = if img_w > img_h {
            LANDSCAPE_WIDE
        } else {
            PORTRAIT_WIDE
        };
        let tall = (wide * img_h as f32 / img_w.max(1) as f32)
            .round()
            .clamp(160., 460.);

        let origin = self.sims.origin.clone();
        let (at, touched, scrolled) = (self.sims.origin.clone(), device.clone(), device.clone());
        let pixel = move |pt: gpui::Point<gpui::Pixels>| {
            let orig = *at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let rel = pt - orig;
            let scale_x = img_w as f32 / wide;
            let scale_y = img_h as f32 / tall;
            let px_x = f32::from(rel.x) * scale_x;
            let px_y = f32::from(rel.y) * scale_y;
            (px_x.max(0.) as u32, px_y.max(0.) as u32)
        };

        let picture = div()
            .id("device-screen")
            .relative()
            .self_center()
            .w(px(wide))
            .h(px(tall))
            .bg(theme::role(ColorRole::Layer02))
            .border_1()
            .border_color(theme::role(ColorRole::BorderStrong01))
            .overflow_hidden()
            .child(gpui::img(image.clone()).size_full())
            .child(
                gpui::canvas(
                    move |bounds, _, _| {
                        *origin
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = bounds.origin;
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .cursor(gpui::CursorStyle::Crosshair)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, e: &gpui::MouseDownEvent, _, _| {
                    this.sims.pressed = Some((e.position, Instant::now()));
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseUpEvent, _, _| {
                    let Some((down, started)) = this.sims.pressed.take() else {
                        return;
                    };
                    let moved = e.position - down;
                    let ((x, y), (to_x, to_y)) = (pixel(down), pixel(e.position));
                    let act = if f32::from(moved.x).hypot(f32::from(moved.y)) > STILL {
                        Act::Swipe { x, y, to_x, to_y }
                    } else if started.elapsed() >= HELD {
                        Act::Press { x, y }
                    } else {
                        Act::Tap { x, y }
                    };
                    this.device_act(touched.clone(), act);
                }),
            )
            .on_scroll(cx.listener(move |this, e: &gpui::ScrollEvent, _, _| {
                if this.sims.wheeled.is_some_and(|at| at.elapsed() < WHEELED) {
                    return;
                }
                let delta = e.delta.pixel_delta(px(16.));
                let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
                let toward = match (dx.abs() >= dy.abs(), dx > 0., dy > 0.) {
                    _ if dx.abs().max(dy.abs()) < 1. => return,
                    (false, _, true) => Toward::Up,
                    (false, _, false) => Toward::Down,
                    (true, true, _) => Toward::Left,
                    (true, false, _) => Toward::Right,
                };
                this.sims.wheeled = Some(Instant::now());
                this.device_act(scrolled.clone(), Act::Scroll { toward });
            }));

        let tool_btn = |id: &'static str, label: &'static str, act: Act| {
            let d_btn = desk.clone();
            let target = device.clone();
            Button::new(id, crate::tr(label))
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .on_click(move |_, _, cx| {
                    let dev = target.clone();
                    let next = act.clone();
                    d_btn.update(cx, |this, _| this.device_act(dev, next));
                })
        };

        let mut tools = div()
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .child(tool_btn("dev-dark", "Dark", Act::Dark { dark: true }))
            .child(tool_btn("dev-light", "Light", Act::Dark { dark: false }))
            .child(tool_btn("dev-home", "Home", Act::Key { key: Key::Home }))
            .child(tool_btn(
                "dev-rot-l",
                "Rotate left",
                Act::Rotate { right: false },
            ))
            .child(tool_btn(
                "dev-rot-r",
                "Rotate right",
                Act::Rotate { right: true },
            ));
        if device.android {
            tools = tools
                .child(tool_btn("dev-back", "Back", Act::Key { key: Key::Back }))
                .child(tool_btn(
                    "dev-recents",
                    "Recents",
                    Act::Key { key: Key::Recents },
                ));
        }

        let d_type = desk;
        let typed_dev = device;
        let type_row = div()
            .flex()
            .items_end()
            .gap(px(8.))
            .child(
                div().flex_1().min_w(px(0.)).child(
                    Input::new("device-type-input", &self.sims.typed)
                        .label(crate::tr("Type into it"))
                        .placeholder(crate::tr("Text to type"))
                        .size(FieldSize::Sm)
                        .layer(LayerLevel::Two),
                ),
            )
            .child(
                Button::new("device-type-btn", crate::tr("Type"))
                    .variant(ButtonVariant::Tertiary)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, window, cx| {
                        let target = typed_dev.clone();
                        d_type.update(cx, |this, cx| {
                            let text = this.sims.typed.read(cx).value().to_string();
                            this.sims
                                .typed
                                .update(cx, |i, cx| i.set_value("", window, cx));
                            this.device_act(target, Act::Type { text });
                        });
                    }),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(4.))
            .child(picture)
            .child(
                ui::text(
                    faces,
                    crate::tr(
                        "Click to tap · hold ½ s to press · drag to swipe · scroll to scroll",
                    ),
                )
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextSecondary)),
            )
            .child(tools)
            .child(type_row)
    }
}
