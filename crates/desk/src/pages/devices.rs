//! Settings › Devices: Ghost used from one Mac on another, each way, a
//! switch for each at the top. "Let other devices control this Mac": its
//! server reached from elsewhere (listening, the tunnel, the pairing link
//! and its QR code, the devices paired and their keys), the server's,
//! whichever machine it is on. "Control other Macs": the Ghosts elsewhere
//! this Mac has paired with, the key it made for each, the one the window
//! is on, and pairing with a link. And the sheet that asks before pairing,
//! which can pair both ways.

use gpui::component::{
    Disableable as _, Sizable as _,
    checkbox::Checkbox,
    input::Input,
    setting::{SettingGroup, SettingPage},
    switch::Switch,
};
use gpui::{
    AnyElement, ClipboardItem, Context, SharedString, WeakEntity, Window, div, prelude::*, px,
};

use super::prefs::{group, item, whole};
use crate::{
    access::Named,
    desk::Desk,
    fonts::Faces,
    hub::Transport,
    key::{self, Key},
    link,
    servers::Servers,
    share::{Share, Tunnel},
    ui::{self, Weight},
    when,
};

/// How long ago `secs` (since 1970) was.
fn ago(secs: u64) -> String {
    when::ago(crate::store::time(secs), std::time::SystemTime::now())
}

/// The QR code of `link`, black on white whatever the theme, inside its
/// quiet zone of four modules: drawn by qrcode as an SVG. A module is 4
/// points, a whole number of pixels at any scale, so its edges stay sharp.
fn qr(link: &str) -> gpui::Img {
    const MODULE: u32 = 4;
    let code = link::qr(link);
    let side = (code.width() as u32 + 8) * MODULE;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .module_dimensions(MODULE, MODULE)
        .build();
    gpui::img(std::sync::Arc::new(gpui::Image::from_bytes(
        gpui::ImageFormat::Svg,
        svg.into_bytes(),
    )))
    .size(px(side as f32))
}

