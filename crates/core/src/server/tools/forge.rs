//! Ghost's tools for a project's pull requests and their checks, from
//! the host of its remote as the PRs page has them (`forge::answer`, what
//! `Call::Forge` asks): listed and read, reviewed and merged, their
//! conversations answered and resolved, a check's failed run read and
//! run again.

use std::path::PathBuf;

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{Tools, off, said};
use crate::{
    git::Method,
    github::{Act, Answer, Ask, Detail, Item, Kind, Line, Thread},
    settings::Settings,
};

#[derive(Deserialize, schemars::JsonSchema)]
struct ListPrs {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    /// open (none), closed, merged or all.
    state: Option<String>,
    /// As the host's search reads it (`author:@me`, `label:bug`).
    #[serde(default)]
    search: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ReadPr {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
    /// Its whole patch too.
    #[serde(default)]
    diff: bool,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct ReviewPr {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
    verdict: Verdict,
    /// What the review says.
    body: String,
    /// Comments on lines of its new side.
    #[serde(default)]
    lines: Vec<Line>,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct MergePr {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
    /// none, as your settings merge.
    how: Option<Method>,
    /// Merged once its checks pass, rather than now.
    #[serde(default)]
    auto: bool,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct ReplyToThread {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
    /// The conversation, by its id as `read_pr` gives it (`threads`).
    thread: String,
    body: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct ResolveThread {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
    /// The conversation, by its id as `read_pr` gives it (`threads`).
    thread: String,
    /// Open it again; else resolve it.
    #[serde(default)]
    reopen: bool,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct RerunCi {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
    /// The failed run, by its id as `pr_checks` gives it.
    run: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Checks {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    number: u64,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct CiLog {
    /// The project's folder; none, where this session works.
    folder: Option<PathBuf>,
    /// The run, by its id as `pr_checks` gives it.
    run: String,
}

/// What the host of `folder`'s remote answers to `ask` of its pull
/// requests.
async fn asked(folder: PathBuf, ask: Ask) -> Result<Answer, String> {
    off(move || crate::forge::answer(&folder, Kind::Pr, ask, None)).await
}

/// What was done to the pull request or issue `number`, as `answer` says.
pub(super) fn acted(answer: Answer, number: u64) -> Result<String, String> {
    match answer {
        Answer::Acted(_, done) => done.map(|said| {
            let said = said.trim();
            if said.is_empty() {
                format!("Done to #{number}.")
            } else {
                said.to_owned()
            }
        }),
        _ => Err("The host said nothing of it".into()),
    }
}

/// The merge `ask` asks for, as `settings` (those of its project) merge
/// where it says not how: its branch deleted after, as the PRs page
/// merges.
fn merge(ask: &MergePr, settings: &Settings) -> Act {
    Act::Merge {
        how: ask.how.unwrap_or(settings.merge).gh().to_owned(),
        auto: ask.auto,
        delete: true,
        uncredit: settings.uncredit,
    }
}

/// The pull request `number` of `folder`'s host, as it is now: what its
/// card names. Asked only of an agent in a session of Ghost's, for none
/// other may act.
async fn about(tools: &Tools, folder: PathBuf, number: u64) -> Result<Item, String> {
    Ok(viewed(tools, folder, number).await?.item)
}

/// The pull request `number` read whole, as `about` reads it.
async fn viewed(tools: &Tools, folder: PathBuf, number: u64) -> Result<Detail, String> {
    tools.session().ok_or(super::OUTSIDE)?;
    let Answer::Viewed(_, read) = asked(folder, Ask::View(number)).await? else {
        return Err("The host said nothing of it".into());
    };
    Ok(*read?)
}

/// The conversation of `pr` whose id is `thread`, exactly as the host
/// lists it: an id an agent made up (`123#`, `../..`) would be put into a
/// host's address and reach elsewhere.
fn thread_of<'a>(pr: &'a Detail, thread: &str) -> Result<&'a Thread, String> {
    (pr.threads.iter().find(|t| t.id == thread))
        .ok_or_else(|| "That is not a conversation of this pull request: read_pr lists them".into())
}

/// A run of the checks, by its id: digits alone, as the hosts number
/// them, so that none is read as an option of gh.
fn run_id(run: &str) -> Result<&str, String> {
    (!run.is_empty() && run.bytes().all(|b| b.is_ascii_digit()))
        .then_some(run)
        .ok_or_else(|| "A run is named by its number, as pr_checks gives it".into())
}

/// A pull request or an issue (`what`) as a card names it: its number,
/// title and address, which names its repository.
pub(super) fn named(what: &str, item: &Item) -> String {
    format!("{what} #{} “{}” ({})", item.number, item.title, item.url)
}

/// The card's title for `act` on `pr`: which pull request, of which
/// repository (its address), and all it does.
fn card(pr: &Item, act: &Act) -> String {
    let named = named("pull request", pr);
    match act {
        Act::Merge { how, auto, .. } => {
            let when = if *auto { ", once its checks pass" } else { "" };
            format!(
                "Merge {named} into {} by {how}{when}, then delete its branch {}",
                pr.base, pr.branch
            )
        }
        Act::Review { verdict, .. } => match verdict.as_str() {
            "approve" => format!("Approve {named}"),
            "request-changes" => format!("Request changes on {named}"),
            _ => format!("Comment on {named}"),
        },
        _ => named,
    }
}

/// The review `ask` asks for, its verdict as the hosts take it.
fn review(ask: ReviewPr) -> Act {
    let verdict = match ask.verdict {
        Verdict::Approve => "approve",
        Verdict::RequestChanges => "request-changes",
        Verdict::Comment => "comment",
    };
    Act::Review {
        verdict: verdict.to_owned(),
        body: ask.body,
        lines: ask.lines,
    }
}

/// The checks of `detail`, each with the run a failed one's log is read
/// by.
fn checks(detail: &Detail) -> Vec<serde_json::Value> {
    (detail.checks.iter())
        .map(|c| {
            serde_json::json!({
                "name": c.name, "state": c.state, "url": c.url,
                "run": c.run().filter(|_| c.failed()),
            })
        })
        .collect()
}

#[tool_router(router = forge_router, vis = "pub(super)")]
impl Tools {
    /// A project's pull requests, from the host of its remote: each one's
    /// number, title, state, author, branch, review and checks.
    #[tool]
    async fn list_prs(&self, Parameters(ask): Parameters<ListPrs>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        let state = ask.state.unwrap_or_else(|| "open".into());
        if !["open", "closed", "merged", "all"].contains(&state.as_str()) {
            return Err("state is open, closed, merged or all".into());
        }
        let search = ask.search;
        match asked(folder, Ask::List { state, search }).await? {
            Answer::Listed(listed) => said(&listed?),
            _ => Err("The host listed nothing".into()),
        }
    }

    /// A pull request read whole: what it says, what was said on it and on
    /// its lines, its checks, reviewers and whether it conflicts; with
    /// `diff`, its patch too.
    #[tool]
    async fn read_pr(&self, Parameters(ask): Parameters<ReadPr>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        let Answer::Viewed(_, read) = asked(folder.clone(), Ask::View(ask.number)).await? else {
            return Err("The host said nothing of it".into());
        };
        let mut read = said(&read?)?;
        if ask.diff {
            let Answer::Diffed(_, patch) = asked(folder, Ask::Diff(ask.number)).await? else {
                return Err("The host gave no patch".into());
            };
            read = format!("{read}\n\n{}", patch?);
        }
        Ok(read)
    }

    /// Review a pull request: approve it, ask for changes, or comment;
    /// with comments on lines of its new side. You are asked first.
    #[tool]
    async fn review_pr(&self, Parameters(ask): Parameters<ReviewPr>) -> Result<String, String> {
        let folder = self.folder(ask.folder.clone())?;
        let pr = about(self, folder.clone(), ask.number).await?;
        let number = ask.number;
        let input = serde_json::to_value(&ask).map_err(|e| e.to_string())?;
        let act = review(ask);
        self.allowed("review_pr", card(&pr, &act), &input).await?;
        acted(asked(folder, Ask::Act(number, act)).await?, number)
    }

    /// Merge a pull request, its branch deleted after: now, or once its
    /// checks pass (`auto`). You are asked first.
    #[tool]
    async fn merge_pr(&self, Parameters(ask): Parameters<MergePr>) -> Result<String, String> {
        let folder = self.folder(ask.folder.clone())?;
        let pr = about(self, folder.clone(), ask.number).await?;
        let act = merge(&ask, &Settings::load(&self.shared.home).0.of(&folder));
        self.allowed("merge_pr", card(&pr, &act), &ask).await?;
        acted(asked(folder, Ask::Act(ask.number, act)).await?, ask.number)
    }

    /// Answer a conversation on a pull request's lines. You are asked
    /// first.
    #[tool]
    async fn reply_to_thread(
        &self,
        Parameters(ask): Parameters<ReplyToThread>,
    ) -> Result<String, String> {
        let folder = self.folder(ask.folder.clone())?;
        let pr = viewed(self, folder.clone(), ask.number).await?;
        let on = thread_of(&pr, &ask.thread)?;
        let title = format!(
            "Answer the conversation on {}:{} of {}",
            on.path,
            on.line.unwrap_or_default(),
            named("pull request", &pr.item)
        );
        self.allowed("reply_to_thread", title, &ask).await?;
        let act = Act::Reply {
            thread: ask.thread,
            body: ask.body,
        };
        acted(asked(folder, Ask::Act(ask.number, act)).await?, ask.number)
    }

    /// Resolve a conversation on a pull request's lines, or open it again
    /// (`reopen`). You are asked first.
    #[tool]
    async fn resolve_thread(
        &self,
        Parameters(ask): Parameters<ResolveThread>,
    ) -> Result<String, String> {
        let folder = self.folder(ask.folder.clone())?;
        let pr = viewed(self, folder.clone(), ask.number).await?;
        let on = thread_of(&pr, &ask.thread)?;
        let doing = if ask.reopen { "Reopen" } else { "Resolve" };
        let title = format!(
            "{doing} the conversation on {}:{} of {}",
            on.path,
            on.line.unwrap_or_default(),
            named("pull request", &pr.item)
        );
        self.allowed("resolve_thread", title, &ask).await?;
        let act = Act::Resolve {
            thread: ask.thread,
            resolved: !ask.reopen,
        };
        acted(asked(folder, Ask::Act(ask.number, act)).await?, ask.number)
    }

    /// Run the failed jobs of a run of a pull request's checks again (on
    /// GitHub Actions). You are asked first.
    #[tool]
    async fn rerun_ci(&self, Parameters(ask): Parameters<RerunCi>) -> Result<String, String> {
        let folder = self.folder(ask.folder.clone())?;
        run_id(&ask.run)?;
        let pr = about(self, folder.clone(), ask.number).await?;
        let title = format!(
            "Run the failed checks of run {} again on {}",
            ask.run,
            named("pull request", &pr)
        );
        self.allowed("rerun_ci", title, &ask).await?;
        let act = Act::Rerun(ask.run);
        acted(asked(folder, Ask::Act(ask.number, act)).await?, ask.number)
    }

    /// The checks of a pull request: each one's name, how it stands, where
    /// it ran, and the run whose log `ci_log` reads, where it failed on
    /// GitHub Actions.
    #[tool]
    async fn pr_checks(&self, Parameters(ask): Parameters<Checks>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        let Answer::Viewed(_, read) = asked(folder, Ask::View(ask.number)).await? else {
            return Err("The host said nothing of it".into());
        };
        said(&checks(&*read?))
    }

    /// What a failed run of a pull request's checks printed where it
    /// failed: the end of its log.
    #[tool]
    async fn ci_log(&self, Parameters(ask): Parameters<CiLog>) -> Result<String, String> {
        let folder = self.folder(ask.folder)?;
        run_id(&ask.run)?;
        match asked(folder, Ask::Log(ask.run)).await? {
            Answer::Logged(_, log) => log,
            _ => Err("The host gave no log".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        super::{
            NOT_HERE, OUTSIDE,
            tests::{Client, repo_at},
        },
        *,
    };
    use crate::server::tests::server;

    #[test]
    fn a_merge_is_as_your_settings_say_where_the_agent_says_not_how() {
        let settings: Settings = serde_json::from_value(json!({"enabled": [], "folders": [],
                "merge": "rebase", "uncredit": true}))
        .unwrap();
        let ask = |how: Option<&str>| -> MergePr {
            serde_json::from_value(json!({"number": 7, "how": how, "auto": true})).unwrap()
        };
        let merged = |act| match act {
            Act::Merge {
                how,
                auto,
                delete,
                uncredit,
            } => (how, auto, delete, uncredit),
            _ => panic!("not a merge"),
        };
        assert_eq!(
            merged(merge(&ask(None), &settings)),
            ("rebase".into(), true, true, true)
        );
        assert_eq!(merged(merge(&ask(Some("squash")), &settings)).0, "squash");
        let made_up: Result<MergePr, _> =
            serde_json::from_value(json!({"number": 7, "how": "--admin"}));
        assert!(made_up.is_err(), "gh is told squash, merge or rebase alone");
    }

    #[test]
    fn a_review_is_said_to_the_host_as_its_verdict_and_a_failed_check_by_its_run() {
        let ask: ReviewPr = serde_json::from_value(json!({"number": 7,
            "verdict": "request-changes", "body": "Not yet",
            "lines": [{"path": "a.rs", "line": 3, "body": "This"}]}))
        .unwrap();
        let Act::Review { verdict, lines, .. } = review(ask) else {
            panic!("not a review")
        };
        assert_eq!((verdict.as_str(), lines.len()), ("request-changes", 1));
        let detail: Detail = serde_json::from_value(json!({
            "item": {"number": 7, "title": "t", "state": "open", "draft": false,
                "author": "a", "labels": [], "updated": "", "url": "", "branch": "b",
                "base": "main", "review": "", "checks": [1, 1, 0], "comments": 0},
            "body": "", "said": [], "assignees": [], "reviewers": [],
            "conflicts": false, "head": "", "threads": [],
            "checks": [
                {"name": "build", "state": "failure",
                    "url": "https://github.com/o/r/actions/runs/123/job/9"},
                {"name": "lint", "state": "success",
                    "url": "https://github.com/o/r/actions/runs/124/job/1"}]}))
        .unwrap();
        let runs: Vec<_> = checks(&detail).iter().map(|c| c["run"].clone()).collect();
        assert_eq!(runs, [json!("123"), json!(null)]);
    }

    #[test]
    fn a_conversation_is_named_as_the_host_lists_it_and_a_run_by_its_number() {
        let pr: Detail = serde_json::from_value(json!({
            "item": {"number": 7, "title": "t", "state": "open", "draft": false,
                "author": "a", "labels": [], "updated": "", "url": "", "branch": "b",
                "base": "main", "review": "", "checks": [0, 0, 0], "comments": 0},
            "body": "", "said": [], "assignees": [], "reviewers": [], "checks": [],
            "conflicts": false, "head": "", "threads": [{"id": "123", "path": "a.rs",
                "line": 3, "resolved": false, "outdated": false, "bots": false, "said": []}]}))
        .unwrap();
        assert_eq!(thread_of(&pr, "123").unwrap().path, "a.rs");
        for made_up in ["123#", "../../..#", "123/../124", " 123", ""] {
            assert!(thread_of(&pr, made_up).is_err(), "{made_up:?}");
        }
        assert!(run_id("123456").is_ok());
        for made_up in ["--repo=o/r", "1 2", "", "12a"] {
            assert!(run_id(made_up).is_err(), "{made_up:?}");
        }
    }

    #[test]
    fn a_cards_title_names_the_repository_the_pull_request_and_all_the_act_does() {
        let pr: Item = serde_json::from_value(json!({"number": 7, "title": "Fix it",
            "state": "open", "draft": false, "author": "a", "labels": [], "updated": "",
            "url": "https://github.com/o/r/pull/7", "branch": "fix", "base": "main",
            "review": "", "checks": [0, 0, 0], "comments": 0}))
        .unwrap();
        let settings = Settings::default();
        let ask: MergePr =
            serde_json::from_value(json!({"number": 7, "how": "rebase", "auto": true})).unwrap();
        assert_eq!(
            card(&pr, &merge(&ask, &settings)),
            "Merge pull request #7 “Fix it” (https://github.com/o/r/pull/7) into main by rebase, once its checks pass, then delete its branch fix"
        );
        let ask: ReviewPr = serde_json::from_value(json!({"number": 7,
            "verdict": "approve", "body": "Yes"}))
        .unwrap();
        assert_eq!(
            card(&pr, &review(ask)),
            "Approve pull request #7 “Fix it” (https://github.com/o/r/pull/7)"
        );
    }

    #[test]
    fn an_agent_outside_ghost_reads_your_projects_pull_requests_alone_and_merges_none() {
        let home = tempfile::tempdir().unwrap();
        let (project, elsewhere) = (home.path().join("p"), home.path().join("q"));
        repo_at(&project, &[("a.txt", "a")]);
        repo_at(&elsewhere, &[("a.txt", "a")]);
        let settings = json!({"enabled": [], "folders": [project]});
        std::fs::write(home.path().join("settings.json"), settings.to_string()).unwrap();
        let (_server, _said) = server(home.path());
        let mut client = Client::new(home.path(), None);
        for (tool, args) in [
            ("list_prs", json!({"folder": elsewhere})),
            ("read_pr", json!({"folder": elsewhere, "number": 1})),
            ("pr_checks", json!({"folder": elsewhere, "number": 1})),
            ("ci_log", json!({"folder": elsewhere, "run": "1"})),
        ] {
            client.call(tool, &args);
            assert_eq!(client.result(), (NOT_HERE.to_owned(), true), "{tool}");
        }
        for (tool, args) in [
            ("merge_pr", json!({"folder": project, "number": 1})),
            (
                "review_pr",
                json!({"folder": project, "number": 1,
                "verdict": "approve", "body": "Yes"}),
            ),
            (
                "reply_to_thread",
                json!({"folder": project, "number": 1, "thread": "t", "body": "Done"}),
            ),
            (
                "resolve_thread",
                json!({"folder": project, "number": 1, "thread": "t"}),
            ),
            (
                "rerun_ci",
                json!({"folder": project, "number": 1, "run": "9"}),
            ),
        ] {
            client.call(tool, &args);
            assert_eq!(client.result(), (OUTSIDE.to_owned(), true), "{tool}");
        }
    }
}
