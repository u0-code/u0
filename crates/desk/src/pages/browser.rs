//! The Browser tab (the `SetBrowser` board): where a page you open shows,
//! which browser of yours "Open in your browser" uses, and the website
//! data the Preview tab's pages kept.

use std::path::PathBuf;

use gpui::component::{
    Sizable as _,
    setting::{SettingGroup, SettingItem, SettingPage},
    switch::Switch,
};
use gpui::{WeakEntity, prelude::*};
use u0ui::components::{Button, ButtonSize, ButtonVariant};

use super::prefs::{group, item};
use crate::{desk::Desk, settings::PagesIn, ui};

impl Desk {
    /// The Browser tab.
    pub(super) fn browser_tab(w: &WeakEntity<Desk>, banners: Option<SettingGroup>) -> SettingPage {
        let mut page = SettingPage::new(super::Tab::Browser.title()).description(crate::tr(
            "Where the pages you open show: a dev server's address, a link, an HTML file of yours. u0 shows them in the system's own web view, or your browser does.",
        ));
        if let Some(b) = banners {
            page = page.group(b);
        }
        page = page
            .group(group(crate::tr("Pages"), Self::pages_items(w)))
            .group(group(crate::tr("Your browser"), Self::browsers(w)));
        #[cfg(target_os = "macos")]
        {
            page = page.group(group(
                crate::tr("Preview's data"),
                vec![Self::website_data(w)],
            ));
        }
        page
    }

    fn pages_items(w: &WeakEntity<Desk>) -> Vec<SettingItem> {
        vec![
            item(
                w,
                crate::tr("Open pages in"),
                crate::tr(
                    "Preview shows the page in the right column, with Open in your browser beside it.",
                ),
                |d, _, cx| {
                    d.pick(
                        "open_pages",
                        [
                            (crate::tr("u0's Preview"), PagesIn::Preview),
                            (crate::tr("Your browser"), PagesIn::Browser),
                        ],
                        d.settings.open_pages,
                        |s, v| s.open_pages = v,
                        cx,
                    )
                },
            )
            .keywords(["Preview", "browser", "web view", "page"]),
            item(
                w,
                crate::tr("Web engine"),
                crate::tr(
                    "Pages use the system's own engine, kept current with the system; u0 brings none. Where the system has none, pages open in your browser.",
                ),
                |_, faces, _| {
                    ui::text(
                        faces,
                        if cfg!(target_os = "macos") {
                            crate::tr("WebKit, macOS's own")
                        } else {
                            crate::tr("None yet on this system")
                        },
                    )
                    .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("⌘-click a printed address"),
                crate::tr(
                    "An address a terminal prints opens where pages open; ⌥⌘-click opens it in your browser.",
                ),
                |d, _, cx| {
                    Switch::new("cmd-click-urls")
                        .accessibility_label(crate::tr("⌘-click a printed address"))
                        .small()
                        .checked(d.settings.cmd_click_urls)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.cmd_click_urls = !this.settings.cmd_click_urls;
                            this.save_settings();
                            cx.notify();
                        }))
                        .into_any_element()
                },
            ),
        ]
    }

    /// The browsers on this Mac: the system's default first, then each
    /// other that opens web addresses, the one in use said so.
    fn browsers(w: &WeakEntity<Desk>) -> Vec<SettingItem> {
        let (default, others) = crate::macos::browsers();
        let name = |app: &PathBuf| {
            app.file_stem()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        };
        let mut rows = vec![Self::browser_row(
            w,
            match &default {
                Some(app) => crate::tr_with("System default · %{name}", &[("name", &name(app))]),
                None => crate::tr("System default"),
            },
            crate::tr("What macOS opens links with"),
            None,
        )];
        for app in others.into_iter().filter(|a| Some(a) != default.as_ref()) {
            rows.push(Self::browser_row(
                w,
                name(&app).into(),
                app.display().to_string().into(),
                Some(app),
            ));
        }
        rows
    }

    /// A browser, `app` (`None`: the system's default), and the way to use it.
    fn browser_row(
        w: &WeakEntity<Desk>,
        title: gpui::SharedString,
        about: gpui::SharedString,
        app: Option<PathBuf>,
    ) -> SettingItem {
        let id = gpui::SharedString::from(format!("browser-{title}"));
        item(w, title, about, move |d, faces, cx| {
            if d.settings.browser == app {
                return ui::text(faces, crate::tr("In use")).into_any_element();
            }
            let app = app.clone();
            Button::new(id.clone(), crate::tr("Use this browser"))
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings.browser.clone_from(&app);
                    this.save_settings();
                    cx.notify();
                }))
                .into_any_element()
        })
    }

    /// The website data the pages kept, and Clear…, asked first.
    #[cfg(target_os = "macos")]
    fn website_data(w: &WeakEntity<Desk>) -> SettingItem {
        item(
            w,
            crate::tr("Website data"),
            crate::tr(
                "Cookies, storage and cache the pages you opened kept, in a store of u0's own, apart from your browser's.",
            ),
            |d, _, _| {
                let tx = d.tx.clone();
                Button::new("website-data-clear", crate::tr("Clear…"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(move |_, window, cx| {
                        use gpui::component::WindowExt as _;
                        let tx = tx.clone();
                        window.open_alert_dialog(cx, move |alert, _, _| {
                            let tx = tx.clone();
                            alert
                                .title(crate::tr("Clear website data?"))
                                .description(crate::tr(
                                    "Every page you opened in Preview forgets its cookies, storage and cache. Your browser's are not touched.",
                                ))
                                .ok_text(crate::tr("Clear"))
                                .show_cancel(true)
                                .on_ok(move |_, _, _| {
                                    crate::macos::clear_website_data(tx.clone());
                                    true
                                })
                        });
                    })
                    .into_any_element()
            },
        )
    }
}
