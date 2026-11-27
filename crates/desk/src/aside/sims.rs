//! The Devices tab: the iOS simulators and Android emulators of the server
//! the session is on, as t3code's Devices panel. One chosen and running
//! shows its screen, each frame asked for as the last one comes while the
//! tab is shown; it is touched by clicking its screen (held, a long press;
//! dragged, a swipe; the wheel scrolls it), typed into, turned, and has
//! its Home key (an Android one's Back and Recents too). Where `AXe`, which
//! iOS simulators are watched live and touched with, is missing, the tab
//! offers to install it.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gpui::component::input::{Input, InputState};
use gpui::{AppContext as _, Context, Div, Entity, Window, div, prelude::*, px};

use super::{empty, fine};
use crate::{
    desk::Desk,
    devices::{AXE, AXE_FORMULA, Act, Device, Key, Toward},
    fonts::Faces,
    hub::Host,
    proto::Call,
    ui::{self, Weight},
};

/// How wide a device's screen is drawn.
const WIDE: f32 = 300.;

/// How far a press may move (in points of the window) and stay a touch,
/// not a swipe.
const STILL: f32 = 6.;

/// How long a touch is held to be a long press.
const HELD: Duration = Duration::from_millis(500);

/// How often the wheel scrolls a device, at most: each scroll is a swipe
/// that takes its time.
const WHEELED: Duration = Duration::from_millis(400);

