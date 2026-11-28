//! The Preview tab with a page in it, as the `TerminalBrowser` and
//! `RightColumnStates` boards draw it in the 400 column: u0ui's narrow
//! `PreviewChrome` (its More a menu holding the viewport and the zoom),
//! u0ui's `PreviewStrip` naming the dev server, a note for a port brought
//! from another Mac, then the page, or why it does not load
//! (`WebViewStates`); with none, the dev servers and the addresses opened
//! lately.

use gpui::{ClipboardItem, Context, Div, Entity, SharedString, canvas, div, prelude::*, px};
use gpui_webview::WebView;
use u0ui::{
    components::{
        Button, ButtonSize, ButtonVariant, EmptyDescription, EmptyHeader, EmptyTitle, MenuItem,
    },
    icons::{CarbonIcon, Icon},
    t3::workspace::{PreviewChrome, PreviewChromeTexts, PreviewStrip},
    tokens::{ColorRole, SPACING_03, SPACING_04, SPACING_05, SPACING_06, SPACING_07, TypeStyle},
    typography::TypeStyled as _,
};

use super::{Page, VIEWPORTS, fit, one_server, openable};
use crate::aside::rules;
use crate::{
    desk::{Desk, runs::To},
    theme,
};

impl Desk {
    /// The Preview tab on macOS: the address bar, and under it the page
    /// with the strip naming its server, or the servers to open.
    pub(in crate::aside) fn page_tab(&self, tab: Div, key: u64, cx: &mut Context<Self>) -> Div {
        let page = self.web.pages.get(&key);
        let tab = tab.child(self.address_bar(key, page, cx));
        if let Some(page) = page {
            let address = page.address.clone();
            let tab = tab
                .child(
                    self.page_strip(key, &address, cx)
                        .answering(page.unreached.is_none()),
                )
                .children(self.forwarded_note(&address, cx));
            if let Some(unreached) = &page.unreached {
                return tab.child(self.unreached(key, unreached, cx));
            }
            // Its webview is made once this frame is drawn.
            return match page.view.clone() {
                Some(view) => tab.child(self.page_view(view, page)),
                None => tab.child(div().flex_1()),
            };
        }
        let desk = cx.entity();
        let lately = self.web.lately.clone();
        tab.child(
            self.no_page(cx)
                .recent(lately.clone())
                .on_recent(move |n, _, cx| {
                    if let Some(address) = lately.get(n).cloned() {
                        desk.update(cx, |d, cx| d.show_page(key, &address, true, cx));
                    }
                }),
        )
    }

    /// The page's address bar (u0ui's `PreviewChrome`, narrow, as the 400
    /// column has it): back, forward, reload or stop, the address (typed,
    /// it opens), and More, the page's menu; your browser is in the strip
    /// under it.
    fn address_bar(&self, key: u64, page: Option<&Page>, cx: &mut Context<Self>) -> PreviewChrome {
        let desk = cx.entity();
        let address = page.map(|p| p.address.clone()).unwrap_or_default();
        let view = page.and_then(|p| p.view.clone());
        let (back, forward, loading) = view.as_ref().map_or((false, false, false), |v| {
            use wry::WebViewExtMacOS as _;
            let webkit = v.read(cx).raw().webview();
            // SAFETY: WebKit's view, read on the main thread it lives on.
            unsafe {
                (
                    webkit.canGoBack(),
                    webkit.canGoForward(),
                    webkit.isLoading(),
                )
            }
        });
        let mut chrome = PreviewChrome::new("page-chrome", address.clone())
            .narrow(true)
            .zoom(page.map_or(100, |p| (p.zoom * 100.).round() as u32))
            .secure(page.is_some_and(|p| p.address.starts_with("https:")))
            .loading(loading)
            .can_back(back)
            .can_forward(forward)
            .texts(PreviewChromeTexts {
                back: crate::tr("Back"),
                forward: crate::tr("Forward"),
                reload: crate::tr("Reload"),
                stop: crate::tr("Stop"),
                address: crate::tr("Address"),
                viewport: crate::tr("Viewport"),
                open_external: crate::tr("Open in your browser"),
                more: crate::tr("More actions"),
            })
            .more_menu(self.page_menu(key, cx))
            .on_navigate({
                let desk = desk.clone();
                move |typed, _, cx| desk.update(cx, |d, cx| d.navigate(key, &typed, cx))
            });
        if let Some(view) = view {
            let (back, forward, stop) = (view.clone(), view.clone(), view);
            chrome = chrome
                // WebKit's own history, not the page's `history`, which
                // the page may have redefined.
                .on_back(move |_, cx| {
                    use wry::WebViewExtMacOS as _;
                    // SAFETY: on the main thread WebKit's view lives on.
                    drop(unsafe { back.read(cx).raw().webview().goBack() });
                })
                .on_forward(move |_, cx| {
                    use wry::WebViewExtMacOS as _;
                    // SAFETY: on the main thread WebKit's view lives on.
                    drop(unsafe { forward.read(cx).raw().webview().goForward() });
                })
                .on_reload({
                    let desk = desk.clone();
                    move |_, cx| desk.update(cx, |d, cx| d.reload_page(key, false, cx))
                })
                .on_stop(move |_, cx| {
                    use wry::WebViewExtMacOS as _;
                    // SAFETY: on the main thread WebKit's view lives on.
                    unsafe { stop.read(cx).raw().webview().stopLoading() }
                });
        }
        chrome
    }

