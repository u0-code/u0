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
//! type into: what, one check per line, and how long it may take; its
//! buttons stand where send does.

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

    /// Add a check after the last, and type into it.
    fn add_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let check = Form::check(window, cx);
        check.focus_handle(cx).focus(window);
        self.task.checks.push(check);
        cx.notify();
    }

    /// The form, where the line you type into is while the mode is Task:
    /// a line saying what a task is, then what, the checks and the time,
    /// each under a label in one column, so that the fields line up.
    pub(crate) fn task_form(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let label = |text: &'static str| {
            ui::mono(faces, text)
                .flex()
                .flex_none()
                .items_center()
                .w(px(LABEL))
                .h(px(FIELD))
                .text_size(px(10.5))
                .text_color(theme::faint())
        };
        let row = |name: &'static str, field: Div| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .py(px(2.))
                .child(label(name))
                .child(field.flex_1().min_w(px(0.)))
        };
        let mut checks = div().flex().flex_col();
        for (i, check) in self.task.checks.iter().enumerate() {
            checks = checks.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .child(
                        ui::mono(faces, (i + 1).to_string())
                            .flex()
                            .flex_none()
                            .items_center()
                            .w(px(12.))
                            .h(px(FIELD))
                            .text_size(px(11.))
                            .text_color(theme::faint()),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(check.clone())),
            );
        }
        let add = div()
            .id("task-add-check")
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(24.))
            .ml(px(-4.))
            .px(px(4.))
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(theme::hover()))
            .child(ui::icon("plus", 11.).text_color(theme::faint()))
            .child(
                ui::text(faces, "Add a check")
                    .text_size(px(12.))
                    .text_color(theme::faint()),
            )
            .on_click(cx.listener(|this, _, window, cx| this.add_check(window, cx)));
        let name = self.speaking_to();
        let rule = || div().h(px(1.)).bg(theme::border());
        div()
            .flex()
            .flex_col()
            .px(px(4.))
            .pt(px(4.))
            .pb(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pb(px(6.))
                    .child(
                        ui::text(faces, "New task")
                            .flex_none()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme::text()),
                    )
                    .child(ui::one_line(
                        ui::text(
                            faces,
                            format!(
                                "{name} takes it on as work, and ends it when every check holds."
                            ),
                        )
                        .text_size(px(12.))
                        .text_color(theme::muted()),
                    )),
            )
            .child(rule())
            .child(row("WHAT", div().child(self.task.what.clone())))
            .child(rule())
            .child(row("DONE WHEN", checks.child(div().pl(px(18.)).child(add))))
            .child(rule())
            .child(row("WITHIN", div().child(self.task.within.clone())))
            .child(rule())
    }

    /// Cancel, and Give task, where send is for the line: Give task can
    /// be pressed once the task says what is to be done.
    pub(crate) fn task_buttons(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let ready = !self.task.what.read(cx).is_empty();
        let give = if ready {
            ui::Weight::Primary
        } else {
            ui::Weight::Off
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .child(
                ui::button(faces, "task-cancel", "Cancel", ui::Weight::Quiet)
                    .tooltip(ui::tip("Back to the line · Esc"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_mode(Mode::Say, window, cx);
                    })),
            )
            .child(
                ui::button(faces, "task-give", "Give task", give)
                    .on_click(cx.listener(|this, _, _, cx| this.give_task(cx))),
            )
    }
}

/// The width of the form's labels, and the height of one of its fields.
const LABEL: f32 = 76.;
const FIELD: f32 = 28.;

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
