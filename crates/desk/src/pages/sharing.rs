//! Settings › Sharing & access: this Ghost reached from other machines
//! (listening, the tunnel, pairing links and their QR codes, the devices
//! paired and their keys) — the server's, whichever machine it is on. And
//! Settings › Connections: the Ghosts elsewhere this Mac has paired with,
//! the key it made for each, and the one the window is on.

use gpui::component::{
    Disableable as _, Sizable as _,
    input::Input,
    setting::{SettingGroup, SettingPage},
    switch::Switch,
};
use gpui::{
    AnyElement, ClipboardItem, Context, Div, SharedString, WeakEntity, Window, div, prelude::*, px,
};

use super::prefs::{group, item, whole};
use crate::{
    access::Named,
    desk::Desk,
    fonts::Faces,
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

/// The QR code of `link`, dark on light whatever the theme, each row's
/// runs of one colour drawn as one box.
fn qr(link: &str) -> Div {
    const MODULE: f32 = 4.;
    let code = link::qr(link);
    let dark: Vec<bool> = code
        .to_colors()
        .iter()
        .map(|c| c.select(true, false))
        .collect();
    let rows = dark.chunks(code.width()).map(|row| {
        div()
            .flex()
            .children(row.chunk_by(|a, b| a == b).map(|run| {
                div()
                    .w(px(MODULE * run.len() as f32))
                    .h(px(MODULE))
                    .when(run[0], |d| d.bg(gpui::black()))
            }))
    });
    div()
        .p(px(MODULE * 4.))
        .bg(gpui::white())
        .flex()
        .flex_col()
        .children(rows)
}

impl Desk {
    /// Sharing & access: this Ghost reached from other machines.
    pub(super) fn sharing_tab(&self, w: &WeakEntity<Desk>) -> SettingPage {
        let on = self.transport.elsewhere().unwrap_or("this Mac").to_owned();
        let page = SettingPage::new("Sharing & access").description(format!(
            "Reach the Ghost on {on} from another Mac: its agents, sessions, files and terminals, as here."
        ));
        let Some(shared) = &self.pages.shared else {
            return page.group(SettingGroup::new().item(whole(w, &[], |_, faces, _| {
                ui::fine(faces, "Asking the server how it is shared…").into_any_element()
            })));
        };
        let serving = shared.serving.as_ref().is_some_and(Result::is_ok);
        let listening = match &shared.serving {
            Some(Ok(port)) => {
                format!("Listening on 127.0.0.1:{port}, for a tunnel or an ssh -L to reach.")
            }
            Some(Err(why)) => why.clone(),
            None => "Only through 127.0.0.1, and only a device paired with it.".into(),
        };
        let asked = shared.serving.is_some();
        let listen = item(
            w,
            "Let other machines reach this Ghost",
            listening,
            move |_, _, cx| {
                Switch::new("serve")
                    .small()
                    .checked(asked)
                    .on_click(cx.listener(move |this, _, _, _| this.share(Share::Serve(!serving))))
                    .into_any_element()
            },
        );
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
            .disabled(!serving)
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
        let mut pairing = vec![item(
            w,
            "Pair a device",
            "A link that works once, for ten minutes: open it on the other Mac, or paste it in Settings › Connections there.",
            move |_, _, cx| {
                ui::button("pair", "Create link", Weight::Primary)
                    .disabled(!serving)
                    .on_click(cx.listener(|this, _, _, _| this.share(Share::Pair)))
                    .into_any_element()
            },
        )];
        if let Some((link, at)) = shared.link.clone() {
            // The link on a line of its own: it is long, and wraps.
            let copy = link.clone();
            pairing.push(item(
                w,
                link.clone(),
                format!("Made {}", ago(at)),
                move |_, _, _| {
                    let copy = copy.clone();
                    ui::button("copy-link", "Copy", Weight::Secondary)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                        })
                        .into_any_element()
                },
            ));
            // Its QR code under it, for a camera to read off this screen.
            pairing.push(whole(w, &[], move |_, _, _| {
                div()
                    .w_full()
                    .p(px(16.))
                    .flex()
                    .justify_center()
                    .child(qr(&link))
                    .into_any_element()
            }));
        }
        let none = |_: &mut Desk, _: &Faces, _: &mut Context<Desk>| div().into_any_element();
        let devices = match &shared.devices {
            Err(why) => vec![item(w, "Devices", why.clone(), none)],
            Ok(devices) if devices.is_empty() => vec![item(w, "No device is paired", "", none)],
            Ok(devices) => (devices.iter())
                .map(|d| {
                    let print = key::fingerprint(&d.key);
                    let public = d.key.clone();
                    item(
                        w,
                        d.label.clone(),
                        format!(
                            "{print}. Paired {}; last here {}",
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
        };
        page.group(group("Reach", vec![listen, tunnel, mode, keep_awake]))
            .group(group("Pairing", pairing))
            .group(group("Devices paired", devices))
    }

    /// Connections: the Ghosts elsewhere this Mac has paired with, the key
    /// it made for each, and the one the window is on.
    pub(super) fn connections_tab(&self, w: &WeakEntity<Desk>) -> SettingPage {
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
        let add = item(
            w,
            "Add server",
            "Paste the link made in Sharing & access on the other machine. This Mac makes a key for it, and sends only its public half.",
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
        );
        SettingPage::new("Connections")
            .description(
                "The Ghosts on other machines this Mac is paired with. The window opens on one at a time; Ghost restarts to change it. Each key's private half is kept in this Mac's Keychain; a server keeps only the public half.",
            )
            .group(group("Servers", rows))
            .group(group("Pair", vec![add]))
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
                this.pages.servers.forget(&name);
                this.keep_servers();
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
    fn share(&mut self, share: Share) {
        self.transport.send(crate::proto::Call::Share(share));
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
    /// fingerprint of the key this Mac makes for it.
    pub(crate) fn ask_pair(
        host: String,
        code: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::component::WindowExt as _;
        let key = Key::generate();
        let kept = key.kept().to_owned();
        let about = format!(
            "This Mac makes a key for it and sends only its public half, {}. The server lists it under Devices paired.",
            key::fingerprint(&key.public())
        );
        let desk = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (desk, host, code, kept) = (desk.clone(), host.clone(), code.clone(), kept.clone());
            alert
                .title(format!("Pair with {host}?"))
                .description(about.clone())
                .ok_text("Pair")
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    let key = Key::read(&kept).expect("a key just made reads");
                    desk.update(cx, |d, cx| {
                        d.pages.pairing = true;
                        crate::hub::pair(host.clone(), code.clone(), key, d.tx.clone());
                        cx.notify();
                    });
                    true
                })
        });
    }

    /// Keep the servers paired with.
    pub(crate) fn keep_servers(&mut self) {
        if let Err(why) = self.pages.servers.save(&self.home) {
            self.trouble("Couldn't keep your servers", Some(why), None);
        }
    }

    /// Open the window on the server `name` (none, this Mac's own):
    /// Ghost restarts on it.
    fn open_on(&mut self, name: Option<String>, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.pages.servers.open = name;
        self.keep_servers();
        self.reopen(window, cx);
    }

    /// The servers paired with, read again for Connections.
    pub(crate) fn read_servers(&mut self) {
        match Servers::load(&self.home) {
            Ok(servers) => self.pages.servers = servers,
            Err(why) => self.trouble("Couldn't read your servers", Some(why), None),
        }
    }
}
