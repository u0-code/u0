//! Settings › Devices: Ghost used from one Mac on another, each way, a
//! switch for each at the top. "Let other devices control this Mac": its
//! server reached from elsewhere (listening, the tunnel, the pairing link
//! and its QR code, the devices paired and their keys), the server's the
//! settings are of. "Control other Macs": the Ghosts elsewhere this Mac
//! has paired with, which the window holds beside its own, the key it made
//! for each and how each stands, and pairing with a link. And the sheet
//! that asks before pairing, which can pair both ways.

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
    access::{Grant, Named, Scope},
    desk::Desk,
    fonts::Faces,
    hub::{Host, Machine, Up},
    key::{self, Key},
    link,
    reach::{Bind, Route},
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

/// What a pairing link made for `grant` lets in, said in a phrase.
fn granted(grant: Grant) -> String {
    let what = match grant.scope {
        Scope::Full => "everything",
        Scope::Read => "read only",
    };
    let long = match grant.days {
        None => "until revoked".to_owned(),
        Some(1) => "for a day".to_owned(),
        Some(d) => format!("for {d} days"),
    };
    let asked = if grant.ask {
        ", once you let it in"
    } else {
        ""
    };
    format!("For a device: {what}, {long}{asked}")
}

/// What a device paired may do, and until when, said in a phrase.
fn standing(d: &crate::access::Device) -> String {
    let what = match d.scope {
        Scope::Full => "Everything",
        Scope::Read => "Read only",
    };
    let until = d.until.map_or_else(String::new, |u| {
        format!(
            ", until {}",
            when::said_at(crate::store::time(u), std::time::SystemTime::now(), false)
        )
    });
    format!("{what}{until}")
}