impl Desk {
    /// Devices: this Mac controlled from others, and others from it.
    pub(super) fn devices_tab(&self, w: &WeakEntity<Desk>) -> SettingPage {
        let on = self.transport.elsewhere().unwrap_or("this Mac").to_owned();
        // None until the server has said how it is shared.
        let serving = self.pages.shared.as_ref().map(|s| s.serving.clone());
        let about = match &serving {
            None => format!("Asking {on} how it is shared…"),
            Some(Some(Ok(port))) => format!(
                "Listening on 127.0.0.1:{port}, for a tunnel or an ssh -L to reach. Only a device paired with it comes in."
            ),
            Some(Some(Err(why))) => why.clone(),
            Some(None) => "Off: nothing listens, and no device comes in.".into(),
        };
        let (known, asked) = (serving.is_some(), serving.flatten().is_some());
        let serve = item(
            w,
            format!("Let other devices control {on}"),
            about,
            move |_, _, cx| {
                Switch::new("serve")
                    .small()
                    .checked(asked)
                    .disabled(!known)
                    .on_click(cx.listener(move |this, _, _, _| this.share(Share::Serve(!asked))))
                    .into_any_element()
            },
        );
        let control = self.controls();
        let controlling = item(
            w,
            "Control other Macs",
            if control {
                "Open a Mac paired with in this window, or pair with another's link."
            } else {
                "Off: this Mac controls only itself."
            },
            move |_, _, cx| {
                Switch::new("control")
                    .small()
                    .checked(control)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_control(!control, window, cx);
                    }))
                    .into_any_element()
            },
        );
        let mut page = SettingPage::new("Devices")
            .description(
                "Use Ghost on one Mac from another: its agents, sessions, files and terminals, as there. Each device comes in by a key of its own, made as it pairs: its private half stays in that device's Keychain, and only the public half, shown by its fingerprint, is sent.",
            )
            .group(SettingGroup::new().items(vec![serve, controlling]));
        if asked {
            page = page
                .group(group("Pair a device with this Mac", self.pairing_link(w)))
                .group(group("Reach", self.reached(w)))
                .group(group("Devices that control this Mac", self.devices(w)));
        }
        if control {
            page = page
                .group(group("Macs this Mac controls", self.machines(w)))
                .group(group("Pair with another Mac", vec![Self::pair_field(w)]));
        }
        page
    }

    /// Whether this Mac controls others: as kept, or as it does now.
    fn controls(&self) -> bool {
        self.pages.servers.control || self.transport.elsewhere().is_some()
    }

    /// The pairing link offered, its QR code under it; or the way to make
    /// another.
    fn pairing_link(&self, w: &WeakEntity<Desk>) -> Vec<gpui::component::setting::SettingItem> {
        let renew = |w: &WeakEntity<Desk>, about: &str| {
            item(w, "Pairing link", about.to_owned(), |_, _, cx| {
                ui::button("pair", "New link", Weight::Primary)
                    .on_click(cx.listener(|this, _, _, _| this.share(Share::Serve(true))))
                    .into_any_element()
            })
        };
        let Some((link, at)) = self.pages.shared.as_ref().and_then(|s| s.link.clone()) else {
            return vec![renew(
                w,
                "The last link was used, or is out of date: make another.",
            )];
        };
        // The link on a line of its own: it is long, and wraps.
        let copy = link.clone();
        vec![
            item(
                w,
                link.clone(),
                format!(
                    "Works once, for ten minutes (made {}): open it on the other Mac, or scan its code, or paste it under Pair with another Mac there.",
                    ago(at)
                ),
                move |_, _, cx| {
                    let copy = copy.clone();
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(ui::button("copy-link", "Copy", Weight::Secondary).on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                            },
                        ))
                        .child(
                            ui::button("pair", "New link", Weight::Quiet).on_click(
                                cx.listener(|this, _, _, _| this.share(Share::Serve(true))),
                            ),
                        )
                        .into_any_element()
                },
            ),
            // Its QR code under it, for a camera to read off this screen.
            whole(w, &[], move |_, _, _| {
                div()
                    .w_full()
                    .p(px(16.))
                    .flex()
                    .justify_center()
                    .child(qr(&link))
                    .into_any_element()
            }),
        ]
    }

    /// How this Mac is reached: the tunnel, its address, staying awake.
    fn reached(&self, w: &WeakEntity<Desk>) -> Vec<gpui::component::setting::SettingItem> {
        let Some(shared) = &self.pages.shared else {
            return Vec::new();
        };
        let (status, running) = match &shared.tunnel {
            Tunnel::Off => ("Off".to_owned(), false),
            Tunnel::Starting => ("Starting".to_owned(), true),
            Tunnel::Up(host) => (format!("Connected at {host}"), true),
            Tunnel::Down(why) => (format!("Stopped: {why}"), false),
        };
        let tunnel = item(w, "Tunnel (cloudflared)", status, move |_, _, cx| {
            ui::button(
                "tunnel",
                if running { "Stop" } else { "Start" },
                Weight::Secondary,
            )
            .on_click(cx.listener(move |this, _, _, _| this.share(Share::Tunnel(!running))))
            .into_any_element()
        });
        let named = shared.named.is_some();
        let address = match &shared.named {
            Some(host) => format!("A named tunnel of yours, at {host}."),
            None => {
                "A quick tunnel: a new address each time it starts, so pair again with a new link."
                    .into()
            }
        };
        let mode = item(w, "Address", address, move |d, _, cx| {
            if named {
                return ui::button("unname", "Use a quick tunnel", Weight::Quiet)
                    .on_click(cx.listener(|this, _, _, _| this.share(Share::Named(None))))
                    .into_any_element();
            }
            div()
                .flex()
                .gap(px(6.))
                .child(
                    div()
                        .w(px(150.))
                        .child(Input::new(&d.pages.named_host).small()),
                )
                .child(
                    div()
                        .w(px(150.))
                        .child(Input::new(&d.pages.named_token).small()),
                )
                .child(
                    ui::button("name", "Use", Weight::Secondary).on_click(cx.listener(
                        |this, _, _, cx| {
                            let host = this.pages.named_host.read(cx).value().trim().to_owned();
                            let token = this.pages.named_token.read(cx).value().trim().to_owned();
                            if !host.is_empty() && !token.is_empty() {
                                this.share(Share::Named(Some(Named { token, host })));
                            }
                        },
                    )),
                )
                .into_any_element()
        });
        let awake = shared.awake;
        let keep_awake = item(
            w,
            "Keep the Mac awake while serving",
            "So that it can be reached with its lid open and no one at it.",
            move |_, _, cx| {
                Switch::new("serve-awake")
                    .small()
                    .checked(awake)
                    .on_click(cx.listener(move |this, _, _, _| this.share(Share::Awake(!awake))))
                    .into_any_element()
            },
        );
        vec![tunnel, mode, keep_awake]
    }

    /// The devices paired with this Mac, each by its key's fingerprint.
    fn devices(&self, w: &WeakEntity<Desk>) -> Vec<gpui::component::setting::SettingItem> {
        let none = |_: &mut Desk, _: &Faces, _: &mut Context<Desk>| div().into_any_element();
        match self.pages.shared.as_ref().map(|s| &s.devices) {
            None => Vec::new(),
            Some(Err(why)) => vec![item(w, "Devices", why.clone(), none)],
            Some(Ok(devices)) if devices.is_empty() => {
                vec![item(w, "No device is paired", "", none)]
            }
            Some(Ok(devices)) => (devices.iter())
                .map(|d| {
                    let print = key::fingerprint(&d.key);
                    let public = d.key.clone();
                    item(
                        w,
                        d.label.clone(),
                        format!(
                            "Its key: {print}. Paired {}; last here {}",
                            ago(d.paired),
                            ago(d.seen)
                        ),
                        move |_, _, cx| {
                            let public = public.clone();
                            ui::button(
                                SharedString::from(format!("revoke-{print}")),
                                "Revoke",
                                Weight::Secondary,
                            )
                            .on_click(cx.listener(move |this, _, _, _| {
                                this.share(Share::Revoke(public.clone()));
                            }))
                            .into_any_element()
                        },
                    )
                })
                .collect(),
        }
    }

    /// This Mac and the Macs paired with: the one the window is on, and
    /// the way to open each other one.
    fn machines(&self, w: &WeakEntity<Desk>) -> Vec<gpui::component::setting::SettingItem> {
        let mut rows = vec![item(
            w,
            "This Mac",
            "Its own agents and projects.",
            |d, faces, cx| d.server_control(None, faces, cx),
        )];
        for s in &self.pages.servers.servers {
            let name = s.name.clone();
            rows.push(item(
                w,
                s.name.clone(),
                format!("{}. Your key: {}", s.host, key::fingerprint(&s.key)),
                move |d, faces, cx| d.server_control(Some(&name), faces, cx),
            ));
        }
        rows
    }

    /// Where a link from another Mac is pasted to pair with it.
    fn pair_field(w: &WeakEntity<Desk>) -> gpui::component::setting::SettingItem {
        item(
            w,
            "Pair with a link",
            "Paste the link the other Mac shows under Pair a device with this Mac, or open it here. Ghost restarts on a Mac to open it.",
            |d, _, cx| {
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .w(px(260.))
                            .child(Input::new(&d.pages.pair_link).small()),
                    )
                    .child(
                        ui::button("add-server", "Pair", Weight::Primary)
                            .loading(d.pages.pairing)
                            .on_click(cx.listener(|this, _, window, cx| this.pair(window, cx))),
                    )
                    .into_any_element()
            },
        )
    }

    /// What can be done with the server `name` (none, this Mac): open the
    /// window on it, or say it is open here; and forget one elsewhere.
    fn server_control(
        &self,
        name: Option<&str>,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let here = self.transport.elsewhere() == name;
        let open = if here {
            ui::fine(faces, "This window").into_any_element()
        } else {
            let to = name.map(str::to_owned);
            let id = to
                .as_ref()
                .map_or_else(|| "this-mac".into(), |n| format!("open-{n}"));
            ui::button(SharedString::from(id), "Open", Weight::Secondary)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_on(to.clone(), window, cx);
                }))
                .into_any_element()
        };
        let forget = name.map(|name| {
            let name = name.to_owned();
            ui::button(
                SharedString::from(format!("forget-{name}")),
                "Forget",
                Weight::Quiet,
            )
            .disabled(here)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_servers(|s| {
                    s.forget(&name);
                    Ok(())
                });
                cx.notify();
            }))
        });
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(open)
            .children(forget)
            .into_any_element()
    }

    /// Ask the server something of sharing.
    pub(crate) fn share(&mut self, share: Share) {
        self.transport.send(crate::proto::Call::Share(share));
    }

    /// Control other Macs, or only this one: turned off on another Mac,
    /// the window opens on this Mac's own again.
    pub(crate) fn set_control(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.change_servers(|s| {
            s.control = on;
            Ok(())
        });
        if !on && self.transport.elsewhere().is_some() {
            self.open_on(None, window, cx);
        }
        cx.notify();
    }

    /// Pair with the server whose link is pasted.
    fn pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pasted = self.pages.pair_link.read(cx).value().trim().to_owned();
        match link::pairing(&pasted) {
            Some((host, code)) => Self::ask_pair(host, code, window, cx),
            None => self.trouble(
                "That is not a pairing link",
                Some("It begins ghost://pair".into()),
                None,
            ),
        }
        cx.notify();
    }

    /// Ask before pairing with the server at `host` by `code`, showing the
    /// fingerprint of the key this Mac makes for it; and, where this Mac's
    /// own server is in this window, whether it pairs back.
    pub(crate) fn ask_pair(
        host: String,
        code: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::component::WindowExt as _;
        let key = Key::generate();
        let kept = key.kept().to_owned();
        let print = key::fingerprint(&key.public());
        let desk = cx.entity();
        // Whether it pairs both ways, as the sheet's box is ticked.
        let ticked = std::rc::Rc::new(std::cell::Cell::new(false));
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let offer = matches!(desk.read(cx).transport, Transport::Local(_));
            let both = offer && ticked.get();
            let faces = crate::fonts::faces(cx).clone();
            let mut about = div().flex().flex_col().gap(px(12.)).child(ui::text(
                &faces,
                format!(
                    "This Mac makes a key for it and sends only its public half, {print}. {host} lists it under the devices that control it."
                ),
            ));
            if offer {
                let ticked = ticked.clone();
                about = about.child(
                    Checkbox::new("pair-both")
                        .label(format!("Also let {host} control this Mac"))
                        .checked(both)
                        .on_click(move |on, window, _| {
                            ticked.set(*on);
                            window.refresh();
                        }),
                );
            }
            if both {
                about = about.child(ui::fine(
                    &faces,
                    format!(
                        "This Mac listens, and {host} pairs back with a key of its own, which this Mac lists under the devices that control it."
                    ),
                ));
            }
            let (desk, host, code, kept) = (desk.clone(), host.clone(), code.clone(), kept.clone());
            alert
                .title(format!("Pair with {host}?"))
                .description(about)
                .ok_text("Pair")
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    desk.update(cx, |d, cx| d.pair_with(&host, &code, &kept, both, cx));
                    true
                })
        });
    }

    /// Pair with the server at `host` by `code` with the key kept as
    /// `kept`; `both` ways, if this Mac can be reached back.
    fn pair_with(
        &mut self,
        host: &str,
        code: &str,
        kept: &str,
        both: bool,
        cx: &mut Context<Self>,
    ) {
        let back = match &self.transport {
            Transport::Local(server) if both => match server.back_link(host) {
                Ok(back) => Some(back),
                Err(why) => {
                    self.trouble(
                        format!("{host} can't be let control this Mac"),
                        Some(format!("{why} This Mac pairs with it one way only.")),
                        None,
                    );
                    None
                }
            },
            _ => None,
        };
        self.pages.pairing = true;
        let key = Key::read(kept).expect("a key just made reads");
        crate::hub::pair(
            self.home.clone(),
            host.to_owned(),
            code.to_owned(),
            key,
            back,
            self.tx.clone(),
        );
        cx.notify();
    }

    /// Change the servers paired with as `change` does, and show them so.
    pub(crate) fn change_servers(
        &mut self,
        change: impl FnOnce(&mut Servers) -> Result<(), String>,
    ) {
        if let Err(why) = Servers::change(&self.home, change) {
            self.trouble("Couldn't keep your servers", Some(why), None);
        }
        self.read_servers();
    }

    /// Open the window on the server `name` (none, this Mac's own):
    /// Ghost restarts on it.
    fn open_on(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.change_servers(|s| {
            s.open = name;
            Ok(())
        });
        self.reopen(window, cx);
    }

    /// The servers paired with, read again for Devices.
    pub(crate) fn read_servers(&mut self) {
        match Servers::load(&self.home) {
            Ok(servers) => self.pages.servers = servers,
            Err(why) => self.trouble("Couldn't read your servers", Some(why), None),
        }
    }
}
