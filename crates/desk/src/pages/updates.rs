//! Updates, on the About page: where an update of u0 stands and what
//! to do about it, the channel, and whether u0 looks by itself.

use gpui::component::{Disableable as _, Sizable as _, setting::SettingGroup, switch::Switch};
use gpui::{AnyElement, Context, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::{
    desk::{Desk, updates::Updating},
    theme,
    ui::{self, Weight},
    update::{self, Channel},
};

impl Desk {
    /// This build's version and commit, put on the clipboard: what to say
    /// in a bug.
    pub(crate) fn copy_version(cx: &mut gpui::App) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(update::this_build()));
    }

    /// The updates group: where an update stands and what to do, the
    /// channel, and whether u0 looks by itself.
    pub(super) fn updates_group(&self, w: &WeakEntity<Desk>) -> SettingGroup {
        let build = update::this_build();
        let said = match &self.updates.now {
            Updating::Idle => crate::tr_with("This is %{build}.", &[("build", &build)]),
            Updating::Checking => crate::tr("Looking on GitHub…"),
            Updating::Newest => {
                crate::tr_with("This is %{build}. It is the newest.", &[("build", &build)])
            }
            Updating::Found(found) => crate::tr_with(
                "u0 %{version} is out. This is %{build}.",
                &[("version", &found.name), ("build", &build)],
            ),
            Updating::Installing(what) => format!("{what}…").into(),
            Updating::Ready => crate::tr("The update is in place: u0 runs it once opened again."),
            Updating::Failed(why) => crate::tr_with("Couldn't look: %{why}", &[("why", why)]),
        };
        let mut items = vec![item(w, crate::tr("u0's version"), said, |d, _, cx| {
            d.update_button(cx)
        })];
        // What the release says of itself, under it.
        if let Updating::Found(found) = &self.updates.now
            && !found.notes.trim().is_empty()
        {
            let notes = ui::fit(found.notes.trim(), 600);
            items.push(whole(w, &[], move |_, faces, _| {
                ui::text(faces, notes.clone())
                    .text_size(px(crate::style::CAPTION))
                    .line_height(px(18.))
                    .text_color(theme::text2())
                    .into_any_element()
            }));
        }
        items.extend([
            item(
                w,
                crate::tr("Updates from"),
                crate::tr("Stable: each release. Nightly: main, rebuilt on each change."),
                |d, _, cx| {
                    d.pick(
                        "channel",
                        [
                            (crate::tr("Stable"), Channel::Stable),
                            (crate::tr("Nightly"), Channel::Nightly),
                        ],
                        d.settings.channel,
                        |s, v| s.channel = v,
                        cx,
                    )
                    .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("Look for updates"),
                crate::tr("At launch and every four hours. Installs only when you say."),
                |d, _, cx| {
                    let switch = Switch::new("check-updates")
                        .accessibility_label(crate::tr("Look for updates"))
                        .small()
                        .checked(d.settings.check_updates)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.check_updates = !this.settings.check_updates;
                            this.save_settings();
                            cx.notify();
                        }));
                    match crate::policy::locked("check_updates") {
                        None => switch.into_any_element(),
                        Some(by) => super::prefs::managed(switch.disabled(true), by),
                    }
                },
            ),
        ]);
        group(crate::tr("Updates"), items)
    }

    /// What can be done about an update now: look, install, restart.
    fn update_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let button = match &self.updates.now {
            Updating::Idle | Updating::Newest => Some(("Check now", Weight::Secondary)),
            Updating::Found(_) => Some(("Install", Weight::Primary)),
            Updating::Ready => Some(("Restart", Weight::Primary)),
            Updating::Failed(_) => Some(("Try again", Weight::Secondary)),
            Updating::Checking | Updating::Installing(_) => None,
        };
        div()
            .flex()
            .gap_1()
            .child(
                ui::icon_button("copy-version", "copy", crate::tr("Copy the version"))
                    .small()
                    .on_click(|_, _, cx| Self::copy_version(cx)),
            )
            .children(button.map(|(label, weight)| {
                ui::button("update", crate::tr(label), weight).on_click(cx.listener(
                    |this, _, window, cx| match this.updates.now {
                        Updating::Found(_) => this.install_update(cx),
                        Updating::Ready => this.reopen(window, cx),
                        _ => {
                            this.check_update(true);
                            cx.notify();
                        }
                    },
                ))
            }))
            .into_any_element()
    }
}