/// The devices as the server last listed them (none until it has; whether
/// it was asked), and
/// what is missing to have any; the one chosen, its screen last read with
/// its size in pixels, whether one is being read; what is to be typed.
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
            typed: cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("Type into it"))),
            origin: Arc::default(),
            pressed: None,
            wheeled: None,
        }
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

    /// The Devices tab.
    pub(super) fn sims_tab(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let Some((found, missing)) = &self.sims.listed else {
            use u0ui::components::spinner::{Spinner, SpinnerSize};
            return div().p(px(16.)).child(
                Spinner::new("sims-loading")
                    .size(SpinnerSize::Inline)
                    .description(crate::tr("Looking for devices…")),
            );
        };
        let refresh = ui::button("devices-refresh", crate::tr("Look again"), Weight::Quiet)
            .on_click(cx.listener(|this, _, _, cx| {
                (this.sims.listed, this.sims.asked) = (None, false);
                cx.notify();
            }));
        let rows = found.iter().map(|d| {
            let (chosen, id) = (Some(&d.id) == self.sims.on.as_ref(), d.id.clone());
            let (start, stop) = (d.clone(), d.clone());
            div()
                .id(gpui::SharedString::from(format!("device-{}", d.id)))
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(28.))
                .px(px(6.))
                .rounded(px(4.))
                .when(chosen, |r| r.bg(crate::theme::hover()))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.sims.on = Some(id.clone());
                    this.sims.screen = None;
                    cx.notify();
                }))
                .child(div().flex_1().child(format!(
                    "{} · {}",
                    d.name,
                    if d.os.is_empty() {
                        if d.android { "Android" } else { "iOS" }
                    } else {
                        d.os.as_str()
                    }
                )))
                .child(if d.running {
                    ui::button(
                        format!("stop-{}", d.id),
                        crate::tr("Power off"),
                        Weight::Quiet,
                    )
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.device_act(stop.clone(), Act::Stop);
                    }))
                } else {
                    ui::button(
                        format!("start-{}", d.id),
                        crate::tr("Start"),
                        Weight::Secondary,
                    )
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.device_act(start.clone(), Act::Start);
                    }))
                })
        });
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(div().flex_1())
                    .child(refresh),
            )
            .children(rows)
            .when(found.is_empty(), |d| {
                use u0ui::components::empty::{
                    Empty, EmptyDescription, EmptyHeader, EmptyTitle,
                };
                d.child(
                    Empty::new().child(
                        EmptyHeader::new()
                            .child(EmptyTitle::new(crate::tr("No simulator or emulator here.")))
                            .child(EmptyDescription::new(crate::tr(
                                "u0 looks for iOS simulators with Xcode and Android emulators with the Android SDK.",
                            ))),
                    ),
                )
            })
            .children(missing.iter().map(|m| {
                use u0ui::prelude::*;
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        div().mt(px(2.)).child(
                            Icon::carbon(CarbonIcon::ErrorFilled)
                                .size(IconSize::S16)
                                .color(color(ColorRole::SupportError)),
                        ),
                    )
                    .child(
                        div()
                            .type_style(TypeStyle::Body01)
                            .text_color(color(ColorRole::TextSecondary))
                            .child(crate::tr_with("Not found: %{m}", &[("m", m)])),
                    )
            }))
            .when(missing.iter().any(|m| m == AXE), |d| {
                // Your press is the asking: nothing is installed without it.
                let what = format!("brew install {AXE_FORMULA}");
                d.child(
                    ui::button(
                        "install-axe",
                        crate::tr_with("Install AXe (%{what})", &[("what", &what)]),
                        Weight::Secondary,
                    )
                    .loading(self.sims.axe_installing)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sims.axe_installing = true;
                        this.sims.axe_outcome = None;
                        this.call(this.device_host(), Call::InstallAxe);
                        this.sims.asked = true;
                        cx.notify();
                    })),
                )
            })
            .children(self.sims.axe_outcome.as_ref().map(|outcome| match outcome {
                Ok(()) => fine(faces, crate::tr("AXe is installed.")),
                Err(why) => fine(faces, why.clone()),
            }))
            .children(
                self.device()
                    .filter(|d| d.running)
                    .map(|d| self.device_screen_view(faces, d, cx)),
            )
    }

    /// The chosen device's screen, and what can be done to it.
    #[allow(
        clippy::many_single_char_names,
        reason = "a point's x and y, a size's w and h"
    )]
    fn device_screen_view(&self, faces: &Faces, device: Device, cx: &mut Context<Self>) -> Div {
        let d = device;
        let key = |label: &'static str, k: Key, d: &Device| {
            let d = d.clone();
            ui::button(label, crate::tr(label), Weight::Quiet).on_click(
                cx.listener(move |this, _, _, _| this.device_act(d.clone(), Act::Key { key: k })),
            )
        };
        let (dark, light) = (d.clone(), d.clone());
        let mut tools = div()
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .child(
                ui::button("dark", crate::tr("Dark"), Weight::Quiet).on_click(cx.listener(
                    move |this, _, _, _| this.device_act(dark.clone(), Act::Dark { dark: true }),
                )),
            )
            .child(
                ui::button("light", crate::tr("Light"), Weight::Quiet).on_click(cx.listener(
                    move |this, _, _, _| this.device_act(light.clone(), Act::Dark { dark: false }),
                )),
            )
            .child(key("Home", Key::Home, &d));
        for (label, right) in [("Rotate left", false), ("Rotate right", true)] {
            let d = d.clone();
            tools = tools.child(ui::button(label, crate::tr(label), Weight::Quiet).on_click(
                cx.listener(move |this, _, _, _| this.device_act(d.clone(), Act::Rotate { right })),
            ));
        }
        if d.android {
            tools = tools
                .child(key("Back", Key::Back, &d))
                .child(key("Recents", Key::Recents, &d));
        }
        let screen = self.sims.screen.as_ref().filter(|(id, ..)| *id == d.id);
        let picture = screen.map(|(_, image, (w, h))| {
            let (w, h) = (*w, *h);
            let tall = WIDE * h as f32 / w.max(1) as f32;
            let origin = self.sims.origin.clone();
            let (at, touched, scrolled) = (self.sims.origin.clone(), d.clone(), d.clone());
            // A point of the window, in the pixels of the device's screen.
            let pixel = move |p: gpui::Point<gpui::Pixels>| {
                let o = *at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let p = p - o;
                let scale = w as f32 / WIDE;
                let (x, y) = (f32::from(p.x) * scale, f32::from(p.y) * scale);
                (x.max(0.) as u32, y.max(0.) as u32)
            };
            div()
                .id("device-screen")
                .relative()
                .w(px(WIDE))
                .h(px(tall))
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
                .cursor_pointer()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, e: &gpui::MouseDownEvent, _, _| {
                        this.sims.pressed = Some((e.position, Instant::now()));
                    }),
                )
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, e: &gpui::MouseUpEvent, _, _| {
                        let Some((down, at)) = this.sims.pressed.take() else {
                            return;
                        };
                        let moved = e.position - down;
                        let ((x, y), (to_x, to_y)) = (pixel(down), pixel(e.position));
                        let act = if f32::from(moved.x).hypot(f32::from(moved.y)) > STILL {
                            Act::Swipe { x, y, to_x, to_y }
                        } else if at.elapsed() >= HELD {
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
                    // The wheel moved toward what it would show, as it
                    // scrolls a page here.
                    let toward = match (dx.abs() >= dy.abs(), dx > 0., dy > 0.) {
                        _ if dx.abs().max(dy.abs()) < 1. => return,
                        (false, _, true) => Toward::Up,
                        (false, _, false) => Toward::Down,
                        (true, true, _) => Toward::Left,
                        (true, false, _) => Toward::Right,
                    };
                    this.sims.wheeled = Some(Instant::now());
                    this.device_act(scrolled.clone(), Act::Scroll { toward });
                }))
        });
        let typed = d.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(8.))
            .child(tools)
            .children(picture)
            .when(screen.is_none(), |s| {
                s.child(empty(faces, crate::tr("Reading its screen…")))
            })
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child(div().flex_1().child(Input::new(&self.sims.typed)))
                    .child(
                        ui::button("type", crate::tr("Type"), Weight::Secondary).on_click(
                            cx.listener(move |this, _, window, cx| {
                                let text = this.sims.typed.read(cx).value().to_string();
                                this.sims
                                    .typed
                                    .update(cx, |i, cx| i.set_value("", window, cx));
                                this.device_act(typed.clone(), Act::Type { text });
                            }),
                        ),
                    ),
            )
    }
}
