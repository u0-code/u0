//! Scheduled tasks on the desk: the list the server keeps and runs
//! (`tasks`), the one being written or changed, and each task's turn in
//! its session as the server says it.

use std::time::SystemTime;

use gpui::component::input::{InputState, TextareaState};
use gpui::component::time_field::{HourCycle, TimeFieldState};
use gpui::{AppContext as _, Context, Entity, Window};

use super::Desk;
use crate::{
    proto::Call,
    session::{Session, State},
    store::{self, Meta},
    tasks::{Schedule, Task},
    when,
};

/// The scheduled tasks as the desk has them.
pub struct Tasks {
    /// As the server last said them, or why they do not read.
    pub list: Result<Vec<Task>, String>,
    /// The one being written or changed.
    pub draft: Option<Draft>,
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            list: Ok(Vec::new()),
            draft: None,
        }
    }
}

/// A task being written: what is chosen, and the fields as typed.
pub struct Draft {
    pub task: Task,
    pub title: Entity<InputState>,
    pub prompt: Entity<TextareaState>,
    /// How often, in minutes or as a while ("2h"), for one run every so
    /// often.
    pub every: Entity<InputState>,
    /// For one run, when ("tomorrow", "5pm", "in 3h").
    pub at: Entity<InputState>,
    /// The time of day of a daily one: gpui-kit's time field, on the
    /// clock your settings show.
    pub time: Entity<TimeFieldState>,
    /// Why it was not kept.
    pub wrong: Option<String>,
}