impl Desk {
    /// Devices: this Mac controlled from others, and others from it.
    pub(super) fn devices_tab(&self, w: &WeakEntity<Desk>) -> SettingPage {
        let on = (self.machine(self.pages.on))
            .filter(|_| self.pages.on != Host::HERE)
            .map_or_else(|| crate::tr("this Mac").to_string(), |m| m.name.clone());
        // None until the server has said how it is shared.
        let serving = self.pages.shared.as_ref().map(|s| s.serving.clone());
        let about = match &serving {
            None => crate::tr_with("Asking %{on} how it is shared…", &[("on", &on)]),
            Some(Some(Ok(port))) => crate::tr_with(
                "Listening on 127.0.0.1:%{port}, and on the networks chosen under Reach. Only a device paired with it comes in.",
                &[("port", port)],
            ),
            Some(Some(Err(_)) | None) => crate::tr("Off: nothing listens, and no device comes in."),
        };
        // The switch says what runs: on only while it listens. Asked for
        // and failed, it is off, and why stands under it as trouble; a
        // click lets go of the wish, and the next asks again.
        let failed = match &serving {
            Some(Some(Err(why))) => Some(why.clone()),
            _ => None,
        };
        let known = serving.is_some();
        let listening = matches!(serving, Some(Some(Ok(_))));
        let asked = matches!(serving, Some(Some(_)));
        let serve = item(
            w,
            crate::tr_with("Let other devices control %{on}", &[("on", &on)]),
            about,
            move |_, _, cx| {
                Switch::new("serve")
                    .accessibility_label(crate::tr("Let other devices control this Mac"))
                    .small()
                    .checked(listening)
                    .disabled(!known)
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.share(Share::Serve(!asked));
                        if !asked {
                            this.new_link();
                        }
                    }))
                    .into_any_element()
            },
        );
        let control = self.controls();
        let controlling = item(
            w,
            crate::tr("Control other Macs"),
            if control {
                "The Macs paired with are in this window beside this one; pair with another's link."
            } else {
                "Off: this Mac controls only itself."
            },
            move |_, _, cx| {
                Switch::new("control")
                    .accessibility_label(crate::tr("Control other Macs"))
                    .small()
                    .checked(control)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_control(!control, window, cx);
                    }))
                    .into_any_element()
            },
        );
        let mut page = SettingPage::new(crate::tr("Devices"))
            .description(
                crate::tr("Use Ghost on one Mac from another: its agents, sessions, files and terminals, as there. Each device comes in by a key of its own, made as it pairs: its private half stays in that device's Keychain, and only the public half, shown by its fingerprint, is sent."),
            )
            .group(SettingGroup::new().items(
                std::iter::once(serve)
                    .chain(failed.map(|why| {
                        whole(w, &[], move |_, _, _| ui::trouble(why.clone()).into_any_element())
                    }))
                    .chain([controlling])
                    .collect::<Vec<_>>(),
            ));
        if listening {
            page = page
                .group(group(
                    crate::tr("Pair a device with this Mac"),
                    self.pairing_link(w),
                ))
                .group(group(crate::tr("Reach"), self.reached(w)))
                .group(group(
                    crate::tr("Devices that control this Mac"),
                    self.devices(w),
                ));
        }
        if control {
            page = page
                .group(group(crate::tr("Macs this Mac controls"), self.machines(w)))
                .group(group(
                    crate::tr("Pair with another Mac"),
                    vec![Self::pair_field(w)],
                ));
        }
        page
    }

    /// Whether this Mac controls others, as kept.
    fn controls(&self) -> bool {
        self.pages.servers.control
    }

    /// The pairing link offered, its QR code under it; or the way to make
    /// another.
    fn pairing_link(&self, w: &WeakEntity<Desk>) -> Vec<gpui::component::setting::SettingItem> {
        let grant = self.pages.grant;
        // What the next link lets in: each choice set, and a link made
        // for it at once.
        let choose = |change: fn(&mut Grant), cx: &mut Context<Desk>| -> ui::Pick {
            Box::new(cx.listener(move |this, _, _, cx| {
                change(&mut this.pages.grant);
                this.new_link();
                cx.notify();
            }))
        };
        let scope = item(
            w,
            "What it may do",
            "Read only: it watches sessions, diffs and your projects' files, and changes nothing. Its calls are checked here, on this Mac.",
            move |_, _, cx| {
                ui::segmented(
                    "grant-scope",
                    vec![
                        (
                            "Everything",
                            grant.scope == Scope::Full,
                            choose(|g| g.scope = Scope::Full, cx),
                        ),
                        (
                            "Read only",
                            grant.scope == Scope::Read,
                            choose(|g| g.scope = Scope::Read, cx),
                        ),
                    ],
                )
                .into_any_element()
            },
        );
        let ends = item(
            w,
            "For how long",
            "After that it is turned away, and pairs again with a new link.",
            move |_, _, cx| {
                ui::segmented(
                    "grant-days",
                    vec![
                        (
                            "Until revoked",
                            grant.days.is_none(),
                            choose(|g| g.days = None, cx),
                        ),
                        (
                            "A day",
                            grant.days == Some(1),
                            choose(|g| g.days = Some(1), cx),
                        ),
                        (
                            "A week",
                            grant.days == Some(7),
                            choose(|g| g.days = Some(7), cx),
                        ),
                        (
                            "30 days",
                            grant.days == Some(30),
                            choose(|g| g.days = Some(30), cx),
                        ),
                    ],
                )
                .into_any_element()
            },
        );
        let ask = item(
            w,
            "Ask me before letting it in",
            "The device pairs, then waits under Devices that control this Mac until you let it in: for a link you send someone.",
            move |_, _, cx| {
                Switch::new("grant-ask")
                    .accessibility_label("Ask me before letting it in")
                    .small()
                    .checked(grant.ask)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pages.grant.ask = !grant.ask;
                        this.new_link();
                        cx.notify();
                    }))
                    .into_any_element()
            },
        );
        // Which way the link names: each this Mac is reached by now.
        let mut kinds: Vec<Route> = (self.pages.shared.as_ref())
            .and_then(|s| s.routes.as_ref().ok())
            .map(|r| r.iter().map(|(route, _)| *route).collect())
            .unwrap_or_default();
        kinds.dedup();
        let chosen = self.pages.route;
        let way = item(
            w,
            "Reached by",
            "The way the link names. One on a network pins this Mac's certificate in the link, so the device trusts this Mac alone.",
            move |_, _, cx| {
                let pick = |route: Option<Route>, cx: &mut Context<Desk>| -> ui::Pick {
                    Box::new(cx.listener(move |this, _, _, cx| {
                        this.pages.route = route;
                        this.new_link();
                        cx.notify();
                    }))
                };
                let mut choices = vec![("Best", chosen.is_none(), pick(None, cx))];
                for r in &kinds {
                    choices.push((r.name(), chosen == Some(*r), pick(Some(*r), cx)));
                }
                ui::segmented("link-route", choices).into_any_element()
            },
        );
        let renew = |w: &WeakEntity<Desk>, about: &str| {
            item(
                w,
                crate::tr("Pairing link"),
                about.to_owned(),
                |_, _, cx| {
                    ui::button("pair", crate::tr("New link"), Weight::Primary)
                        .on_click(cx.listener(|this, _, _, _| this.new_link()))
                        .into_any_element()
                },
            )
        };
        // The link this desk made, while the server says it is still good.
        let live = self.pages.shared.as_ref().and_then(|s| s.offer);
        let Some((link, at, made_for)) =
            (self.pages.offered.clone()).filter(|(_, made, grant)| live == Some((*made, *grant)))
        else {
            return vec![
                scope,
                ends,
                ask,
                way,
                renew(
                    w,
                    "The last link was used, or is out of date: make another.",
                ),
            ];
        };
        let lets_in = granted(made_for);
        // The link on a line of its own: it is long, and wraps.
        let copy = link.clone();
        vec![
            scope,
            ends,
            ask,
            way,
            item(
                w,
                link.clone(),
                crate::tr_with(
                    "%{lets_in}. Works once, for ten minutes (made %{ago}): open it on the other Mac, or scan its code, or paste it under Pair with another Mac there.",
                    &[("lets_in", &lets_in), ("ago", &ago(at))],
                ),
                move |_, _, cx| {
                    let copy = copy.clone();
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            ui::button("copy-link", crate::tr("Copy"), Weight::Secondary).on_click(
                                move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                },
                            ),
                        )
                        .child(
                            ui::button("pair", crate::tr("New link"), Weight::Quiet)
                                .on_click(cx.listener(|this, _, _, _| this.new_link())),
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
            Tunnel::Off => (crate::tr("Off"), false),
            Tunnel::Starting => (crate::tr("Starting"), true),
            Tunnel::Up(host) => (
                crate::tr_with("Connected at %{host}", &[("host", host)]),
                true,
            ),
            Tunnel::Down(why) => (crate::tr_with("Stopped: %{why}", &[("why", why)]), false),
        };
        let tunnel = item(
            w,
            crate::tr("Tunnel (cloudflared)"),
            status,
            move |_, _, cx| {
                ui::button(
                    "tunnel",
                    crate::tr(if running { "Stop" } else { "Start" }),
                    Weight::Secondary,
                )
                .on_click(cx.listener(move |this, _, _, _| this.share(Share::Tunnel(!running))))
                .into_any_element()
            },
        );
        let named = shared.named.is_some();
        let address = match &shared.named {
            Some(host) => crate::tr_with("A named tunnel of yours, at %{host}.", &[("host", host)]),
            None => {
                "A quick tunnel: a new address each time it starts, so pair again with a new link."
                    .into()
            }
        };
        let mode = item(w, crate::tr("Address"), address, move |d, _, cx| {
            if named {
                return ui::button("unname", crate::tr("Use a quick tunnel"), Weight::Quiet)
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
                    ui::button("name", crate::tr("Use"), Weight::Secondary).on_click(cx.listener(
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
            crate::tr("Keep the Mac awake while serving"),
            crate::tr("So that it can be reached with its lid open and no one at it."),
            move |_, _, cx| {
                Switch::new("serve-awake")
                    .accessibility_label(crate::tr("Keep the Mac awake while serving"))
                    .small()
                    .checked(awake)
                    .on_click(cx.listener(move |this, _, _, _| this.share(Share::Awake(!awake))))
                    .into_any_element()
            },
        );
        let bind = shared.bind;
        let listen = item(
            w,
            "Listen on",
            "Beyond this Mac and its tunnel: Tailscale, or every network it is on, Tailscale too. There Ghost speaks TLS itself, on the port after its own; a device pairs with a link that pins its certificate.",
            move |_, _, cx| {
                let pick = |to: Bind, cx: &mut Context<Desk>| -> ui::Pick {
                    Box::new(cx.listener(move |this, _, _, _| this.share(Share::Bind(to))))
                };
                ui::segmented(
                    "listen-on",
                    vec![
                        ("This Mac", bind == Bind::Here, pick(Bind::Here, cx)),
                        (
                            "Tailscale",
                            bind == Bind::Tailscale,
                            pick(Bind::Tailscale, cx),
                        ),
                        (
                            "Every network",
                            bind == Bind::Network,
                            pick(Bind::Network, cx),
                        ),
                    ],
                )
                .into_any_element()
            },
        );
        let none = |_: &mut Desk, _: &Faces, _: &mut Context<Desk>| div().into_any_element();
        let routes = match &shared.routes {
            Err(why) => vec![item(w, "Not on a network", why.clone(), none)],
            Ok(routes) => (routes.iter())
                .map(|(route, host)| item(w, route.name(), host.clone(), none))
                .collect(),
        };
        [tunnel, mode, listen]
            .into_iter()
            .chain(routes)
            .chain([keep_awake])
            .collect()
    }

    /// Make a new pairing link, as chosen above it.
    pub(crate) fn new_link(&mut self) {
        self.share(Share::Link(self.pages.grant, self.pages.route));
        // This Mac's own server, in this process, is asked for it at once;
        // one elsewhere tells this desk alone (`Event::Offered`).
        if let Some(server) = self.machine(self.pages.on).and_then(|m| m.local()) {
            self.pages.offered = server.offered();
        }
    }

    /// The devices paired with this Mac, each by its key's fingerprint.
    fn devices(&self, w: &WeakEntity<Desk>) -> Vec<gpui::component::setting::SettingItem> {
        let none = |_: &mut Desk, _: &Faces, _: &mut Context<Desk>| div().into_any_element();
        match self.pages.shared.as_ref().map(|s| &s.devices) {
            None => Vec::new(),
            Some(Err(why)) => vec![item(w, crate::tr("Devices"), why.clone(), none)],
            Some(Ok(devices)) if devices.is_empty() => {
                vec![item(w, crate::tr("No device is paired"), "", none)]
            }
            Some(Ok(devices)) => (devices.iter())
                .map(|d| {
                    let print = key::fingerprint(&d.key);
                    let public = d.key.clone();
                    let waiting = d.waiting;
                    let about = if waiting {
                        crate::tr_with(
                            "Waits to be let in (%{standing}). Its key: %{print}: check it is the one the device shows. Paired %{paired}",
                            &[
                                ("standing", &standing(d).to_lowercase()),
                                ("print", &print),
                                ("paired", &ago(d.paired)),
                            ],
                        )
                    } else {
                        crate::tr_with(
                            "%{standing}. Its key: %{print}. Paired %{paired}; last here %{seen}",
                            &[
                                ("standing", &standing(d)),
                                ("print", &print),
                                ("paired", &ago(d.paired)),
                                ("seen", &ago(d.seen)),
                            ],
                        )
                    };
                    item(w, d.label.clone(), about, move |_, _, cx| {
                        let (let_in, away) = (public.clone(), public.clone());
                        div()
                            .flex()
                            .gap(px(6.))
                            .when(waiting, |d| {
                                d.child(
                                    ui::button(
                                        SharedString::from(format!("approve-{print}")),
                                        crate::tr("Let in"),
                                        Weight::Primary,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, _| {
                                            this.share(Share::Approve(let_in.clone()));
                                        },
                                    )),
                                )
                            })
                            .child(
                                ui::button(
                                    SharedString::from(format!("revoke-{print}")),
                                    if waiting {
                                        crate::tr("Turn away")
                                    } else {
                                        crate::tr("Revoke")
                                    },
                                    Weight::Secondary,
                                )
                                .on_click(cx.listener(move |this, _, _, _| {
                                    this.share(Share::Revoke(away.clone()));
                                })),
                            )
                            .into_any_element()
                    })
                })
                .collect(),
        }
    }

    /// The Macs paired with, each in this window: how it stands, the way
    /// to reach it again and to forget it.
    fn machines(&self, w: &WeakEntity<Desk>) -> Vec<gpui::component::setting::SettingItem> {
        let port = crate::access::Access::new(&self.home)
            .kept()
            .map_or(0, |k| k.port);
        (self.pages.servers.held(port))
            .map(|s| {
                let name = s.name.clone();
                let held = (self.machines.iter()).find(|m| m.host != Host::HERE && m.name == name);
                let here = crate::update::this_build();
                let (host, print) = (&s.host, key::fingerprint(&s.key));
                let runs = match held.and_then(|m| m.version.as_ref()) {
                    Some(v) if *v == here => crate::tr_with(
                        "%{host}. Your key: %{print}. It runs %{v}, as this Mac does.",
                        &[("host", host), ("print", &print), ("v", v)],
                    ),
                    Some(v) => crate::tr_with(
                        "%{host}. Your key: %{print}. It runs %{v}; this Mac runs %{here}.",
                        &[("host", host), ("print", &print), ("v", v), ("here", &here)],
                    ),
                    None => crate::tr_with(
                        "%{host}. Your key: %{print}.",
                        &[("host", host), ("print", &print)],
                    ),
                };
                let runs = if held.is_some_and(|m| m.scope == Scope::Read) {
                    format!("{runs} {}", crate::tr("This Mac may only watch there."))
                } else {
                    runs.to_string()
                };
                item(w, s.name.clone(), runs, move |d, faces, cx| {
                    d.server_control(&name, faces, cx)
                })
            })
            .collect()
    }

    /// Where a link from another Mac is pasted to pair with it.
    fn pair_field(w: &WeakEntity<Desk>) -> gpui::component::setting::SettingItem {
        item(
            w,
            crate::tr("Pair with a link"),
            crate::tr(
                "Paste the link the other Mac shows under Pair a device with this Mac, or open it here. Its projects come down the left, beside this Mac's.",
            ),
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
                        ui::button("add-server", crate::tr("Pair"), Weight::Primary)
                            .loading(d.pages.pairing)
                            .on_click(cx.listener(|this, _, window, cx| this.pair(window, cx))),
                    )
                    .into_any_element()
            },
        )
    }

    /// How the server paired with as `name` stands in this window, the way
    /// to reach it again, and to forget it.
    fn server_control(&self, name: &str, faces: &Faces, cx: &mut Context<Self>) -> AnyElement {
        let held = (self.machines.iter()).find(|m| m.host != Host::HERE && m.name == name);
        let said = match held.map(|m| &m.up) {
            Some(Up::On) => crate::tr("In this window"),
            Some(Up::Reaching) => crate::tr("Reaching it"),
            Some(Up::Lost(_)) => crate::tr("Offline; reaching it again"),
            Some(Up::Refused(why)) => crate::tr_with("Turned away: %{why}", &[("why", why)]),
            None => crate::tr("Not in this window"),
        };
        let again = held.filter(|m| m.up != Up::On).map(|m| {
            let host = m.host;
            ui::button(
                SharedString::from(format!("reach-{name}")),
                crate::tr("Reach again"),
                Weight::Secondary,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.reach_again(host, cx)))
        });
        let forget = {
            let name = name.to_owned();
            ui::button(
                SharedString::from(format!("forget-{name}")),
                crate::tr("Forget"),
                Weight::Quiet,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_servers(|s| {
                    s.forget(&name);
                    Ok(())
                });
                this.hold_servers();
                cx.notify();
            }))
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::fine(faces, said))
            .children(again)
            .child(forget)
            .into_any_element()
    }

    /// Ask the server the settings are of something of sharing.
    pub(crate) fn share(&mut self, share: Share) {
        self.call(self.pages.on, crate::proto::Call::Share(share));
    }

    /// Control other Macs, or only this one: the window holds those
    /// paired with, or lets go of them.
    pub(crate) fn set_control(&mut self, on: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.change_servers(|s| {
            s.control = on;
            Ok(())
        });
        self.hold_servers();
        cx.notify();
    }

    /// Hold each server paired with while this Mac controls others, as
    /// `servers.json` says now, reaching those not held yet; let go of
    /// those it no longer says (forgotten, paired again, or control turned
    /// off), and of what the desk had of them.
    pub(crate) fn hold_servers(&mut self) {
        let port = crate::access::Access::new(&self.home)
            .kept()
            .map_or(0, |k| k.port);
        let wanted: Vec<crate::servers::Server> = self.pages.servers.held(port).cloned().collect();
        let gone: Vec<Host> = (self.machines.iter())
            .filter(|m| m.host != Host::HERE)
            .filter(|m| !wanted.iter().any(|s| m.paired.as_ref() == Some(s)))
            .map(|m| m.host)
            .collect();
        for host in gone {
            self.forget_host(host, true);
            self.machines.retain(|m| m.host != host);
            if self.pages.on == host {
                self.pages.on = Host::HERE;
            }
        }
        for s in wanted {
            if !(self.machines.iter()).any(|m| m.paired.as_ref() == Some(&s)) {
                self.last_host += 1;
                let m = Machine::linked(Host(self.last_host), &s, &self.tx);
                self.machines.push(m);
            }
        }
    }

    /// Reach the server `host` again now, rather than when its wait is
    /// over.
    pub(crate) fn reach_again(&mut self, host: Host, cx: &mut Context<Self>) {
        if let Some(m) = self.machine(host) {
            m.again();
        }
        cx.notify();
    }

    /// Pair with the server whose link is pasted.
    fn pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pasted = self.pages.pair_link.read(cx).value().trim().to_owned();
        match crate::reach::pairing(&pasted) {
            Some((way, code)) => Self::ask_pair(way, code, window, cx),
            None => self.trouble(
                crate::tr("That is not a pairing link"),
                Some(crate::tr("It begins ghost://pair").into()),
                None,
            ),
        }
        cx.notify();
    }

    /// Ask before pairing with the server `way` by `code`, showing the
    /// fingerprint of the key this Mac makes for it; and, where this Mac's
    /// own server is in this window, whether it pairs back.
    pub(crate) fn ask_pair(
        way: crate::reach::Way,
        code: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::component::WindowExt as _;
        let key = Key::generate();
        let kept = key.kept().to_owned();
        let print = key::fingerprint(&key.public());
        let host = way.host.clone();
        let desk = cx.entity();
        // Whether it pairs both ways, as the sheet's box is ticked.
        let ticked = std::rc::Rc::new(std::cell::Cell::new(false));
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let offer = desk.read(cx).machines[0].local().is_some();
            let both = offer && ticked.get();
            let faces = crate::fonts::faces(cx).clone();
            let mut about = div().flex().flex_col().gap(px(12.)).child(ui::text(
                &faces,
                crate::tr_with(
                    "This Mac makes a key for it and sends only its public half, %{print}. %{host} lists it under the devices that control it.",
                    &[("print", &print), ("host", &host)],
                ),
            ));
            if offer {
                let ticked = ticked.clone();
                about = about.child(
                    Checkbox::new("pair-both")
                        .label(crate::tr_with("Also let %{host} control this Mac", &[("host", &host)]))
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
                    crate::tr_with(
                        "This Mac listens, and %{host} pairs back with a key of its own, which this Mac lists under the devices that control it.",
                        &[("host", &host)],
                    ),
                ));
            }
            let (desk, way, code, kept) = (desk.clone(), way.clone(), code.clone(), kept.clone());
            alert
                .title(crate::tr_with("Pair with %{host}?", &[("host", &host)]))
                .description(about)
                .ok_text(crate::tr("Pair"))
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    desk.update(cx, |d, cx| d.pair_with(&way, &code, &kept, both, cx));
                    true
                })
        });
    }

    /// Pair with the server `way` by `code` with the key kept as
    /// `kept`; `both` ways, if this Mac can be reached back.
    fn pair_with(
        &mut self,
        way: &crate::reach::Way,
        code: &str,
        kept: &str,
        both: bool,
        cx: &mut Context<Self>,
    ) {
        let back = match self.machines[0].local() {
            Some(server) if both => match server.back_link(way) {
                Ok(back) => Some(back),
                Err(why) => {
                    self.trouble(
                        crate::tr_with(
                            "%{host} can't be let control this Mac",
                            &[("host", &way.host)],
                        ),
                        Some(
                            crate::tr_with(
                                "%{why} This Mac pairs with it one way only.",
                                &[("why", &why)],
                            )
                            .into(),
                        ),
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
            way.clone(),
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
            self.trouble(crate::tr("Couldn't keep your servers"), Some(why), None);
        }
        self.read_servers();
    }

    /// The servers paired with, read again for Devices.
    pub(crate) fn read_servers(&mut self) {
        match Servers::load(&self.home) {
            Ok(servers) => self.pages.servers = servers,
            Err(why) => self.trouble(crate::tr("Couldn't read your servers"), Some(why), None),
        }
    }
}
