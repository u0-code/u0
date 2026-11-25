//! Settings › Source control, as t3code has it: each host a project's code
//! or issues can be on, the CLI found for it here and a token of yours,
//! kept in the Keychain; and what merging leaves out.

use gpui::component::{Sizable as _, input::Input};
use gpui::{Context, Div, SharedString, Window, div, prelude::*, px};

use super::prefs::{group, page, row};
use crate::{
    desk::Desk,
    fonts::Faces,
    forge::Host,
    ui::{self, Weight},
};

impl Desk {
    /// The Source control tab; none where the search `q` finds nothing in it.
    pub(super) fn source_control(
        &self,
        faces: &Faces,
        q: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let hosts = Host::ALL
            .into_iter()
            .enumerate()
            .map(|(n, host)| {
                let seen = self.pages.forges.iter().find(|s| s.host == host);
                let cli = match (host.cli(), seen.and_then(|s| s.cli.as_ref())) {
                    (Some(_), Some(version)) => format!("{version} found"),
                    (Some(cli), None) if seen.is_some() => format!("{cli} not found on the PATH"),
                    (None, _) => "Read over its API with a token".to_owned(),
                    _ => String::new(),
                };
                let kept = seen.is_some_and(|s| s.token);
                let about = if kept {
                    format!("{cli} · your token is in the Keychain")
                } else {
                    cli
                };
                let control = if kept {
                    ui::button(
                        SharedString::from(format!("drop-token-{n}")),
                        "Remove token",
                        Weight::Secondary,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_token(host, None, cx)))
                    .into_any_element()
                } else {
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
                                "Keep",
                                Weight::Secondary,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.keep_token(n, window, cx);
                                },
                            )),
                        )
                        .into_any_element()
                };
                row(faces, q, host.name(), &about, control)
            })
            .collect();
        let groups: Vec<Div> = [
            group(faces, "Hosts", hosts),
            group(
                faces,
                "Merging",
                vec![row(
                    faces,
                    q,
                    "Agents' credits in a squash",
                    "Co-authored-by lines and footers that credit an agent are taken out of a squashed commit's message; a person's stay.",
                    Self::pick(
                        "uncredit",
                        [("Kept", false), ("Taken out", true)],
                        self.settings.uncredit,
                        |s, v| s.uncredit = v,
                        cx,
                    ),
                )],
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!groups.is_empty()).then(|| {
            page(
                faces,
                "Source control",
                "Where your projects' code and issues are. A token of yours is used over the CLI's own sign-in, and kept in the macOS Keychain, never in a file; it may be a 1Password reference (op://).",
            )
            .children(groups)
        })
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
