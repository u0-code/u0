//! Carbon pairing dialogs, approval cards, outside-agent MCP setup cards, and
//! read-only paired-device (`Guest`) chrome (`Pairing.dc.html` and
//! `Guest.dc.html`).

use std::{rc::Rc, time::SystemTime};

use gpui::component::{WindowExt as _, scroll::ScrollableElement as _};
use gpui::{AnyElement, Context, Div, ElementId, SharedString, Window, div, prelude::*, px};
use u0ui::{
    components::{
        alert::{Alert, AlertAction, AlertVariant},
        button::{Button, ButtonSize, ButtonVariant},
        checkbox::Checkbox,
        form::FieldSize,
        input::Input,
        list_box::ListItem,
        select::Select,
    },
    icons::{CarbonIcon, Icon as CarbonGlyph},
    t3::{Status, StatusPill, fmt_elapsed},
    theme::LayerLevel,
    tokens::{ColorRole, TypeStyle},
    typography::TypeStyled as _,
    u0::{
        devices::{PairingCard, PairingCardTexts},
        primitives::CopyField,
    },
};

use crate::{
    access::{Device, Scope},
    desk::{Desk, Over, Place},
    fonts::{Faces, Weighted as _},
    hub::Host,
    key::{self, Key},
    reach::{Route, Way},
    session::State as SessionState,
    share::Share,
    store, theme, ui, when,
};

/// Active Carbon pairing modal opened from `u0 › Show Pairing Link…`,
/// `u0 › Pair a Device…`, or a `u0://pair` link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairingModal {
    /// `"Pair a device with this Mac"` (`Show Pairing Link…`).
    Offer,
    /// `"Control another Mac from this one"` / `"Pair with “<Host>”?"`.
    Control { confirm: Option<PendingPair> },
}

/// A parsed `u0://pair` link awaiting user confirmation (`Pair with “<Host>”?`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPair {
    pub way: Way,
    pub code: String,
    pub kept: String,
    pub print: String,
    pub both: bool,
}

/// Shorten a `SHA256:...` fingerprint to `SHA256:abcdef…1234` when compact
/// space requires it (`Guest.dc.html` `.gv-who`, `Pairing.dc.html` `Alert`).
pub(crate) fn short_fingerprint(fp: &str) -> String {
    let rest = fp.strip_prefix("SHA256:").unwrap_or(fp);
    let chars: Vec<char> = rest.chars().collect();
    if chars.len() <= 12 {
        return fp.to_owned();
    }
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("SHA256:{head}\u{2026}{tail}")
}

/// Carbon `.pgc` card container (`background: layer-01; padding: 20px; gap: 16px`).
pub(crate) fn pgc_card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .p(px(20.))
        .bg(theme::role(ColorRole::Layer01))
}

/// Carbon `.pgc-h` header with 32×32 `layer-accent-01` icon square, 16/22 bold
/// title, and 14/20 secondary subtitle.
pub(crate) fn pgc_header(
    icon: CarbonIcon,
    title: impl Into<SharedString>,
    subtitle: impl Into<SharedString>,
    trailing: Option<AnyElement>,
) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(32.))
                .bg(theme::role(ColorRole::LayerAccent01))
                .child(
                    CarbonGlyph::carbon(icon)
                        .size_length(px(16.))
                        .color(theme::role(ColorRole::IconPrimary)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(22.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::role(ColorRole::TextPrimary))
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(subtitle.into()),
                ),
        )
        .children(trailing)
}

/// Two-column `.pgc-kv` key-value row (`96px` label + `1fr` value).
pub(crate) fn pgc_kv_row(label: impl Into<SharedString>, value: impl IntoElement) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_size(px(12.))
                .line_height(px(20.))
                .text_color(theme::role(ColorRole::TextSecondary))
                .child(label.into()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(14.))
                .line_height(px(20.))
                .text_color(theme::role(ColorRole::TextPrimary))
                .child(value),
        )
}

