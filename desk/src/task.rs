//! Giving her a task: a piece of work with its standard said up front.
//!
//! The protocol has no method for giving work, and needs none: she takes
//! work on herself when she is asked for something that needs it. So a
//! task is said to her as a directive from whoever runs her, in words that
//! say plainly that this is a piece of work to take on, what it is, when it
//! is done, and in what time. Nobody signs it off: she ends it herself
//! when every check holds, and says so.
//!
//! The form opens from the composer's Task mode, in place of the line you
//! type into: what, one check per line, and how long it may take.

use std::fmt::Write as _;

use gpui::{Context, Div, Entity, Focusable as _, Window, div, prelude::*, px};

use crate::{
    desk::{Desk, Mode},
    fonts::Faces,
    input::{Cancelled, Submitted, TextInput},
    theme, ui,
};

/// The words a task is said in, around what was typed. They are yours,
/// said to her as a directive, so they speak as you; and they say why
/// each part is there, since she reads them as words and not as fields.
fn words(what: &str, checks: &[String], within: Option<&str>) -> String {
    let mut text = format!(
        "Here is a piece of work for you to take on. Take it on as work of its own, \
         not as something to answer in passing: it needs carrying through to the end, \
         and I would rather you took the time it needs than gave me a quick reply.\n\n\
         What it is:\n{what}\n\n"
    );
    if checks.is_empty() {
        text.push_str(
            "It is done when you judge it done. When you end it, say how you know, \
             so that I can see it without doing it again myself.\n\n",
        );
    } else {
        text.push_str(
            "It is done when each of these holds. Make each one the check of a step of \
             your plan, and see for yourself that it holds before you call the work \
             finished, so that done means the same to both of us:\n",
        );
        for check in checks {
            let _ = writeln!(text, "- {check}");
        }
        text.push('\n');
    }
    if let Some(within) = within {
        let _ = write!(
            text,
            "Time allowed: {within}. If it cannot be done in that time, tell me as soon \
             as you know, rather than at the end, so that I can change the plan.\n\n"
        );
    }
    text.push_str(
        "Nobody will sign it off: when it is done, end it yourself, and tell me what you \
         did, how each check came out, and where anything you made is.",
    );
    text
}

/// A task as typed into the form, before it is said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub what: String,
    /// One check a line; blank lines are not checks.
    pub checks: Vec<String>,
    /// Free text: "2h", "tonight". Blank is no limit.
    pub within: String,
}

impl Draft {
    /// What is said to her for it; nothing while it says nothing to do.
    pub fn directive(&self) -> Option<String> {
        let what = self.what.trim();
        if what.is_empty() {
            return None;
        }
        let checks: Vec<String> = self
            .checks
            .iter()
            .map(|c| c.trim().to_owned())
            .filter(|c| !c.is_empty())
            .collect();
        let within = Some(self.within.trim()).filter(|w| !w.is_empty());
        Some(words(what, &checks, within))
    }
}

/// The form's fields.
pub(crate) struct Form {
    pub what: Entity<TextInput>,
    /// One field a check; Return in the last one adds another.
    pub checks: Vec<Entity<TextInput>>,
    pub within: Entity<TextInput>,
}

impl Form {
    pub fn new(window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let what = cx.new(|cx| TextInput::new(cx, "What needs doing").field().wrapping(4));
        cx.subscribe_in(&what, window, |this, _, _: &Submitted, window, cx| {
            if let Some(first) = this.task.checks.first() {
                first.focus_handle(cx).focus(window);
            }
        })
        .detach();
        let within = cx.new(|cx| TextInput::new(cx, "e.g. 2h, tonight, by Friday").field());
        cx.subscribe(&within, |this, _, _: &Submitted, cx| this.give_task(cx))
            .detach();
        for field in [&what, &within] {
            cx.subscribe_in(field, window, |this, _, _: &Cancelled, window, cx| {
                this.set_mode(Mode::Say, window, cx);
            })
            .detach();
        }
        let mut form = Self {
            what,
            checks: Vec::new(),
            within,
        };
        form.checks.push(Self::check(window, cx));
        form
    }

    /// A field for one check.
    fn check(window: &mut Window, cx: &mut Context<Desk>) -> Entity<TextInput> {
        let field = cx.new(|cx| TextInput::new(cx, "a check, in plain words").field());
        cx.subscribe_in(&field, window, |this, field, _: &Submitted, window, cx| {
            this.next_check(field, window, cx);
        })
        .detach();
        cx.subscribe_in(&field, window, |this, _, _: &Cancelled, window, cx| {
            this.set_mode(Mode::Say, window, cx);
        })
        .detach();
        field
    }

    /// What is typed in it now.
    pub fn draft(&self, cx: &gpui::App) -> Draft {
        Draft {
            what: self.what.read(cx).text(),
            checks: self.checks.iter().map(|c| c.read(cx).text()).collect(),
            within: self.within.read(cx).text(),
        }
    }
}

