//! Workflows on the desk: those your files define and their runs, as the
//! server says them (`workflows`), and what a run's input is typed in.

use gpui::component::input::InputState;
use gpui::{AppContext as _, Context, Entity, Window};

use super::Desk;
use crate::{
    proto::Call,
    workflows::{Run, Workflow},
};

/// The workflows as the desk has them.
pub struct Flows {
    /// Each workflow, or why its file does not read (by name).
    pub list: Vec<Result<Workflow, (String, String)>>,
    /// The runs, or why they do not read.
    pub runs: Result<Vec<Run>, String>,
    /// What a run is begun with, as typed.
    pub input: Option<Entity<InputState>>,
    /// Only the runs that wait on you, or only those going; or all.
    pub showing: Showing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Showing {
    NeedsYou,
    Going,
    All,
}

impl Default for Flows {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            runs: Ok(Vec::new()),
            input: None,
            showing: Showing::All,
        }
    }
}

impl Desk {
    /// Ask for the workflows and their runs, and make the line a run's
    /// input is typed in.
    pub(crate) fn read_flows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.flows.input.is_none() {
            self.flows.input =
                Some(cx.new(|cx| InputState::new(window, cx).placeholder("Its input (optional)")));
        }
        self.call(self.pages.on, Call::Workflows);
    }

    /// Begin a run of `workflow` with what is typed as its input.
    pub(crate) fn start_flow(
        &mut self,
        workflow: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self
            .flows
            .input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_owned());
        self.call(
            self.pages.on,
            Call::StartFlow {
                workflow,
                input: input.unwrap_or_default(),
            },
        );
        if let Some(i) = &self.flows.input {
            i.update(cx, |i, cx| i.set_value("", window, cx));
        }
    }

    /// Answer the question the run `run` waits on.
    pub(crate) fn answer_flow(&self, run: u64, choice: String) {
        self.call(self.pages.on, Call::AnswerFlow { run, choice });
    }

    pub(crate) fn resume_flow(&self, run: u64) {
        self.call(self.pages.on, Call::ResumeFlow(run));
    }

    pub(crate) fn cancel_flow(&self, run: u64) {
        self.call(self.pages.on, Call::CancelFlow(run));
    }
}