/// `"What a read-only device does"` 2-column capability grid (`Guest.dc.html` `.gh-see`).
pub(crate) fn readonly_capabilities_grid() -> Div {
    let yes = |label: SharedString| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(theme::role(ColorRole::TextPrimary))
            .child(
                CarbonGlyph::carbon(CarbonIcon::Checkmark)
                    .size_length(px(16.))
                    .color(theme::role(ColorRole::SupportSuccess))
                    .flex_none(),
            )
            .child(label)
    };
    let no = |label: SharedString| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(theme::role(ColorRole::TextSecondary))
            .child(
                CarbonGlyph::named("cancel")
                    .size_length(px(16.))
                    .color(theme::role(ColorRole::IconSecondary))
                    .flex_none(),
            )
            .child(label)
    };
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .pt(px(12.))
        .border_t_1()
        .border_color(theme::role(ColorRole::BorderSubtle01))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(theme::role(ColorRole::TextSecondary))
                .child(crate::tr("What a read-only device does")),
        )
        .child(
            div()
                .flex()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap(px(6.))
                        .child(yes(crate::tr("Watches threads and plans")))
                        .child(yes(crate::tr("Reads diffs and files")))
                        .child(yes(crate::tr("Sees commands and output"))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap(px(6.))
                        .child(no(crate::tr("Sends or approves")))
                        .child(no(crate::tr("Terminals and settings"))),
                ),
        )
}

/// `"Add u0 to an agent"` card (`Pairing.dc.html` Section 5).
pub(crate) fn add_u0_to_agent_card(id_prefix: &'static str) -> Div {
    pgc_card()
        .child(pgc_header(
            CarbonIcon::Link,
            crate::tr("Add u0 to an agent"),
            crate::tr(
                "Run once in a terminal. The agent then reads through u0, and asks on a card before it acts.",
            ),
            None,
        ))
        .child(
            CopyField::new(
                (ElementId::from(id_prefix), "claude"),
                "claude mcp add -s user u0 -- u0 --mcp",
            )
            .label("Claude Code")
            .level(LayerLevel::Two),
        )
        .child(
            CopyField::new(
                (ElementId::from(id_prefix), "codex"),
                "codex mcp add u0 -- u0 --mcp",
            )
            .label("Codex")
            .level(LayerLevel::Two),
        )
        .child(
            CopyField::new((ElementId::from(id_prefix), "other"), "u0 --mcp")
                .label(crate::tr("Any other MCP client"))
                .level(LayerLevel::Two),
        )
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(theme::role(ColorRole::TextHelper))
                .child(crate::tr(
                    "With -s user, Claude Code adds u0 for every project. u0 --mcp finds the port and key in ~/.u0/mcp.json, made anew each time u0 starts.",
                )),
        )
}

/// `"No computer with u0 yet?"` card (`Pairing.dc.html` Section 6).
pub(crate) fn install_u0_elsewhere_card(id_prefix: &'static str, faces: &Faces) -> Div {
    pgc_card()
        .child(pgc_header(
            CarbonIcon::Download,
            crate::tr("No computer with u0 yet?"),
            crate::tr(
                "On a Mac, install the app. On a Linux box, run the server without a window and pair a Mac with it.",
            ),
            None,
        ))
        .child(
            div().flex().child(
                Button::new(
                    (ElementId::from(id_prefix), "dl-macos"),
                    crate::tr("Download for macOS"),
                )
                .variant(ButtonVariant::Tertiary)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Download)
                    .on_click(|_, _, cx| {
                        crate::aside::rules::to_browser("https://github.com/u0-code/u0/releases/latest", cx);
                    }),
            ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(theme::role(ColorRole::TextSecondary))
                        .child(crate::tr("Linux server, from the GitHub release")),
                )
                .child(
                    ui::mono(
                        faces,
                        "$ u0-server\nu0 serves on ws://127.0.0.1:4317\nPair a desk with it, once, in the next ten minutes:\nu0://pair?host=127.0.0.1:4317#…",
                    )
                    .px(px(12.))
                    .py(px(8.))
                    .bg(theme::role(ColorRole::Layer02))
                    .border_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(theme::role(ColorRole::TextPrimary)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(theme::role(ColorRole::TextHelper))
                        .child(crate::tr(
                            "It also prints the link's QR. Reach it with ssh -L 4317:127.0.0.1:4317 host, Tailscale, or its own cloudflared tunnel.",
                        )),
                ),
        )
}

