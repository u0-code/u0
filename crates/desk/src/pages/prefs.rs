//! The settings that are Ghost's own, a tab each: how it tells you things
//! and what it does while you are away; how the thread reads; what you
//! write while a turn goes on; how new sessions begin; the keys; and what
//! Ghost is. Each row says what happens, and its control says the choice.

use gpui::component::button::ButtonGroup;
use gpui::component::{Disableable as _, Sizable as _, input::Input, kbd::Kbd, switch::Switch};
use gpui::{Context, Div, IntoElement, SharedString, div, prelude::*, px};

use super::card;
use crate::{
    desk::Desk,
    fonts::{Faces, Weighted as _},
    keys,
    settings::{Notify, Quit, Submodules, Width},
    theme,
    ui::{self, Pick, Weight},
};

impl Desk {
    /// The page of the tab `tab`, when it is one of Ghost's own.
    pub(super) fn own_tab(
        &self,
        faces: &Faces,
        tab: super::Tab,
        q: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        use super::Tab;
        let (lede, groups) = match tab {
            Tab::General => (
                "How Ghost tells you things, and what it does while you are away.",
                vec![
                    group(
                        faces,
                        "While you are away",
                        vec![
                            row(
                                faces,
                                q,
                                "When a session ends a turn or needs you",
                                "From macOS while Ghost is behind, clicked to go to the session; in Ghost while another is on screen.",
                                Self::pick(
                                    "notify",
                                    [
                                        ("Off", Notify::Off),
                                        ("Notify", Notify::Banner),
                                        ("Notify and sound", Notify::BannerAndSound),
                                    ],
                                    self.settings.notify,
                                    |s, v| s.notify = v,
                                    cx,
                                ),
                            ),
                            self.told(
                                faces,
                                q,
                                ("A turn finished", "Its answer is in."),
                                |n| &mut n.finished,
                                cx,
                            ),
                            self.told(
                                faces,
                                q,
                                (
                                    "A turn failed or was stopped",
                                    "It ended in an error, or you stopped it.",
                                ),
                                |n| &mut n.failed,
                                cx,
                            ),
                            self.told(
                                faces,
                                q,
                                (
                                    "An agent asks before it goes on",
                                    "A permission or a question waits on you.",
                                ),
                                |n| &mut n.asks,
                                cx,
                            ),
                            row(
                                faces,
                                q,
                                "Keep the Mac awake while an agent works",
                                "The display may sleep; a turn going on is not cut short by the Mac sleeping.",
                                Switch::new("awake")
                                    .small()
                                    .checked(self.settings.awake)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.awake = !this.settings.awake;
                                        this.save_settings();
                                        this.stay_awake();
                                        cx.notify();
                                    })),
                            ),
                        ],
                    ),
                    group(
                        faces,
                        "Files",
                        vec![row(
                            faces,
                            q,
                            "Files open in",
                            "A file the agent named, an edit, or keybindings.json.",
                            self.editor_pick(cx),
                        )],
                    ),
                    group(
                        faces,
                        "Pull requests",
                        vec![row(
                            faces,
                            q,
                            "Merged as",
                            "How Merge beside a session, and merging once its checks pass, merge a pull request; the PRs page offers each.",
                            Self::pick(
                                "merge",
                                [
                                    ("Squash", crate::git::Method::Squash),
                                    ("Merge commit", crate::git::Method::Merge),
                                    ("Rebase", crate::git::Method::Rebase),
                                ],
                                self.settings.merge,
                                |s, v| s.merge = v,
                                cx,
                            ),
                        )],
                    ),
                    group(
                        faces,
                        "Done for now",
                        vec![
                            row(
                                faces,
                                q,
                                "A session is done after it is left for",
                                "It moves to Settled in the sidebar; anything said to it brings it back.",
                                Self::pick(
                                    "settle",
                                    [
                                        ("3 days", Some(3)),
                                        ("7 days", Some(7)),
                                        ("14 days", Some(14)),
                                        ("30 days", Some(30)),
                                        ("Never", None),
                                    ],
                                    self.settings.settle_days,
                                    |s, v| s.settle_days = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "When its pull request merges or closes",
                                "The session is done then, while it waits on nothing.",
                                Self::pick(
                                    "settle-merged",
                                    [("Settle it", true), ("Leave it", false)],
                                    self.settings.settle_merged,
                                    |s, v| s.settle_merged = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Snooze for",
                                "The ways to snooze offered first, each as you would type it in Custom…: 1h, 5pm, tomorrow, next week. Commas between.",
                                div()
                                    .w(px(220.))
                                    .child(Input::new(&self.pages.snoozes).small()),
                            ),
                            row(
                                faces,
                                q,
                                "Keep sessions with a pull request",
                                "One whose pull request is open is never done for being left.",
                                Self::pick(
                                    "keep-with-pr",
                                    [("Keep them", true), ("Let them settle", false)],
                                    self.settings.keep_with_pr,
                                    |s, v| s.keep_with_pr = v,
                                    cx,
                                ),
                            ),
                        ],
                    ),
                    group(
                        faces,
                        "In the background",
                        vec![
                            row(
                                faces,
                                q,
                                "Fetch each project's remote every",
                                "So branches and pull requests are up to date; projects set to pull are moved on too.",
                                Self::pick(
                                    "fetch-every",
                                    [
                                        ("Minute", Some(1)),
                                        ("5 minutes", Some(5)),
                                        ("15 minutes", Some(15)),
                                        ("Never", None),
                                    ],
                                    self.settings.fetch_every,
                                    |s, v| s.fetch_every = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Look for agents' new versions",
                                "The ACP registry is read again each hour; an agent that stops working is updated to the newest.",
                                Self::pick(
                                    "check-agents",
                                    [("Each hour", true), ("Never", false)],
                                    self.settings.check_agents,
                                    |s, v| s.check_agents = v,
                                    cx,
                                ),
                            ),
                        ],
                    ),
                    group(
                        faces,
                        "Confirmations",
                        vec![
                            self.asks(
                                faces,
                                q,
                                ("Unpin", "Ask before a session is unpinned."),
                                |c| &mut c.unpin,
                                cx,
                            ),
                            self.asks(
                                faces,
                                q,
                                ("Archive", "Ask before a session is archived."),
                                |c| &mut c.archive,
                                cx,
                            ),
                            self.asks(
                                faces,
                                q,
                                (
                                    "Delete",
                                    "Ask before a session and what was said in it are deleted.",
                                ),
                                |c| &mut c.delete,
                                cx,
                            ),
                            self.asks(
                                faces,
                                q,
                                (
                                    "Close a terminal",
                                    "Ask before a terminal is closed with something running in it.",
                                ),
                                |c| &mut c.terminal,
                                cx,
                            ),
                            row(
                                faces,
                                q,
                                "Quit with ⌘Q",
                                "Quit in the menu always quits at once.",
                                Self::pick(
                                    "quit",
                                    [
                                        ("Hold", Quit::Hold),
                                        ("Double press", Quit::Double),
                                        ("Direct", Quit::Direct),
                                    ],
                                    self.settings.confirm.quit,
                                    |s, v| s.confirm.quit = v,
                                    cx,
                                ),
                            ),
                        ],
                    ),
                ],
            ),
            Tab::Appearance => (
                "How the thread and what is beside it read.",
                vec![
                    group(
                        faces,
                        "Reading",
                        vec![
                            row(
                                faces,
                                q,
                                "Look",
                                "Light and dark keep the same amber for what is going on now.",
                                Self::pick(
                                    "look",
                                    [
                                        ("Dark", crate::settings::Look::Dark),
                                        ("Light", crate::settings::Look::Light),
                                        ("As the system", crate::settings::Look::System),
                                    ],
                                    self.settings.look,
                                    |s, v| s.look = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Size",
                                "Everything in the window, larger or smaller: ⌘+, ⌘− and ⌘0.",
                                Self::pick(
                                    "scale",
                                    crate::settings::SCALES.map(|s| (format!("{s}%"), s)),
                                    self.settings.scale,
                                    |s, v| s.scale = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Contrast",
                                "More draws the greys of what matters less, and the edges, stronger.",
                                Self::pick(
                                    "contrast",
                                    [("As designed", false), ("More", true)],
                                    self.settings.contrast,
                                    |s, v| s.contrast = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Glass",
                                "The window's grounds let the desktop show through, blurred.",
                                Self::pick(
                                    "glass",
                                    [("Solid", false), ("Glass", true)],
                                    self.settings.glass,
                                    |s, v| s.glass = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Code font",
                                "What code, commands and numbers are set in.",
                                Self::pick(
                                    "mono",
                                    [
                                        ("JetBrains Mono", crate::settings::CodeFont::JetBrains),
                                        ("SF Mono", crate::settings::CodeFont::SfMono),
                                        ("Menlo", crate::settings::CodeFont::Menlo),
                                    ],
                                    self.settings.mono,
                                    |s, v| s.mono = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Terminal text",
                                "How large your terminals' text is, in the code font.",
                                Self::pick(
                                    "terminal-size",
                                    [
                                        ("Small", crate::settings::TermSize::Small),
                                        ("Medium", crate::settings::TermSize::Medium),
                                        ("Large", crate::settings::TermSize::Large),
                                    ],
                                    self.settings.terminal_size,
                                    |s, v| s.terminal_size = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Terminal shell",
                                "What a new terminal runs: your login shell, or one named.",
                                Self::pick(
                                    "shell",
                                    [
                                        ("Login shell", crate::settings::Shell::Login),
                                        ("zsh", crate::settings::Shell::Zsh),
                                        ("bash", crate::settings::Shell::Bash),
                                        ("fish", crate::settings::Shell::Fish),
                                    ],
                                    self.settings.shell,
                                    |s, v| s.shell = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Reading width",
                                "How wide the thread and the composer are.",
                                Self::pick(
                                    "width",
                                    [
                                        ("Comfortable", Width::Comfortable),
                                        ("Wide", Width::Wide),
                                        ("Full", Width::Full),
                                    ],
                                    self.settings.width,
                                    |s, v| s.width = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "What a change added and took away",
                                "Blue and orange tell them apart without green and red.",
                                Self::pick(
                                    "diff-colours",
                                    [("Green and red", false), ("Blue and orange", true)],
                                    self.settings.blue_orange,
                                    |s, v| s.blue_orange = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Times",
                                "When a message was said, and what Ghost says of times.",
                                Self::pick(
                                    "clock",
                                    [("24-hour", false), ("12-hour", true)],
                                    self.settings.twelve,
                                    |s, v| s.twelve = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "A diff's long lines",
                                "Wrapped, they show whole; kept to a line, they are cut at the edge.",
                                Self::pick(
                                    "diff-wrap",
                                    [("Wrap", true), ("Cut", false)],
                                    self.settings.diff_wrap,
                                    |s, v| s.diff_wrap = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "Changes only in spacing",
                                "Left out, a diff shows what changed in the words alone.",
                                Self::pick(
                                    "spacing",
                                    [("Shown", false), ("Left out", true)],
                                    self.settings.no_spacing,
                                    |s, v| s.no_spacing = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "A file's diff opens",
                                "Side by side widens the column beside the thread.",
                                Self::pick(
                                    "diff",
                                    [("Stacked", false), ("Side by side", true)],
                                    self.settings.side_by_side,
                                    |s, v| s.side_by_side = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "A diff's files",
                                "Folded, each shows its name and counts until opened.",
                                Self::pick(
                                    "diff-folded",
                                    [("Open", false), ("Folded", true)],
                                    self.settings.diff_folded,
                                    |s, v| s.diff_folded = v,
                                    cx,
                                ),
                            ),
                        ],
                    ),
                    self.themes_group(faces, q, cx),
                ],
            ),
            Tab::Chat => (
                "What you write, and what the agent did.",
                vec![
                    group(
                        faces,
                        "While a turn goes on",
                        vec![row(
                            faces,
                            q,
                            "What you send waits, or steers",
                            "Steer stops the turn and says it now; waiting says it once the turn ends.",
                            Self::pick(
                                "steer",
                                [("Wait its turn", false), ("Steer", true)],
                                self.settings.steer,
                                |s, v| s.steer = v,
                                cx,
                            ),
                        )],
                    ),
                    group(
                        faces,
                        "Sending",
                        vec![row(
                            faces,
                            q,
                            "What sends what you write",
                            "Where Return does not send, it breaks the line. The other key (⌘Return, or ⇧⌘Return) begins another session beside it, or while a turn goes on does the opposite of the setting above.",
                            Self::pick(
                                "send",
                                [
                                    ("Return", crate::settings::SendKey::Return),
                                    ("⌘Return if multiline", crate::settings::SendKey::Multiline),
                                    ("⌘Return", crate::settings::SendKey::Command),
                                ],
                                self.settings.send,
                                |s, v| s.send = v,
                                cx,
                            ),
                        )],
                    ),
                    self.dictation_group(faces, q, cx),
                    group(
                        faces,
                        "The agent's work",
                        vec![
                            row(
                                faces,
                                q,
                                "Transcript view (⌃O)",
                                "Answers shows what you said and its answers; quiet leaves out the calls that went well; normal folds each run of calls into a line; thinking shows its reasoning too; verbose shows every call and step.",
                                Self::pick(
                                    "view",
                                    [
                                        ("Answers", crate::settings::View::Zen),
                                        ("Quiet", crate::settings::View::Quiet),
                                        ("Normal", crate::settings::View::Normal),
                                        ("Thinking", crate::settings::View::Thinking),
                                        ("Verbose", crate::settings::View::Verbose),
                                    ],
                                    self.settings.view,
                                    |s, v| s.view = v,
                                    cx,
                                ),
                            ),
                            row(
                                faces,
                                q,
                                "An answer streams in",
                                "A paragraph at a time holds each back until it is whole.",
                                Self::pick(
                                    "paragraphs",
                                    [("Word by word", false), ("A paragraph at a time", true)],
                                    self.settings.paragraphs,
                                    |s, v| s.paragraphs = v,
                                    cx,
                                ),
                            ),
                        ],
                    ),
                ],
            ),
            Tab::NewSessions => (
                "How a session begins in a repository.",
                vec![group(
                    faces,
                    "In a repository",
                    vec![
                        row(
                            faces,
                            q,
                            "A new session works",
                            "A project can choose otherwise from its menu.",
                            Self::pick(
                                "where",
                                [("In the folder", false), ("In a new worktree", true)],
                                self.settings.worktree,
                                |s, v| s.worktree = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "Worktree branches begin with",
                            "Then the name the writer gives the work, as in ghost/retry-s3-uploads.",
                            div()
                                .w(px(160.))
                                .font_family(faces.mono.clone())
                                .child(Input::new(&self.pages.prefix).small()),
                        ),
                        row(
                            faces,
                            q,
                            "Or they begin with",
                            "The kind of work, as the writer sees it: feat/, fix/, docs/.",
                            Self::pick(
                                "branch-kind",
                                [("The prefix", false), ("The kind of work", true)],
                                self.settings.branch_kind,
                                |s, v| s.branch_kind = v,
                                cx,
                            ),
                        ),
                        row(
                            faces,
                            q,
                            "Worktree location",
                            &format!(
                                "New worktrees are made in {}, on this Mac. Those made before stay where they are.",
                                Desk::shown(&self.settings.worktree_folders(&self.home)[0])
                            ),
                            self.worktree_location(cx),
                        ),
                        row(
                            faces,
                            q,
                            "Submodules",
                            "How far a new worktree's submodules are checked out. Where there are many, the top level alone, or none for a setup script to do.",
                            Self::pick(
                                "submodules",
                                [
                                    ("Recursive", Submodules::Recursive),
                                    ("Top level", Submodules::Top),
                                    ("Skip", Submodules::Skip),
                                ],
                                self.settings.submodules,
                                |s, v| s.submodules = v,
                                cx,
                            ),
                        ),
                    ],
                )],
            ),
            Tab::Keyboard => return self.keyboard(faces, q, cx),
            Tab::Writing => return self.writing_tab(faces, q, cx),
            Tab::Usage => return q.is_empty().then(|| self.usage(faces, cx)),
            Tab::About => return self.about_ghost(faces, q, cx),
            Tab::SourceControl => return self.source_control(faces, q, cx),
            Tab::Diagnostics => return self.diagnostics(faces, q, cx),
            Tab::Agents | Tab::Sessions | Tab::Projects => return None,
        };
        let groups: Vec<Div> = groups.into_iter().flatten().collect();
        (!groups.is_empty()).then(|| page(faces, tab.title(), lede).children(groups))
    }

    /// Whether what a session did, `name` (`about` saying more), is told:
    /// the switch for it in
    /// `settings.notify_on`.
    fn told(
        &self,
        faces: &Faces,
        q: &str,
        (name, about): (&'static str, &str),
        of: fn(&mut crate::settings::NotifyOn) -> &mut bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let mut on = self.settings.notify_on.clone();
        let now = *of(&mut on);
        row(
            faces,
            q,
            name,
            about,
            Switch::new(SharedString::from(name))
                .small()
                .checked(now)
                .disabled(self.settings.notify == Notify::Off)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let told = of(&mut this.settings.notify_on);
                    *told = !*told;
                    this.save_settings();
                    cx.notify();
                })),
        )
    }

    /// Whether Ghost asks before `name` (`about` saying more): the switch
    /// for it in `settings.confirm`.
    fn asks(
        &self,
        faces: &Faces,
        q: &str,
        (name, about): (&'static str, &str),
        of: fn(&mut crate::settings::Confirm) -> &mut bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let mut confirm = self.settings.confirm.clone();
        row(
            faces,
            q,
            name,
            about,
            Switch::new(SharedString::from(format!("confirm-{name}")))
                .small()
                .checked(*of(&mut confirm))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let asks = of(&mut this.settings.confirm);
                    *asks = !*asks;
                    this.save_settings();
                    cx.notify();
                })),
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

    /// The keys as they are now, yours over the window's, and the file
    /// yours are kept in.
    fn keyboard(&self, faces: &Faces, q: &str, cx: &mut Context<Self>) -> Option<Div> {
        let grouped = |name: &str| keys::GROUPS.iter().position(|(_, g)| g.contains(&name));
        let mut groups: Vec<(&str, Vec<Option<Div>>)> = keys::GROUPS
            .iter()
            .map(|(title, _)| (*title, Vec::new()))
            .chain(std::iter::once(("Other", Vec::new())))
            .collect();
        let mut n = 0;
        for (name, does, _) in keys::ACTIONS {
            let bound = keys::keys_of(cx, name);
            if !said(q, &[does, &keys::written(&bound)]) {
                continue;
            }
            n += 1;
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
            let at = grouped(name).unwrap_or(keys::GROUPS.len());
            groups[at].1.push(Some(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.))
                    .px(px(16.))
                    .child(ui::text(faces, does).flex_1().text_size(px(13.)))
                    // A click records new keys for it; the arrow gives it
                    // back Ghost's.
                    .child(
                        div()
                            .id(SharedString::from(format!("record-{name}")))
                            .cursor_pointer()
                            .rounded(px(6.))
                            .px(px(4.))
                            .hover(|st| st.bg(theme::hover()))
                            .child(if self.pages.recording == Some(name) {
                                ui::text(faces, "Press the keys…  Escape to keep these")
                                    .text_size(px(12.))
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
                    ),
            ));
        }
        let path = self.home.join("keybindings.json");
        let open = ui::button("open-keys", "Open keybindings.json", Weight::Secondary).on_click(
            cx.listener(move |this, _, _, _| {
                this.edit(path.clone(), Some(b"[]\n"));
            }),
        );
        if n == 0 {
            return None;
        }
        let page = page(
            faces,
            super::Tab::Keyboard.title(),
            "The keys as they are now, yours over Ghost's.",
        )
        .gap(px(22.))
        .children(
            groups
                .into_iter()
                .filter_map(|(title, rows)| group(faces, title, rows)),
        );
        if !q.is_empty() {
            return Some(page);
        }
        Some(page
            .child(ui::fine(
                faces,
                "A rule is { \"key\": \"cmd-shift-n\", \"action\": \"new_session\" }, with \"when\": \"Terminal\" (or \"Composer\", with !, && and ||) for where it holds, \"action\": \"none\" to take a key away, and \"-new_session\" to take it from that action alone; \"script:dev\" runs the script dev. The last rule that holds wins. Ghost reads the file as it starts.",
            ))
            .child(div().flex().child(open)))
    }

    /// What Ghost is, and where it keeps what it keeps.
    fn about_ghost(&self, faces: &Faces, q: &str, cx: &mut Context<Self>) -> Option<Div> {
        let home = self.home.clone();
        let show = ui::button("show-home", "Show", Weight::Quiet)
            .on_click(move |_, _, cx| cx.open_with_system(&home));
        let logs = self.home.join("logs");
        let show_logs = ui::button("show-logs", "Show", Weight::Quiet)
            .on_click(move |_, _, cx| cx.open_with_system(&logs));
        let registry = ui::button("open-registry", "Open", Weight::Quiet).on_click(
            cx.listener(|_, _, _, cx| cx.open_url("https://agentclientprotocol.com/registry")),
        );
        let command = ui::button("install-command", "Add", Weight::Quiet)
            .on_click(cx.listener(|this, _, _, _| this.install_command()));
        let rows: Vec<Div> = [
            row(
                faces,
                q,
                "The ghost command",
                "ghost <folder> in a terminal opens the folder here as a project. It goes in ~/.local/bin.",
                command,
            ),
            row(
                faces,
                q,
                "Where Ghost keeps its files",
                &Desk::shown(&self.home),
                show,
            ),
            row(
                faces,
                q,
                "Ghost's log",
                "A file a day in logs/, kept as long as Keep logs says: what to send with a bug.",
                show_logs,
            ),
            row(
                faces,
                q,
                "Keep logs",
                "The days of log kept; older files go as Ghost starts.",
                Self::pick(
                    "log-days",
                    [("3 days", 3), ("14 days", 14), ("30 days", 30)],
                    self.settings.log_days,
                    |s, v| s.log_days = v,
                    cx,
                ),
            ),
            row(
                faces,
                q,
                "The agents come from",
                "The ACP registry, read again every six hours.",
                registry,
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        let updates = self.updates_group(faces, q, cx);
        (!rows.is_empty() || updates.is_some()).then(|| {
            page(
                faces,
                &format!("Ghost {}", env!("CARGO_PKG_VERSION")),
                "Agent Client Protocol v1 · macOS",
            )
            .when(!rows.is_empty(), |p| p.child(group_of(rows)))
            .children(updates)
        })
    }
}

/// A tab's page: its title and what it is for, then its groups.
pub(super) fn page(faces: &Faces, title: &str, lede: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(22.))
        .px(px(40.))
        .py(px(28.))
        .max_w(px(760.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    ui::text(faces, title.to_owned())
                        .text_size(px(20.))
                        .line_height(px(28.))
                        .weight(gpui::FontWeight::SEMIBOLD),
                )
                .child(
                    ui::text(faces, lede.to_owned())
                        .text_size(px(13.5))
                        .text_color(theme::text2()),
                ),
        )
}

/// The rows left by the search under a caption, in one frame; none when
/// the search left none.
pub(super) fn group(faces: &Faces, caption: &str, rows: Vec<Option<Div>>) -> Option<Div> {
    let rows: Vec<Div> = rows.into_iter().flatten().collect();
    (!rows.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(ui::label(faces, caption))
            .child(group_of(rows))
    })
}

/// Rows in one frame, a line between each.
pub(super) fn group_of(rows: Vec<Div>) -> Div {
    let mut frame = card().overflow_hidden();
    for (n, r) in rows.into_iter().enumerate() {
        frame = frame.child(r.when(n > 0, |d| d.border_t_1().border_color(theme::border())));
    }
    frame
}

/// Whether what is looked for, `q` (lower case), is in any of `words`;
/// nothing looked for is in all.
pub(super) fn said(q: &str, words: &[&str]) -> bool {
    q.is_empty() || words.iter().any(|w| w.to_lowercase().contains(q))
}

/// A setting: what happens, said plainly, and its control; none when
/// what is looked for, `q`, is in neither.
pub(super) fn row(
    faces: &Faces,
    q: &str,
    name: &str,
    about: &str,
    control: impl IntoElement,
) -> Option<Div> {
    said(q, &[name, about]).then(|| setting(faces, name, about, control))
}

/// A row: what it is, said plainly, and its control.
pub(super) fn setting(faces: &Faces, name: &str, about: &str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(16.))
        .py(px(12.))
        .min_h(px(56.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(2.))
                .child(ui::text(faces, name.to_owned()).text_size(px(13.5)))
                .child(
                    ui::text(faces, about.to_owned())
                        .text_size(px(12.5))
                        .line_height(px(18.))
                        .text_color(theme::faint()),
                ),
        )
        .child(control)
}