impl Desk {
    /// Write a new task: for the project and agent on screen, or the first.
    pub(crate) fn new_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let on = self
            .session()
            .map(|s| (s.meta.agent.clone(), s.meta.project.clone()));
        let agent = on
            .as_ref()
            .map(|(a, _)| a.clone())
            .or_else(|| self.slots.first().map(|s| s.entry.id.clone()))
            .unwrap_or_default();
        let project = on
            .map(|(_, p)| p)
            .or_else(|| self.settings.folders.first().cloned())
            .unwrap_or_default();
        let task = Task {
            id: 0,
            title: String::new(),
            prompt: String::new(),
            agent,
            model: None,
            mode: None,
            project,
            schedule: Schedule::Daily {
                minute: 9 * 60,
                days: vec![1, 2, 3, 4, 5],
            },
            paused: false,
            same: false,
            worktree: false,
            session: None,
            next: None,
            last: None,
            runs: 0,
        };
        self.draft_task(task, window, cx);
    }

    /// Change the task `id`.
    pub(crate) fn edit_task(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let found = self
            .tasks
            .list
            .as_ref()
            .ok()
            .and_then(|l| l.iter().find(|t| t.id == id));
        if let Some(task) = found.cloned() {
            self.draft_task(task, window, cx);
        }
    }

    fn draft_task(&mut self, task: Task, window: &mut Window, cx: &mut Context<Self>) {
        let field = |text: String,
                     placeholder: gpui::SharedString,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(text)
            })
        };
        let (every, at, minute) = match &task.schedule {
            Schedule::Every(m) => (m.to_string(), "09:00".to_owned(), 9 * 60),
            Schedule::Daily { minute, .. } => ("60".to_owned(), "09:00".to_owned(), *minute),
            Schedule::Once(at) => (
                "60".to_owned(),
                when::said_at(store::time(*at), SystemTime::now(), false),
                9 * 60,
            ),
            Schedule::Hook(_) => ("60".to_owned(), "09:00".to_owned(), 9 * 60),
        };
        let cycle = if self.settings.twelve {
            HourCycle::H12
        } else {
            HourCycle::H23
        };
        let time = cx.new(|cx| {
            let mut state = TimeFieldState::new(window, cx).hour_cycle(cycle);
            let of_day = state
                .time()
                .with()
                .hour(i8::try_from(minute / 60).unwrap_or(9))
                .minute(i8::try_from(minute % 60).unwrap_or(0))
                .second(0)
                .build();
            if let Ok(t) = of_day {
                state.set_time(t, window, cx);
            }
            state
        });
        let draft = Draft {
            title: field(task.title.clone(), crate::tr("Morning triage"), window, cx),
            prompt: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(3, 10)
                    .placeholder(crate::tr("What to say to the agent each time"))
                    .default_value(task.prompt.clone())
            }),
            every: field(every, crate::tr("Minutes, or 2h"), window, cx),
            at: field(at, "tomorrow, 5pm, in 3h".into(), window, cx),
            time,
            task,
            wrong: None,
        };
        self.tasks.draft = Some(draft);
        cx.notify();
    }

    /// Keep the task being written, as its fields say, if they say enough.
    pub(crate) fn save_task(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = &mut self.tasks.draft else {
            return;
        };
        let read = |input: &Entity<InputState>, cx: &Context<Self>| {
            input.read(cx).value().trim().to_owned()
        };
        let mut task = draft.task.clone();
        task.title = read(&draft.title, cx);
        draft
            .prompt
            .read(cx)
            .value()
            .trim()
            .clone_into(&mut task.prompt);
        let (every, at) = (read(&draft.every, cx), read(&draft.at, cx));
        let now = SystemTime::now();
        let wrong = match &mut task.schedule {
            _ if task.prompt.is_empty() => Some("Say what to ask the agent."),
            _ if task.agent.is_empty() || task.project.as_os_str().is_empty() => {
                Some("Choose an agent and a project.")
            }
            Schedule::Every(minutes) => match every.parse::<u32>().ok().or_else(|| {
                when::span(&every).map(|d| u32::try_from(d.as_secs() / 60).unwrap_or(u32::MAX))
            }) {
                Some(m) if m >= crate::tasks::LEAST_MINUTES => {
                    *minutes = m;
                    None
                }
                _ => Some("Say how often: minutes, or a while such as 2h."),
            },
            Schedule::Daily { minute, .. } => {
                let t = draft.time.read(cx).time();
                *minute = u16::try_from(t.hour()).unwrap_or(0) * 60
                    + u16::try_from(t.minute()).unwrap_or(0);
                None
            }
            Schedule::Once(secs) => match when::ahead(&at, now) {
                Some(t) => {
                    *secs = store::secs(t);
                    None
                }
                None => Some("Say when, such as tomorrow, 5pm or in 3h."),
            },
            Schedule::Hook(_) => None,
        };
        if let Some(why) = wrong {
            draft.wrong = Some(crate::tr(why).into());
            cx.notify();
            return;
        }
        if task.title.is_empty() {
            task.title = crate::ui::fit(task.prompt.lines().next().unwrap_or_default(), 40);
        }
        self.call(self.pages.on, Call::SetTask(task));
        self.tasks.draft = None;
        cx.notify();
    }

    /// Pause the task `id`, or take it up again.
    pub(crate) fn pause_task(&mut self, id: u64) {
        let found = self
            .tasks
            .list
            .as_ref()
            .ok()
            .and_then(|l| l.iter().find(|t| t.id == id));
        if let Some(mut task) = found.cloned() {
            task.paused = !task.paused;
            self.call(self.pages.on, Call::SetTask(task));
        }
    }

    /// Run the task `id` now, whatever its schedule says.
    pub(crate) fn run_task(&self, id: u64) {
        self.call(self.pages.on, Call::RunTask(id));
    }

    pub(crate) fn drop_task(&self, id: u64) {
        self.call(self.pages.on, Call::DropTask(id));
    }

    /// A task's turn in the session `meta` begins (with what was said in it
    /// so far, where `record`) or ends: the session is put on the desk if
    /// it is not, and works, or is idle and told of as any turn's end.
    pub(super) fn task_turn(
        &mut self,
        host: crate::hub::Host,
        meta: &Meta,
        record: Option<Vec<store::Event>>,
        ended: Option<Result<agent_client_protocol::v1::schema::StopReason, String>>,
        cx: &mut Context<Self>,
    ) {
        let id = meta.id.clone().expect("a task's session has an id");
        if self.sessions.by_id(host, &meta.agent, &id).is_none() {
            let key = self.sessions.take_key();
            self.kept
                .insert((host, meta.agent.clone(), id.clone()), meta.clone());
            let mut s = Session::stored(key, host, meta.clone());
            s.state = State::Idle;
            s.recorded = true;
            self.sessions.all.push(s);
        }
        let Some(s) = self.sessions.by_id(host, &meta.agent, &id) else {
            return;
        };
        let key = s.key;
        match ended {
            None => {
                if let Some(events) = record.filter(|_| s.state == State::Closed) {
                    s.forget();
                    s.replay(events);
                    s.recorded = true;
                }
                s.state = State::Working;
                s.since = Some(std::time::Instant::now());
            }
            Some(result) => {
                s.state = State::Idle;
                s.since = None;
                s.meta.updated = meta.updated;
                self.turn_over(key, result.is_ok(), cx);
            }
        }
        cx.notify();
    }
}
