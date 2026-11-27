//! Workflows on the desk: those your files define and their runs, as the
//! server says them (`workflows`), what a run's input is typed in, and the
//! inline editor for creating or modifying a workflow.

use std::path::PathBuf;

use gpui::component::input::InputState;
use gpui::{AppContext as _, Context, Entity, Window};
use u0ui::u0::tasks::{StepType, WorkflowStep as EditorStep};

use super::{Desk, Page, Place};
use crate::{
    proto::Call,
    workflows::{Run, Step, Workflow},
};

/// The workflows as the desk has them.
pub struct Flows {
    /// Each workflow, or why its file does not read (by name).
    pub list: Vec<Result<Workflow, (String, String)>>,
    /// The runs, or why they do not read.
    pub runs: Result<Vec<Run>, String>,
    /// What a run is begun with, as typed.
    pub input: Option<Entity<InputState>>,
    /// Selected workflow name in the workspace tabs (`None` = first available).
    pub selected: Option<String>,
    /// Selected run id in the left `Runs` column (`None` = latest run of selected workflow).
    pub selected_run: Option<u64>,
    /// Run id whose full step output log is expanded (`Full log`).
    pub full_log: Option<u64>,
    /// Workflow currently open in the inline step editor (`None` = run timeline view).
    pub editing: Option<FlowDraft>,
}

/// Draft state for creating or editing a workflow in the workspace view.
pub struct FlowDraft {
    /// Serial bump so `WorkflowStepEditor` re-seeds cleanly when opening another workflow.
    pub seq: u64,
    /// Original workflow name (`None` when creating a new workflow).
    pub original: Option<String>,
    pub name: Entity<InputState>,
    pub about: Entity<InputState>,
    pub project: Option<PathBuf>,
    pub worktree: bool,
    pub steps: Vec<EditorStep>,
    pub error: Option<String>,
}

impl Default for Flows {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            runs: Ok(Vec::new()),
            input: None,
            selected: None,
            selected_run: None,
            full_log: None,
            editing: None,
        }
    }
}

/// Convert a core [`Step`] into `u0ui`'s [`EditorStep`].
pub fn to_editor_step(step: &Step, default_agent: Option<&str>) -> EditorStep {
    match step {
        Step::Agent {
            prompt,
            agent,
            mode,
            ..
        } => {
            let mut s = EditorStep::agent(
                agent.as_deref().or(default_agent).unwrap_or("claude-acp"),
                prompt.clone(),
            );
            if let Some(m) = mode {
                s = s.with_mode(m.clone());
            }
            s
        }
        Step::Run {
            command,
            continue_on_error,
            ..
        } => EditorStep::command(command.clone()).with_continue_on_error(*continue_on_error),
        Step::Ask { title, choices, .. } => {
            EditorStep::question(title.clone(), choices.iter().cloned())
        }
    }
}

/// Convert `u0ui`'s [`EditorStep`]s back into core [`Step`]s with unique step names.
pub fn from_editor_steps(steps: &[EditorStep]) -> Vec<Step> {
    let mut used = std::collections::HashSet::new();
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let raw = match s.kind {
                StepType::Agent => s.prompt.lines().next().unwrap_or_default().trim(),
                StepType::Command => s.command.trim(),
                StepType::Question => s.question.trim(),
            };
            let base = if raw.is_empty() {
                format!("step-{}", i + 1)
            } else {
                raw.chars().take(48).collect::<String>()
            };
            let mut name = base.clone();
            let mut suffix = 2usize;
            while !used.insert(name.clone()) {
                name = format!("{base} ({suffix})");
                suffix += 1;
            }
            match s.kind {
                StepType::Agent => Step::Agent {
                    name,
                    prompt: s.prompt.to_string(),
                    agent: s.agent.as_ref().map(ToString::to_string),
                    model: None,
                    mode: s
                        .mode
                        .as_ref()
                        .map(ToString::to_string)
                        .filter(|m| !m.is_empty() && m != "Default"),
                    project: None,
                    worktree: false,
                    after: None,
                },
                StepType::Command => Step::Run {
                    name,
                    command: s.command.to_string(),
                    dir: None,
                    continue_on_error: s.continue_on_error,
                },
                StepType::Question => {
                    let choices: Vec<String> = if s.answers.is_empty() {
                        vec!["Continue".into(), "Stop".into()]
                    } else {
                        s.answers.iter().map(ToString::to_string).collect()
                    };
                    let stop = choices
                        .iter()
                        .filter(|c| {
                            matches!(
                                c.to_ascii_lowercase().as_str(),
                                "stop" | "wait" | "hold" | "cancel" | "skip" | "no"
                            )
                        })
                        .cloned()
                        .collect();
                    Step::Ask {
                        name,
                        title: s.question.to_string(),
                        choices,
                        stop,
                    }
                }
            }
        })
        .collect()
}

