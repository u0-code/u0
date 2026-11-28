//! The Preview tab (`TerminalBrowser` and `RightColumnStates` boards): with
//! no page open, the dev servers the session's terminals printed, to open;
//! on macOS, a page you opened, in the system's webview (`page`), under its
//! address bar and a strip naming its server.

use gpui::{Context, Div, div, prelude::*};
use u0ui::t3::workspace::{LocalServer, PreviewEmptyState, PreviewEmptyTexts};

use crate::desk::Desk;

impl Desk {
    /// The Preview tab: the page opened in it, or the dev servers to open.
    pub(super) fn preview_tab(&self, cx: &mut Context<Self>) -> Div {
        let tab = div().flex().flex_col().flex_1().min_h(gpui::px(0.));
        #[cfg(target_os = "macos")]
        if let Some(key) = self.current {
            return self.page_tab(tab, key, cx);
        }
        tab.child(self.no_page(cx))
    }

    /// With no page open: the dev servers to open, as cards, each opened
    /// here where u0 can show a page, else in your browser.
    pub(super) fn no_page(&self, cx: &mut Context<Self>) -> PreviewEmptyState {
        let desk = cx.entity();
        let to = self.pages_to();
        let servers = self.dev_servers();
        let cards: Vec<LocalServer> = servers
            .iter()
            .map(|s| LocalServer::new(s.port().unwrap_or_default(), s.title.clone()))
            .collect();
        let body = if cfg!(target_os = "macos") {
            crate::tr(
                "Type a URL above, or run a dev script. Local servers this thread starts appear here.",
            )
        } else {
            crate::tr("Dev servers your terminals print open in your browser.")
        };
        PreviewEmptyState::new("preview-empty")
            .servers(cards)
            .texts(PreviewEmptyTexts {
                title: crate::tr("No preview yet"),
                body,
                servers: crate::tr("Local servers"),
                recent: crate::tr("Recently used"),
            })
            .on_server(move |n, _, cx| {
                if let Some(s) = servers.get(n) {
                    let (host, printed) = (s.host, s.printed.clone());
                    desk.update(cx, |d, cx| d.open_address(host, printed, to, cx));
                }
            })
    }
}
