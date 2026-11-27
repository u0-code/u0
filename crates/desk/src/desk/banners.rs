//! Cross-screen banners for unreadable files, organisation policy, and
//! servers out of reach (`Banners`).

use std::time::SystemTime;

use gpui::component::setting::{SettingGroup, SettingItem};
use gpui::{AnyElement, Context, Div, SharedString, WeakEntity, div, prelude::*, px};

use super::Desk;
use crate::fonts::Weighted as _;
use crate::{
    fonts::Faces,
    hub::{Host, Machine, Up},
    pages::Tab,
    theme,
    ui::{self, Weight},
    when,
};

/// A banner's severity, setting its border and icon colour.
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Info,
    Warning,
    Error,
}

/// A structured banner card: a left accent bar, icon, bold title, body,
/// optional link, and optional action button.
pub(crate) fn banner(
    faces: &Faces,
    kind: Kind,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    link: Option<(&str, SharedString)>,
    action: Option<AnyElement>,
) -> Div {
    let (accent, icon) = match kind {
        Kind::Info => (theme::link(), "bulb"),
        Kind::Warning => (theme::accent(), "alert"),
        Kind::Error => (theme::coral(), "alert"),
    };
    let link_el = link.map(|(url, label)| {
        let url = url.to_owned();
        div()
            .id(SharedString::from(format!("banner-link-{url}")))
            .cursor_pointer()
            .child(
                ui::text(faces, label)
                    .text_size(px(crate::style::CAPTION))
                    .text_color(theme::link()),
            )
            .on_click(move |_, _, cx| cx.open_url(&url))
    });
    div()
        .flex()
        .items_start()
        .gap(px(10.))
        .w_full()
        .px(px(12.))
        .py(px(10.))
        .bg(theme::panel())
        .border_1()
        .border_l(px(3.))
        .border_color(accent)
        .child(ui::icon(icon, 16.).mt(px(2.)).text_color(accent))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(2.))
                .child(
                    ui::text(faces, title)
                        .text_size(px(crate::style::LABEL))
                        .weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::text()),
                )
                .child(
                    ui::text(faces, body)
                        .text_size(px(crate::style::CAPTION))
                        .line_height(px(18.))
                        .text_color(theme::text2()),
                )
                .children(link_el),
        )
        .children(action)
}

/// Short display host/path of a support URL (`https://it.example.com/u0` →
/// `it.example.com/u0`).
fn short_support(url: &str) -> &str {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
}

impl Desk {
    /// Whether the settings shown on `host` are read-only (unreadable file,
    /// broken policy, or offline server).
    pub(crate) fn settings_readonly(&self, host: Host) -> bool {
        match self.machine(host) {
            Some(m) if host != Host::HERE => m.refused.is_some() || m.up != Up::On,
            _ => self.settings_refused.is_some(),
        }
    }

    /// Banners shown over the thread area: unreadable policy or
    /// `settings.json`, and the compact offline band when the session's
    /// server is out of reach.
    pub(crate) fn thread_banners(&self, faces: &Faces, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if let Err(bad) = crate::policy::current() {
            out.push(
                div()
                    .px(px(16.))
                    .pt(px(8.))
                    .child(Self::bad_policy_banner(faces, bad))
                    .into_any_element(),
            );
        } else if let Some(why) = &self.settings_refused {
            let again = ui::button(
                "thread-read-settings",
                crate::tr("Read them again"),
                Weight::Secondary,
            )
            .on_click(cx.listener(|this, _, _, cx| this.read_settings_again(cx)))
            .into_any_element();
            out.push(
                div()
                    .px(px(16.))
                    .pt(px(8.))
                    .child(banner(
                        faces,
                        Kind::Error,
                        crate::tr("Your settings did not read"),
                        crate::tr_with(
                            "%{why} u0 runs on its defaults and saves nothing until it reads.",
                            &[("why", why)],
                        ),
                        None,
                        Some(again),
                    ))
                    .into_any_element(),
            );
        }
        if let Some(s) = self.session()
            && let Some(m) = self.machine(s.host).filter(|m| m.up != Up::On)
        {
            out.push(self.offline_band(faces, m, cx).into_any_element());
        }
        out
    }

