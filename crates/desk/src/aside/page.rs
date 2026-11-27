//! A page you opened in a session's Preview tab, shown in macOS's own
//! webview (`WKWebView`, through gpui-kit's `gpui-webview`): made as its tab
//! first shows it, hidden while another tab or session is shown, and gone
//! with its tab. Nothing but your own choice opens one: a dev server's
//! card or the drawer's **Open preview**, an address you type, one you
//! opened lately.

use std::collections::HashMap;

use gpui::component::menu::{PopupMenu, PopupMenuItem};
use gpui::{
    ClipboardItem, Context, Div, Entity, SharedString, Window, canvas, div, prelude::*, px,
};
use gpui_webview::WebView;
use u0ui::{
    components::{Button, ButtonSize, ButtonVariant},
    icons::{CarbonIcon, Icon},
    t3::workspace::{PreviewChrome, PreviewChromeTexts},
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
};

use super::{Column, Surface};
use crate::{
    desk::{Desk, Popup, runs::To},
    fonts,
    hub::{Msg, Sender},
    theme, ui,
};

/// The sizes a page can be laid out at, by name: the column's own width,
/// or a device's, zoomed down where the column is narrower.
pub const VIEWPORTS: [(&str, Option<f32>); 4] = [
    ("Responsive", None),
    ("iPhone 16", Some(393.)),
    ("iPad", Some(820.)),
    ("Desktop 1440", Some(1440.)),
];

/// How many of the addresses opened lately are offered again.
const LATELY: usize = 5;

/// What the Preview tabs of the window show.
#[derive(Default)]
pub struct Web {
    /// Each session's page, by its key.
    pub pages: HashMap<u64, Page>,
    /// The viewport pages are laid out at, of [`VIEWPORTS`].
    pub viewport: usize,
    /// The addresses opened lately, the last first.
    pub lately: Vec<String>,
}

/// One session's page: where it is, and the webview once it has shown.
pub struct Page {
    /// What the address bar shows: where it was opened, or last went.
    pub address: String,
    pub view: Option<Entity<WebView>>,
}

/// What a page said, for the window.
pub enum Went {
    /// It is now at this address.
    At(String),
    /// It asked for a window of its own at this address: that opens in
    /// your browser.
    Elsewhere(String),
}

/// `typed` as a page to open here: a web address (`http` when it names no
/// scheme) or a file of yours by its path or `file:` address; nothing else.
pub fn openable(typed: &str) -> Option<url::Url> {
    let typed = typed.trim();
    let url = if typed.is_empty() {
        return None;
    } else if typed.starts_with('/') {
        url::Url::from_file_path(typed).ok()?
    } else if let Some(rest) = typed.strip_prefix("~/") {
        url::Url::from_file_path(Desk::home_path()?.join(rest)).ok()?
    } else if typed.contains("://") || typed.starts_with("file:") {
        url::Url::parse(typed).ok()?
    } else {
        // `localhost:5173` names a host and its port, not a scheme; what
        // does name one (`javascript:`, `data:`) is no host's, and fails.
        url::Url::parse(&format!("http://{typed}")).ok()?
    };
    matches!(url.scheme(), "http" | "https" | "file").then_some(url)
}

/// Whether a session's page is kept: while its Preview tab is open.
fn kept(columns: &HashMap<u64, Column>, key: u64) -> bool {
    columns
        .get(&key)
        .is_some_and(|c| c.tabs.contains(&Surface::Preview))
}

/// The zoom that fits a page laid out at `viewport` into `room` points.
pub fn fit(viewport: usize, room: f32) -> f64 {
    VIEWPORTS
        .get(viewport)
        .and_then(|(_, width)| *width)
        .map_or(1., |width| f64::from((room / width).min(1.)))
}

/// Whether two addresses are of one server: its scheme, host and port.
fn one_server(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}

