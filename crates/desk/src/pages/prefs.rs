//! The settings that are Ghost's own, a tab each: how it tells you things
//! and what it does while you are away; how the thread reads; what you
//! write while a turn goes on; how new sessions begin; the keys; and what
//! Ghost is. Each row says what happens, and its control says the choice.

use gpui::component::button::ButtonGroup;
use gpui::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui::component::{Disableable as _, Sizable as _, input::Input, kbd::Kbd, switch::Switch};
use gpui::{AnyElement, App, Context, Div, SharedString, WeakEntity, div, prelude::*, px};

use crate::{
    desk::Desk,
    fonts::Faces,
    keys,
    settings::{Notify, Quit, Submodules, Width},
    theme,
    ui::{self, Pick, Weight},
};

impl Desk {
    /// The page of the tab `tab`, when it is one of Ghost's own: its
    /// settings, each drawn by `w`, the desk.
    pub(super) fn own_tab(
        &self,
        tab: super::Tab,
        w: &WeakEntity<Desk>,
        cx: &App,
    ) -> Option<SettingPage> {
        use super::Tab;
        let (lede, groups) = match tab {
            Tab::General => (
                "How Ghost tells you things, and what it does while you are away.",
                vec![
                    Self::scope_group(w),
                    group(
                        "While you are away",
                        vec![
                            item(
                                w,
                                "When a session ends a turn or needs you",
                                "From macOS while Ghost is behind, clicked to go to the session; in Ghost while another is on screen.",
                                |d, _, cx| {
                                    Self::pick(
                                        "notify",
                                        [
                                            ("Off", Notify::Off),
                                            ("Notify", Notify::Banner),
                                            ("Notify and sound", Notify::BannerAndSound),
                                        ],
                                        d.settings.notify,
                                        |s, v| s.notify = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            told(w, ("A turn finished", "Its answer is in."), |n| {
                                &mut n.finished
                            }),
                            told(
                                w,
                                (
                                    "A turn failed or was stopped",
                                    "It ended in an error, or you stopped it.",
                                ),
                                |n| &mut n.failed,
                            ),
                            told(
                                w,
                                (
                                    "An agent asks before it goes on",
                                    "A permission or a question waits on you.",
                                ),
                                |n| &mut n.asks,
                            ),
                            item(
                                w,
                                "Keep the Mac awake while an agent works",
                                "The display may sleep; a turn going on is not cut short by the Mac sleeping.",
                                |d, _, cx| {
                                    Switch::new("awake")
                                        .small()
                                        .checked(d.settings.awake)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.settings.awake = !this.settings.awake;
                                            this.save_settings();
                                            this.stay_awake();
                                            cx.notify();
                                        }))
                                        .into_any_element()
                                },
                            ),
                        ],
                    ),
                    group(
                        "Files",
                        vec![item(
                            w,
                            "Files open in",
                            "A file the agent named, an edit, or keybindings.json.",
                            |d, _, cx| d.editor_pick(cx).into_any_element(),
                        )],
                    ),
                    group(
                        "Pull requests",
                        vec![item(
                            w,
                            "Merged as",
                            "How Merge beside a session, and merging once its checks pass, merge a pull request; the PRs page offers each.",
                            |d, _, cx| {
                                d.own(
                                    "merge",
                                    [
                                        ("Squash", crate::git::Method::Squash),
                                        ("Merge commit", crate::git::Method::Merge),
                                        ("Rebase", crate::git::Method::Rebase),
                                    ],
                                    |s| s.merge,
                                    |s, v| s.merge = v,
                                    cx,
                                )
                                .into_any_element()
                            },
                        )],
                    ),
                    group(
                        "Done for now",
                        vec![
                            item(
                                w,
                                "A session is done after it is left for",
                                "It moves to Settled in the sidebar; anything said to it brings it back.",
                                |d, _, cx| {
                                    Self::pick(
                                        "settle",
                                        [
                                            ("3 days", Some(3)),
                                            ("7 days", Some(7)),
                                            ("14 days", Some(14)),
                                            ("30 days", Some(30)),
                                            ("Never", None),
                                        ],
                                        d.settings.settle_days,
                                        |s, v| s.settle_days = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "When its pull request merges or closes",
                                "The session is done then, while it waits on nothing.",
                                |d, _, cx| {
                                    Self::pick(
                                        "settle-merged",
                                        [("Settle it", true), ("Leave it", false)],
                                        d.settings.settle_merged,
                                        |s, v| s.settle_merged = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Snooze for",
                                "The ways to snooze offered first, each as you would type it in Custom…: 1h, 5pm, tomorrow, next week. Commas between.",
                                |d, _, _| {
                                    div()
                                        .w(px(220.))
                                        .child(Input::new(&d.pages.snoozes).small())
                                        .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Keep sessions with a pull request",
                                "One whose pull request is open is never done for being left.",
                                |d, _, cx| {
                                    Self::pick(
                                        "keep-with-pr",
                                        [("Keep them", true), ("Let them settle", false)],
                                        d.settings.keep_with_pr,
                                        |s, v| s.keep_with_pr = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                        ],
                    ),
                    group(
                        "In the background",
                        vec![
                            item(
                                w,
                                "Go on when a limit resets",
                                "A turn an agent's usage limit stopped is said to go on, as a scheduled task, at the time the agent gives.",
                                |d, _, cx| {
                                    d.own(
                                        "resume-at-reset",
                                        [("Go on", true), ("Wait for me", false)],
                                        |s| s.resume_at_reset,
                                        |s, v| s.resume_at_reset = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Fetch each project's remote every",
                                "So branches and pull requests are up to date; projects set to pull are moved on too.",
                                |d, _, cx| {
                                    Self::pick(
                                        "fetch-every",
                                        [
                                            ("Minute", Some(1)),
                                            ("5 minutes", Some(5)),
                                            ("15 minutes", Some(15)),
                                            ("Never", None),
                                        ],
                                        d.settings.fetch_every,
                                        |s, v| s.fetch_every = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Look for agents' new versions",
                                "The ACP registry is read again each hour; an agent that stops working is updated to the newest.",
                                |d, _, cx| {
                                    Self::pick(
                                        "check-agents",
                                        [("Each hour", true), ("Never", false)],
                                        d.settings.check_agents,
                                        |s, v| s.check_agents = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                        ],
                    ),
                    group(
                        "Confirmations",
                        vec![
                            asks(w, ("Unpin", "Ask before a session is unpinned."), |c| {
                                &mut c.unpin
                            }),
                            asks(w, ("Archive", "Ask before a session is archived."), |c| {
                                &mut c.archive
                            }),
                            asks(
                                w,
                                (
                                    "Delete",
                                    "Ask before a session and what was said in it are deleted.",
                                ),
                                |c| &mut c.delete,
                            ),
                            asks(
                                w,
                                (
                                    "Close a terminal",
                                    "Ask before a terminal is closed with something running in it.",
                                ),
                                |c| &mut c.terminal,
                            ),
                            item(
                                w,
                                "Quit with ⌘Q",
                                "Quit in the menu always quits at once.",
                                |d, _, cx| {
                                    Self::pick(
                                        "quit",
                                        [
                                            ("Hold", Quit::Hold),
                                            ("Double press", Quit::Double),
                                            ("Direct", Quit::Direct),
                                        ],
                                        d.settings.confirm.quit,
                                        |s, v| s.confirm.quit = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                        ],
                    ),
                ],
            ),
            Tab::Appearance => (
                "How the thread and what is beside it read.",
                vec![
                    group(
                        "Reading",
                        vec![
                            item(
                                w,
                                "The sidebar sorts by",
                                "A project's sessions, and the projects: your last message, anything happening, when each began, or name. Pinned ones stay on top.",
                                |d, _, cx| {
                                    Self::pick(
                                        "sort",
                                        [
                                            ("Your message", crate::settings::Sort::Used),
                                            ("Activity", crate::settings::Sort::Updated),
                                            ("Begun", crate::settings::Sort::Begun),
                                            ("Name", crate::settings::Sort::Name),
                                        ],
                                        d.settings.sort,
                                        |s, v| s.sort = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Look",
                                "Light and dark keep the same amber for what is going on now.",
                                |d, _, cx| {
                                    Self::pick(
                                        "look",
                                        [
                                            ("Dark", crate::settings::Look::Dark),
                                            ("Light", crate::settings::Look::Light),
                                            ("As the system", crate::settings::Look::System),
                                        ],
                                        d.settings.look,
                                        |s, v| s.look = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Size",
                                "Everything in the window, larger or smaller: ⌘+, ⌘− and ⌘0.",
                                |d, _, cx| {
                                    Self::pick(
                                        "scale",
                                        crate::settings::SCALES.map(|s| (format!("{s}%"), s)),
                                        d.settings.scale,
                                        |s, v| s.scale = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Contrast",
                                "More draws the greys of what matters less, and the edges, stronger.",
                                |d, _, cx| {
                                    Self::pick(
                                        "contrast",
                                        [("As designed", false), ("More", true)],
                                        d.settings.contrast,
                                        |s, v| s.contrast = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Glass",
                                "The window's grounds let the desktop show through, blurred.",
                                |d, _, cx| {
                                    Self::pick(
                                        "glass",
                                        [("Solid", false), ("Glass", true)],
                                        d.settings.glass,
                                        |s, v| s.glass = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Code font",
                                "What code, commands and numbers are set in.",
                                |d, _, cx| {
                                    Self::pick(
                                        "mono",
                                        [
                                            (
                                                "JetBrains Mono",
                                                crate::settings::CodeFont::JetBrains,
                                            ),
                                            ("SF Mono", crate::settings::CodeFont::SfMono),
                                            ("Menlo", crate::settings::CodeFont::Menlo),
                                        ],
                                        d.settings.mono,
                                        |s, v| s.mono = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Terminal text",
                                "How large your terminals' text is, in the code font.",
                                |d, _, cx| {
                                    Self::pick(
                                        "terminal-size",
                                        [
                                            ("Small", crate::settings::TermSize::Small),
                                            ("Medium", crate::settings::TermSize::Medium),
                                            ("Large", crate::settings::TermSize::Large),
                                        ],
                                        d.settings.terminal_size,
                                        |s, v| s.terminal_size = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Terminal shell",
                                "What a new terminal runs: your login shell, or one named.",
                                |d, _, cx| {
                                    Self::pick(
                                        "shell",
                                        [
                                            ("Login shell", crate::settings::Shell::Login),
                                            ("zsh", crate::settings::Shell::Zsh),
                                            ("bash", crate::settings::Shell::Bash),
                                            ("fish", crate::settings::Shell::Fish),
                                        ],
                                        d.settings.shell,
                                        |s, v| s.shell = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Reading width",
                                "How wide the thread and the composer are.",
                                |d, _, cx| {
                                    Self::pick(
                                        "width",
                                        [
                                            ("Comfortable", Width::Comfortable),
                                            ("Wide", Width::Wide),
                                            ("Full", Width::Full),
                                        ],
                                        d.settings.width,
                                        |s, v| s.width = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "What a change added and took away",
                                "Blue and orange tell them apart without green and red.",
                                |d, _, cx| {
                                    Self::pick(
                                        "diff-colours",
                                        [("Green and red", false), ("Blue and orange", true)],
                                        d.settings.blue_orange,
                                        |s, v| s.blue_orange = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Times",
                                "When a message was said, and what Ghost says of times.",
                                |d, _, cx| {
                                    Self::pick(
                                        "clock",
                                        [("24-hour", false), ("12-hour", true)],
                                        d.settings.twelve,
                                        |s, v| s.twelve = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "A diff's long lines",
                                "Wrapped, they show whole; kept to a line, they are cut at the edge.",
                                |d, _, cx| {
                                    Self::pick(
                                        "diff-wrap",
                                        [("Wrap", true), ("Cut", false)],
                                        d.settings.diff_wrap,
                                        |s, v| s.diff_wrap = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Changes only in spacing",
                                "Left out, a diff shows what changed in the words alone.",
                                |d, _, cx| {
                                    Self::pick(
                                        "spacing",
                                        [("Shown", false), ("Left out", true)],
                                        d.settings.no_spacing,
                                        |s, v| s.no_spacing = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "A file's diff opens",
                                "Side by side widens the column beside the thread.",
                                |d, _, cx| {
                                    Self::pick(
                                        "diff",
                                        [("Stacked", false), ("Side by side", true)],
                                        d.settings.side_by_side,
                                        |s, v| s.side_by_side = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "A diff's files",
                                "Folded, each shows its name and counts until opened.",
                                |d, _, cx| {
                                    Self::pick(
                                        "diff-folded",
                                        [("Open", false), ("Folded", true)],
                                        d.settings.diff_folded,
                                        |s, v| s.diff_folded = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                        ],
                    ),
                    self.themes_group(w, cx),
                ],
            ),
            Tab::Chat => (
                "What you write, and what the agent did.",
                vec![
                    group(
                        "While a turn goes on",
                        vec![item(
                            w,
                            "What you send waits, or steers",
                            "Steer stops the turn and says it now; waiting says it once the turn ends.",
                            |d, _, cx| {
                                Self::pick(
                                    "steer",
                                    [("Wait its turn", false), ("Steer", true)],
                                    d.settings.steer,
                                    |s, v| s.steer = v,
                                    cx,
                                )
                                .into_any_element()
                            },
                        )],
                    ),
                    group(
                        "Sending",
                        vec![item(
                            w,
                            "What sends what you write",
                            "Where Return does not send, it breaks the line. The other key (⌘Return, or ⇧⌘Return) begins another session beside it, or while a turn goes on does the opposite of the setting above.",
                            |d, _, cx| {
                                Self::pick(
                                    "send",
                                    [
                                        ("Return", crate::settings::SendKey::Return),
                                        (
                                            "⌘Return if multiline",
                                            crate::settings::SendKey::Multiline,
                                        ),
                                        ("⌘Return", crate::settings::SendKey::Command),
                                    ],
                                    d.settings.send,
                                    |s, v| s.send = v,
                                    cx,
                                )
                                .into_any_element()
                            },
                        )],
                    ),
                    Self::dictation_group(w),
                    group(
                        "The agent's work",
                        vec![
                            item(
                                w,
                                "Transcript view (⌃O)",
                                "Answers shows what you said and its answers; quiet leaves out the calls that went well; normal folds each run of calls into a line; thinking shows its reasoning too; verbose shows every call and step.",
                                |d, _, cx| {
                                    Self::pick(
                                        "view",
                                        [
                                            ("Answers", crate::settings::View::Zen),
                                            ("Quiet", crate::settings::View::Quiet),
                                            ("Normal", crate::settings::View::Normal),
                                            ("Thinking", crate::settings::View::Thinking),
                                            ("Verbose", crate::settings::View::Verbose),
                                        ],
                                        d.settings.view,
                                        |s, v| s.view = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "An answer streams in",
                                "A paragraph at a time holds each back until it is whole.",
                                |d, _, cx| {
                                    Self::pick(
                                        "paragraphs",
                                        [("Word by word", false), ("A paragraph at a time", true)],
                                        d.settings.paragraphs,
                                        |s, v| s.paragraphs = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                        ],
                    ),
                ],
            ),
            Tab::NewSessions => (
                "How a session begins in a repository.",
                vec![
                    Self::scope_group(w),
                    group(
                        "In a repository",
                        vec![
                            item(
                                w,
                                "A new session works",
                                "A project can choose otherwise from its menu.",
                                |d, _, cx| {
                                    Self::pick(
                                        "where",
                                        [("In the folder", false), ("In a new worktree", true)],
                                        d.settings.worktree,
                                        |s, v| s.worktree = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Worktree branches begin with",
                                "Then the name the writer gives the work, as in ghost/retry-s3-uploads.",
                                |d, faces, _| {
                                    div()
                                        .w(px(160.))
                                        .font_family(faces.mono.clone())
                                        .child(Input::new(&d.pages.prefix).small())
                                        .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Or they begin with",
                                "The kind of work, as the writer sees it: feat/, fix/, docs/.",
                                |d, _, cx| {
                                    d.own(
                                        "branch-kind",
                                        [("The prefix", false), ("The kind of work", true)],
                                        |s| s.branch_kind,
                                        |s, v| s.branch_kind = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "Worktree location",
                                format!(
                                    "New worktrees are made in {}, on this Mac. Those made before stay where they are.",
                                    Desk::shown(
                                        &self.settings.worktree_folders(
                                            self.transport.home_there(&self.home)
                                        )[0]
                                    )
                                ),
                                |d, _, cx| d.worktree_location(cx).into_any_element(),
                            ),
                            item(
                                w,
                                "Submodules",
                                "How far a new worktree's submodules are checked out. Where there are many, the top level alone, or none for a setup script to do.",
                                |d, _, cx| {
                                    d.own(
                                        "submodules",
                                        [
                                            ("Recursive", Submodules::Recursive),
                                            ("Top level", Submodules::Top),
                                            ("Skip", Submodules::Skip),
                                        ],
                                        |s| s.submodules,
                                        |s, v| s.submodules = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                "An archived session's worktree is taken away after",
                                "Its branch stays, and so does a worktree with changes not committed; opened again, the session checks it out again.",
                                |d, _, cx| {
                                    d.own(
                                        "tidy-after",
                                        [
                                            ("A day", Some(1)),
                                            ("7 days", Some(7)),
                                            ("30 days", Some(30)),
                                            ("Never", None),
                                        ],
                                        |s| s.tidy_after,
                                        |s, v| s.tidy_after = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                        ],
                    ),
                ],
            ),
            Tab::Keyboard => return Some(keyboard(w)),
            Tab::Writing => return Some(Self::writing_tab(w)),
            Tab::About => return Some(self.about_ghost(w)),
            Tab::SourceControl => return Some(self.source_control(w)),
            _ => return None,
        };
        Some(
            SettingPage::new(tab.title())
                .description(lede)
                .groups(groups),
        )
    }

    /// A choice among `choices`, the one equal to `now` lit; picking one
    /// sets it with `set` and keeps the settings.
    pub(super) fn pick<T: Copy + PartialEq + 'static, const N: usize>(
        id: &str,
        choices: [(impl Into<SharedString>, T); N],
        now: T,
        set: fn(&mut crate::settings::Settings, T),
        cx: &mut Context<Self>,
    ) -> ButtonGroup {
        let choices = choices
            .into_iter()
            .map(|(label, value)| {
                let pick: Pick = Box::new(cx.listener(move |this, _, _, cx| {
                    set(&mut this.settings, value);
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                }));
                (label, value == now, pick)
            })
            .collect();
        ui::segmented(id, choices)
    }

    /// Where new worktrees go: a folder chosen, or Ghost's own again.
    fn worktree_location(&self, cx: &mut Context<Self>) -> Div {
        fn keep(d: &mut Desk, at: Option<std::path::PathBuf>) {
            d.settings.worktrees = at;
            d.save_settings();
        }
        div()
            .flex()
            .gap(px(6.))
            .when(self.settings.worktrees.is_some(), |d| {
                d.child(
                    ui::button("worktrees-own", "Ghost's own", Weight::Quiet).on_click(
                        cx.listener(move |this, _, _, cx| {
                            keep(this, None);
                            cx.notify();
                        }),
                    ),
                )
            })
            .child(
                ui::button("worktrees-choose", "Choose…", Weight::Secondary).on_click(cx.listener(
                    move |_, _, _, cx| {
                        Desk::ask_folder("Make worktrees here", |d, f| keep(d, Some(f)), cx);
                    },
                )),
            )
    }

    /// The editors here to open files in, and macOS's own choice.
    fn editor_pick(&self, cx: &mut Context<Self>) -> ButtonGroup {
        let mut choices: Vec<(&'static str, bool, Pick)> = vec![(
            "First found",
            self.settings.editor.is_none(),
            Box::new(cx.listener(|this, _, _, cx| {
                this.settings.editor = None;
                this.save_settings();
                this.settings_took(cx);
                cx.notify();
            })),
        )];
        for (command, name) in crate::desk::EDITORS {
            if self.on_path.contains(&command) {
                choices.push((
                    name,
                    self.settings.editor.as_deref() == Some(command),
                    Box::new(cx.listener(move |this, _, _, cx| {
                        this.settings.editor = Some(command.to_owned());
                        this.save_settings();
                        this.settings_took(cx);
                        cx.notify();
                    })),
                ));
            }
        }
        ui::segmented("editor", choices)
    }

    /// What Ghost is, and where it keeps what it keeps.
    fn about_ghost(&self, w: &WeakEntity<Desk>) -> SettingPage {
        let rows = vec![
            item(
                w,
                "The ghost command",
                "ghost <folder> in a terminal opens the folder here as a project. It goes in ~/.local/bin.",
                |_, _, cx| {
                    ui::button("install-command", "Add", Weight::Quiet)
                        .on_click(cx.listener(|this, _, _, _| this.install_command()))
                        .into_any_element()
                },
            ),
            item(
                w,
                "Where Ghost keeps its files",
                Desk::shown(&self.home),
                |d, _, _| {
                    let home = d.home.clone();
                    ui::button("show-home", "Show", Weight::Quiet)
                        .on_click(move |_, _, cx| cx.open_with_system(&home))
                        .into_any_element()
                },
            ),
            item(
                w,
                "Ghost's log",
                "A file a day in logs/, kept as long as Keep logs says: what to send with a bug.",
                |d, _, _| {
                    let logs = d.home.join("logs");
                    ui::button("show-logs", "Show", Weight::Quiet)
                        .on_click(move |_, _, cx| cx.open_with_system(&logs))
                        .into_any_element()
                },
            ),
            item(
                w,
                "Keep logs",
                "The days of log kept; older files go as Ghost starts.",
                |d, _, cx| {
                    Self::pick(
                        "log-days",
                        [("3 days", 3), ("14 days", 14), ("30 days", 30)],
                        d.settings.log_days,
                        |s, v| s.log_days = v,
                        cx,
                    )
                    .into_any_element()
                },
            ),
            item(
                w,
                "The agents come from",
                "The ACP registry, read again every six hours.",
                |_, _, cx| {
                    ui::button("open-registry", "Open", Weight::Quiet)
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.open_url("https://agentclientprotocol.com/registry");
                        }))
                        .into_any_element()
                },
            ),
        ];
        SettingPage::new(super::Tab::About.title())
            .description(format!(
                "Ghost {} · Agent Client Protocol v1 · macOS",
                env!("CARGO_PKG_VERSION")
            ))
            .group(SettingGroup::new().items(rows))
            .group(self.updates_group(w))
    }
}

/// Whether what a session did, `name` (`about` saying more), is told: the
/// switch for it in `settings.notify_on`.
fn told(
    w: &WeakEntity<Desk>,
    (name, about): (&'static str, &str),
    of: fn(&mut crate::settings::NotifyOn) -> &mut bool,
) -> SettingItem {
    item(w, name, about, move |d, _, cx| {
        let mut on = d.settings.notify_on.clone();
        Switch::new(SharedString::from(name))
            .small()
            .checked(*of(&mut on))
            .disabled(d.settings.notify == Notify::Off)
            .on_click(cx.listener(move |this, _, _, cx| {
                let told = of(&mut this.settings.notify_on);
                *told = !*told;
                this.save_settings();
                cx.notify();
            }))
            .into_any_element()
    })
}

/// Whether Ghost asks before `name` (`about` saying more): the switch for
/// it in `settings.confirm`.
fn asks(
    w: &WeakEntity<Desk>,
    (name, about): (&'static str, &str),
    of: fn(&mut crate::settings::Confirm) -> &mut bool,
) -> SettingItem {
    item(w, name, about, move |d, _, cx| {
        let mut confirm = d.settings.confirm.clone();
        Switch::new(SharedString::from(format!("confirm-{name}")))
            .small()
            .checked(*of(&mut confirm))
            .on_click(cx.listener(move |this, _, _, cx| {
                let asks = of(&mut this.settings.confirm);
                *asks = !*asks;
                this.save_settings();
                cx.notify();
            }))
            .into_any_element()
    })
}

/// The keys as they are now, yours over the window's, a setting each
/// (found by what it does or its keys), and the file yours are kept in.
fn keyboard(w: &WeakEntity<Desk>) -> SettingPage {
    let grouped = |name: &str| keys::GROUPS.iter().position(|(_, g)| g.contains(&name));
    let mut groups: Vec<(&str, Vec<SettingItem>)> = keys::GROUPS
        .iter()
        .map(|(title, _)| (*title, Vec::new()))
        .chain(std::iter::once(("Other", Vec::new())))
        .collect();
    for (name, does, _) in keys::ACTIONS {
        let at = grouped(name).unwrap_or(keys::GROUPS.len());
        let w = w.clone();
        groups[at].1.push(
            SettingItem::new(
                does,
                SettingField::render(move |_, _, cx| {
                    w.update(cx, |d, cx| d.keys_of(name, cx).into_any_element())
                        .unwrap_or_else(|_| div().into_any_element())
                }),
            )
            .keywords([name]),
        );
    }
    let file = w.clone();
    let file = SettingItem::render(move |_, _, cx| {
        file.update(cx, |d, cx| {
            let faces = crate::fonts::faces(cx).clone();
            let path = d.home.join("keybindings.json");
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(ui::fine(
                    &faces,
                    "A rule is { \"key\": \"cmd-shift-n\", \"action\": \"new_session\" }, with \"when\": \"Terminal\" (or \"Composer\", with !, && and ||) for where it holds, \"action\": \"none\" to take a key away, and \"-new_session\" to take it from that action alone; \"script:dev\" runs the script dev. The last rule that holds wins. Ghost reads the file as it starts.",
                ))
                .child(div().flex().child(
                    ui::button("open-keys", "Open keybindings.json", Weight::Secondary).on_click(
                        cx.listener(move |this, _, _, _| {
                            this.edit(path.clone(), Some(b"[]\n"));
                        }),
                    ),
                ))
                .into_any_element()
        })
        .unwrap_or_else(|_| div().into_any_element())
    })
    .keywords(["keybindings.json"]);
    SettingPage::new(super::Tab::Keyboard.title())
        .description("The keys as they are now, yours over Ghost's.")
        .groups(
            groups
                .into_iter()
                .filter(|(_, items)| !items.is_empty())
                .map(|(title, items)| group(title, items)),
        )
        .group(SettingGroup::new().item(file))
}

impl Desk {
    /// The keys of the action `name`: a click records new ones, the minus
    /// takes them away, the arrow gives it back Ghost's.
    fn keys_of(&self, name: &'static str, cx: &mut Context<Self>) -> Div {
        let faces = crate::fonts::faces(cx).clone();
        let bound = keys::keys_of(cx, name);
        let os = cx.platform_info().os;
        let unparsed: Vec<String> = bound.iter().map(|k| k.unparse(os)).collect();
        let off = (!bound.is_empty()).then(|| {
            ui::icon_button(
                SharedString::from(format!("unassign-{name}")),
                "minus",
                "Take its keys away",
            )
            .size(px(22.))
            .text_color(theme::faint())
            .on_click(cx.listener(move |this, _, _, _| {
                this.set_keys(name, keys::Keys::Off(unparsed.clone()));
            }))
        });
        let keys = div()
            .flex()
            .gap(px(4.))
            .children(bound.into_iter().map(Kbd::new));
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .id(SharedString::from(format!("record-{name}")))
                    .cursor_pointer()
                    .rounded(px(6.))
                    .px(px(4.))
                    .hover(|st| st.bg(theme::hover()))
                    .child(if self.pages.recording == Some(name) {
                        ui::text(&faces, "Press the keys…  Escape to keep these")
                            .text_size(px(crate::style::CAPTION))
                            .text_color(theme::link())
                            .into_any_element()
                    } else {
                        keys.into_any_element()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pages.recording = Some(name);
                        cx.notify();
                    })),
            )
            .children(off)
            .child(
                ui::icon_button(
                    SharedString::from(format!("unbind-{name}")),
                    "retry",
                    "Back to Ghost's keys",
                )
                .size(px(22.))
                .text_color(theme::faint())
                .on_click(cx.listener(move |this, _, _, _| {
                    this.set_keys(name, keys::Keys::Ghosts);
                })),
            )
    }
}

/// A setting as gpui-kit's Settings draws one: what it is, said plainly
/// (and found by its search), and its control, which the desk, `w`, draws.
pub(super) fn item(
    w: &WeakEntity<Desk>,
    name: impl Into<SharedString>,
    about: impl Into<SharedString>,
    control: impl Fn(&mut Desk, &Faces, &mut Context<Desk>) -> AnyElement + 'static,
) -> SettingItem {
    let w = w.clone();
    let about: SharedString = about.into();
    let item = SettingItem::new(
        name,
        SettingField::render(move |_, _, cx| {
            w.update(cx, |d, cx| {
                let faces = crate::fonts::faces(cx).clone();
                control(d, &faces, cx)
            })
            .unwrap_or_else(|_| div().into_any_element())
        }),
    );
    // One with nothing more to say has no empty line under its name.
    if about.is_empty() {
        item
    } else {
        item.description(about)
    }
}

/// What the desk, `w`, draws whole as one setting, found by `words`.
pub(super) fn whole(
    w: &WeakEntity<Desk>,
    words: &[&'static str],
    draw: impl Fn(&mut Desk, &Faces, &mut Context<Desk>) -> AnyElement + 'static,
) -> SettingItem {
    let w = w.clone();
    SettingItem::render(move |_, _, cx| {
        w.update(cx, |d, cx| {
            let faces = crate::fonts::faces(cx).clone();
            draw(d, &faces, cx)
        })
        .unwrap_or_else(|_| div().into_any_element())
    })
    .keywords(words.iter().copied())
}

/// Settings under a caption.
pub(super) fn group(caption: &str, items: Vec<SettingItem>) -> SettingGroup {
    SettingGroup::new().title(caption.to_owned()).items(items)
}
