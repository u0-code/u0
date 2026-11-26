//! Settings › Sharing & access: this Ghost reached from other machines
//! (listening, the tunnel, pairing links and their QR codes, the devices
//! paired and their keys) — the server's, whichever machine it is on. And
//! Settings › Connections: the Ghosts elsewhere this Mac has paired with,
//! the key it made for each, and the one the window is on.

use gpui::component::{Disableable as _, Sizable as _, input::Input, switch::Switch};
use gpui::{ClipboardItem, Context, Div, SharedString, Window, div, prelude::*, px};

use super::prefs::{group, page, setting};
use crate::{
    access::Named,
    desk::Desk,
    fonts::Faces,
    key::{self, Key},
    link,
    servers::Servers,
    share::{Share, Tunnel},
    theme,
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
    pub(super) fn sharing_tab(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let on = self.transport.elsewhere().unwrap_or("this Mac").to_owned();
        let lede = format!(
            "Reach the Ghost on {on} from another Mac: its agents, sessions, files and terminals, as here."
        );
        let Some(shared) = &self.pages.shared else {
            return page(faces, "Sharing & access", &lede);
        };
        let serving = shared.serving.as_ref().is_some_and(Result::is_ok);
        let listen = setting(
            faces,
            "Let other machines reach this Ghost",
            &match &shared.serving {
                Some(Ok(port)) => {
                    format!("Listening on 127.0.0.1:{port}, for a tunnel or an ssh -L to reach.")
                }
                Some(Err(why)) => why.clone(),
                None => "Only through 127.0.0.1, and only a device paired with it.".into(),
            },
            Switch::new("serve")
                .small()
                .checked(shared.serving.is_some())
                .on_click(cx.listener(move |this, _, _, _| this.share(Share::Serve(!serving)))),
        );
        let (status, running) = match &shared.tunnel {
            Tunnel::Off => ("Off".to_owned(), false),
            Tunnel::Starting => ("Starting".to_owned(), true),
            Tunnel::Up(host) => (format!("Connected at {host}"), true),
            Tunnel::Down(why) => (format!("Stopped: {why}"), false),
        };
        let tunnel = setting(
            faces,
            "Tunnel (cloudflared)",
            &status,
            ui::button(
                "tunnel",
                if running { "Stop" } else { "Start" },
                Weight::Secondary,
            )
            .disabled(!serving)
            .on_click(cx.listener(move |this, _, _, _| this.share(Share::Tunnel(!running)))),
        );
        let named = shared.named.clone();
        let mode = setting(
            faces,
            "Address",
            &match &named {
                Some(host) => format!("A named tunnel of yours, at {host}."),
                None => "A quick tunnel: a new address each time it starts, so pair again with a new link.".into(),
            },
            match named {
                Some(_) => ui::button("unname", "Use a quick tunnel", Weight::Quiet)
                    .on_click(cx.listener(|this, _, _, _| this.share(Share::Named(None))))
                    .into_any_element(),
                None => div()
                    .flex()
                    .gap(px(6.))
                    .child(div().w(px(150.)).child(Input::new(&self.pages.named_host).small()))
                    .child(div().w(px(150.)).child(Input::new(&self.pages.named_token).small()))
                    .child(ui::button("name", "Use", Weight::Secondary).on_click(cx.listener(
                        |this, _, _, cx| {
                            let host = this.pages.named_host.read(cx).value().trim().to_owned();
                            let token = this.pages.named_token.read(cx).value().trim().to_owned();
                            if !host.is_empty() && !token.is_empty() {
                                this.share(Share::Named(Some(Named { token, host })));
                            }
                        },
                    )))
                    .into_any_element(),
            },
        );
        let awake = setting(
            faces,
            "Keep the Mac awake while serving",
            "So that it can be reached with its lid open and no one at it.",
            Switch::new("serve-awake")
                .small()
                .checked(shared.awake)
                .on_click(cx.listener({
                    let awake = shared.awake;
                    move |this, _, _, _| this.share(Share::Awake(!awake))
                })),
        );
        let link = setting(
            faces,
            "Pair a device",
            "A link that works once, for ten minutes: open it on the other Mac, or paste it in Settings › Connections there.",
            ui::button("pair", "Create link", Weight::Primary)
                .disabled(!serving)
                .on_click(cx.listener(|this, _, _, _| this.share(Share::Pair))),
        );
        let fresh = shared.link.clone().map(|(link, at)| {
            let copy = link.clone();
            // The link on a line of its own: it is long, and wraps.
            setting(faces, &link, &format!("Made {}", ago(at)), {
                ui::button("copy-link", "Copy", Weight::Secondary).on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                })
            })
        });
        // Its QR code under it, for a camera to read off this screen.
        let code = (shared.link.as_ref())
            .map(|(link, _)| div().p(px(16.)).flex().justify_center().child(qr(link)));
        let devices = match &shared.devices {
            Err(why) => vec![Some(setting(faces, "Devices", why, div()))],
            Ok(devices) if devices.is_empty() => {
                vec![Some(setting(faces, "No device is paired", "", div()))]
            }
            Ok(devices) => (devices.iter())
                .map(|d| {
                    let print = key::fingerprint(&d.key);
                    let public = d.key.clone();
                    Some(setting(
                        faces,
                        &d.label,
                        &format!(
                            "{print}. Paired {}; last here {}",
                            ago(d.paired),
                            ago(d.seen)
                        ),
                        ui::button(
                            SharedString::from(format!("revoke-{print}")),
                            "Revoke",
                            Weight::Secondary,
                        )
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.share(Share::Revoke(public.clone()));
                        })),
                    ))
                })
                .collect(),
        };
        page(faces, "Sharing & access", &lede)
            .children(group(
                faces,
                "Reach",
                vec![Some(listen), Some(tunnel), Some(mode), Some(awake)],
            ))
            .children(group(faces, "Pairing", vec![Some(link), fresh, code]))
            .children(group(faces, "Devices paired", devices))
    }

    pub(super) fn connections_tab(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let opened = self.transport.elsewhere().map(str::to_owned);
        let row = |id: SharedString, name: &str, about: &str, here: bool| {
            setting(
                faces,
                name,
                about,
                if here {
                    ui::fine(faces, "This window").into_any_element()
                } else {
                    let name = (id.as_ref() != "this-mac").then(|| name.to_owned());
                    ui::button(id, "Open", Weight::Secondary)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_on(name.clone(), window, cx);
                        }))
                        .into_any_element()
                },
            )
        };
        let mut rows = vec![Some(row(
            "this-mac".into(),
            "This Mac",
            "Its own agents and projects.",
            opened.is_none(),
        ))];
        for s in &self.pages.servers.servers {
            let here = opened.as_ref() == Some(&s.name);
            let name = s.name.clone();
            rows.push(Some(
                row(
                    SharedString::from(format!("open-{}", s.name)),
                    &s.name,
                    &format!("{}. Your key: {}", s.host, key::fingerprint(&s.key)),
                    here,
                )
                .child(
                    ui::button(
                        SharedString::from(format!("forget-{}", s.name)),
                        "Forget",
                        Weight::Quiet,
                    )
                    .disabled(here)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pages.servers.forget(&name);
                        this.keep_servers();
                        cx.notify();
                    })),
                ),
            ));
        }
        let add = setting(
            faces,
            "Add server",
            "Paste the link made in Sharing & access on the other machine. This Mac makes a key for it, and sends only its public half.",
            div()
                .flex()
                .gap(px(6.))
                .child(
                    div()
                        .w(px(260.))
                        .child(Input::new(&self.pages.pair_link).small()),
                )
                .child(
                    ui::button("add-server", "Pair", Weight::Primary)
                        .loading(self.pages.pairing)
                        .on_click(cx.listener(|this, _, window, cx| this.pair(window, cx))),
                ),
        );
        page(
            faces,
            "Connections",
            "The Ghosts on other machines this Mac is paired with. The window opens on one at a time; Ghost restarts to change it.",
        )
        .children(group(faces, "Servers", rows))
        .children(group(faces, "Pair", vec![Some(add)]))
        .child(
            ui::fine(
                faces,
                "Each key's private half is kept in this Mac's Keychain; a server keeps only the public half.",
            )
            .text_color(theme::faint()),
        )
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