    /// More's menu (`RightColumnStates`, "More, above the page"): what to
    /// do with the page, then its viewport and zoom, each a menu of its
    /// own; nothing but the viewport with no page open.
    fn page_menu(&self, key: u64, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let desk = cx.entity();
        let names: Vec<SharedString> = VIEWPORTS.iter().map(|(n, _)| crate::tr(n)).collect();
        let viewport = MenuItem::radio_group(crate::tr("Viewport"), names)
            .selected_item(Some(self.web.viewport))
            .on_select_item({
                let desk = desk.clone();
                move |n, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.web.viewport = n;
                        cx.notify();
                    });
                }
            });
        let Some(page) = self.web.pages.get(&key) else {
            return vec![viewport];
        };
        let (copied, opened) = (page.address.clone(), page.address.clone());
        let unshown = page.view.is_none() || page.unreached.is_some();
        let zoom = |label: &str, by: i32| {
            let desk = desk.clone();
            MenuItem::item(crate::tr(label))
                .on_select(move |_, cx| desk.update(cx, |d, cx| d.zoom_page(key, by, cx)))
        };
        // The browser it opens in, by name, as the board's `Safari`: the
        // one chosen in Settings › Browser, else the system's, once known.
        let mut browse = MenuItem::item(crate::tr("Open in your browser"));
        if let Some(name) = self
            .settings
            .browser
            .as_ref()
            .or_else(|| self.browsers.iter().find(|b| b.default).map(|b| &b.app))
            .and_then(|app| app.file_stem())
        {
            browse = browse.shortcut(name.to_string_lossy().into_owned());
        }
        vec![
            MenuItem::item(crate::tr("Reload")).on_select({
                let desk = desk.clone();
                move |_, cx| desk.update(cx, |d, cx| d.reload_page(key, false, cx))
            }),
            MenuItem::item(crate::tr("Reload, skipping the cache")).on_select({
                let desk = desk.clone();
                move |_, cx| desk.update(cx, |d, cx| d.reload_page(key, true, cx))
            }),
            MenuItem::item(crate::tr("Copy address")).on_select(move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
            }),
            // A file page is yours, not your browser's.
            browse
                .disabled(rules::for_browser(&opened).is_none())
                .on_select({
                    let desk = desk.clone();
                    move |_, cx| desk.update(cx, |d, cx| d.browse(&opened, cx))
                }),
            MenuItem::divider(),
            // The page as WebKit draws it, beside what you write: only
            // while one is shown.
            MenuItem::item(crate::tr("Add a screenshot to the chat"))
                .disabled(unshown)
                .on_select({
                    let desk = desk.clone();
                    move |_, cx| desk.update(cx, |d, cx| d.shoot_page(key, cx))
                }),
            MenuItem::item(crate::tr("Viewport")).submenu([viewport]),
            MenuItem::item(crate::tr("Zoom")).submenu([
                zoom("Zoom in", 1),
                zoom("Zoom out", -1),
                zoom("Actual size", 0),
            ]),
            MenuItem::divider(),
            MenuItem::item(crate::tr("Close the page")).on_select(move |_, cx| {
                desk.update(cx, |d, cx| {
                    d.web.pages.remove(&key);
                    cx.notify();
                });
            }),
        ]
    }

    /// The strip under the address bar (u0ui's `PreviewStrip`): the dev
    /// server the page is of, its menu switching to another, the machine
    /// it runs on, and your browser.
    fn page_strip(&self, key: u64, address: &str, cx: &mut Context<Self>) -> PreviewStrip {
        let desk = cx.entity();
        let servers = self.dev_servers();
        let at = servers.iter().position(|s| one_server(&s.url, address));
        let strip = if let Some(s) = at.map(|n| &servers[n]) {
            PreviewStrip::new("page-strip", s.title.clone(), s.port()).machine(s.machine.clone())
        } else {
            let url = url::Url::parse(address).ok();
            PreviewStrip::new(
                "page-strip",
                url.as_ref()
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .unwrap_or_else(|| address.to_owned()),
                url.as_ref().and_then(url::Url::port),
            )
            .machine(crate::tr("This Mac"))
        };
        // `dev · pnpm dev · localhost:5173 · This Mac`: the board's two
        // lines of each server in one, as u0ui's radio item has one.
        let labels: Vec<SharedString> = servers
            .iter()
            .map(|s| {
                [
                    Some(s.title.clone()),
                    s.command.clone(),
                    url::Url::parse(&s.printed).ok().and_then(|u| {
                        let port = u.port_or_known_default()?;
                        Some(format!("{}:{port}", u.host_str()?))
                    }),
                    Some(s.machine.clone()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" \u{00b7} ")
                .into()
            })
            .collect();
        let targets: Vec<_> = servers.into_iter().map(|s| (s.host, s.printed)).collect();
        let menu = MenuItem::radio_group(crate::tr("Dev servers your terminals printed"), labels)
            .selected_item(at)
            .on_select_item({
                let desk = desk.clone();
                move |n, _, cx| {
                    if let Some((host, printed)) = targets.get(n).cloned() {
                        desk.update(cx, |d, cx| {
                            d.open_address(host, printed, To::Preview(key), cx);
                        });
                    }
                }
            });
        let strip = strip.servers(crate::tr("Dev servers your terminals printed"), [menu]);
        // A file page is yours, not your browser's.
        match rules::for_browser(address) {
            Some(url) => strip.action(crate::tr("Open in your browser"), move |_, cx| {
                desk.update(cx, |d, cx| d.browse(url.as_str(), cx));
            }),
            None => strip,
        }
    }

    /// Under the strip, for a page of a server on another Mac
    /// (`RightColumnStates`' `.pv-fwd`): that its port is brought here,
    /// and Stop, which ends that.
    fn forwarded_note(&self, address: &str, cx: &mut Context<Self>) -> Option<Div> {
        let s = self
            .dev_servers()
            .into_iter()
            .find(|s| s.forwarded.is_some() && one_server(&s.url, address))?;
        let (there, here) = s.forwarded?;
        let (desk, host) = (cx.entity(), s.host);
        // `.pv-fwd { display: flex; align-items: flex-start; gap: 8px; padding:
        // 8px 8px 8px 12px; border-bottom: 1px solid border-subtle-01;
        // background: layer-02; font-size: 12px; line-height: 16px; color:
        // text-secondary }`, its icon and words 8px down, level with the
        // button's label.
        Some(
            div()
                .flex()
                .flex_none()
                .items_start()
                .gap(px(SPACING_03.px_value()))
                .py(px(SPACING_03.px_value()))
                .pr(px(SPACING_03.px_value()))
                .pl(px(SPACING_04.px_value()))
                .border_b_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .bg(theme::role(ColorRole::Layer02))
                .child(
                    div().flex_none().mt(px(SPACING_03.px_value())).child(
                        Icon::carbon(CarbonIcon::Plug)
                            .size_length(px(16.))
                            .color(theme::role(ColorRole::IconSecondary)),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .pt(px(SPACING_03.px_value()))
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(crate::tr_with(
                            "Port %{there} on %{machine}, forwarded to 127.0.0.1:%{here} on this Mac over its own signed link. Your browser opens the same address.",
                            &[("there", &there), ("machine", &s.machine), ("here", &here)],
                        )),
                )
                .child(
                    Button::new("page-forwarded-stop", crate::tr("Stop"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk.update(cx, |d, cx| d.stop_forward(host, there, cx));
                        }),
                ),
        )
    }

    /// In the page's place while nothing answers at its address on this
    /// Mac (the `WebViewStates` board's "The page does not load"): what did
    /// not answer and why that may be (a dev server here, or the Mac a
    /// port is brought from), and Try again; your browser is in the strip.
    fn unreached(&self, key: u64, address: &str, cx: &mut Context<Self>) -> Div {
        let desk = cx.entity();
        let url = url::Url::parse(address).ok();
        let port = url.as_ref().and_then(url::Url::port_or_known_default);
        let host = url
            .as_ref()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        let at = port.map_or_else(|| host.clone(), |p| format!("{host}:{p}"));
        let brought = self
            .dev_servers()
            .into_iter()
            .find(|s| one_server(&s.url, address))
            .and_then(|s| Some((s.forwarded?.0, s.machine)));
        let why = match brought {
            Some((there, machine)) => crate::tr_with(
                "Nothing answered on port %{port} of %{machine}. Is its dev server running?",
                &[("port", &there), ("machine", &machine)],
            ),
            None => crate::tr_with(
                "Is the dev server running? Nothing answered on port %{port} on this Mac.",
                &[("port", &port.unwrap_or_default())],
            ),
        };
        // `.wv { display: flex; flex-direction: column; gap: 16px; padding:
        // 32px 24px 24px }`: its icon (`support-error`, 32), the empty
        // state's words with no padding of their own, then `.wv-acts`, a
        // column 8px apart.
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .gap(px(SPACING_05.px_value()))
            .px(px(SPACING_06.px_value()))
            .pt(px(SPACING_07.px_value()))
            .pb(px(SPACING_06.px_value()))
            .overflow_hidden()
            .child(
                Icon::carbon(CarbonIcon::ErrorFilled)
                    .size_length(px(32.))
                    .color(theme::role(ColorRole::SupportError)),
            )
            .child(
                EmptyHeader::new()
                    .child(EmptyTitle::new(crate::tr_with(
                        "Can't reach %{address}",
                        &[("address", &at)],
                    )))
                    .child(EmptyDescription::new(why)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(SPACING_03.px_value()))
                    .child(
                        Button::new("page-again", crate::tr("Try again"))
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Sm)
                            .icon(CarbonIcon::Renew)
                            .on_click(move |_, _, cx| {
                                desk.update(cx, |d, cx| d.reload_page(key, false, cx));
                            }),
                    ),
            )
    }

    /// The page, laid out at the viewport chosen: as wide as the column, or
    /// as the device's width and zoomed down where the column is narrower;
    /// then zoomed as you set it.
    fn page_view(&self, view: Entity<WebView>, page: &Page) -> Div {
        let viewport = self.web.viewport;
        let (zoomed, yours) = (view.clone(), page.zoom);
        let fitted = canvas(
            move |bounds, _, cx| {
                let zoom = fit(viewport, f32::from(bounds.size.width)) * yours;
                let page = zoomed.read(cx);
                if (page.zoom() - zoom).abs() > f64::EPSILON {
                    let _ = page.set_zoom(zoom);
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .size_full();
        div()
            .relative()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .justify_center()
            .bg(theme::role(ColorRole::Layer02))
            .child(fitted)
            .child(
                div()
                    .h_full()
                    .w_full()
                    .when_some(VIEWPORTS.get(viewport).and_then(|v| v.1), |d, width| {
                        d.max_w(px(width))
                    })
                    .child(view),
            )
    }

    /// An address typed in the bar: opened here when it is a page's.
    fn navigate(&mut self, key: u64, typed: &str, cx: &mut Context<Self>) {
        match openable(typed) {
            Some(url) => self.show_page(key, url.as_str(), true, cx),
            None => self.trouble(
                crate::tr_with(
                    "%{address} is not a web address or a file of yours",
                    &[("address", &typed)],
                ),
                None,
                None,
            ),
        }
    }
}
