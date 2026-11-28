//! The Browser tab (the `SetBrowser` board): where a page you open shows,
//! which browser of yours "Open in your browser" uses, and the website
//! data the Preview tab's pages kept.

use std::path::PathBuf;

use gpui::component::{
    Disableable as _, Sizable as _,
    setting::{SettingGroup, SettingItem, SettingPage},
    switch::Switch,
};
use gpui::{WeakEntity, prelude::*};
use u0ui::components::{Button, ButtonSize, ButtonVariant};

use super::prefs::{group, item, managed};
use crate::{desk::Desk, servers::Host, settings::PagesIn, ui};

impl Desk {
    /// The Browser tab.
    pub(super) fn browser_tab(
        &self,
        w: &WeakEntity<Desk>,
        banners: Option<SettingGroup>,
    ) -> SettingPage {
        let mut page = SettingPage::new(super::Tab::Browser.title()).description(crate::tr(
            "Dev servers, links and HTML files open in u0's web view or your browser.",
        ));
        if let Some(b) = banners {
            page = page.group(b);
        }
        page = page
            .group(group(crate::tr("Pages"), Self::pages_items(w)))
            .group(group(crate::tr("Your browser"), self.browsers(w)));
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
                crate::tr("Preview shows pages in the right column."),
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
                crate::tr("The system's own engine; without one, pages open in your browser."),
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
                crate::tr("Opens where pages open; ⌥⌘-click opens it in your browser."),
                |d, _, cx| {
                    let switch = Switch::new("cmd-click-urls")
                        .accessibility_label(crate::tr("⌘-click a printed address"))
                        .small()
                        .checked(d.settings.cmd_click_urls)
                        .disabled(d.held_here("cmd_click_urls"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.cmd_click_urls = !this.settings.cmd_click_urls;
                            this.save_settings();
                            cx.notify();
                        }));
                    Self::as_held("cmd_click_urls", switch)
                },
            ),
        ]
    }

    /// Whether the setting `key` of this Mac's is kept from being changed
    /// here: an organisation's policy locks it, or the settings did not read.
    fn held_here(&self, key: &str) -> bool {
        crate::policy::locked(key).is_some() || self.settings_readonly(Host::HERE)
    }

    /// `control` for the setting `key`, said to be managed where a policy
    /// locks it (as `pick`'s are).
    fn as_held(key: &str, control: impl IntoElement) -> gpui::AnyElement {
        match crate::policy::locked(key) {
            Some(by) => managed(control, by),
            None => control.into_any_element(),
        }
    }

    /// The browsers on this Mac, as the settings dialog opened: the
    /// system's default first, then each other, the one in use said so.
    fn browsers(&self, w: &WeakEntity<Desk>) -> Vec<SettingItem> {
        let name = |app: &PathBuf| {
            app.file_stem()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        };
        let default = self.browsers.iter().find(|b| b.default);
        let mut rows = vec![Self::browser_row(
            w,
            0,
            match default {
                Some(b) => crate::tr_with("System default · %{name}", &[("name", &name(&b.app))]),
                None => crate::tr("System default"),
            },
            crate::tr("What macOS opens links with"),
            None,
        )];
        for (n, b) in self.browsers.iter().filter(|b| !b.default).enumerate() {
            rows.push(Self::browser_row(
                w,
                n + 1,
                name(&b.app).into(),
                b.app.display().to_string().into(),
                Some(b.app.clone()),
            ));
        }
        rows
    }

    /// The browser of row `n`, `app` (`None`: the system's default), and
    /// the way to use it.
    fn browser_row(
        w: &WeakEntity<Desk>,
        n: usize,
        title: gpui::SharedString,
        about: gpui::SharedString,
        app: Option<PathBuf>,
    ) -> SettingItem {
        let id = gpui::SharedString::from(format!("browser-{n}"));
        item(w, title, about, move |d, faces, cx| {
            if d.settings.browser == app {
                return ui::text(faces, crate::tr("In use")).into_any_element();
            }
            let app = app.clone();
            let button = Button::new(id.clone(), crate::tr("Use this browser"))
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .disabled(d.held_here("browser"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings.browser.clone_from(&app);
                    this.save_settings();
                    cx.notify();
                }));
            Self::as_held("browser", button)
        })
    }

    /// The website data the pages kept, and Clear…, asked first.
    #[cfg(target_os = "macos")]
    fn website_data(w: &WeakEntity<Desk>) -> SettingItem {
        item(
            w,
            crate::tr("Website data"),
            crate::tr("Cookies, storage and cache, kept apart from your browser's."),
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
                                    "Preview forgets all cookies, storage and cache. Your browser is untouched.",
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
