//! Settings › Source control, as t3code has it: each host a project's code
//! or issues can be on, the CLI found for it here and a token of yours,
//! kept in the Keychain; and what merging leaves out.

use gpui::component::{Sizable as _, input::Input, setting::SettingPage};
use gpui::{AnyElement, Context, SharedString, WeakEntity, Window, div, prelude::*, px};

use super::prefs::{group, item};
use crate::{
    desk::Desk,
    forge::Host,
    ui::{self, Weight},
};

impl Desk {
    /// The Source control tab: each host, and what merging leaves out.
    pub(super) fn source_control(&self, w: &WeakEntity<Desk>) -> SettingPage {
        // A token holds everywhere: with a project chosen, it is not changed.
        let scoped = self.pages.scope.is_some();
        let hosts = Host::ALL
            .into_iter()
            .enumerate()
            .map(|(n, host)| {
                let seen = self.pages.forges.iter().find(|s| s.host == host);
                let cli = match (host.cli(), seen.and_then(|s| s.cli.as_ref())) {
                    (Some(_), Some(version)) => {
                        crate::tr_with("%{version} found", &[("version", version)])
                    }
                    (Some(cli), None) if seen.is_some() => {
                        crate::tr_with("%{cli} not found on the PATH", &[("cli", &cli)])
                    }
                    (None, _) => crate::tr("Read over its API with a token"),
                    _ => SharedString::default(),
                };
                let about = if seen.is_some_and(|s| s.token) {
                    crate::tr_with("%{cli} · your token is in the Keychain", &[("cli", &cli)])
                } else {
                    cli
                };
                item(w, host.name(), about, move |d, _, cx| d.token_field(n, cx)).disabled(scoped)
            })
            .collect();
        SettingPage::new(crate::tr("Source control"))
            .description(
                crate::tr("Where your projects' code and issues are. A token of yours is used over the CLI's own sign-in, and kept in the macOS Keychain, never in a file; it may be a 1Password reference (op://)."),
            )
            .group(Self::scope_group(w))
            .group(group(crate::tr("Hosts"), hosts))
            .group(group(
                crate::tr("Merging"),
                vec![item(
                    w,
                    crate::tr("Agents' credits in a squash"),
                    crate::tr("Co-authored-by lines and footers that credit an agent are taken out of a squashed commit's message; a person's stay."),
                    |d, _, cx| {
                        d.own(
                            "uncredit",
                            [("Kept", false), ("Taken out", true)],
                            |s| s.uncredit,
                            |s, v| s.uncredit = v,
                            cx,
                        )
                        .into_any_element()
                    },
                )],
            ))
    }

    /// The `n`th host's token: the way to remove it where one is kept, else
    /// a field to give one.
    fn token_field(&self, n: usize, cx: &mut Context<Self>) -> AnyElement {
        let host = Host::ALL[n];
        if self.pages.forges.iter().any(|s| s.host == host && s.token) {
            return ui::button(
                SharedString::from(format!("drop-token-{n}")),
                crate::tr("Remove token"),
                Weight::Secondary,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_token(host, None, cx)))
            .into_any_element();
        }
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .w(px(200.))
                    .child(Input::new(&self.pages.tokens[n]).small().mask_toggle()),
            )
            .child(
                ui::button(
                    SharedString::from(format!("keep-token-{n}")),
                    crate::tr("Keep"),
                    Weight::Secondary,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.keep_token(n, window, cx);
                })),
            )
            .into_any_element()
    }

    /// Keep the token typed for the `n`th host.
    fn keep_token(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.pages.tokens[n].read(cx).value().trim().to_owned();
        if token.is_empty() {
            return;
        }
        self.pages.tokens[n].update(cx, |i, cx| i.set_value("", window, cx));
        self.set_token(Host::ALL[n], Some(token), cx);
    }
}
