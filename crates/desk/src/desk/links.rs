//! Links that open Ghost, as t3code's deep and prompt links: macOS hands
//! the app each `ghost://` link opened (a click in a browser, `open`), and
//! each folder opened with it (`ghost <folder>`, a folder dropped on the
//! Dock's icon), queued until the window is up.
//!
//! - `ghost://new?project=<path>&agent=<id>&prompt=<text>`: a new session,
//!   what you would write in it written. `&pr=<n>` or `&issue=<n>` begin it
//!   on a pull request (in a worktree of its own branch) or about an issue.
//! - `ghost://session/<agent>/<id>?prompt=<text>`: that session, and with
//!   a prompt, it written there (asked first over what you were writing).
//! - `ghost://pair?host=<host>#<code>`: pair with the Ghost there, as
//!   Settings › Devices made the link; asked first, and whether to pair
//!   both ways.
//! - `file:///path/to/folder`: the folder as a project, and a session in it.
//!
//! A prompt is what someone else wrote: characters that do not show
//! (zero-width, direction marks, tags) are taken out, and you are told.

use std::path::PathBuf;

use gpui::{Context, Window};
use percent_encoding::percent_decode_str;

use super::Desk;

/// What a link opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    New {
        project: Option<PathBuf>,
        agent: Option<String>,
        prompt: Option<String>,
        pr: Option<u64>,
        issue: Option<u64>,
    },
    Session {
        agent: String,
        id: String,
        prompt: Option<String>,
    },
    Pair {
        host: String,
        code: String,
    },
    Folder(PathBuf),
}

impl Link {
    /// The link `text` says; none where it is not one of Ghost's.
    pub fn parse(text: &str) -> Option<Self> {
        let url = url::Url::parse(text).ok()?;
        if url.scheme() == "file" {
            return url.to_file_path().ok().map(Self::Folder);
        }
        if url.scheme() != "ghost" {
            return None;
        }
        let query = |name: &str| {
            url.query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
                .filter(|v| !v.is_empty())
        };
        let number = |name: &str| query(name).and_then(|v| v.trim_start_matches('#').parse().ok());
        let parts: Vec<String> = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|p| !p.is_empty())
            .map(|p| percent_decode_str(p).decode_utf8_lossy().into_owned())
            .collect();
        match (url.host_str()?, parts.as_slice()) {
            ("new", []) => Some(Self::New {
                project: query("project").map(PathBuf::from),
                agent: query("agent"),
                prompt: query("prompt"),
                pr: number("pr"),
                issue: number("issue"),
            }),
            ("session", [agent, id]) => Some(Self::Session {
                agent: agent.clone(),
                id: id.clone(),
                prompt: query("prompt"),
            }),
            ("pair", []) => {
                let (host, code) = crate::link::pairing(text)?;
                Some(Self::Pair { host, code })
            }
            _ => None,
        }
    }

    /// The link to the session `id` of `agent`, as Copy ▸ Link gives it.
    pub fn to_session(agent: &str, id: &str) -> String {
        let mut url = url::Url::parse("ghost://session/").expect("a link");
        url.path_segments_mut()
            .expect("a link with a path")
            .pop_if_empty()
            .push(agent)
            .push(id);
        url.into()
    }

    /// The link to a new session in `project` with `agent`, `prompt`
    /// written in it.
    pub fn to_new(project: &std::path::Path, agent: &str, prompt: &str) -> String {
        let mut url = url::Url::parse("ghost://new").expect("a link");
        url.query_pairs_mut()
            .append_pair("project", &project.to_string_lossy())
            .append_pair("agent", agent)
            .append_pair("prompt", prompt);
        url.into()
    }
}

/// `text` without the characters that do not show, and whether any were
/// there.
pub fn visible(text: &str) -> (String, bool) {
    let hidden = |c: char| {
        matches!(c,
            '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{E0000}'..='\u{E007F}')
    };
    let kept: String = text.chars().filter(|c| !hidden(*c)).collect();
    let took = kept.len() != text.len();
    (kept, took)
}

impl Desk {
    /// The links macOS handed the app, each opened in turn.
    pub fn open_links(&mut self, links: &[String], window: &mut Window, cx: &mut Context<Self>) {
        for text in links {
            match Link::parse(text) {
                Some(link) => self.open_link(link, window, cx),
                None => self.trouble("Ghost doesn't open that link", Some(text.clone()), None),
            }
        }
        cx.notify();
    }

