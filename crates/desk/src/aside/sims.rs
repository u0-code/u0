//! The Devices tab: the iOS simulators and Android emulators of the server
//! the session is on, as t3code's Devices panel. One chosen and running
//! shows its screen, each frame asked for as the last one comes while the
//! tab is shown; it is touched by clicking its screen, typed into, and has
//! its Home key (an Android one's Back and Recents too).

use std::sync::{Arc, Mutex};

use gpui::component::input::{Input, InputState};
use gpui::{AppContext as _, Context, Div, Entity, Window, div, prelude::*, px};

use super::{empty, fine};
use crate::{
    desk::Desk,
    devices::{Act, Device, Key},
    fonts::Faces,
    hub::Host,
    proto::Call,
    ui::{self, Weight},
};

/// How wide a device's screen is drawn.
const WIDE: f32 = 300.;

/// The devices as the server last listed them (none until it has; whether
/// it was asked), and
/// what is missing to have any; the one chosen, its screen last read with
/// its size in pixels, whether one is being read; what is to be typed.
pub struct Sims {
    pub listed: Option<(Vec<Device>, Vec<String>)>,
    pub asked: bool,
    pub on: Option<String>,
    pub screen: Option<(String, Arc<gpui::Image>, (u32, u32))>,
    pub reading: bool,
    pub typed: Entity<InputState>,
    /// Where the screen was drawn last, for a click on it to be read against.
    pub origin: Arc<Mutex<gpui::Point<gpui::Pixels>>>,
}

impl Sims {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        Self {
            listed: None,
            asked: false,
            on: None,
            screen: None,
            reading: false,
            typed: cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("Type into it"))),
            origin: Arc::default(),
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
        if self.surface() != Some(crate::desk::Surface::Devices) {
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
            return div()
                .p(px(16.))
                .child(empty(faces, crate::tr("Looking for devices…")));
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
                    if d.android { "Android" } else { "iOS" }
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
                d.child(empty(faces, crate::tr("No simulator or emulator here.")))
            })
            .children(
                missing
                    .iter()
                    .map(|m| fine(faces, crate::tr_with("Not found: %{m}", &[("m", m)]))),
            )
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
            let (at, touched) = (self.sims.origin.clone(), d.clone());
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
                .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, _| {
                    let o = *at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let p = e.position() - o;
                    let scale = w as f32 / WIDE;
                    let (x, y) = (f32::from(p.x) * scale, f32::from(p.y) * scale);
                    let tap = Act::Tap {
                        x: x as u32,
                        y: y as u32,
                    };
                    this.device_act(touched.clone(), tap);
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