/// `address` put first among those opened lately, kept to [`LATELY`].
fn remember(lately: &mut Vec<String>, address: &str) {
    lately.retain(|a| a != address);
    lately.insert(0, address.to_owned());
    lately.truncate(LATELY);
}

impl Desk {
    /// Show `address` in the Preview tab of the session `key`, opening the
    /// tab: the page already there goes to it.
    pub(crate) fn show_page(&mut self, key: u64, address: String, cx: &mut Context<Self>) {
        remember(&mut self.web.lately, &address);
        match self.web.pages.get_mut(&key) {
            Some(page) => {
                if let Some(view) = &page.view {
                    view.update(cx, |v, _| v.load_url(&address));
                }
                page.address = address;
            }
            None => {
                self.web.pages.insert(
                    key,
                    Page {
                        address,
                        view: None,
                    },
                );
            }
        }
        self.columns.entry(key).or_default().open(Surface::Preview);
        if self.current == Some(key) {
            self.panes.right = true;
        }
        cx.notify();
    }

    /// What a page said.
    pub(crate) fn page_went(&mut self, key: u64, went: Went, cx: &mut Context<Self>) {
        match went {
            Went::At(address) => {
                if let Some(page) = self.web.pages.get_mut(&key) {
                    page.address = address;
                }
            }
            Went::Elsewhere(address) => cx.open_url(&address),
        }
        cx.notify();
    }

    /// Before the window is drawn: each page whose tab closed goes, the
    /// one the column shows (when it is `drawn`) is made and shown, every
    /// other hidden. A webview is a view of macOS's over the window: one
    /// left showing would cover what is drawn in its place.
    pub(crate) fn sync_web(&mut self, drawn: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (columns, sessions) = (&self.columns, &self.sessions);
        self.web
            .pages
            .retain(|key, _| sessions.at(*key).is_some() && kept(columns, *key));
        let shown = self
            .current
            .filter(|_| drawn && self.surface() == Some(Surface::Preview));
        let mut failed = None;
        for (key, page) in &mut self.web.pages {
            let on = shown == Some(*key);
            match &page.view {
                Some(view) => {
                    if view.read(cx).visible() != on {
                        let _ = view.read(cx).set_visible(on);
                    }
                }
                None if on => match make_view(*key, &page.address, self.tx.clone(), window, cx) {
                    Ok(view) => page.view = Some(view),
                    Err(why) => failed = Some((*key, page.address.clone(), why)),
                },
                None => {}
            }
        }
        // Where the system makes no webview, the page opens in your
        // browser, and you are told why.
        if let Some((key, address, why)) = failed {
            self.web.pages.remove(&key);
            cx.open_url(&address);
            self.trouble(
                crate::tr("Couldn't show the page here: it opens in your browser"),
                Some(why.to_string()),
                None,
            );
        }
    }

    /// The Preview tab on macOS: the address bar, and under it the page
    /// with the strip naming its server, or the servers to open.
    pub(super) fn page_tab(&self, tab: Div, key: u64, cx: &mut Context<Self>) -> Div {
        let page = self.web.pages.get(&key);
        let tab = tab.child(self.address_bar(key, page, cx));
        if let Some((address, view)) =
            page.and_then(|p| p.view.clone().map(|v| (p.address.clone(), v)))
        {
            return tab
                .child(self.page_strip(key, &address, cx))
                .children(self.forwarded_note(&address, cx))
                .child(self.page_view(view));
        }
        let desk = cx.entity();
        let lately = self.web.lately.clone();
        tab.child(
            self.no_page(cx)
                .recent(lately.clone())
                .on_recent(move |n, _, cx| {
                    if let Some(address) = lately.get(n).cloned() {
                        desk.update(cx, |d, cx| d.show_page(key, address, cx));
                    }
                }),
        )
    }

