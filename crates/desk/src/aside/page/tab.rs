//! The Preview tab with a page in it, as the `TerminalBrowser` board draws
//! it in the 400 column: u0ui's narrow `PreviewChrome` (its More a menu
//! holding the viewport), u0ui's `PreviewStrip` naming the dev server, an
//! actionable notification for a port brought from another Mac, then the
//! page; with none, the dev servers and the addresses opened lately.

use gpui::{ClipboardItem, Context, Div, Entity, SharedString, canvas, div, prelude::*, px};
use gpui_webview::WebView;
use u0ui::{
    components::{
        MenuItem,
        alert::{Alert, AlertAction, AlertVariant},
    },
    t3::workspace::{PreviewChrome, PreviewChromeTexts, PreviewStrip},
    tokens::ColorRole,
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
                .child(self.page_strip(key, &address, cx))
                .children(self.forwarded_note(&address, cx));
            // Its webview is made once this frame is drawn.
            return match page.view.clone() {
                Some(view) => tab.child(self.page_view(view)),
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
        let mut chrome = PreviewChrome::new("page-chrome", address)
            .narrow(true)
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
            .on_navigate(move |typed, _, cx| desk.update(cx, |d, cx| d.navigate(key, &typed, cx)));
        if let Some(view) = view {
            let (back, forward, reload, stop) = (view.clone(), view.clone(), view.clone(), view);
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
                .on_reload(move |_, cx| drop(reload.read(cx).raw().reload()))
                .on_stop(move |_, cx| {
                    use wry::WebViewExtMacOS as _;
                    // SAFETY: on the main thread WebKit's view lives on.
                    unsafe { stop.read(cx).raw().webview().stopLoading() }
                });
        }
        chrome
    }

    /// More's menu: what to do with the page, and the viewport it is laid
    /// out at; nothing but the viewport with no page open.
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
        let view = page.view.clone();
        vec![
            MenuItem::item(crate::tr("Reload")).on_select(move |_, cx| {
                if let Some(view) = &view {
                    let _ = view.read(cx).raw().reload();
                }
            }),
            MenuItem::item(crate::tr("Copy address")).on_select(move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
            }),
            // A file page is yours, not your browser's.
            MenuItem::item(crate::tr("Open in your browser"))
                .disabled(rules::for_browser(&opened).is_none())
                .on_select({
                    let desk = desk.clone();
                    move |_, cx| desk.update(cx, |d, cx| d.browse(&opened, cx))
                }),
            MenuItem::divider(),
            viewport,
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
        // `dev · pnpm dev · :5173 · This Mac`, as the board's menu.
        let labels: Vec<SharedString> = servers
            .iter()
            .map(|s| {
                [
                    Some(s.title.clone()),
                    s.command.clone(),
                    s.port().map(|p| format!(":{p}")),
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

    /// Under the strip, for a page of a server on another Mac: that its
    /// port is brought here, and the way to stop that (Carbon's actionable
    /// notification, u0ui's `Alert` with an action).
    fn forwarded_note(&self, address: &str, cx: &mut Context<Self>) -> Option<Alert> {
        let s = self
            .dev_servers()
            .into_iter()
            .find(|s| s.forwarded.is_some() && one_server(&s.url, address))?;
        let (there, here) = s.forwarded?;
        let (desk, host) = (cx.entity(), s.host);
        Some(
            Alert::new("page-forwarded")
                .variant(AlertVariant::Info)
                .full_width(true)
                .description(crate::tr_with(
                    "Port %{there} on %{machine}, forwarded to 127.0.0.1:%{here} on this Mac over its own signed link",
                    &[("there", &there), ("machine", &s.machine), ("here", &here)],
                ))
                .action(AlertAction::new(crate::tr("Stop forwarding")).on_click(move |_, cx| {
                    desk.update(cx, |d, cx| d.stop_forward(host, there, cx));
                })),
        )
    }

    /// The page, laid out at the viewport chosen: as wide as the column, or
    /// as the device's width and zoomed down where the column is narrower.
    fn page_view(&self, view: Entity<WebView>) -> Div {
        let viewport = self.web.viewport;
        let zoomed = view.clone();
        let fitted = canvas(
            move |bounds, _, cx| {
                let zoom = fit(viewport, f32::from(bounds.size.width));
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
