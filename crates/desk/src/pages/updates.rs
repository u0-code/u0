//! Updates, on the About page: where an update of Ghost stands and what
//! to do about it, the channel, and whether Ghost looks by itself.

use gpui::component::{Sizable as _, switch::Switch};
use gpui::{Context, Div, div, prelude::*, px};

use super::prefs::{group, row};
use crate::{
    desk::{Desk, updates::Updating},
    fonts::Faces,
    theme,
    ui::{self, Weight},
    update::{self, Channel},
};

impl Desk {
    /// The updates group; none when what is looked for, `q`, is in none
    /// of it.
    pub(super) fn updates_group(
        &self,
        faces: &Faces,
        q: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let this_build = match update::COMMIT.filter(|c| !c.is_empty()) {
            Some(commit) => format!(
                "This is {} ({}).",
                update::VERSION,
                &commit[..commit.len().min(7)]
            ),
            None => format!("This is {}.", update::VERSION),
        };
        let (said, button) = match &self.updates.now {
            Updating::Idle => (this_build, Some(("Check now", Weight::Secondary))),
            Updating::Checking => ("Looking on GitHub…".to_owned(), None),
            Updating::Newest => (
                format!("{this_build} It is the newest."),
                Some(("Check now", Weight::Secondary)),
            ),
            Updating::Found(found) => (
                format!("Ghost {} is out. {this_build}", found.name),
                Some(("Install", Weight::Primary)),
            ),
            Updating::Installing(what) => (format!("{what}…"), None),
            Updating::Ready => (
                "The update is in place: Ghost runs it once opened again.".to_owned(),
                Some(("Restart", Weight::Primary)),
            ),
            Updating::Failed(why) => (
                format!("Couldn't look: {why}"),
                Some(("Try again", Weight::Secondary)),
            ),
        };
        let act = div().children(button.map(|(label, weight)| {
            ui::button("update", label, weight).on_click(cx.listener(|this, _, window, cx| {
                match this.updates.now {
                    Updating::Found(_) => this.install_update(cx),
                    Updating::Ready => this.restart_updated(window, cx),
                    _ => {
                        this.check_update(true);
                        cx.notify();
                    }
                }
            }))
        }));
        let notes = match &self.updates.now {
            Updating::Found(found) if !found.notes.trim().is_empty() => Some(
                div().px(px(16.)).pb(px(12.)).child(
                    ui::text(faces, ui::fit(found.notes.trim(), 600))
                        .text_size(px(12.5))
                        .line_height(px(18.))
                        .text_color(theme::text2()),
                ),
            ),
            _ => None,
        };
        let status = row(faces, q, "Ghost's version", &said, act)
            .map(|r| div().flex().flex_col().child(r).children(notes));
        group(
            faces,
            "Updates",
            vec![
                status,
                row(
                    faces,
                    q,
                    "Updates from",
                    "Stable is each release, as the ghost cask installs it; Nightly is what main is now, rebuilt with each change, as ghost@nightly installs it.",
                    Self::pick(
                        "channel",
                        [("Stable", Channel::Stable), ("Nightly", Channel::Nightly)],
                        self.settings.channel,
                        |s, v| s.channel = v,
                        cx,
                    ),
                ),
                row(
                    faces,
                    q,
                    "Look for updates",
                    "As Ghost starts and every four hours; an update is installed only when you say.",
                    Switch::new("check-updates")
                        .small()
                        .checked(self.settings.check_updates)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.check_updates = !this.settings.check_updates;
                            this.save_settings();
                            cx.notify();
                        })),
                ),
            ],
        )
    }
}