    /// Compact band over a thread whose server `m` is offline or refused.
    fn offline_band(&self, faces: &Faces, m: &Machine, cx: &mut Context<Self>) -> Div {
        let host = m.host;
        let now = SystemTime::now();
        let (lead, detail) = match &m.up {
            Up::Refused(why) => (
                crate::tr_with("%{name} turned this Mac away", &[("name", &m.name)]),
                crate::tr_with(
                    "%{why} · not tried again until Reach again",
                    &[("why", why)],
                ),
            ),
            Up::Reaching => (
                crate::tr_with("Reaching %{name}…", &[("name", &m.name)]),
                crate::tr("what this window had stays readable"),
            ),
            Up::Lost(_) | Up::On => {
                let since = m
                    .lost_since
                    .map(|t| when::said_at(t, now, self.settings.twelve));
                let secs = m.retry_in().map(|d| d.as_secs().max(1));
                let detail = match (since, secs) {
                    (Some(since), Some(secs)) => crate::tr_with(
                        "since %{since} · what this window had stays readable · next try in %{secs} s",
                        &[("since", &since), ("secs", &secs)],
                    ),
                    (Some(since), None) => crate::tr_with(
                        "since %{since} · what this window had stays readable",
                        &[("since", &since)],
                    ),
                    _ => crate::tr("what this window had stays readable"),
                };
                (
                    crate::tr_with("%{name} is offline", &[("name", &m.name)]),
                    detail,
                )
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(36.))
            .px(px(16.))
            .py(px(6.))
            .bg(theme::panel())
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .border_1()
                    .border_color(theme::muted()),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(6.))
                    .child(
                        ui::text(faces, lead)
                            .text_size(px(crate::style::CAPTION))
                            .weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::text()),
                    )
                    .child(
                        ui::text(faces, format!("· {detail}"))
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::muted()),
                    ),
            )
            .child(
                ui::button(
                    "thread-reach-again",
                    crate::tr("Reach again"),
                    Weight::Quiet,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.reach_again(host, cx))),
            )
    }

    /// Warning banner in the sidebar when `sessions.json` did not read.
    pub(crate) fn sidebar_banner(&self, faces: &Faces) -> Option<Div> {
        let why = self
            .machines
            .iter()
            .find_map(|m| m.store_refused.as_deref())?;
        Some(div().px(px(8.)).pb(px(8.)).child(banner(
            faces,
            Kind::Warning,
            crate::tr("sessions.json does not read"),
            why.to_owned(),
            None,
            None,
        )))
    }

    /// Banner group at the top of a Settings page `tab` when settings/policy
    /// did not read, the chosen server is offline, `keybindings.json` did
    /// not read, or an organisation's policy governs this Mac.
    pub(crate) fn settings_banner_group(
        &self,
        tab: Tab,
        w: &WeakEntity<Self>,
    ) -> Option<SettingGroup> {
        let mut items: Vec<SettingItem> = Vec::new();
        let on = self.pages.on;
        if on == Host::HERE
            && let Err(bad) = crate::policy::current()
        {
            items.push(crate::pages::whole(w, &[], move |_, faces, _| {
                Self::bad_policy_banner(faces, bad).into_any_element()
            }));
        } else if let Some(why) = match on {
            Host::HERE => self.settings_refused.clone(),
            _ => self.machine(on).and_then(|m| m.refused.clone()),
        } {
            items.push(crate::pages::whole(w, &[], move |_, faces, cx| {
                let again = (on == Host::HERE).then(|| {
                    ui::button(
                        "settings-read-again",
                        crate::tr("Read them again"),
                        Weight::Secondary,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.read_settings_again(cx)))
                    .into_any_element()
                });
                let body = if on == Host::HERE {
                    crate::tr("settings.json did not read, so these are the defaults, and a change would not be kept. Each control is off until the file reads.")
                } else {
                    SharedString::from(why.clone())
                };
                banner(
                    faces,
                    Kind::Error,
                    crate::tr("Nothing here is saved"),
                    body,
                    None,
                    again,
                )
                .into_any_element()
            }));
        } else if on != Host::HERE
            && let Some(m) = self.machine(on).filter(|m| m.up != Up::On)
        {
            let name = m.name.clone();
            items.push(crate::pages::whole(w, &[], move |_, faces, cx| {
                let again = ui::button(
                    "settings-reach-again",
                    crate::tr("Reach again"),
                    Weight::Secondary,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.reach_again(on, cx)))
                .into_any_element();
                banner(
                    faces,
                    Kind::Warning,
                    crate::tr_with("%{name} is offline: read only", &[("name", &name)]),
                    crate::tr("Its settings are shown as it last sent them, and can be changed again once it is back. u0 tries it on its own, waiting longer each time, up to 30 seconds."),
                    None,
                    Some(again),
                )
                .into_any_element()
            }));
        } else if on == Host::HERE
            && let Some(org) = crate::policy::organization()
        {
            let url = crate::policy::support_url();
            items.push(crate::pages::whole(w, &[], move |_, faces, _| {
                let link = url.map(|u| {
                    (
                        u,
                        crate::tr_with("Questions go to %{url}", &[("url", &short_support(u))]),
                    )
                });
                banner(
                    faces,
                    Kind::Info,
                    crate::tr_with("Managed by %{by}", &[("by", &org)]),
                    crate::tr_with(
                        "Some settings here are set by %{org}’s policy: each shows a lock, its value and who set it.",
                        &[("org", &org)],
                    ),
                    link,
                    None,
                )
                .into_any_element()
            }));
        }

        if tab == Tab::Keybindings
            && let Some(why) = self.keys_refused.clone()
        {
            items.push(crate::pages::whole(w, &[], move |d, faces, cx| {
                let path = d.home.join("keybindings.json");
                let open = ui::button(
                    "banner-open-keys",
                    crate::tr("Open keybindings.json"),
                    Weight::Secondary,
                )
                .on_click(cx.listener(move |this, _, _, _| {
                    this.edit(path.clone(), None);
                }))
                .into_any_element();
                banner(
                    faces,
                    Kind::Warning,
                    crate::tr("keybindings.json did not read, so it is ignored"),
                    crate::tr_with(
                        "%{why} u0 uses its own keys meanwhile and logs why; this page leaves the file untouched rather than writing over it.",
                        &[("why", &why)],
                    ),
                    None,
                    Some(open),
                )
                .into_any_element()
            }));
        }

        (!items.is_empty()).then(|| SettingGroup::new().items(items))
    }

    fn bad_policy_banner(faces: &Faces, bad: &crate::policy::BadPolicy) -> Div {
        let title = match &bad.organization {
            Some(org) => crate::tr_with("%{org}’s policy does not read", &[("org", org)]),
            None => crate::tr("The policy does not read"),
        };
        let link = bad.support_url.as_deref().map(|u| {
            (
                u,
                crate::tr_with("Ask at %{url}", &[("url", &short_support(u))]),
            )
        });
        banner(
            faces,
            Kind::Error,
            title,
            crate::tr_with(
                "%{why}. u0 starts on its defaults and saves nothing until the policy is fixed: nothing runs half governed. Ask whoever manages this Mac.",
                &[("why", &bad.why)],
            ),
            link,
            None,
        )
    }
}