impl Desk {
    /// Return in a check: the next one, or a new one after the last.
    fn next_check(
        &mut self,
        field: &Entity<TextInput>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = self.task.checks.iter().position(|c| c == field);
        let next = match at {
            Some(i) if i + 1 < self.task.checks.len() => self.task.checks[i + 1].clone(),
            _ if field.read(cx).is_empty() => self.task.within.clone(),
            _ => {
                let check = Form::check(window, cx);
                self.task.checks.push(check.clone());
                check
            }
        };
        next.focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Say the task to her as a directive, and empty the form.
    pub(crate) fn give_task(&mut self, cx: &mut Context<Self>) {
        let draft = self.task.draft(cx);
        let Some(text) = draft.directive() else {
            return;
        };
        // A session is named after what the task is, not after the words
        // around it.
        if self.open.is_none()
            && let Some(session) = self.settings.sessions.get_mut(self.current)
            && session.title.is_empty()
        {
            session.title = draft.what.chars().take(48).collect();
            self.settings.save();
        }
        self.speak(text, ghost_link::protocol::SayMode::Directive, cx);
        for field in [&self.task.what, &self.task.within] {
            field.update(cx, |f, cx| f.set_text("", cx));
        }
        self.task.checks.truncate(1);
        if let Some(first) = self.task.checks.first() {
            first.update(cx, |f, cx| f.set_text("", cx));
        }
        self.mode = Mode::Say;
        cx.notify();
    }

    /// The form, where the line you type into is while the mode is Task.
    pub(crate) fn task_form(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let label = |text: &'static str| {
            ui::mono(faces, text)
                .text_size(px(10.5))
                .text_color(theme::faint())
                .w(px(76.))
                .flex_none()
                .pt(px(6.))
        };
        let row = |name: &'static str, field: Div| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(label(name))
                .child(field.flex_1().min_w(px(0.)))
        };
        let mut checks = div().flex().flex_col();
        for check in &self.task.checks {
            checks = checks.child(check.clone());
        }
        let name = self
            .open
            .as_ref()
            .map_or_else(|| self.solver.name(), |o| o.her.name());
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(6.))
            .pt(px(4.))
            .child(
                ui::text(
                    faces,
                    format!(
                        "A task for {name}: taken on as work, and ended by {name} when every check holds."
                    ),
                )
                .text_size(px(12.))
                .text_color(theme::muted())
                .pb(px(4.)),
            )
            .child(row("WHAT", div().child(self.task.what.clone())))
            .child(row("DONE WHEN", checks))
            .child(row("WITHIN", div().child(self.task.within.clone())))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.))
                    .pt(px(4.))
                    .child(
                        div()
                            .id("task-cancel")
                            .flex()
                            .items_center()
                            .h(px(26.))
                            .px(px(10.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child(
                                ui::text(faces, "Cancel")
                                    .text_size(px(12.5))
                                    .text_color(theme::muted()),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_mode(Mode::Say, window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("task-give")
                            .flex()
                            .items_center()
                            .h(px(26.))
                            .px(px(12.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .bg(theme::accent())
                            .child(
                                ui::text(faces, "Give task")
                                    .text_size(px(12.5))
                                    .text_color(theme::bg()),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.give_task(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(what: &str, checks: &[&str], within: &str) -> Draft {
        Draft {
            what: what.to_owned(),
            checks: checks.iter().map(|c| (*c).to_owned()).collect(),
            within: within.to_owned(),
        }
    }

    #[test]
    fn a_task_says_it_is_work_what_it_is_each_check_and_the_time_allowed() {
        let text = draft(
            "  Collect this week's memes into one folder ",
            &["at least 20 images", "", "  each named by its source "],
            " 2h ",
        )
        .directive()
        .unwrap();
        assert!(text.starts_with("Here is a piece of work for you to take on."));
        assert!(text.contains("What it is:\nCollect this week's memes into one folder\n"));
        assert!(text.contains("- at least 20 images\n- each named by its source\n"));
        assert_eq!(text.matches("\n- ").count(), 2, "a blank line is no check");
        assert!(text.contains("Time allowed: 2h."));
        assert!(text.ends_with("where anything you made is."));
    }

    #[test]
    fn a_task_without_checks_or_time_leaves_her_to_judge_and_sets_no_limit() {
        let text = draft("tidy ~/Downloads", &["  "], "").directive().unwrap();
        assert!(text.contains("It is done when you judge it done."));
        assert!(!text.contains("Time allowed"));
        assert!(!text.contains("\n- "));
    }

    #[test]
    fn a_task_that_says_nothing_to_do_is_not_said() {
        assert_eq!(draft("   ", &["a check"], "1h").directive(), None);
    }
}