impl Desk {
    /// Active read-only remote [`crate::hub::Machine`] for the thread currently on screen.
    pub(crate) fn guest_machine(&self) -> Option<&crate::hub::Machine> {
        let s = self.session()?;
        let m = self.machine(s.host)?;
        (m.host != Host::HERE && m.scope == Scope::Read).then_some(m)
    }

    /// `48px` thread header shown below the window title bar when viewing a
    /// thread on a read-only paired machine (`Guest.dc.html` `.gv-head`).
    pub(crate) fn guest_thread_header(
        &self,
        faces: &Faces,
        column: f32,
    ) -> Option<impl IntoElement> {
        if self.page != crate::desk::Page::Thread {
            return None;
        }
        let s = self.session()?;
        let m = self.guest_machine()?;
        let proj = self.project_name(&Place::project_of(s));
        let sub = crate::tr_with(
            "On %{host} · %{project} · you can watch; suggesting a message is Later",
            &[("host", &m.name), ("project", &proj)],
        );
        let paired_as = m
            .guest_label
            .clone()
            .unwrap_or_else(|| crate::tr("This Mac").to_string());
        let fp = m
            .paired
            .as_ref()
            .map(|p| short_fingerprint(&key::fingerprint(&p.key)))
            .unwrap_or_default();
        let (pill_status, pill_label) = match s.state {
            SessionState::Working | SessionState::Opening => {
                let secs = s.since.map_or(0, |t| t.elapsed().as_secs());
                (
                    Status::Working,
                    crate::tr_with("Working · %{elapsed}", &[("elapsed", &fmt_elapsed(secs))]),
                )
            }
            SessionState::Closed if s.meta.flags.settled.is_some() => {
                (Status::Done, crate::tr("Done"))
            }
            _ => (Status::Idle, crate::tr("Idle")),
        };

        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(px(48.))
                .border_b_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .bg(theme::bg())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .w_full()
                        .max_w(px(column + 48.))
                        .mx_auto()
                        .px(px(24.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(0.))
                                .child(
                                    ui::text(faces, s.shown_title().to_owned())
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .weight(gpui::FontWeight::SEMIBOLD)
                                        .truncate(),
                                )
                                .child(
                                    ui::text(faces, sub)
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(theme::role(ColorRole::TextSecondary))
                                        .truncate(),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_none()
                                .items_end()
                                .child(
                                    ui::text(
                                        faces,
                                        crate::tr_with(
                                            "Paired as %{name}",
                                            &[("name", &paired_as)],
                                        ),
                                    )
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(theme::role(ColorRole::TextPrimary)),
                                )
                                .when(!fp.is_empty(), |d| {
                                    d.child(
                                        ui::mono(faces, fp)
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(theme::role(ColorRole::TextHelper)),
                                    )
                                }),
                        )
                        .child(
                            StatusPill::new(pill_status)
                                .id("guest-status")
                                .label(pill_label),
                        ),
                ),
        )
    }

    /// Open `"Pair a device with this Mac"` (`Show Pairing Link…`, or
    /// `"Share read-only link…"` when `read_only` is true).
    pub(crate) fn open_pairing_dialog(
        &mut self,
        read_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pages.on = Host::HERE;
        if read_only {
            self.pages.grant.scope = Scope::Read;
            self.pages.grant.days = Some(1);
            self.pages.grant.ask = true;
            self.pages.grant.both = false;
        }
        self.share(Share::Serve(true));
        self.call(Host::HERE, crate::proto::Call::Share(Share::Look));
        self.new_link();
        self.show_pairing_modal(PairingModal::Offer, 560., window, cx);
    }

    /// Open `"Control another Mac from this one"` (`u0 › Pair a Device…` or a
    /// `u0://pair` link).
    pub(crate) fn open_pair_another_dialog(
        &mut self,
        confirm: Option<PendingPair>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_control(true, window, cx);
        let width = if confirm.is_some() { 820. } else { 520. };
        self.show_pairing_modal(PairingModal::Control { confirm }, width, window, cx);
    }

    fn show_pairing_modal(
        &mut self,
        modal: PairingModal,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pages.open {
            self.close_settings(window, cx);
        }
        if self.pages.pairing_modal.is_some() {
            window.close_dialog(cx);
        }
        self.pages.pairing_modal = Some(modal);
        let desk = cx.entity();
        let shown = cx.new(|cx| Over::new(desk.clone(), Desk::pairing_dialog, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            let desk = desk.clone();
            dialog
                .w(px(width))
                .p_0()
                .border_0()
                .rounded(px(0.))
                .close_button(false)
                .on_close(move |_, _, cx| {
                    desk.update(cx, |d, cx| {
                        d.pages.pairing_modal = None;
                        cx.notify();
                    });
                })
                .child(shown.clone())
        });
        cx.notify();
    }

    /// Close any open pairing modal dialog.
    pub(crate) fn close_pairing_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pages.pairing_modal.take().is_some() {
            window.close_dialog(cx);
            cx.notify();
        }
    }

    /// Render the active [`PairingModal`].
    pub(crate) fn pairing_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = window;
        let faces = crate::fonts::faces(cx).clone();
        let desk_close = cx.entity();
        let close_btn = Button::new("close-pairing-modal", crate::tr("Close"))
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Sm)
            .icon_only(true)
            .icon(CarbonIcon::Close)
            .on_click(move |_, w, cx| {
                desk_close.update(cx, |this, cx| this.close_pairing_dialog(w, cx));
            })
            .into_any_element();

        let content: AnyElement = match self.pages.pairing_modal.clone() {
            Some(PairingModal::Offer) => self
                .render_offer_pairing_modal(&faces, close_btn, cx)
                .into_any_element(),
            Some(PairingModal::Control { confirm }) => self
                .render_control_pairing_modal(&faces, confirm, close_btn, cx)
                .into_any_element(),
            None => div().into_any_element(),
        };

        div()
            .id("pairing-modal-scroll")
            .max_h(px(780.))
            .overflow_y_scrollbar()
            .bg(theme::role(ColorRole::Layer01))
            .child(content)
            .into_any_element()
    }

    ///Waiting-device approval card (`Pairing.dc.html` `"Let “iPhone 16 Pro” in?"`).
    pub(crate) fn waiting_device_card(
        &self,
        dev: &Device,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Div {
        let now = SystemTime::now();
        let ago = when::ago(store::time(dev.paired), now);
        let print = key::fingerprint(&dev.key);
        let may_do = match dev.scope {
            Scope::Read => crate::tr("Read only: watch threads, diffs and files; change nothing"),
            Scope::Full => crate::tr("Everything: threads, files, terminals and settings"),
        };
        let until_str = match dev.until {
            Some(ts) => when::short_at(store::time(ts), now, self.settings.twelve),
            None => crate::tr("Until revoked").to_string(),
        };
        let standing_short = match dev.scope {
            Scope::Read => crate::tr("read only").to_string(),
            Scope::Full => crate::tr("everything").to_string(),
        };
        let summary = match dev.until {
            Some(ts) => format!(
                "{standing_short}, until {}",
                when::short_at(store::time(ts), now, self.settings.twelve)
            ),
            None => standing_short,
        };

        let is_phone = dev.label.to_lowercase().contains("iphone")
            || dev.label.to_lowercase().contains("phone")
            || dev.label.to_lowercase().contains("android");
        let icon = if is_phone {
            CarbonIcon::Mobile
        } else {
            CarbonIcon::Laptop
        };

        let pub_approve = dev.key.clone();
        let pub_deny = dev.key.clone();
        let label_approve = dev.label.clone();
        let desk_approve = cx.entity();
        let desk_deny = cx.entity();

        pgc_card()
            .border_1()
            .border_color(theme::role(ColorRole::BorderSubtle01))
            .child(pgc_header(
                icon,
                crate::tr_with("Let “%{name}” in?", &[("name", &dev.label)]),
                crate::tr_with("Paired %{ago} with the link you made.", &[("ago", &ago)]),
                None,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(16.))
                    .py(px(12.))
                    .bg(theme::role(ColorRole::Layer02))
                    .border_1()
                    .border_color(theme::role(ColorRole::BorderSubtle01))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(crate::tr(
                                "Its key. Check that the device shows the same fingerprint.",
                            )),
                    )
                    .child(
                        ui::mono(faces, print.clone())
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::role(ColorRole::TextPrimary)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(pgc_kv_row(crate::tr("May do"), may_do))
                    .child(pgc_kv_row(crate::tr("Until"), until_str)),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(theme::role(ColorRole::TextHelper))
                    .child(crate::tr(
                        "What it may do and for how long were set on the link. To give more, turn it away and pair again with a new link.",
                    )),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new(
                            SharedString::from(format!("turn-away-{print}")),
                            crate::tr("Turn away"),
                        )
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            let k = pub_deny.clone();
                            desk_deny.update(cx, |this, cx| {
                                this.share(Share::Revoke(k));
                                cx.notify();
                            });
                        }),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!("let-in-{print}")),
                            crate::tr("Let in"),
                        )
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            let k = pub_approve.clone();
                            let lbl = label_approve.clone();
                            let sum = summary.clone();
                            desk_approve.update(cx, |this, cx| {
                                this.pages.recently_approved = Some((lbl, k.clone(), sum));
                                this.share(Share::Approve(k));
                                cx.notify();
                            });
                        }),
                    ),
            )
    }

    /// Post-approval green Carbon `Alert` (`Pairing.dc.html` `"iPhone 16 Pro let in"`).
    pub(crate) fn approved_device_alert(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (label, pub_key, standing) = self.pages.recently_approved.clone()?;
        let short_fp = short_fingerprint(&key::fingerprint(&pub_key));
        let desk_revoke = cx.entity();
        let desk_close = cx.entity();
        let body = crate::tr_with(
            "Just now · %{standing} · %{print}.",
            &[("standing", &standing), ("print", &short_fp)],
        );
        Some(
            Alert::new("device-approved-alert")
                .variant(AlertVariant::Success)
                .title(crate::tr_with("%{name} let in", &[("name", &label)]))
                .description(body)
                .action(
                    AlertAction::new(crate::tr("Revoke")).on_click(move |_, cx| {
                        let k = pub_key.clone();
                        desk_revoke.update(cx, |this, cx| {
                            this.pages.recently_approved = None;
                            this.share(Share::Revoke(k));
                            cx.notify();
                        });
                    }),
                )
                .on_close(move |_, cx| {
                    desk_close.update(cx, |this, cx| {
                        this.pages.recently_approved = None;
                        cx.notify();
                    });
                })
                .into_any_element(),
        )
    }

    /// Reusable Carbon pairing-link generator controls + `PairingCard`
    /// (`Pairing.dc.html` Section 2 & `SetAccess.dc.html`).
    pub(crate) fn pairing_offer_controls(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let grant = self.pages.grant;
        let scope_val = match grant.scope {
            Scope::Full => "full",
            Scope::Read => "read",
        };
        let desk_scope = cx.entity();
        let scope_select = Select::new("pair-grant-scope")
            .label(crate::tr("What it may do"))
            .size(FieldSize::Sm)
            .layer(LayerLevel::Two)
            .items(vec![
                ListItem::labeled("full", crate::tr("Everything")),
                ListItem::labeled("read", crate::tr("Read only")),
            ])
            .value(Some(scope_val))
            .on_value_change(move |val, _, cx| {
                if let Some(v) = val {
                    desk_scope.update(cx, |this, cx| {
                        this.pages.grant.scope = if v.as_ref() == "read" {
                            Scope::Read
                        } else {
                            Scope::Full
                        };
                        if this.pages.grant.scope == Scope::Read {
                            this.pages.grant.both = false;
                        }
                        this.new_link();
                        cx.notify();
                    });
                }
            });

        let dur_val = match grant.days {
            None => "none",
            Some(1) => "1",
            Some(7) => "7",
            _ => "30",
        };
        let desk_dur = cx.entity();
        let dur_select = Select::new("pair-grant-days")
            .label(crate::tr("For how long"))
            .size(FieldSize::Sm)
            .layer(LayerLevel::Two)
            .items(vec![
                ListItem::labeled("none", crate::tr("Until revoked")),
                ListItem::labeled("1", crate::tr("A day")),
                ListItem::labeled("7", crate::tr("A week")),
                ListItem::labeled("30", crate::tr("30 days")),
            ])
            .value(Some(dur_val))
            .on_value_change(move |val, _, cx| {
                if let Some(v) = val {
                    let days = match v.as_ref() {
                        "1" => Some(1),
                        "7" => Some(7),
                        "30" => Some(30),
                        _ => None,
                    };
                    desk_dur.update(cx, |this, cx| {
                        this.pages.grant.days = days;
                        this.new_link();
                        cx.notify();
                    });
                }
            });

        let desk_ask = cx.entity();
        let ask_cb = Checkbox::new("pair-grant-ask")
            .label(crate::tr("Ask me before letting it in"))
            .checked(grant.ask)
            .on_checked_change(move |on, _, cx| {
                desk_ask.update(cx, |this, cx| {
                    this.pages.grant.ask = on;
                    this.new_link();
                    cx.notify();
                });
            });

        let desk_both = cx.entity();
        let both_cb = Checkbox::new("pair-grant-both")
            .label(crate::tr("Pair both ways (Everything only)"))
            .checked(grant.both && grant.scope == Scope::Full)
            .disabled(grant.scope != Scope::Full)
            .on_checked_change(move |on, _, cx| {
                desk_both.update(cx, |this, cx| {
                    this.pages.grant.both = on;
                    this.new_link();
                    cx.notify();
                });
            });

        let route_select = self
            .route_select("pair-grant-route", cx)
            .label(crate::tr("Reached by"))
            .size(FieldSize::Sm)
            .layer(LayerLevel::Two);

        let now_secs = store::secs(SystemTime::now());
        let live = self.pages.shared.as_ref().and_then(|s| s.offer);
        let active_offer = self
            .pages
            .offered
            .clone()
            .filter(|(_, made, g)| live == Some((*made, *g)));
        let (link_str, expires_in) = match active_offer {
            Some((l, made, _)) => {
                let elapsed = now_secs.saturating_sub(made);
                (l, 600u64.saturating_sub(elapsed))
            }
            None => (String::new(), 0),
        };

        let texts = PairingCardTexts {
            title: crate::tr("Pair a phone or another Mac"),
            body: crate::tr(
                "Scan with the app on your phone, or open the link on the other device.",
            ),
            link: crate::tr("Pairing link"),
            qr: crate::tr("Pairing QR code"),
            link_expired: crate::tr("Link expired"),
            expired: crate::tr("Expired"),
            expires_in: Rc::new(|t| crate::tr_with("Expires in %{time}", &[("time", &t)])),
            new_link: crate::tr("New link"),
        };
        let desk_renew = cx.entity();
        let card = PairingCard::new("pair-live-card")
            .link(link_str.clone())
            .expires_in(expires_in)
            .texts(texts)
            .on_new_link(move |_, cx| {
                desk_renew.update(cx, |this, cx| {
                    this.new_link();
                    cx.notify();
                });
            });

        // Active read-only guests (`Guest.dc.html` `.gh-guest`).
        let now = SystemTime::now();
        let ro_guests: Vec<Device> = self
            .pages
            .shared
            .as_ref()
            .and_then(|s| s.devices.as_ref().ok())
            .map(|ds| {
                ds.iter()
                    .filter(|d| !d.waiting && d.scope == Scope::Read)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        let desk_withdraw = cx.entity();
        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(div().flex_1().min_w(px(0.)).child(scope_select))
                    .child(div().flex_1().min_w(px(0.)).child(dur_select)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(ask_cb)
                    .child(both_cb),
            )
            .child(route_select)
            .child(div().mx(px(-16.)).child(card));

        if grant.scope == Scope::Read {
            col = col.child(readonly_capabilities_grid());
            for g in ro_guests {
                let fp = short_fingerprint(&key::fingerprint(&g.key));
                let until_note = g.until.map_or_else(
                    || crate::tr("until revoked").to_string(),
                    |u| {
                        format!(
                            "until {}",
                            when::short_at(store::time(u), now, self.settings.twelve)
                        )
                    },
                );
                let pub_key = g.key.clone();
                let desk_rev = cx.entity();
                col = col.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .pt(px(12.))
                        .border_t_1()
                        .border_color(theme::role(ColorRole::BorderSubtle01))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(theme::role(ColorRole::TextSecondary))
                                .child(format!("{} · {fp} · {until_note}", g.label)),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("ro-revoke-{fp}")),
                                crate::tr("Revoke"),
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                let k = pub_key.clone();
                                desk_rev.update(cx, |this, cx| {
                                    this.share(Share::Revoke(k));
                                    cx.notify();
                                });
                            }),
                        ),
                );
            }
            if !link_str.is_empty() {
                col = col.child(
                    div().ml(px(-16.)).mt(px(-8.)).child(
                        Button::new("withdraw-ro-link", crate::tr("Withdraw this link"))
                            .variant(ButtonVariant::DangerGhost)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                desk_withdraw.update(cx, |this, cx| {
                                    this.pages.offered = None;
                                    this.share(Share::Withdraw);
                                    cx.notify();
                                });
                            }),
                    ),
                );
            }
        }

        let _ = faces;
        col.child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(theme::role(ColorRole::TextHelper))
                .child(crate::tr(
                    "Each choice above makes a new link at once. Copy copies it; New link makes another and withdraws this one.",
                )),
        )
    }

    fn render_offer_pairing_modal(
        &self,
        faces: &Faces,
        close_btn: AnyElement,
        cx: &mut Context<Self>,
    ) -> Div {
        let waiting: Vec<Device> = self
            .pages
            .shared
            .as_ref()
            .and_then(|s| s.devices.as_ref().ok())
            .map(|ds| ds.iter().filter(|d| d.waiting).cloned().collect())
            .unwrap_or_default();

        let mut root = pgc_card()
            .child(pgc_header(
                CarbonIcon::QrCode,
                crate::tr("Pair a device with this Mac"),
                crate::tr(
                    "Open the link on the other Mac, scan the QR with the phone, or paste it there. It works once, for 10 minutes.",
                ),
                Some(close_btn),
            ));

        if let Some(alert) = self.approved_device_alert(cx) {
            root = root.child(alert);
        }
        for dev in &waiting {
            root = root.child(self.waiting_device_card(dev, faces, cx));
        }

        root.child(self.pairing_offer_controls(faces, cx))
    }

    fn render_control_pairing_modal(
        &self,
        faces: &Faces,
        confirm: Option<PendingPair>,
        close_btn: AnyElement,
        cx: &mut Context<Self>,
    ) -> Div {
        let desk_parse = cx.entity();
        let left_col = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(12.))
            .child(
                div()
                    .type_style(TypeStyle::HeadingCompact01)
                    .text_color(theme::role(ColorRole::TextPrimary))
                    .child(crate::tr("Pair with another Mac")),
            )
            .child(
                Input::new("modal-pair-link", &self.pages.pair_link)
                    .label(crate::tr("Pairing link"))
                    .placeholder("u0://pair?host=…")
                    .helper_text(crate::tr(
                        "Made on the other Mac: Settings › Devices › Pair a device with this Mac.",
                    ))
                    .size(FieldSize::Sm)
                    .layer(LayerLevel::Two),
            )
            .child(
                div().flex().child(
                    Button::new("modal-pair-submit", crate::tr("Pair"))
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Sm)
                        .loading(self.pages.pairing)
                        .on_click(move |_, window, cx| {
                            desk_parse.update(cx, |this, cx| this.pair(window, cx));
                        }),
                ),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(theme::role(ColorRole::TextHelper))
                    .child(crate::tr(
                        "A page in a browser can't pair or reach u0; a browser only hands the u0:// link to the app.",
                    )),
            );

        let right_col = confirm.map(|pending| {
            let can_pair_back = self.machines[0].local().is_some();
            let host = pending.way.host.clone();
            let desk_toggle = cx.entity();
            let desk_cancel = cx.entity();
            let desk_ok = cx.entity();
            let pending_for_toggle = pending.clone();
            let pending_for_ok = pending.clone();

            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(12.))
                .pl(px(32.))
                .border_l_1()
                .border_color(theme::role(ColorRole::BorderSubtle01))
                .child(
                    div()
                        .type_style(TypeStyle::HeadingCompact01)
                        .text_color(theme::role(ColorRole::TextPrimary))
                        .child(crate::tr_with("Pair with “%{host}”?", &[("host", &host)])),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(pgc_kv_row(
                            crate::tr("Host"),
                            ui::mono(faces, host.clone()).text_size(px(12.)),
                        ))
                        .child(pgc_kv_row(
                            crate::tr("Key"),
                            crate::tr_with(
                                "%{print}, made on this Mac for %{host} alone; its private half stays in this Mac's Keychain",
                                &[
                                    ("print", &short_fingerprint(&pending.print)),
                                    ("host", &host),
                                ],
                            ),
                        )),
                )
                .when(can_pair_back, |d| {
                    d.child(
                        Checkbox::new("modal-pair-both-cb")
                            .label(crate::tr("Also let it control this Mac"))
                            .checked(pending.both)
                            .on_checked_change(move |on, _, cx| {
                                let mut next = pending_for_toggle.clone();
                                next.both = on;
                                desk_toggle.update(cx, |this, cx| {
                                    this.pages.pairing_modal =
                                        Some(PairingModal::Control { confirm: Some(next) });
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(theme::role(ColorRole::TextHelper))
                            .child(crate::tr_with(
                                "Ticked, this Mac starts listening and %{host} pairs back with a key of its own, if its link was made with Pair both ways. Where it can't reach this Mac, the pairing stays one way and says why.",
                                &[("host", &host)],
                            )),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .ml(px(-16.))
                        .child(
                            Button::new("modal-pair-cancel", crate::tr("Cancel"))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, window, cx| {
                                    desk_cancel.update(cx, |this, cx| {
                                        this.close_pairing_dialog(window, cx);
                                    });
                                }),
                        )
                        .child(
                            Button::new("modal-pair-confirm", crate::tr("Pair"))
                                .variant(ButtonVariant::Primary)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, window, cx| {
                                    let p = pending_for_ok.clone();
                                    desk_ok.update(cx, |this, cx| {
                                        this.pair_with(
                                            &p.way,
                                            &p.code,
                                            &p.kept,
                                            p.both && can_pair_back,
                                            cx,
                                        );
                                        this.close_pairing_dialog(window, cx);
                                    });
                                }),
                        ),
                )
        });

        pgc_card()
            .child(pgc_header(
                CarbonIcon::Laptop,
                crate::tr("Control another Mac from this one"),
                crate::tr(
                    "Settings › Devices › Control other Macs, or the u0 menu's Pair a Device…. Opening a u0://pair link from a QR scan, Mail or open does the same.",
                ),
                Some(close_btn),
            ))
            .child(
                div()
                    .flex()
                    .gap(px(32.))
                    .pl(px(44.))
                    .child(left_col)
                    .children(right_col),
            )
    }

    /// Build a [`PendingPair`] with a freshly generated Ed25519 key and open
    /// the Carbon `"Pair with “<Host>”?"` modal (`Pairing.dc.html` Section 7).
    pub(crate) fn begin_pair_confirm(
        &mut self,
        way: Way,
        code: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = Key::generate();
        let kept = key.kept().to_owned();
        let print = key::fingerprint(&key.public());
        let pending = PendingPair {
            way,
            code,
            kept,
            print,
            both: false,
        };
        self.open_pair_another_dialog(Some(pending), window, cx);
    }
}

impl Desk {
    /// "Reached by": the way a pairing link names, as the server says
    /// each reaches it (`This Mac (127.0.0.1:4317)`), or the best there is.
    pub(crate) fn route_select(&self, id: &'static str, cx: &mut Context<Self>) -> Select {
        let mut routes: Vec<(Route, String)> = (self.pages.shared.as_ref())
            .and_then(|s| s.routes.as_ref().ok())
            .cloned()
            .unwrap_or_default();
        routes.dedup_by_key(|(r, _)| *r);
        let mut items = vec![ListItem::labeled(
            "best",
            crate::tr("Best: tunnel, then Tailscale, then this network"),
        )];
        for (r, host) in &routes {
            items.push(ListItem::labeled(
                r.name(),
                format!("{} ({host})", crate::tr(r.name())),
            ));
        }
        let value: SharedString =
            (self.pages.route).map_or_else(|| "best".into(), |r| r.name().into());
        let desk = cx.entity();
        Select::new(id)
            .items(items)
            .value(Some(value))
            .on_value_change(move |val, _, cx| {
                let picked = val.and_then(|v| {
                    routes
                        .iter()
                        .map(|(r, _)| *r)
                        .find(|r| r.name() == v.as_ref())
                });
                desk.update(cx, |this, cx| {
                    this.pages.route = picked;
                    this.new_link();
                    cx.notify();
                });
            })
    }
}
