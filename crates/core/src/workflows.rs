//! Workflows, as t3code has them: work in stages, each a step, run as one
//! durable run that outlives a restart. A step says a prompt to an agent,
//! runs a command, or asks you; each step's result is kept as it ends, so
//! a run that stops picks up at the step it stopped at, and later steps
//! read earlier results (`{{name}}`) and the run's input (`{{input}}`).
//!
//! A workflow is a JSON file of yours in the home's `workflows/` folder,
//! read again each time one is listed or started; its runs are the
//! server's, kept in `runs.json`.

use std::path::{Path, PathBuf};

use agent_client_protocol::v1::schema::SessionId;
use serde::{Deserialize, Serialize};

use crate::whole;

/// A workflow, as its file says it; its name is the file's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    #[serde(skip)]
    pub name: String,
    /// What it is for, in a line.
    #[serde(default)]
    pub about: String,
    /// The agent its agent steps are said to, unless one says another.
    #[serde(default)]
    pub agent: Option<String>,
    /// The project its steps work in, unless one says another.
    #[serde(default)]
    pub project: Option<PathBuf>,
    pub steps: Vec<Step>,
}

/// One stage of a workflow, named so that later ones can read its result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Say `prompt` to an agent: in a new session, or in the session of
    /// the earlier agent step `after`. Its result is what it answered.
    Agent {
        name: String,
        prompt: String,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        project: Option<PathBuf>,
        #[serde(default)]
        worktree: bool,
        #[serde(default)]
        after: Option<String>,
    },
    /// Run `command` in a shell, in the project (or `dir`); its result is
    /// what it printed. Each `{{name}}` in it is given it as an argument,
    /// never read as words of the command; one where the shell reads a
    /// word as code (after `eval` or `let`, within `[[ ]]` or `(( ))`)
    /// is refused: a guard, not a promise, for the shell evaluates words
    /// elsewhere too. One given a command as an operand goes after `--`; a
    /// command that runs its words (`sh -c`, `ssh`) runs a value. One
    /// that fails pauses the run.
    Run {
        name: String,
        command: String,
        #[serde(default)]
        dir: Option<PathBuf>,
    },
    /// Ask you `title`, with `choices`; its result is your choice. A
    /// choice among `stop` ends the run there.
    Ask {
        name: String,
        title: String,
        choices: Vec<String>,
        #[serde(default)]
        stop: Vec<String>,
    },
}

impl Step {
    pub fn name(&self) -> &str {
        match self {
            Self::Agent { name, .. } | Self::Run { name, .. } | Self::Ask { name, .. } => name,
        }
    }
}

/// Why a command was not run: a value would be read as code.
pub const EVALUATED: &str = "A {{name}} in this command would be read by the shell as code (after eval or let, or within [[ ]] or (( ))): give it to a command as a word, and test it there";

/// Whether the shell reads a word put at the end of `before` as code: in
/// a command begun by `eval` or `let`, or within `[[ ]]` or `(( ))` (and
/// so `$(( ))`), where a value such as `a[$(cmd)]` runs `cmd`.
fn evaluates(before: &str) -> bool {
    let open =
        |opens: &str, closes: &str| before.matches(opens).count() > before.matches(closes).count();
    let this = before
        .rsplit(['\n', ';', '|', '&', '(', '`'])
        .next()
        .unwrap_or(before);
    let first = this.split_whitespace().next();
    open("[[", "]]") || open("((", "))") || matches!(first, Some("eval" | "let"))
}

/// Where a shell reading a command stands: bare, or within quotes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Within {
    #[default]
    Bare,
    Single,
    Double,
}

/// A shell's quoting, as a command is read left to right.
#[derive(Debug, Default)]
struct Quoting {
    within: Within,
    /// The last character read was a backslash that escapes the next.
    escaped: bool,
}

impl Quoting {
    /// Read `text` on from where it stands.
    fn read(&mut self, text: &str) {
        for c in text.chars() {
            if std::mem::take(&mut self.escaped) {
                continue;
            }
            self.within = match (self.within, c) {
                (Within::Bare | Within::Double, '\\') => {
                    self.escaped = true;
                    self.within
                }
                (Within::Bare, '\'') => Within::Single,
                (Within::Bare, '"') => Within::Double,
                (Within::Single, '\'') | (Within::Double, '"') => Within::Bare,
                (now, _) => now,
            };
        }
    }
}

/// What `runs.json` holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runs {
    pub runs: Vec<Run>,
}

/// A run of a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub id: u64,
    pub workflow: String,
    pub input: String,
    /// When it began (seconds since the epoch).
    pub began: u64,
    pub state: State,
    /// Each step done, in order: the next is the one it is at.
    pub done: Vec<Done>,
    /// The session of the agent step going on, by agent and id.
    #[serde(default)]
    pub turn: Option<(String, SessionId)>,
}

