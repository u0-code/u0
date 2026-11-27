//! The settings that are u0's own, a tab each: how it tells you things
//! and what it does while you are away; how the thread reads; what you
//! write while a turn goes on; how new sessions begin; the keys; and what
//! u0 is. Each row says what happens, and its control says the choice.

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
    /// The page of the tab `tab`, when it is one of u0's own: its
    /// settings, each drawn by `w`, the desk.
    pub(super) fn own_tab(
        &self,
        tab: super::Tab,
        w: &WeakEntity<Desk>,
        cx: &App,
    ) -> Option<SettingPage> {
        use super::Tab;
        // With a project chosen atop a page, only what a project may hold
        // can be changed: the rest holds everywhere, and is shown so.
        let scoped = self.pages.scope.is_some();
        let (lede, groups) = match tab {
            Tab::General => (
                "How u0 tells you things, and what it does while you are away.",
                vec![
                    Self::scope_group(w),
                    group(
                        crate::tr("While you are away"),
                        vec![
                            item(
                                w,
                                crate::tr("When a thread ends a turn or needs you"),
                                crate::tr("From macOS while u0 is behind, clicked to go to the thread; in u0 while another is on screen."),
                                |d, _, cx| {
                                    Self::pick("notify",
                                        [
                                            (crate::tr("Off"), Notify::Off),
                                            (crate::tr("Notify"), Notify::Banner),
                                            (crate::tr("Notify and sound"), Notify::BannerAndSound),
                                        ],
                                        d.settings.notify,
                                        |s, v| s.notify = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ).disabled(scoped),
                            told(w, ("A turn finished", "Its answer is in."), |n| {
                                &mut n.finished
                            }).disabled(scoped),
                            told(
                                w,
                                (
                                    "A turn failed or was stopped",
                                    "It ended in an error, or you stopped it.",
                                ),
                                |n| &mut n.failed,
                            ).disabled(scoped),
                            told(
                                w,
                                (
                                    "An agent asks before it goes on",
                                    "A permission or a question waits on you.",
                                ),
                                |n| &mut n.asks,
                            ).disabled(scoped),
                            item(
                                w,
                                crate::tr("Keep the Mac awake while an agent works"),
                                crate::tr("The display may sleep; a turn going on is not cut short by the Mac sleeping."),
                                |d, _, cx| {
                                    Switch::new("awake")
                                        .accessibility_label(crate::tr("Keep the Mac awake"))
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
                            ).disabled(scoped),
                        ],
                    ),
                    group(
                        crate::tr("Files"),
                        vec![item(
                            w,
                            crate::tr("Files open in"),
                            crate::tr("A file the agent named, an edit, or keybindings.json."),
                            |d, _, cx| d.editor_pick(cx).into_any_element(),
                        ).disabled(scoped)],
                    ),
                    group(
                        crate::tr("Pull requests"),
                        vec![item(
                            w,
                            crate::tr("Merged as"),
                            crate::tr("How Merge beside a thread, and merging once its checks pass, merge a pull request; the PRs page offers each."),
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
                        crate::tr("Done for now"),
                        vec![
                            item(
                                w,
                                crate::tr("A thread is done after it is left for"),
                                crate::tr("It moves to Settled in the sidebar; anything said to it brings it back."),
                                |d, _, cx| {
                                    Self::pick("settle_days",
                                        [
                                            (crate::tr("3 days"), Some(3)),
                                            (crate::tr("7 days"), Some(7)),
                                            (crate::tr("14 days"), Some(14)),
                                            (crate::tr("30 days"), Some(30)),
                                            (crate::tr("Never"), None),
                                        ],
                                        d.settings.settle_days,
                                        |s, v| s.settle_days = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ).disabled(scoped),
                            item(
                                w,
                                crate::tr("When its pull request merges or closes"),
                                crate::tr("The thread is done then, while it waits on nothing."),
                                |d, _, cx| {
                                    Self::pick("settle_merged",
                                        [(crate::tr("Settle it"), true), (crate::tr("Leave it"), false)],
                                        d.settings.settle_merged,
                                        |s, v| s.settle_merged = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ).disabled(scoped),
                            item(
                                w,
                                crate::tr("Snooze for"),
                                crate::tr("The ways to snooze offered first, each as you would type it in Custom…: 1h, 5pm, tomorrow, next week. Commas between."),
                                |d, _, _| {
                                    div()
                                        .w(px(220.))
                                        .child(Input::new(&d.pages.snoozes).small())
                                        .into_any_element()
                                },
                            ).disabled(scoped),
                            item(
                                w,
                                crate::tr("Keep threads with a pull request"),
                                crate::tr("One whose pull request is open is never done for being left."),
                                |d, _, cx| {
                                    Self::pick("keep_with_pr",
                                        [(crate::tr("Keep them"), true), (crate::tr("Let them settle"), false)],
                                        d.settings.keep_with_pr,
                                        |s, v| s.keep_with_pr = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ).disabled(scoped),
                        ],
                    ),
                    group(
                        crate::tr("In the background"),
                        vec![
                            item(
                                w,
                                crate::tr("Go on when a limit resets"),
                                crate::tr("A turn an agent's usage limit stopped is said to go on, as a scheduled task, at the time the agent gives."),
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
                                crate::tr("Fetch each project's remote every"),
                                crate::tr("So branches and pull requests are up to date; projects set to pull are moved on too."),
                                |d, _, cx| {
                                    Self::pick("fetch_every",
                                        [
                                            (crate::tr("Minute"), Some(1)),
                                            (crate::tr("5 minutes"), Some(5)),
                                            (crate::tr("15 minutes"), Some(15)),
                                            (crate::tr("Never"), None),
                                        ],
                                        d.settings.fetch_every,
                                        |s, v| s.fetch_every = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ).disabled(scoped),
                            item(
                                w,
                                crate::tr("Look for agents' new versions"),
                                crate::tr("The ACP registry is read again each hour; an agent that stops working is updated to the newest."),
                                |d, _, cx| {
                                    Self::pick("check_agents",
                                        [(crate::tr("Each hour"), true), (crate::tr("Never"), false)],
                                        d.settings.check_agents,
                                        |s, v| s.check_agents = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ).disabled(scoped),
                        ],
                    ),
                    group(
                        crate::tr("Confirmations"),
                        vec![
                            asks(w, ("Unpin", "Ask before a thread is unpinned."), |c| {
                                &mut c.unpin
                            }),
                            asks(w, ("Archive", "Ask before a thread is archived."), |c| {
                                &mut c.archive
                            }),
                            asks(
                                w,
                                (
                                    "Delete",
                                    "Ask before a thread and what was said in it are deleted.",
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
                                crate::tr("Quit with ⌘Q"),
                                crate::tr("Quit in the menu always quits at once."),
                                |d, _, cx| {
                                    Self::pick("confirm-quit",
                                        [
                                            (crate::tr("Hold"), Quit::Hold),
                                            (crate::tr("Double press"), Quit::Double),
                                            (crate::tr("Direct"), Quit::Direct),
                                        ],
                                        d.settings.confirm.quit,
                                        |s, v| s.confirm.quit = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ).disabled(scoped),
                        ],
                    ),
                ],
            ),
            Tab::Appearance => (
                "How the thread and what is beside it read.",
                vec![
                    group(
                        crate::tr("Reading"),
                        vec![
                            item(
                                w,
                                crate::tr("The sidebar sorts by"),
                                crate::tr("A project's threads, and the projects: your last message, anything happening, when each began, or name. Pinned ones stay on top."),
                                |d, _, cx| {
                                    Self::pick("sort",
                                        [
                                            (crate::tr("Your message"), crate::settings::Sort::Used),
                                            (crate::tr("Activity"), crate::settings::Sort::Updated),
                                            (crate::tr("Begun"), crate::settings::Sort::Begun),
                                            (crate::tr("Name"), crate::settings::Sort::Name),
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
                                crate::tr("Look"),
                                crate::tr("Light and dark keep the same amber for what is going on now."),
                                |d, _, cx| {
                                    Self::pick("look",
                                        [
                                            (crate::tr("Dark"), crate::settings::Look::Dark),
                                            (crate::tr("Light"), crate::settings::Look::Light),
                                            (crate::tr("As the system"), crate::settings::Look::System),
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
                                crate::tr("Size"),
                                crate::tr("Everything in the window, larger or smaller: ⌘+, ⌘− and ⌘0."),
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
                                crate::tr("Contrast"),
                                crate::tr("More draws the greys of what matters less, and the edges, stronger."),
                                |d, _, cx| {
                                    Self::pick("contrast",
                                        [(crate::tr("As designed"), false), (crate::tr("More"), true)],
                                        d.settings.contrast,
                                        |s, v| s.contrast = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                crate::tr("Glass"),
                                crate::tr("The window's grounds let the desktop show through, blurred."),
                                |d, _, cx| {
                                    Self::pick("glass",
                                        [(crate::tr("Solid"), false), (crate::tr("Glass"), true)],
                                        d.settings.glass,
                                        |s, v| s.glass = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                crate::tr("Code font"),
                                crate::tr("What code, commands and numbers are set in."),
                                |d, _, cx| {
                                    Self::pick("mono",
                                        [
                                            (crate::tr("JetBrains Mono"),
                                                crate::settings::CodeFont::JetBrains,
                                            ),
                                            (crate::tr("SF Mono"), crate::settings::CodeFont::SfMono),
                                            (crate::tr("Menlo"), crate::settings::CodeFont::Menlo),
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
                                crate::tr("Terminal text"),
                                crate::tr("How large your terminals' text is, in the code font."),
                                |d, _, cx| {
                                    Self::pick("terminal_size",
                                        [
                                            (crate::tr("Small"), crate::settings::TermSize::Small),
                                            (crate::tr("Medium"), crate::settings::TermSize::Medium),
                                            (crate::tr("Large"), crate::settings::TermSize::Large),
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
                                crate::tr("Terminal shell"),
                                crate::tr("What a new terminal runs: your login shell, or one named."),
                                |d, _, cx| {
                                    Self::pick("shell",
                                        [
                                            (crate::tr("Login shell"), crate::settings::Shell::Login),
                                            (crate::tr("zsh"), crate::settings::Shell::Zsh),
                                            (crate::tr("bash"), crate::settings::Shell::Bash),
                                            (crate::tr("fish"), crate::settings::Shell::Fish),
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
                                crate::tr("Reading width"),
                                crate::tr("How wide the thread and the composer are."),
                                |d, _, cx| {
                                    Self::pick("width",
                                        [
                                            (crate::tr("Comfortable"), Width::Comfortable),
                                            (crate::tr("Wide"), Width::Wide),
                                            (crate::tr("Full"), Width::Full),
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
                                crate::tr("What a change added and took away"),
                                crate::tr("Blue and orange tell them apart without green and red."),
                                |d, _, cx| {
                                    Self::pick("blue_orange",
                                        [(crate::tr("Green and red"), false), (crate::tr("Blue and orange"), true)],
                                        d.settings.blue_orange,
                                        |s, v| s.blue_orange = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                crate::tr("Times"),
                                crate::tr("When a message was said, and what u0 says of times."),
                                |d, _, cx| {
                                    Self::pick("twelve",
                                        [(crate::tr("24-hour"), false), (crate::tr("12-hour"), true)],
                                        d.settings.twelve,
                                        |s, v| s.twelve = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                crate::tr("A diff's long lines"),
                                crate::tr("Wrapped, they show whole; kept to a line, they are cut at the edge."),
                                |d, _, cx| {
                                    Self::pick("diff_wrap",
                                        [(crate::tr("Wrap"), true), (crate::tr("Cut"), false)],
                                        d.settings.diff_wrap,
                                        |s, v| s.diff_wrap = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                crate::tr("Changes only in spacing"),
                                crate::tr("Left out, a diff shows what changed in the words alone."),
                                |d, _, cx| {
                                    Self::pick("no_spacing",
                                        [(crate::tr("Shown"), false), (crate::tr("Left out"), true)],
                                        d.settings.no_spacing,
                                        |s, v| s.no_spacing = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                crate::tr("A file's diff opens"),
                                crate::tr("Side by side widens the column beside the thread."),
                                |d, _, cx| {
                                    Self::pick("side_by_side",
                                        [(crate::tr("Stacked"), false), (crate::tr("Side by side"), true)],
                                        d.settings.side_by_side,
                                        |s, v| s.side_by_side = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            item(
                                w,
                                crate::tr("A diff's files"),
                                crate::tr("Folded, each shows its name and counts until opened."),
                                |d, _, cx| {
                                    Self::pick("diff_folded",
                                        [(crate::tr("Open"), false), (crate::tr("Folded"), true)],
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
                        crate::tr("While a turn goes on"),
                        vec![item(
                            w,
                            crate::tr("What you send waits, or steers"),
                            crate::tr("Steer stops the turn and says it now; waiting says it once the turn ends."),
                            |d, _, cx| {
                                Self::pick("steer",
                                    [(crate::tr("Wait its turn"), false), (crate::tr("Steer"), true)],
                                    d.settings.steer,
                                    |s, v| s.steer = v,
                                    cx,
                                )
                                .into_any_element()
                            },
                        )],
                    ),
                    group(
                        crate::tr("Spelling"),
                        vec![item(
                            w,
                            crate::tr("Check spelling as you type"),
                            crate::tr("Words the system's spell checker does not know are underlined; right-click one for its guesses."),
                            |d, _, cx| {
                                Switch::new("spell-check")
                                    .accessibility_label(crate::tr("Check spelling as you type"))
                                    .small()
                                    .checked(d.settings.spell_check)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.spell_check = !this.settings.spell_check;
                                        this.save_settings();
                                        this.sync_spelling(cx);
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            },
                        )],
                    ),
                    group(
                        crate::tr("Sending"),
                        vec![item(
                            w,
                            crate::tr("What sends what you write"),
                            crate::tr("Where Return does not send, it breaks the line. The other key (⌘Return, or ⇧⌘Return) begins another thread beside it, or while a turn goes on does the opposite of the setting above."),
                            |d, _, cx| {
                                Self::pick("send",
                                    [
                                        (crate::tr("Return"), crate::settings::SendKey::Return),
                                        (crate::tr("⌘Return if multiline"),
                                            crate::settings::SendKey::Multiline,
                                        ),
                                        (crate::tr("⌘Return"), crate::settings::SendKey::Command),
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
                        crate::tr("The agent's work"),
                        vec![
                            item(
                                w,
                                crate::tr("Transcript view (⌃O)"),
                                crate::tr("Answers shows what you said and its answers; quiet leaves out the calls that went well; normal folds each run of calls into a line; thinking shows its reasoning too; verbose shows every call and step."),
                                |d, _, cx| {
                                    Self::pick("view",
                                        [
                                            (crate::tr("Answers"), crate::settings::View::Zen),
                                            (crate::tr("Quiet"), crate::settings::View::Quiet),
                                            (crate::tr("Normal"), crate::settings::View::Normal),
                                            (crate::tr("Thinking"), crate::settings::View::Thinking),
                                            (crate::tr("Verbose"), crate::settings::View::Verbose),
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
                                crate::tr("An answer streams in"),
                                crate::tr("A paragraph at a time holds each back until it is whole."),
                                |d, _, cx| {
                                    Self::pick("paragraphs",
                                        [(crate::tr("Word by word"), false), (crate::tr("A paragraph at a time"), true)],
                                        d.settings.paragraphs,
                                        |s, v| s.paragraphs = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                            Self::window_language(w),
                            item(
                                w,
                                crate::tr("Math"),
                                crate::tr("TeX in what an agent says, $…$ and $$…$$, set as formulas."),
                                |d, _, cx| {
                                    Self::pick("math",
                                        [(crate::tr("As written"), false), (crate::tr("Set as formulas"), true)],
                                        d.settings.math,
                                        |s, v| s.math = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ),
                        ],
                    ),
                ],
            ),
            Tab::NewThreads => (
                "How a thread begins in a repository.",
                vec![
                    Self::scope_group(w),
                    group(
                        crate::tr("In a repository"),
                        vec![
                            item(
                                w,
                                crate::tr("A new thread works"),
                                crate::tr("A project can choose otherwise from its menu."),
                                |d, _, cx| {
                                    Self::pick("worktree",
                                        [(crate::tr("In the folder"), false), (crate::tr("In a new worktree"), true)],
                                        d.settings.worktree,
                                        |s, v| s.worktree = v,
                                        cx,
                                    )
                                    .into_any_element()
                                },
                            ).disabled(scoped),
                            item(
                                w,
                                crate::tr("Worktree branches begin with"),
                                crate::tr("Then the name the writer gives the work, as in u0/retry-s3-uploads."),
                                |d, faces, _| {
                                    div()
                                        .w(px(160.))
                                        .font_family(faces.mono.clone())
                                        .child(Input::new(&d.pages.prefix).small())
                                        .into_any_element()
                                },
                            ).disabled(scoped),
                            item(
                                w,
                                crate::tr("Or they begin with"),
                                crate::tr("The kind of work, as the writer sees it: feat/, fix/, docs/."),
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
                                crate::tr("Worktree location"),
                                crate::tr_with(
                                    "New worktrees are made in %{folder}. Those made before stay where they are.",
                                    &[("folder", &Desk::shown(&self.worktree_folders(crate::hub::Host::HERE)[0]))],
                                ),
                                |d, _, cx| d.worktree_location(cx).into_any_element(),
                            ).disabled(scoped),
                            item(
                                w,
                                crate::tr("Submodules"),
                                crate::tr("How far a new worktree's submodules are checked out. Where there are many, the top level alone, or none for a setup script to do."),
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
                                crate::tr("An archived thread's worktree is taken away after"),
                                crate::tr("Its branch stays, and so does a worktree with changes not committed; opened again, the thread checks it out again."),
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
            Tab::Keybindings => return Some(keyboard(w)),
            Tab::Writing => return Some(Self::writing_tab(w)),
            Tab::About => return Some(self.about_u0(w)),
            Tab::SourceControl => return Some(self.source_control(w)),
            _ => return None,
        };
        Some(
            SettingPage::new(tab.title())
                .description(crate::tr(lede))
                .groups(groups),
        )
    }

    /// A choice among `choices`, the one equal to `now` lit; picking one
    /// sets it with `set` and keeps the settings. `id` is the setting's
    /// key in settings.json (or begins with it, before a `-`): one the
    /// policy locks cannot be chosen, and says who locked it.
    /// The language of u0's window, of those it speaks: as the
    /// system's (none), or one chosen.
    fn window_language(w: &WeakEntity<Desk>) -> SettingItem {
        let mut choices = vec![(SharedString::default(), crate::tr("As the system"))];
        choices
            .extend((crate::i18n::LANGUAGES.iter()).map(|(l, name)| ((*l).into(), (*name).into())));
        let (read, write) = (w.clone(), w.clone());
        SettingItem::new(
            crate::tr("Language"),
            SettingField::scrollable_dropdown(
                choices,
                move |cx| {
                    let now = read
                        .upgrade()
                        .and_then(|d| d.read(cx).settings.language.clone());
                    now.map(SharedString::from).unwrap_or_default()
                },
                move |locale, cx| {
                    let _ = write.update(cx, |d, cx| {
                        d.settings.language = (!locale.is_empty()).then(|| locale.to_string());
                        d.save_settings();
                        d.settings_took(cx);
                        cx.notify();
                    });
                },
            ),
        )
        .description(crate::tr(
            "The words of u0's window; what agents say stays as they say it.",
        ))
    }

    pub(super) fn pick<T: Copy + PartialEq + 'static, const N: usize>(
        id: &str,
        choices: [(impl Into<SharedString>, T); N],
        now: T,
        set: fn(&mut crate::settings::Settings, T),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let choices = choices
            .into_iter()
            .map(|(label, value)| {
                let pick: Pick = Box::new(cx.listener(move |this, _, _, cx| {
                    set(&mut this.settings, value);
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                }));
                (crate::tr(&label.into()), value == now, pick)
            })
            .collect();
        let group = ui::segmented(id, choices);
        let key = id.split_once('-').map_or(id, |(key, _)| key);
        match crate::policy::locked(id).or_else(|| crate::policy::locked(key)) {
            None => group.into_any_element(),
            Some(by) => managed(group.disabled(true), by),
        }
    }

    /// Where new worktrees go: a folder chosen, or u0's own again.
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
                    ui::button("worktrees-own", crate::tr("u0's own"), Weight::Quiet).on_click(
                        cx.listener(move |this, _, _, cx| {
                            keep(this, None);
                            cx.notify();
                        }),
                    ),
                )
            })
            .child(
                ui::button("worktrees-choose", crate::tr("Choose…"), Weight::Secondary).on_click(
                    cx.listener(move |_, _, _, cx| {
                        Desk::ask_folder(
                            &crate::tr("Make worktrees here"),
                            |d, f| keep(d, Some(f)),
                            cx,
                        );
                    }),
                ),
            )
    }

    /// The editors here to open files in, and macOS's own choice.
    fn editor_pick(&self, cx: &mut Context<Self>) -> ButtonGroup {
        let mut choices: Vec<(SharedString, bool, Pick)> = vec![(
            crate::tr("First found"),
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
                    name.into(),
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

    /// What u0 is, and where it keeps what it keeps.
    fn about_u0(&self, w: &WeakEntity<Desk>) -> SettingPage {
        let rows = vec![
            item(
                w,
                crate::tr("The u0 command"),
                crate::tr(
                    "u0 <folder> in a terminal opens the folder here as a project. It goes in ~/.local/bin.",
                ),
                |_, _, cx| {
                    ui::button("install-command", crate::tr("Add"), Weight::Quiet)
                        .on_click(cx.listener(|this, _, _, _| this.install_command()))
                        .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("Where u0 keeps its files"),
                Desk::shown(&self.home),
                |d, _, _| {
                    let home = d.home.clone();
                    ui::button("show-home", crate::tr("Show"), Weight::Quiet)
                        .on_click(move |_, _, cx| cx.open_with_system(&home))
                        .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("u0's log"),
                crate::tr(
                    "A file a day in logs/, kept as long as Keep logs says: what to send with a bug.",
                ),
                |d, _, _| {
                    let logs = d.home.join("logs");
                    ui::button("show-logs", crate::tr("Show"), Weight::Quiet)
                        .on_click(move |_, _, cx| cx.open_with_system(&logs))
                        .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("Keep logs"),
                crate::tr("The days of log kept; older files go as u0 starts."),
                |d, _, cx| {
                    Self::pick(
                        "log_days",
                        [
                            (crate::tr("3 days"), 3),
                            (crate::tr("14 days"), 14),
                            (crate::tr("30 days"), 30),
                        ],
                        d.settings.log_days,
                        |s, v| s.log_days = v,
                        cx,
                    )
                    .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("The agents come from"),
                crate::tr("The ACP registry, read again each hour."),
                |_, _, cx| {
                    ui::button("open-registry", crate::tr("Open"), Weight::Quiet)
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.open_url("https://agentclientprotocol.com/registry");
                        }))
                        .into_any_element()
                },
            ),
        ];
        SettingPage::new(super::Tab::About.title())
            .description(crate::update::this_build())
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
            .accessibility_label(crate::tr(name))
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

/// Whether u0 asks before `name` (`about` saying more): the switch for
/// it in `settings.confirm`.
fn asks(
    w: &WeakEntity<Desk>,
    (name, about): (&'static str, &str),
    of: fn(&mut crate::settings::Confirm) -> &mut bool,
) -> SettingItem {
    item(w, name, about, move |d, _, cx| {
        let mut confirm = d.settings.confirm.clone();
        Switch::new(SharedString::from(format!("confirm-{name}")))
            .accessibility_label(crate::tr(name))
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
                crate::tr(does),
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
                    crate::tr("A rule is { \"key\": \"cmd-shift-n\", \"action\": \"new_session\" }, with \"when\": \"Terminal\" (or \"Composer\", with !, && and ||) for where it holds, \"action\": \"none\" to take a key away, and \"-new_session\" to take it from that action alone; \"script:dev\" runs the script dev. The last rule that holds wins. u0 reads the file as it starts."),
                ))
                .child(div().flex().child(
                    ui::button("open-keys", crate::tr("Open keybindings.json"), Weight::Secondary).on_click(
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
    SettingPage::new(super::Tab::Keybindings.title())
        .description(crate::tr("The keys as they are now, yours over u0's."))
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
    /// takes them away, the arrow gives it back u0's.
    fn keys_of(&self, name: &'static str, cx: &mut Context<Self>) -> Div {
        let faces = crate::fonts::faces(cx).clone();
        let bound = keys::keys_of(cx, name);
        let os = cx.platform_info().os;
        let unparsed: Vec<String> = bound.iter().map(|k| k.unparse(os)).collect();
        let off = (!bound.is_empty()).then(|| {
            ui::icon_button(
                SharedString::from(format!("unassign-{name}")),
                "minus",
                crate::tr("Take its keys away"),
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
                        ui::text(&faces, crate::tr("Press the keys…  Escape to keep these"))
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
                    crate::tr("Back to u0's keys"),
                )
                .size(px(22.))
                .text_color(theme::faint())
                .on_click(cx.listener(move |this, _, _, _| {
                    this.set_keys(name, keys::Keys::Defaults);
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
    // Said in your language here, whoever called.
    let about = crate::tr(&about.into());
    let item = SettingItem::new(
        crate::tr(&name.into()),
        SettingField::render(move |options, _, cx| {
            let control = w
                .update(cx, |d, cx| {
                    let faces = crate::fonts::faces(cx).clone();
                    control(d, &faces, cx)
                })
                .unwrap_or_else(|_| div().into_any_element());
            // gpui-kit dims an item disabled; the desk's own control is
            // also kept from the pointer, under a cover.
            if options.is_disabled() {
                div()
                    .relative()
                    .child(control)
                    .child(div().absolute().inset_0().occlude())
                    .into_any_element()
            } else {
                control
            }
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

/// A control the policy locks, disabled, with who manages it beside it.
pub(super) fn managed(control: impl IntoElement, by: &str) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(control)
        .child(
            div()
                .text_color(crate::theme::muted())
                .child(crate::tr_with("Managed by %{by}", &[("by", &by)])),
        )
        .into_any_element()
}

/// Settings under a caption.
pub(super) fn group(caption: impl Into<SharedString>, items: Vec<SettingItem>) -> SettingGroup {
    SettingGroup::new()
        .title(crate::tr(&caption.into()))
        .items(items)
}
