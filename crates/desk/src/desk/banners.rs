//! Cross-screen banners for unreadable files, organisation policy, and
//! servers out of reach (`Banners`), built on `u0ui`'s Carbon `Alert`.

use std::time::SystemTime;

use gpui::component::setting::SettingGroup;
use gpui::{AnyElement, Context, Div, SharedString, WeakEntity, div, prelude::*, px};
use u0ui::components::alert::{Alert, AlertAction, AlertVariant};
use u0ui::prelude::*;

use super::Desk;
use crate::{
    fonts::Faces,
    hub::{Host, Machine, Up},
    pages::Tab,
    theme, when,
};

/// Build a Carbon low-contrast `Alert` (`InlineNotification` /
/// `ActionableNotification`), with an optional external support link under
/// its subtitle and an optional ghost action button.
pub(crate) fn banner(
    id: impl Into<gpui::ElementId>,
    variant: AlertVariant,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    link: Option<(&str, SharedString)>,
    action: Option<AlertAction>,
) -> Alert {
    let id = id.into();
    let body = body.into();
    let desc: AnyElement = match link {
        None => div().child(body).into_any_element(),
        Some((url, label)) => {
            let url = url.to_owned();
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(body)
                .child(
                    div()
                        .id((id.clone(), "support-link"))
                        .cursor_pointer()
                        .type_style(TypeStyle::BodyCompact01)
                        .text_color(color(ColorRole::LinkPrimary))
                        .hover(|s| s.text_color(color(ColorRole::LinkPrimaryHover)))
                        .child(format!("{label} ↗"))
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
                .into_any_element()
        }
    };
    Alert::new(id)
        .variant(variant)
        .title(title)
        .description(desc)
        .when_some(action, Alert::action)
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
                    .child(Self::bad_policy_banner("thread-bad-policy", bad))
                    .into_any_element(),
            );
        } else if let Some(why) = &self.settings_refused {
            let desk = cx.entity();
            let again = AlertAction::new(crate::tr("Read them again")).on_click(move |_, cx| {
                desk.update(cx, Self::read_settings_again);
            });
            out.push(
                div()
                    .px(px(16.))
                    .pt(px(8.))
                    .child(banner(
                        "thread-bad-settings",
                        AlertVariant::Error,
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

    /// Compact band over a thread whose server `m` is offline or refused
    /// (`Banners.dc.html` `.bnb-band`).
    fn offline_band(&self, _faces: &Faces, m: &Machine, cx: &mut Context<Self>) -> Div {
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
            .bg(color(ColorRole::Layer01))
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .border_1()
                    .border_color(color(ColorRole::IconSecondary)),
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
                        div()
                            .type_style(TypeStyle::HeadingCompact01)
                            .text_color(color(ColorRole::TextPrimary))
                            .child(lead),
                    )
                    .child(
                        div()
                            .type_style(TypeStyle::Label01)
                            .text_color(color(ColorRole::TextSecondary))
                            .child(format!("· {detail}")),
                    ),
            )
            .child(
                Button::new("thread-reach-again", crate::tr("Reach again"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Renew)
                    .on_click(cx.listener(move |this, _, _, cx| this.reach_again(host, cx))),
            )
    }

    /// Warning banner in the sidebar when `sessions.json` did not read.
    pub(crate) fn sidebar_banner(&self, _faces: &Faces) -> Option<Div> {
        let why = self
            .machines
            .iter()
            .find_map(|m| m.store_refused.as_deref())?;
        Some(div().px(px(8.)).pb(px(8.)).child(banner(
            "sidebar-bad-sessions",
            AlertVariant::Warning,
            crate::tr("sessions.json does not read"),
            why.to_owned(),
            None,
            None,
        )))
    }

    /// Whether `tab` currently has a top-of-page Settings banner.
    fn has_settings_banner(&self, tab: Tab) -> bool {
        let on = self.pages.on;
        if on == Host::HERE
            && (crate::policy::current().is_err()
                || self.settings_refused.is_some()
                || crate::policy::organization().is_some())
        {
            return true;
        }
        if on != Host::HERE
            && self
                .machine(on)
                .is_some_and(|m| m.refused.is_some() || m.up != Up::On)
        {
            return true;
        }
        tab == Tab::Keybindings && self.keys_refused.is_some()
    }

    /// Carbon `Alert` stack at the top of a `carbon_page` Settings tab
    /// (`General`, `NewThreads`, …).
    pub(crate) fn carbon_settings_banner(
        &self,
        tab: Tab,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut out: Vec<AnyElement> = Vec::new();
        let on = self.pages.on;
        if on == Host::HERE
            && let Err(bad) = crate::policy::current()
        {
            out.push(Self::bad_policy_banner("settings-bad-policy", bad).into_any_element());
        } else if let Some(why) = match on {
            Host::HERE => self.settings_refused.clone(),
            _ => self.machine(on).and_then(|m| m.refused.clone()),
        } {
            let desk = cx.entity();
            let again = (on == Host::HERE).then(|| {
                AlertAction::new(crate::tr("Read them again")).on_click(move |_, cx| {
                    desk.update(cx, Self::read_settings_again);
                })
            });
            let body = if on == Host::HERE {
                crate::tr(
                    "settings.json did not read, so these are the defaults, and a change would not be kept. Each control is off until the file reads.",
                )
            } else {
                SharedString::from(why)
            };
            out.push(
                banner(
                    "settings-bad-settings",
                    AlertVariant::Error,
                    crate::tr("Nothing here is saved"),
                    body,
                    None,
                    again,
                )
                .into_any_element(),
            );
        } else if on != Host::HERE
            && let Some(m) = self.machine(on).filter(|m| m.up != Up::On)
        {
            let name = m.name.clone();
            let desk = cx.entity();
            let again = AlertAction::new(crate::tr("Reach again")).on_click(move |_, cx| {
                desk.update(cx, |this, cx| this.reach_again(on, cx));
            });
            out.push(
                banner(
                    "settings-offline-server",
                    AlertVariant::Warning,
                    crate::tr_with("%{name} is offline: read only", &[("name", &name)]),
                    crate::tr("Its settings are shown as it last sent them, and can be changed again once it is back. u0 tries it on its own, waiting longer each time, up to 30 seconds."),
                    None,
                    Some(again),
                )
                .into_any_element(),
            );
        } else if on == Host::HERE
            && let Some(org) = crate::policy::organization()
        {
            let link = crate::policy::support_url().map(|u| {
                (
                    u,
                    crate::tr_with("Questions go to %{url}", &[("url", &short_support(u))]),
                )
            });
            out.push(
                banner(
                    "settings-managed-policy",
                    AlertVariant::Info,
                    crate::tr_with("Managed by %{by}", &[("by", &org)]),
                    crate::tr_with(
                        "Some settings here are set by %{org}’s policy: each shows a lock, its value and who set it.",
                        &[("org", &org)],
                    ),
                    link,
                    None,
                )
                .into_any_element(),
            );
        }

        if tab == Tab::Keybindings
            && let Some(why) = self.keys_refused.clone()
        {
            let path = self.home.join("keybindings.json");
            let desk = cx.entity();
            let open =
                AlertAction::new(crate::tr("Open keybindings.json")).on_click(move |_, cx| {
                    let path = path.clone();
                    desk.update(cx, |this, _| this.edit(path, None));
                });
            out.push(
                banner(
                    "settings-bad-keys",
                    AlertVariant::Warning,
                    crate::tr("keybindings.json did not read, so it is ignored"),
                    crate::tr_with(
                        "%{why} u0 uses its own keys meanwhile and logs why; this page leaves the file untouched rather than writing over it.",
                        &[("why", &why)],
                    ),
                    None,
                    Some(open),
                )
                .into_any_element(),
            );
        }

        (!out.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(out)
                .into_any_element()
        })
    }

    /// Banner group at the top of a `SettingPage` `tab` when settings/policy
    /// did not read, the chosen server is offline, `keybindings.json` did not
    /// read, or an organisation's policy governs this Mac.
    pub(crate) fn settings_banner_group(
        &self,
        tab: Tab,
        w: &WeakEntity<Self>,
    ) -> Option<SettingGroup> {
        self.has_settings_banner(tab).then(|| {
            SettingGroup::new().item(crate::pages::whole(w, &[], move |d, _, cx| {
                d.carbon_settings_banner(tab, cx)
                    .unwrap_or_else(|| div().into_any_element())
            }))
        })
    }

    fn bad_policy_banner(id: &'static str, bad: &crate::policy::BadPolicy) -> Alert {
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
            id,
            AlertVariant::Error,
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