    /// Open `link`: what macOS hands Ghost names this Mac's folders and
    /// sessions.
    fn open_link(&mut self, link: Link, window: &mut Window, cx: &mut Context<Self>) {
        let here = |folder| super::Place::new(crate::hub::Host::HERE, folder);
        match link {
            Link::Folder(folder) => self.add_and_begin(&here(folder), window, cx),
            Link::Pair { host, code } => Self::ask_pair(host, code, window, cx),
            Link::New {
                project,
                agent,
                prompt,
                pr,
                issue,
            } => {
                // Only a project you added: a link does not add one.
                let folder = (project.map(here))
                    .filter(|p| self.projects().contains(p))
                    .or_else(|| self.chosen_project());
                let Some(folder) = folder else {
                    self.new_scratch_on(crate::hub::Host::HERE, window, cx);
                    return self.write_prompt(prompt, window, cx);
                };
                let on = folder.host;
                match (pr, issue, agent.filter(|a| self.slot(on, a).is_some())) {
                    (Some(n), _, _) => {
                        self.start_from_pr(folder, n, String::new(), false, window, cx);
                    }
                    (None, Some(n), _) => self.start_from_issue(folder, n, "", false, window, cx),
                    (None, None, Some(agent)) => self.new_session(&agent, folder, window, cx),
                    (None, None, None) => self.new_in(folder, window, cx),
                }
                self.write_prompt(prompt, window, cx);
            }
            Link::Session { agent, id, prompt } => {
                let id = agent_client_protocol::v1::schema::SessionId::new(id);
                let found = self.sessions.by_id(crate::hub::Host::HERE, &agent, &id);
                let Some(key) = found.map(|s| s.key) else {
                    self.trouble(
                        "That session isn't here",
                        Some(format!("{agent} {}", id.0)),
                        None,
                    );
                    return;
                };
                self.show(key, window, cx);
                let Some(prompt) = prompt else { return };
                if self.typed(cx).is_empty() {
                    self.write_prompt(Some(prompt), window, cx);
                } else {
                    Self::ask_replace(prompt, window, cx);
                }
            }
        }
    }

    /// Write `prompt`, without what does not show, as what you are writing.
    fn write_prompt(
        &mut self,
        prompt: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = prompt else { return };
        let (prompt, took) = visible(&prompt);
        if took {
            self.trouble(
                "The link's prompt had characters that don't show",
                Some("They were taken out. Read it before you send it.".into()),
                None,
            );
        }
        self.put_line(&prompt, window, cx);
    }

    /// Ask before a link's prompt takes the place of what you were writing.
    fn ask_replace(prompt: String, window: &mut Window, cx: &mut Context<Self>) {
        use gpui::component::WindowExt as _;
        let desk = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (desk, prompt) = (desk.clone(), prompt.clone());
            alert
                .title("Replace what you were writing?")
                .description("A link brought a prompt for this session.")
                .ok_text("Replace")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    desk.update(cx, |d, cx| d.write_prompt(Some(prompt.clone()), window, cx));
                    true
                })
        });
    }

    /// Copy a link that opens a new session in this one's project, with its
    /// agent, what you are writing written in it.
    pub fn copy_prompt_link(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.session() else { return };
        let link = Link::to_new(&s.meta.project, &s.meta.agent, &self.typed(cx));
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(link));
        self.done("Copied a link to what you are writing", None);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_reads_back_as_what_it_opens() {
        let new = Link::to_new(std::path::Path::new("/a b/c"), "codex-acp", "fix it & go");
        assert_eq!(
            Link::parse(&new),
            Some(Link::New {
                project: Some(PathBuf::from("/a b/c")),
                agent: Some("codex-acp".into()),
                prompt: Some("fix it & go".into()),
                pr: None,
                issue: None,
            })
        );
        let session = Link::to_session("claude-acp", "a/b c");
        assert_eq!(
            Link::parse(&session),
            Some(Link::Session {
                agent: "claude-acp".into(),
                id: "a/b c".into(),
                prompt: None,
            })
        );
        assert_eq!(
            Link::parse("ghost://new?pr=%2312"),
            Some(Link::New {
                project: None,
                agent: None,
                prompt: None,
                pr: Some(12),
                issue: None,
            })
        );
        assert_eq!(
            Link::parse("file:///tmp/x"),
            Some(Link::Folder(PathBuf::from("/tmp/x")))
        );
        assert_eq!(
            Link::parse("ghost://pair?host=127.0.0.1:4317#c0de"),
            Some(Link::Pair {
                host: "127.0.0.1:4317".into(),
                code: "c0de".into(),
            })
        );
        assert_eq!(Link::parse("https://example.com"), None);
        assert_eq!(Link::parse("ghost://elsewhere"), None);
    }

    #[test]
    fn a_plus_in_a_session_link_stays_a_plus() {
        let session = Link::to_session("claude-acp", "a+b c");
        assert_eq!(
            Link::parse(&session),
            Some(Link::Session {
                agent: "claude-acp".into(),
                id: "a+b c".into(),
                prompt: None,
            })
        );
        assert!(matches!(
            Link::parse("ghost://session/claude-acp/a+b%2Bc"),
            Some(Link::Session { id, .. }) if id == "a+b+c"
        ));
    }

    #[test]
    fn what_does_not_show_is_taken_out_of_a_prompt() {
        assert_eq!(visible("a\u{200B}b\u{202E}c"), ("abc".into(), true));
        assert_eq!(visible("plain"), ("plain".into(), false));
    }
}