/// Where a run stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Going,
    /// Waiting on your answer to the step it is at.
    Asking,
    /// Stopped at the step it is at, why; resumed, that step runs again.
    Paused(String),
    /// All its steps done.
    Done,
    /// Ended early by a choice of yours.
    Stopped(String),
    Cancelled,
}

/// A step done, and what came of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Done {
    pub name: String,
    pub result: String,
    /// The session an agent step was said in, by agent and id.
    #[serde(default)]
    pub session: Option<(String, SessionId)>,
}

impl Run {
    /// `text` with the run's input and its steps' results put in for
    /// `{{input}}` and `{{name}}`.
    pub fn fill(&self, text: &str) -> String {
        self.put(text, |_, value| value.to_owned())
    }

    /// A `run` step's `command` for `sh -c`, and the values its
    /// `{{name}}`s stand for, given after it as its arguments: each
    /// `{{name}}` known becomes a reference to its argument (`${1}`, ...),
    /// quoted as where it stands calls for (bare, in double quotes, in
    /// single quotes), so that what an agent answered, or a webhook sent,
    /// is never read by the shell as words of the command. One put where
    /// the shell reads a word as code is refused (`evaluates`).
    pub fn command(&self, command: &str) -> Result<(String, Vec<String>), String> {
        let mut values = Vec::new();
        let mut quoting = Quoting::default();
        let (mut before, mut code) = (String::new(), false);
        let line = self.put(command, |lead, value| {
            quoting.read(lead);
            before.push_str(lead);
            code |= evaluates(&before);
            values.push(value.to_owned());
            let n = values.len();
            match quoting.within {
                Within::Bare => format!("\"${{{n}}}\""),
                Within::Double => format!("${{{n}}}"),
                Within::Single => format!("'\"${{{n}}}\"'"),
            }
        });
        if code {
            return Err(EVALUATED.to_owned());
        }
        Ok((line, values))
    }

    /// `text` with each `{{name}}` known put in as `each` writes its value
    /// (given the text before it since the last), in one pass: a value put
    /// in is never read again for more.
    fn put(&self, text: &str, mut each: impl FnMut(&str, &str) -> String) -> String {
        let mut out = String::with_capacity(text.len());
        let (mut rest, mut lead) = (text, String::new());
        while let Some(at) = rest.find("{{") {
            out.push_str(&rest[..at]);
            lead.push_str(&rest[..at]);
            let after = &rest[at + 2..];
            let found = after.find("}}").and_then(|end| {
                let name = &after[..end];
                let value = if name == "input" {
                    Some(self.input.as_str())
                } else {
                    (self.done.iter())
                        .find(|d| d.name == name)
                        .map(|d| d.result.trim())
                };
                value.map(|v| (v, end))
            });
            if let Some((value, end)) = found {
                out.push_str(&each(&std::mem::take(&mut lead), value));
                rest = &after[end + 2..];
            } else {
                out.push_str("{{");
                lead.push_str("{{");
                rest = after;
            }
        }
        out.push_str(rest);
        out
    }

    /// Whether it is over: nothing more of it runs.
    pub fn over(&self) -> bool {
        matches!(
            self.state,
            State::Done | State::Stopped(_) | State::Cancelled
        )
    }
}

pub fn folder(home: &Path) -> PathBuf {
    home.join("workflows")
}

/// Each workflow in `home`, or why its file does not read, by name.
pub fn load(home: &Path) -> Vec<Result<Workflow, (String, String)>> {
    let Ok(files) = std::fs::read_dir(folder(home)) else {
        return Vec::new();
    };
    let mut found: Vec<_> = files
        .flatten()
        .map(|f| f.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            Some(read(&path, name))
        })
        .collect();
    found.sort_by(|a, b| name_of(a).cmp(name_of(b)));
    found
}

fn name_of(w: &Result<Workflow, (String, String)>) -> &str {
    match w {
        Ok(w) => &w.name,
        Err((name, _)) => name,
    }
}

fn read(path: &Path, name: String) -> Result<Workflow, (String, String)> {
    let text = std::fs::read_to_string(path).map_err(|e| (name.clone(), e.to_string()))?;
    let mut w: Workflow = serde_json::from_str(&text).map_err(|e| (name.clone(), e.to_string()))?;
    let mut seen = std::collections::HashSet::new();
    if let Some(twice) = w.steps.iter().find(|s| !seen.insert(s.name().to_owned())) {
        return Err((name, format!("two steps are named “{}”", twice.name())));
    }
    w.name = name;
    Ok(w)
}

/// The workflow `name` in `home`.
pub fn find(home: &Path, name: &str) -> Result<Workflow, String> {
    read(&folder(home).join(format!("{name}.json")), name.to_owned()).map_err(|(_, why)| why)
}

pub fn runs_path(home: &Path) -> PathBuf {
    home.join("runs.json")
}

/// The runs kept in `home`; none without a file, and why where it does
/// not read: then nothing is saved over it.
pub fn load_runs(home: &Path) -> Result<Runs, String> {
    whole::read_json(&runs_path(home)).map_err(|e| {
        format!("{e}. Move it away to start afresh; until then no workflow runs or is kept.")
    })
}

