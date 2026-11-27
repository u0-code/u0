//! A page you opened in a session's Preview tab, shown in macOS's own
//! webview (`WKWebView`, through gpui-kit's `gpui-webview`): made as its tab
//! first shows it, hidden while another tab or session is shown, and gone
//! with its tab. Nothing but your own choice opens one: a dev server's
//! card or the drawer's **Open preview**, an address you type, one you
//! opened lately. What a page may reach is in `aside::rules`; the tab it
//! is drawn in, in `tab`.

mod tab;

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};

use gpui::{Context, Entity, Window, prelude::*};
use gpui_webview::WebView;

use super::{Column, Surface, rules};
use crate::{
    desk::Desk,
    hub::{Msg, Sender},
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
    /// Where you opened it, which its webview's rules read
    /// (`rules::may_go`): a file page goes to no other file.
    opened: Rc<RefCell<String>>,
    pub view: Option<Entity<WebView>>,
    /// When a window or download it asked for was last handed to your
    /// browser (`rules::may_hand`).
    handed: Option<Instant>,
    /// The address nothing answered at, while that is so: u0's own state
    /// shows in the page's place, not the engine's error page.
    pub unreached: Option<String>,
}

/// What a page said, for the window.
pub enum Went {
    /// It is now at this address.
    At(String),
    /// It asked for this address elsewhere: a window of its own, or a
    /// download. Your browser is given it if it is a web address
    /// (`rules::for_browser`), one in a while (`rules::may_hand`); nothing
    /// is saved.
    Asked(String),
    /// Whether anything answered at this address as it was opened.
    Reached { address: String, answered: bool },
}

/// The host and port a web address is reached at, to see that something
/// answers there before a page is shown; none for a file.
fn door(address: &str) -> Option<(String, u16)> {
    let url = url::Url::parse(address).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    Some((url.host_str()?.to_owned(), url.port_or_known_default()?))
}