    /// The page's address bar (u0ui's `PreviewChrome`): back, forward,
    /// reload or stop, the address (typed, it opens), the viewport, your
    /// browser, and the page's menu.
    fn address_bar(&self, key: u64, page: Option<&Page>, cx: &mut Context<Self>) -> Div {
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
        let names: Vec<SharedString> = VIEWPORTS.iter().map(|(n, _)| crate::tr(n)).collect();
        let mut chrome = PreviewChrome::new("page-chrome", address.clone())
            .secure(address.starts_with("https:"))
            .loading(loading)
            .can_back(back)
            .can_forward(forward)
            .viewport(names[self.web.viewport.min(names.len() - 1)].clone())
            .viewports(names)
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
            .on_navigate({
                let desk = desk.clone();
                move |typed, _, cx| desk.update(cx, |d, cx| d.navigate(key, &typed, cx))
            })
            .on_viewport_change({
                let desk = desk.clone();
                move |n, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.web.viewport = n;
                        cx.notify();
                    });
                }
            })
            .on_more(move |_, cx| {
                desk.update(cx, |d, cx| {
                    d.popup = Popup::Page;
                    cx.notify();
                });
            });
        if let Some(view) = view {
            let (back, forward, reload, stop) = (view.clone(), view.clone(), view.clone(), view);
            chrome = chrome
                .on_back(move |_, cx| back.update(cx, |v, _| drop(v.back())))
                .on_forward(move |_, cx| forward.update(cx, |v, _| drop(v.forward())))
                .on_reload(move |_, cx| drop(reload.read(cx).raw().reload()))
                .on_stop(move |_, cx| {
                    use wry::WebViewExtMacOS as _;
                    // SAFETY: on the main thread WebKit's view lives on.
                    unsafe { stop.read(cx).raw().webview().stopLoading() }
                })
                .on_open_external(move |_, cx| cx.open_url(&address));
        }
        // More's menu hangs from its corner, over the page.
        let menu = (self.popup == Popup::Page).then(|| {
            div().absolute().top(px(36.)).right(px(8.)).child(
                self.menu(
                    Popup::Page,
                    ui::opener("page-more").size(px(1.)),
                    move |d, menu, desk, _| d.page_menu(key, menu, desk),
                    cx,
                )
                .anchor(gpui::Anchor::TopRight),
            )
        });
        div().relative().flex_none().child(chrome).children(menu)
    }

    /// The page's menu, from the address bar's More.
    fn page_menu(&self, key: u64, menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        let Some(address) = self.web.pages.get(&key).map(|p| p.address.clone()) else {
            return menu;
        };
        let view = self.web.pages.get(&key).and_then(|p| p.view.clone());
        let opened = address.clone();
        menu.item(Self::menu_item(
            desk,
            crate::tr("Reload"),
            move |_, _, cx| {
                if let Some(view) = &view {
                    let _ = view.read(cx).raw().reload();
                }
            },
        ))
        .item(Self::menu_item(
            desk,
            crate::tr("Copy address"),
            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(address.clone())),
        ))
        .item(Self::menu_item(
            desk,
            crate::tr("Open in your browser"),
            move |_, _, cx| cx.open_url(&opened),
        ))
        .item(PopupMenuItem::Separator)
        .item(Self::menu_item(
            desk,
            crate::tr("Close the page"),
            move |d, _, cx| {
                d.web.pages.remove(&key);
                cx.notify();
            },
        ))
    }

    /// The strip under the address bar (`.pv-strip`): the dev server the
    /// page is of, to switch to another, the machine it runs on, and your
    /// browser.
    fn page_strip(&self, key: u64, address: &str, cx: &mut Context<Self>) -> Div {
        let faces = fonts::faces(cx).clone();
        let servers = self.dev_servers();
        let at = servers.iter().find(|s| one_server(&s.url, address));
        let (name, port, machine) = match at {
            Some(s) => (s.title.clone(), s.port(), s.machine.clone()),
            None => (
                url::Url::parse(address)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .unwrap_or_else(|| address.to_owned()),
                url::Url::parse(address).ok().and_then(|u| u.port()),
                crate::tr("This Mac").to_string(),
            ),
        };
        let primary = theme::role(ColorRole::TextPrimary);
        let chip = ui::opener("page-server")
            .h(px(32.))
            .px(px(8.))
            .gap(px(6.))
            .bg(theme::role(ColorRole::LayerSelected01))
            .child(
                div()
                    .size(px(8.))
                    .flex_none()
                    .rounded_full()
                    .bg(theme::role(ColorRole::SupportSuccess)),
            )
            .child(
                div()
                    .flex()
                    .type_style(TypeStyle::Label01)
                    .text_color(primary)
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(name))
                    .children(port.map(|p| format!(":{p}"))),
            )
            .child(
                Icon::carbon(CarbonIcon::ChevronDown)
                    .size_length(px(16.))
                    .color(primary),
            );
        let opened = address.to_owned();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .min_h(px(36.))
            .pl(px(8.))
            .pr(px(4.))
            .border_b_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(
                self.menu(
                    Popup::PageServers,
                    chip,
                    move |d, menu, desk, _| d.servers_menu(key, menu, desk),
                    cx,
                )
                .anchor(gpui::Anchor::TopLeft),
            )
            .child(
                ui::one_line(
                    ui::text(&faces, machine)
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextSecondary)),
                )
                .flex_1()
                .min_w(px(0.)),
            )
            .child(
                Button::new("page-browser", crate::tr("Open in your browser"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Launch)
                    .on_click(move |_, _, cx| cx.open_url(&opened)),
            )
    }

    /// The dev servers to switch the page to, from its strip.
    fn servers_menu(&self, key: u64, mut menu: PopupMenu, desk: &Entity<Self>) -> PopupMenu {
        let shown = self.web.pages.get(&key).map(|p| p.address.clone());
        menu = menu.label(crate::tr("Dev servers your terminals printed"));
        for s in self.dev_servers() {
            let on = shown.as_deref().is_some_and(|a| one_server(&s.url, a));
            // `dev · pnpm dev · :5173 · This Mac`, as the board's menu.
            let label = [
                Some(s.title.clone()),
                s.command.clone(),
                s.port().map(|p| format!(":{p}")),
                Some(s.machine.clone()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" \u{00b7} ");
            let (host, printed) = (s.host, s.printed);
            menu = menu.item(
                Self::menu_item(desk, label, move |d, _, cx| {
                    d.open_address(host, printed.clone(), To::Preview(key), cx);
                })
                .checked(on),
            );
        }
        menu
    }

    /// Under the strip, for a page of a server on another Mac: that its
    /// port is brought here, and the way to stop that.
    fn forwarded_note(&self, address: &str, cx: &mut Context<Self>) -> Option<Div> {
        let s = self
            .dev_servers()
            .into_iter()
            .find(|s| s.forwarded.is_some() && one_server(&s.url, address))?;
        let (there, here) = s.forwarded?;
        let faces = fonts::faces(cx).clone();
        let host = s.host;
        Some(
            div()
                .flex()
                .flex_none()
                .items_start()
                .gap(px(8.))
                .py(px(8.))
                .pl(px(12.))
                .pr(px(8.))
                .bg(theme::role(ColorRole::Layer02))
                .border_b_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(
                    div().pt(px(8.)).child(
                        Icon::carbon(CarbonIcon::Plug)
                            .size_length(px(16.))
                            .color(theme::role(ColorRole::IconSecondary)),
                    ),
                )
                .child(
                    ui::text(
                        &faces,
                        crate::tr_with(
                            "Port %{there} on %{machine}, forwarded to 127.0.0.1:%{here} on this Mac over its own signed link",
                            &[("there", &there), ("machine", &s.machine), ("here", &here)],
                        ),
                    )
                    .flex_1()
                    .min_w(px(0.))
                    .pt(px(8.))
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary)),
                )
                .child(
                    Button::new("page-unforward", crate::tr("Stop forwarding"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(move |d, _, _, cx| d.stop_forward(host, there, cx))),
                ),
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
            Some(url) => self.show_page(key, url.into(), cx),
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

/// The webview of the session `key`'s page, at `address`; what the page
/// does comes back as [`Msg::Page`].
fn make_view(
    key: u64,
    address: &str,
    tx: Sender,
    window: &mut Window,
    cx: &mut Context<Desk>,
) -> wry::Result<Entity<WebView>> {
    let asked = tx.clone();
    let builder = wry::WebViewBuilder::new()
        .with_url(address)
        .with_on_page_load_handler(move |_, at| {
            let _ = tx.send(Msg::Page(key, Went::At(at)));
        })
        .with_new_window_req_handler(move |at, _| {
            let _ = asked.send(Msg::Page(key, Went::Elsewhere(at)));
            wry::NewWindowResponse::Deny
        });
    let view = WebView::build(builder, window, cx)?;
    Ok(cx.new(|_| view))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(typed: &str) -> Option<String> {
        openable(typed).map(String::from)
    }

    #[test]
    fn an_address_typed_without_a_scheme_opens_over_http() {
        assert_eq!(
            opened("localhost:5173/cover").as_deref(),
            Some("http://localhost:5173/cover")
        );
        assert_eq!(
            opened("127.0.0.1:8000").as_deref(),
            Some("http://127.0.0.1:8000/")
        );
        assert_eq!(
            opened(" https://example.com ").as_deref(),
            Some("https://example.com/")
        );
    }

    #[test]
    fn a_path_typed_opens_that_file() {
        assert_eq!(
            opened("/tmp/site/index.html").as_deref(),
            Some("file:///tmp/site/index.html")
        );
    }

    #[test]
    fn only_web_and_file_addresses_open_a_page() {
        for refused in [
            "",
            "javascript:alert(1)",
            "data:text/html,<p>hi</p>",
            "about:blank",
            "ftp://example.com/",
            "u0://pair",
        ] {
            assert_eq!(opened(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_page_is_kept_only_while_its_preview_tab_is_open() {
        let mut columns = HashMap::new();
        assert!(!kept(&columns, 1), "a session with no tabs has no page");
        let mut column = Column::default();
        column.open(Surface::Preview);
        column.open(Surface::Files);
        columns.insert(1, column.clone());
        assert!(kept(&columns, 1), "hidden behind another tab, it stays");
        let at = column.tabs.iter().position(|t| *t == Surface::Preview);
        column.close(|n| Some(n) == at);
        columns.insert(1, column);
        assert!(!kept(&columns, 1), "its tab closed, it goes");
    }

    #[test]
    fn a_viewport_wider_than_the_column_is_zoomed_to_fit() {
        assert!(
            (fit(0, 400.) - 1.).abs() < 1e-9,
            "responsive is never zoomed"
        );
        assert!(
            (fit(1, 400.) - 1.).abs() < 1e-9,
            "a phone fits a 400 column"
        );
        assert!((fit(3, 360.) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn a_page_is_of_the_server_whose_scheme_host_and_port_it_shares() {
        assert!(one_server(
            "http://localhost:5173/",
            "http://localhost:5173/cover?x=1"
        ));
        assert!(!one_server(
            "http://localhost:5173/",
            "http://localhost:6006/"
        ));
        assert!(!one_server(
            "http://localhost:5173/",
            "https://localhost:5173/"
        ));
    }

    #[test]
    fn the_address_opened_last_is_offered_first_and_once() {
        let mut lately = Vec::new();
        for a in ["a", "b", "c", "d", "e", "f", "c"] {
            remember(&mut lately, a);
        }
        assert_eq!(lately, ["c", "f", "e", "d", "b"]);
    }
}
