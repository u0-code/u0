//! Updates, on the About page: where an update of Ghost stands and what
//! to do about it, the channel, and whether Ghost looks by itself.

use gpui::component::{Sizable as _, setting::SettingGroup, switch::Switch};
use gpui::{AnyElement, Context, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item, whole};
use crate::{
    desk::{Desk, updates::Updating},
    theme,
    ui::{self, Weight},
    update::{self, Channel},
};

impl Desk {
    /// The updates group: where an update stands and what to do, the
    /// channel, and whether Ghost looks by itself.
    pub(super) fn updates_group(&self, w: &WeakEntity<Desk>) -> SettingGroup {
        let this_build = match update::COMMIT.filter(|c| !c.is_empty()) {
            Some(commit) => format!(
                "This is {} ({}).",
                update::VERSION,
                &commit[..commit.len().min(7)]
            ),
            None => format!("This is {}.", update::VERSION),
        };
        let said = match &self.updates.now {
            Updating::Idle => this_build,
            Updating::Checking => "Looking on GitHub…".to_owned(),
            Updating::Newest => format!("{this_build} It is the newest."),
            Updating::Found(found) => format!("Ghost {} is out. {this_build}", found.name),
            Updating::Installing(what) => format!("{what}…"),
            Updating::Ready => {
                "The update is in place: Ghost runs it once opened again.".to_owned()
            }
            Updating::Failed(why) => format!("Couldn't look: {why}"),
        };
        let mut items = vec![item(w, "Ghost's version", said, |d, _, cx| {
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
                "Updates from",
                "Stable is each release, as the ghost cask installs it; Nightly is what main is now, rebuilt with each change, as ghost@nightly installs it.",
                |d, _, cx| {
                    Self::pick(
                        "channel",
                        [("Stable", Channel::Stable), ("Nightly", Channel::Nightly)],
                        d.settings.channel,
                        |s, v| s.channel = v,
                        cx,
                    )
                    .into_any_element()
                },
            ),
            item(
                w,
                "Look for updates",
                "As Ghost starts and every four hours; an update is installed only when you say.",
                |d, _, cx| {
                    Switch::new("check-updates")
                        .small()
                        .checked(d.settings.check_updates)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.check_updates = !this.settings.check_updates;
                            this.save_settings();
                            cx.notify();
                        }))
                        .into_any_element()
                },
            ),
        ]);
        group("Updates", items)
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
            .children(button.map(|(label, weight)| {
                ui::button("update", label, weight).on_click(cx.listener(|this, _, window, cx| {
                    match this.updates.now {
                        Updating::Found(_) => this.install_update(cx),
                        Updating::Ready => this.reopen(window, cx),
                        _ => {
                            this.check_update(true);
                            cx.notify();
                        }
                    }
                }))
            }))
            .into_any_element()
    }
}