pub fn bytes(runs: &Runs) -> Vec<u8> {
    serde_json::to_vec_pretty(runs).expect("the runs serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workflow_is_read_from_its_file_and_one_that_does_not_read_says_why() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder(home.path())).unwrap();
        std::fs::write(
            folder(home.path()).join("release.json"),
            r#"{"about": "Ship it", "steps": [
                {"agent": {"name": "notes", "prompt": "Notes for {{input}}"}},
                {"ask": {"name": "ship", "title": "Ship?", "choices": ["Ship", "Hold"], "stop": ["Hold"]}},
                {"run": {"name": "tag", "command": "git tag {{input}}"}}
            ]}"#,
        )
        .unwrap();
        std::fs::write(
            folder(home.path()).join("twice.json"),
            r#"{"steps": [{"run": {"name": "a", "command": "x"}}, {"run": {"name": "a", "command": "y"}}]}"#,
        )
        .unwrap();
        let all = load(home.path());
        let release = all[0].as_ref().unwrap();
        assert_eq!((release.name.as_str(), release.steps.len()), ("release", 3));
        assert!(all[1].as_ref().unwrap_err().1.contains("two steps"));
    }

    #[test]
    fn a_step_reads_the_input_and_the_results_before_it() {
        let run = Run {
            id: 1,
            workflow: "w".into(),
            input: "v1.4".into(),
            began: 0,
            state: State::Going,
            done: vec![Done {
                name: "notes".into(),
                result: "3 fixes\n".into(),
                session: None,
            }],
            turn: None,
        };
        assert_eq!(run.fill("Ship {{input}}? {{notes}}"), "Ship v1.4? 3 fixes");
        assert_eq!(run.fill("{{later}}"), "{{later}}");
    }

    #[test]
    fn a_command_that_would_read_a_value_as_code_is_refused() {
        let run = Run {
            id: 1,
            workflow: "w".into(),
            input: "a[$(touch pwned)]".into(),
            began: 0,
            state: State::Going,
            done: Vec::new(),
            turn: None,
        };
        for evaluated in [
            "[[ \"{{input}}\" -eq 1 ]] && echo one",
            "echo $(( {{input}} + 1 ))",
            "(( x = {{input}} ))",
            "eval \"echo {{input}}\"",
            "cd /tmp; let n={{input}}",
        ] {
            assert_eq!(
                run.command(evaluated),
                Err(EVALUATED.to_owned()),
                "{evaluated}"
            );
        }
        for plain in [
            "[[ -n \"$HOME\" ]] && echo {{input}}",
            "echo $((1 + 1)) {{input}}",
            "git fetch origin -- {{input}}",
            "printf '%s' \"evaluate {{input}}\"",
        ] {
            assert!(run.command(plain).is_ok(), "{plain}");
        }
    }

    #[test]
    fn a_value_in_double_quotes_single_quotes_or_none_is_never_run() {
        let dir = tempfile::tempdir().unwrap();
        for input in [
            "$(touch pwned)",
            "\"; touch pwned; \"",
            "'; touch pwned; '",
            "`touch pwned` \\\" ${HOME}",
        ] {
            let run = Run {
                id: 1,
                workflow: "w".into(),
                input: input.into(),
                began: 0,
                state: State::Going,
                done: Vec::new(),
                turn: None,
            };
            for template in [
                "printf '%s|' \"{{input}}\"",
                "printf '%s|' '{{input}}'",
                "printf '%s|' {{input}}",
                "printf '%s|' \"a {{input}} b\" 'c {{input}} d'",
            ] {
                let (line, values) = run.command(template).unwrap();
                let out = std::process::Command::new("/bin/sh")
                    .args(["-c", &line, "ghost"])
                    .args(values)
                    .current_dir(dir.path())
                    .output()
                    .unwrap();
                let said = String::from_utf8_lossy(&out.stdout);
                assert!(out.status.success(), "{template}: {line}");
                assert!(said.contains(input), "{template} said {said}");
                assert!(!dir.path().join("pwned").exists(), "{template} ran {input}");
            }
        }
    }

    #[test]
    fn what_an_agent_answered_goes_into_a_command_as_one_word_and_never_runs() {
        let dir = tempfile::tempdir().unwrap();
        let said = "fixes; touch pwned '$(touch pwned)`touch pwned`";
        let run = Run {
            id: 1,
            workflow: "w".into(),
            input: "v1 {{notes}}".into(),
            began: 0,
            state: State::Going,
            done: vec![Done {
                name: "notes".into(),
                result: format!("{said}\n"),
                session: None,
            }],
            turn: None,
        };
        let (line, values) = run
            .command("printf '%s|' {{input}} {{notes}} {{later}}")
            .unwrap();
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &line, "ghost"])
            .args(values)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("v1 {{{{notes}}}}|{said}|{{{{later}}}}|")
        );
        assert!(!dir.path().join("pwned").exists());
    }
}