/// Whether anything answers at `address`: a connection taken at its host
/// and port within two seconds; a file, always (the engine says if it is
/// not there).
pub fn answers(address: &str) -> bool {
    use std::net::{TcpStream, ToSocketAddrs as _};
    let Some((host, port)) = door(address) else {
        return true;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    (host, port).to_socket_addrs().is_ok_and(|mut at| {
        at.any(|at| TcpStream::connect_timeout(&at, std::time::Duration::from_secs(2)).is_ok())
    })
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
    rules::shown(url.as_str(), true)
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
    /// tab: the page already there goes to it. Every way a page is opened
    /// comes here, and only a web address is shown, or a file when it is
    /// `yours` (typed, or opened lately: `rules::shown`): a `preview` in a
    /// repository's `u0.json` is what anyone, an agent too, may have written
    /// there.
    pub(crate) fn show_page(
        &mut self,
        key: u64,
        address: &str,
        yours: bool,
        cx: &mut Context<Self>,
    ) {
        if self.sessions.at(key).is_none() {
            return;
        }
        let Some(address) = rules::shown(address, yours).map(String::from) else {
            return self.trouble(
                crate::tr_with(
                    "%{address} is not a web address or a file of yours",
                    &[("address", &address)],
                ),
                None,
                None,
            );
        };
        remember(&mut self.web.lately, &address);
        match self.web.pages.get_mut(&key) {
            Some(page) => {
                page.opened.replace(address.clone());
                if let Some(view) = &page.view {
                    view.update(cx, |v, _| v.load_url(&address));
                }
                page.address.clone_from(&address);
            }
            None => {
                self.web.pages.insert(
                    key,
                    Page {
                        opened: Rc::new(RefCell::new(address.clone())),
                        address: address.clone(),
                        view: None,
                        handed: None,
                        unreached: None,
                    },
                );
            }
        }
        crate::hub::knock(key, address, self.tx.clone());
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
            Went::Reached { address, answered } => {
                if let Some(page) = self
                    .web
                    .pages
                    .get_mut(&key)
                    .filter(|p| *p.opened.borrow() == address)
                {
                    // Something answers again: the page is loaded afresh.
                    if answered
                        && page.unreached.take().is_some()
                        && let Some(view) = &page.view
                    {
                        view.update(cx, |v, _| v.load_url(&address));
                    }
                    if !answered {
                        page.unreached = Some(address);
                    }
                }
            }
            Went::Asked(address) => {
                let now = Instant::now();
                if let Some(page) = self
                    .web
                    .pages
                    .get_mut(&key)
                    .filter(|p| rules::may_hand(p.handed, now))
                {
                    page.handed = Some(now);
                    self.browse(&address, cx);
                }
            }
        }
        cx.notify();
    }

    /// Load the session `key`'s page again, once something is seen to
    /// answer where it was opened (Reload, Try again).
    pub(crate) fn reload_page(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some(page) = self.web.pages.get(&key) {
            let opened = page.opened.borrow().clone();
            if page.unreached.is_none()
                && let Some(view) = &page.view
            {
                let _ = view.read(cx).raw().reload();
            }
            crate::hub::knock(key, opened, self.tx.clone());
        }
    }

    /// The pages go before the window does: a webview must not outlive
    /// the window it is in.
    pub(crate) fn close_pages(&mut self) {
        self.web.pages.clear();
    }

    /// Before the window is drawn: each page whose tab closed goes, the
    /// one the column shows (when it is `drawn`) is shown, and made once
    /// this frame is drawn if it has no webview yet; every other is hidden,
    /// and one of a session not on screen lets its webview go (it loads
    /// again when shown), so no page runs unseen but behind another tab.
    /// A webview is a view of macOS's over the window: one left showing
    /// would cover what is drawn in its place.
    pub(crate) fn sync_web(&mut self, drawn: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (columns, sessions) = (&self.columns, &self.sessions);
        self.web
            .pages
            .retain(|key, _| sessions.at(*key).is_some() && kept(columns, *key));
        let shown = self
            .current
            .filter(|_| drawn && self.surface() == Some(Surface::Preview));
        let mut unmade = None;
        for (key, page) in &mut self.web.pages {
            if Some(*key) != self.current {
                page.view = None;
            }
            let on = shown == Some(*key) && page.unreached.is_none();
            match &page.view {
                Some(view) => {
                    if view.read(cx).visible() != on {
                        let _ = view.read(cx).set_visible(on);
                    }
                }
                None if on => unmade = Some(*key),
                None => {}
            }
        }
        if let Some(key) = unmade {
            let desk = cx.entity();
            window.defer(cx, move |window, cx| {
                desk.update(cx, |d, cx| d.make_view(key, window, cx));
            });
        }
    }

    /// The webview of the session `key`'s page, once a frame is drawn; or,
    /// where the system makes none, the page in your browser and why.
    fn make_view(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.web.pages.get_mut(&key).filter(|p| p.view.is_none()) else {
            return;
        };
        match build(
            key,
            &page.address,
            page.opened.clone(),
            self.tx.clone(),
            window,
            cx,
        ) {
            Ok(view) => page.view = Some(view),
            Err(why) => {
                let address = page.address.clone();
                self.web.pages.remove(&key);
                self.browse(&address, cx);
                self.trouble(
                    crate::tr("Couldn't show the page here: it opens in your browser"),
                    Some(why.to_string()),
                    None,
                );
            }
        }
        cx.notify();
    }
}

/// The webview of the session `key`'s page, at `address`, held to
/// `rules`: where it goes, by `may_go` from where you `opened` it; what it
/// asks for elsewhere (a window, a download), back as [`Went::Asked`] and
/// never saved here. What the page does comes back as [`Msg::Page`].
fn build(
    key: u64,
    address: &str,
    opened: Rc<RefCell<String>>,
    tx: Sender,
    window: &mut Window,
    cx: &mut Context<Desk>,
) -> wry::Result<Entity<WebView>> {
    let (window_asked, download_asked) = (tx.clone(), tx.clone());
    let builder = wry::WebViewBuilder::new()
        .with_url(address)
        .with_navigation_handler(move |to| rules::may_go(&opened.borrow(), &to))
        .with_on_page_load_handler(move |_, at| {
            let _ = tx.send(Msg::Page(key, Went::At(at)));
        })
        .with_new_window_req_handler(move |at, _| {
            let _ = window_asked.send(Msg::Page(key, Went::Asked(at)));
            wry::NewWindowResponse::Deny
        })
        .with_download_started_handler(move |at, _| {
            let _ = download_asked.send(Msg::Page(key, Went::Asked(at)));
            false
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
    fn a_web_page_is_knocked_for_at_its_host_and_port_and_a_file_is_not() {
        assert_eq!(
            door("http://127.0.0.1:5173/cover"),
            Some(("127.0.0.1".into(), 5173))
        );
        assert_eq!(
            door("https://example.com/"),
            Some(("example.com".into(), 443))
        );
        assert_eq!(door("http://[::1]:8000/"), Some(("[::1]".into(), 8000)));
        assert_eq!(door("file:///tmp/a.html"), None);
        assert!(answers("file:///tmp/nowhere.html"));
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