impl Desk {
    /// Ask for the workflows and their runs, and make the line a run's
    /// input is typed in.
    pub(crate) fn read_flows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.flows.input.is_none() {
            self.flows.input =
                Some(cx.new(|cx| {
                    InputState::new(window, cx).placeholder(crate::tr("Input (optional)"))
                }));
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

    /// Open the inline step editor for `existing` (`None` creates a new workflow).
    pub(crate) fn begin_edit_flow(
        &mut self,
        existing: Option<&Workflow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let seq = self.flows.editing.as_ref().map_or(1, |d| d.seq + 1);
        let default_agent = self
            .slots_on(self.pages.on)
            .next()
            .map(|s| s.entry.id.clone());
        let default_project = self
            .edited()
            .folders
            .first()
            .cloned()
            .or_else(|| self.chosen_project().map(|p| p.folder));
        let (original, init_name, init_about, project, worktree, steps) = match existing {
            Some(w) => (
                Some(w.name.clone()),
                w.name.clone(),
                w.about.clone(),
                w.project.clone().or(default_project),
                w.worktree,
                w.steps
                    .iter()
                    .map(|s| to_editor_step(s, w.agent.as_deref().or(default_agent.as_deref())))
                    .collect(),
            ),
            None => (
                None,
                String::new(),
                String::new(),
                default_project,
                true,
                vec![EditorStep::agent(
                    default_agent.as_deref().unwrap_or("claude-acp"),
                    "",
                )],
            ),
        };
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::tr("release-check"))
                .default_value(init_name)
        });
        let about = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::tr("What this workflow does"))
                .default_value(init_about)
        });
        self.flows.editing = Some(FlowDraft {
            seq,
            original,
            name,
            about,
            project,
            worktree,
            steps,
            error: None,
        });
        cx.notify();
    }

    /// Save the workflow currently open in `self.flows.editing`.
    pub(crate) fn save_edited_flow(&mut self, cx: &mut Context<Self>) {
        let default_agent = self
            .slots_on(self.pages.on)
            .next()
            .map(|s| s.entry.id.clone());
        let Some(draft) = &mut self.flows.editing else {
            return;
        };
        let name = draft.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            draft.error = Some(crate::tr("Give the workflow a name.").into());
            cx.notify();
            return;
        }
        let steps = from_editor_steps(&draft.steps);
        if steps.is_empty() {
            draft.error = Some(crate::tr("Add at least one step.").into());
            cx.notify();
            return;
        }
        let flow = Workflow {
            name: name.clone(),
            about: draft.about.read(cx).value().trim().to_owned(),
            agent: default_agent,
            project: draft.project.clone(),
            worktree: draft.worktree,
            steps,
        };
        let old = draft.original.clone().filter(|o| *o != name);
        let host = self.pages.on;
        if let Some(old_name) = old {
            self.call(host, Call::DropWorkflow(old_name));
        }
        self.call(host, Call::SetWorkflow(flow));
        self.flows.selected = Some(name);
        self.flows.editing = None;
        cx.notify();
    }

    /// Delete the workflow `name`.
    pub(crate) fn delete_flow(&mut self, name: &str, cx: &mut Context<Self>) {
        self.call(self.pages.on, Call::DropWorkflow(name.to_owned()));
        if self.flows.selected.as_deref() == Some(name) {
            self.flows.selected = None;
        }
        self.flows.editing = None;
        cx.notify();
    }

    /// Re-run workflow `run` starting from `step` (0-indexed).
    pub(crate) fn rerun_flow_from(&mut self, run: u64, step: usize, cx: &mut Context<Self>) {
        self.call(self.pages.on, Call::RerunFlowFrom { run, step });
        cx.notify();
    }

    /// Open a thread in the run's worktree (or project) pre-filled with the failed step's output.
    pub(crate) fn fix_flow_in_thread(
        &mut self,
        run: &Run,
        step_title: &str,
        error_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let host = self.pages.on;
        let flow = self
            .flows
            .list
            .iter()
            .flatten()
            .find(|f| f.name == run.workflow);
        let folder = run
            .worktree
            .as_ref()
            .map(|w| w.path.clone())
            .or_else(|| flow.and_then(|f| f.project.clone()))
            .or_else(|| self.chosen_project().map(|p| p.folder));
        let Some(folder) = folder else {
            self.open_page(Page::Thread, window, cx);
            return;
        };
        let place = Place::new(host, folder);
        let prompt = format!(
            "Fix failed workflow step “{step_title}” in Run #{} of {}:\n\n```\n{}\n```",
            run.id,
            run.workflow,
            error_text.trim()
        );
        self.new_in(place, window, cx);
        self.put_line(&prompt, window, cx);
    }

    /// Answer the question the run `run` waits on.
    pub(crate) fn answer_flow(&self, run: u64, choice: String) {
        self.call(self.pages.on, Call::AnswerFlow { run, choice });
    }

    pub(crate) fn cancel_flow(&self, run: u64) {
        self.call(self.pages.on, Call::CancelFlow(run));
    }
}
