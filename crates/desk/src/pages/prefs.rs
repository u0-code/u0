//! The settings that are u0's own, a tab each: `General` and `New threads`
//! row by row (`SRow`), Chat and About as pages of items,
//! and the `managed` / `pick` helpers used across `pages/`.

use gpui::{AnyElement, App, Context, Div, SharedString, WeakEntity, Window, div, prelude::*, px};
use u0ui::{
    components::button::{Button as CarbonButton, ButtonSize, ButtonVariant},
    icons::{CarbonIcon, Icon as CarbonGlyph},
    tokens::ColorRole,
};

use super::srow::{Page, SGroup, SRow, carbon_page, group, item, select, whole};
use crate::{
    desk::Desk,
    fonts::{Faces, Weighted as _},
    hub::Host,
    settings::{Notify, Quit, Settings, Submodules},
    theme,
    ui::{self, Weight},
};

impl Desk {
    /// Whether the settings file currently edited did not read (`unreadable`
    /// state on `SRow`).
    pub(crate) fn edited_unreadable(&self) -> bool {
        match self.pages.on {
            Host::HERE => self.settings_refused.is_some(),
            on => self.machine(on).is_some_and(|m| m.refused.is_some()),
        }
    }

    /// Whether the server whose settings are edited is out of reach.
    pub(crate) fn edited_offline(&self) -> bool {
        self.pages.on != Host::HERE
            && self
                .machine(self.pages.on)
                .is_some_and(|m| m.up != crate::hub::Up::On)
    }

    /// Build a Carbon `SRow` wired to organization policy (`policy_key`) and
    /// unreadable-settings protection (`unreadable`).
    pub(crate) fn srow(
        &self,
        id: &'static str,
        title: impl Into<SharedString>,
        about: impl Into<SharedString>,
    ) -> SRow {
        SRow::new(id, title)
            .description(about)
            .policy_key(id)
            .unreadable(self.edited_unreadable())
            .disabled(self.edited_offline())
    }

    /// Carbon-native settings page for migrated tabs (`General`, `NewThreads`).
    pub(super) fn carbon_tab(
        &self,
        tab: super::Tab,
        query: &str,
        faces: &Faces,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use super::Tab;
        let scoped = self.pages.scope.is_some();
        let defaults = Settings::default();
        match tab {
            Tab::General => {
                let any_managed = [
                    "sort",
                    "notify",
                    "awake",
                    "editor",
                    "language",
                    "twelve",
                    "merge",
                    "settle_days",
                    "settle_merged",
                    "snoozes",
                    "keep_with_pr",
                    "resume_at_reset",
                    "fetch_every",
                    "check_agents",
                    "confirm",
                ]
                .into_iter()
                .any(|k| crate::policy::locked(k).is_some());
                let top_banner = self.carbon_settings_banner(tab, cx);
                let has_top = top_banner.is_some();
                let mut banners = div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(top_banner)
                    .child(self.scope_banner(faces, cx));
                if any_managed && !has_top {
                    banners = banners.child(managed_info_banner(faces));
                }
                let groups = vec![
                    SGroup::new(
                        crate::tr("Sidebar"),
                        vec![self.global_select_srow(
                            "sort",
                            "Thread order",
                            "For threads and projects alike; pinned ones stay on top.",
                            [
                                ("Your last message", crate::settings::Sort::Used),
                                ("Activity", crate::settings::Sort::Updated),
                                ("Began", crate::settings::Sort::Begun),
                                ("Name", crate::settings::Sort::Name),
                            ],
                            self.settings.sort,
                            defaults.sort,
                            |s, v| s.sort = v,
                            scoped,
                            cx,
                        )],
                    )
                    .description(crate::tr("How threads are ordered.")),
                    SGroup::new(
                        crate::tr("Notifications"),
                        vec![
                            self.global_select_srow(
                                "notify",
                                "Thread notifications",
                                "From threads you are not looking at.",
                                [
                                    ("Off", Notify::Off),
                                    ("Notify", Notify::Banner),
                                    ("Notify and sound", Notify::BannerAndSound),
                                ],
                                self.settings.notify,
                                defaults.notify,
                                |s, v| s.notify = v,
                                scoped,
                                cx,
                            ),
                            self.told_srow(
                                "notify-finished",
                                "A turn finished",
                                "Its answer is in.",
                                |n| &mut n.finished,
                                scoped,
                                cx,
                            ),
                            self.told_srow(
                                "notify-failed",
                                "A turn failed or was stopped",
                                "It ended in an error, or you stopped it.",
                                |n| &mut n.failed,
                                scoped,
                                cx,
                            ),
                            self.told_srow(
                                "notify-asks",
                                "An agent asks before it goes on",
                                "A permission or a question waits on you.",
                                |n| &mut n.asks,
                                scoped,
                                cx,
                            ),
                            {
                                let now = self.settings.awake;
                                let def = defaults.awake;
                                let desk_set = cx.entity();
                                let desk_reset = cx.entity();
                                self.srow(
                                    "awake",
                                    crate::tr("Keep the Mac awake while an agent works"),
                                    crate::tr("The display may still sleep."),
                                )
                                .disabled(scoped)
                                .modified(now != def, move |_, cx| {
                                    desk_reset.update(cx, |this, cx| {
                                        this.settings.awake = def;
                                        this.save_settings();
                                        this.stay_awake();
                                        cx.notify();
                                    });
                                })
                                .switch(now, move |on, _, cx| {
                                    desk_set.update(cx, |this, cx| {
                                        this.settings.awake = on;
                                        this.save_settings();
                                        this.stay_awake();
                                        cx.notify();
                                    });
                                })
                            },
                        ],
                    )
                    .description(crate::tr("When u0 tells you a thread needs you.")),
                    SGroup::new(
                        crate::tr("Files"),
                        vec![self.editor_srow(scoped, cx)],
                    ),
                    SGroup::new(
                        crate::tr("Language and time"),
                        vec![
                            self.window_language_srow(scoped, cx),
                            self.global_segmented_srow(
                                "twelve",
                                "Time format",
                                "When a message was said, and what u0 says of times.",
                                [("24-hour", false), ("12-hour", true)],
                                self.settings.twelve,
                                defaults.twelve,
                                |s, v| s.twelve = v,
                                scoped,
                                cx,
                            ),
                        ],
                    ),
                    SGroup::new(
                        crate::tr("Pull requests"),
                        vec![self.own_srow(
                            "merge",
                            "Merged as",
                            "Used by Merge and by merging once checks pass.",
                            [
                                ("Squash", crate::git::Method::Squash),
                                ("Merge commit", crate::git::Method::Merge),
                                ("Rebase", crate::git::Method::Rebase),
                            ],
                            |s| s.merge,
                            |s, v| s.merge = v,
                            cx,
                        )],
                    ),
                    SGroup::new(
                        crate::tr("Done for now"),
                        vec![
                            self.global_select_srow(
                                "settle_days",
                                "A thread is done after it is left for",
                                "It moves to Settled in the sidebar; anything said to it brings it back.",
                                [
                                    ("3 days", Some(3)),
                                    ("7 days", Some(7)),
                                    ("14 days", Some(14)),
                                    ("30 days", Some(30)),
                                    ("Never", None),
                                ],
                                self.settings.settle_days,
                                defaults.settle_days,
                                |s, v| s.settle_days = v,
                                scoped,
                                cx,
                            ),
                            self.global_segmented_srow(
                                "settle_merged",
                                "When its pull request merges or closes",
                                "The thread is done then, while it waits on nothing.",
                                [("Settle it", true), ("Leave it", false)],
                                self.settings.settle_merged,
                                defaults.settle_merged,
                                |s, v| s.settle_merged = v,
                                scoped,
                                cx,
                            ),
                            {
                                let desk_reset = cx.entity();
                                let def_snoozes = defaults.snoozes.clone();
                                let changed = self.settings.snoozes != def_snoozes;
                                self.srow(
                                    "snoozes",
                                    crate::tr("Snooze for"),
                                    crate::tr("Comma-separated, as typed in Custom…: 1h, 5pm, tomorrow."),
                                )
                                .disabled(scoped)
                                .modified(changed, move |window, cx| {
                                    let joined = def_snoozes.join(", ");
                                    desk_reset.update(cx, |this, cx| {
                                        this.settings.snoozes.clone_from(&def_snoozes);
                                        this.pages
                                            .snoozes
                                            .update(cx, |i, cx| i.set_value(joined, window, cx));
                                        this.save_settings();
                                        cx.notify();
                                    });
                                })
                                .text_input(&self.pages.snoozes, faces, false)
                            },
                            self.global_segmented_srow(
                                "keep_with_pr",
                                "Keep threads with a pull request",
                                "One whose pull request is open is never done for being left.",
                                [("Keep them", true), ("Let them settle", false)],
                                self.settings.keep_with_pr,
                                defaults.keep_with_pr,
                                |s, v| s.keep_with_pr = v,
                                scoped,
                                cx,
                            ),
                        ],
                    ),
                    SGroup::new(
                        crate::tr("In the background"),
                        vec![
                            self.own_srow(
                                "resume_at_reset",
                                "Go on when a limit resets",
                                "A turn stopped by a usage limit resumes when the limit resets.",
                                [("Go on", true), ("Wait for me", false)],
                                |s| s.resume_at_reset,
                                |s, v| s.resume_at_reset = v,
                                cx,
                            ),
                            self.global_select_srow(
                                "fetch_every",
                                "Fetch each project's remote every",
                                "Keeps branches and pull requests current; pulls projects set to pull.",
                                [
                                    ("Minute", Some(1)),
                                    ("5 minutes", Some(5)),
                                    ("15 minutes", Some(15)),
                                    ("Never", None),
                                ],
                                self.settings.fetch_every,
                                defaults.fetch_every,
                                |s, v| s.fetch_every = v,
                                scoped,
                                cx,
                            ),
                            self.global_segmented_srow(
                                "check_agents",
                                "Look for agents' new versions",
                                "An agent that stops working is updated from the ACP registry.",
                                [("Each hour", true), ("Never", false)],
                                self.settings.check_agents,
                                defaults.check_agents,
                                |s, v| s.check_agents = v,
                                scoped,
                                cx,
                            ),
                        ],
                    ),
                    SGroup::new(
                        crate::tr("Confirmations"),
                        vec![
                            self.asks_srow(
                                "confirm-unpin",
                                "Unpin",
                                "Ask before a thread is unpinned.",
                                |c| &mut c.unpin,
                                scoped,
                                cx,
                            ),
                            self.asks_srow(
                                "confirm-archive",
                                "Archive",
                                "Ask before a thread is archived.",
                                |c| &mut c.archive,
                                scoped,
                                cx,
                            ),
                            self.asks_srow(
                                "confirm-delete",
                                "Delete",
                                "Ask before a thread and what was said in it are deleted.",
                                |c| &mut c.delete,
                                scoped,
                                cx,
                            ),
                            self.asks_srow(
                                "confirm-terminal",
                                "Close a terminal",
                                "Ask before a terminal is closed with something running in it.",
                                |c| &mut c.terminal,
                                scoped,
                                cx,
                            ),
                            self.global_segmented_srow(
                                "confirm-quit",
                                "Quit with ⌘Q",
                                "Quit in the menu always quits at once.",
                                [
                                    ("Hold", Quit::Hold),
                                    ("Double press", Quit::Double),
                                    ("Direct", Quit::Direct),
                                ],
                                self.settings.confirm.quit,
                                defaults.confirm.quit,
                                |s, v| s.confirm.quit = v,
                                scoped,
                                cx,
                            ),
                        ],
                    ),
                ];
                Some(carbon_page(
                    "settings-general",
                    tab.title(),
                    crate::tr("The sidebar, notifications, files, language and confirmations."),
                    Some(banners.into_any_element()),
                    groups,
                    query,
                    faces,
                ))
            }
            Tab::NewThreads => {
                let groups = vec![SGroup::new(
                    crate::tr("In a repository"),
                    vec![
                        self.global_segmented_srow(
                            "worktree",
                            "A new thread works",
                            "A project can choose otherwise from its menu.",
                            [("In the folder", false), ("In a new worktree", true)],
                            self.settings.worktree,
                            defaults.worktree,
                            |s, v| s.worktree = v,
                            scoped,
                            cx,
                        ),
                        {
                            let def_prefix = defaults.branch_prefix.clone();
                            let changed = self.settings.branch_prefix != def_prefix;
                            let desk_reset = cx.entity();
                            self.srow(
                                "branch_prefix",
                                crate::tr("Worktree branches begin with"),
                                crate::tr(
                                    "Then the name the writer gives the work, as in u0/retry-s3-uploads.",
                                ),
                            )
                            .disabled(scoped)
                            .modified(changed, move |window, cx| {
                                let p = def_prefix.clone();
                                desk_reset.update(cx, |this, cx| {
                                    this.settings.branch_prefix.clone_from(&p);
                                    this.pages
                                        .prefix
                                        .update(cx, |i, cx| i.set_value(p, window, cx));
                                    this.save_settings();
                                    cx.notify();
                                });
                            })
                            .text_input(&self.pages.prefix, faces, true)
                        },
                        self.own_srow(
                            "branch_kind",
                            "Or they begin with",
                            "The kind of work, as the writer sees it: feat/, fix/, docs/.",
                            [("The prefix", false), ("The kind of work", true)],
                            |s| s.branch_kind,
                            |s, v| s.branch_kind = v,
                            cx,
                        ),
                        self.worktree_location_srow(scoped, cx),
                        self.own_srow(
                            "submodules",
                            "Submodules",
                            "How far a new worktree's submodules are checked out.",
                            [
                                ("Recursive", Submodules::Recursive),
                                ("Top level", Submodules::Top),
                                ("Skip", Submodules::Skip),
                            ],
                            |s| s.submodules,
                            |s, v| s.submodules = v,
                            cx,
                        ),
                        self.own_srow(
                            "tidy_after",
                            "An archived thread's worktree is taken away after",
                            "Its branch and any uncommitted worktree stay; reopening restores it.",
                            [
                                ("A day", Some(1)),
                                ("7 days", Some(7)),
                                ("30 days", Some(30)),
                                ("Never", None),
                            ],
                            |s| s.tidy_after,
                            |s, v| s.tidy_after = v,
                            cx,
                        ),
                    ],
                )];
                let banners = div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(self.carbon_settings_banner(tab, cx))
                    .child(self.scope_banner(faces, cx));
                Some(carbon_page(
                    "settings-new-threads",
                    tab.title(),
                    crate::tr("How a thread begins in a repository."),
                    Some(banners.into_any_element()),
                    groups,
                    query,
                    faces,
                ))
            }
            Tab::Appearance => Some(self.appearance_tab(query, faces, cx)),
            Tab::Keybindings => Some(self.keybindings_tab(query, faces, cx)),
            Tab::Voice => Some(self.voice_tab(query, faces, cx)),
            _ => None,
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a setting row binds its key, copy, choices, current/default values and setter"
    )]
    pub(super) fn global_segmented_srow<T: Copy + PartialEq + 'static, const N: usize>(
        &self,
        id: &'static str,
        title: &'static str,
        about: &'static str,
        choices: [(&'static str, T); N],
        now: T,
        default_val: T,
        set: fn(&mut Settings, T),
        scoped: bool,
        cx: &mut Context<Self>,
    ) -> SRow {
        let desk_pick = cx.entity();
        let desk_reset = cx.entity();
        let translated = choices.map(|(l, v)| (crate::tr(l), v));
        self.srow(id, crate::tr(title), crate::tr(about))
            .disabled(scoped)
            .modified(now != default_val, move |_, cx| {
                desk_reset.update(cx, |this, cx| {
                    set(&mut this.settings, default_val);
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            })
            .segmented(translated, now, move |picked, _, cx| {
                desk_pick.update(cx, |this, cx| {
                    set(&mut this.settings, picked);
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a setting row binds its key, copy, choices, current/default values and setter"
    )]
    pub(super) fn global_select_srow<T: Copy + PartialEq + 'static, const N: usize>(
        &self,
        id: &'static str,
        title: &'static str,
        about: &'static str,
        choices: [(&'static str, T); N],
        now: T,
        default_val: T,
        set: fn(&mut Settings, T),
        scoped: bool,
        cx: &mut Context<Self>,
    ) -> SRow {
        let desk_pick = cx.entity();
        let desk_reset = cx.entity();
        let translated = choices.map(|(l, v)| (crate::tr(l), v));
        self.srow(id, crate::tr(title), crate::tr(about))
            .disabled(scoped)
            .modified(now != default_val, move |_, cx| {
                desk_reset.update(cx, |this, cx| {
                    set(&mut this.settings, default_val);
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            })
            .select(translated, &now, move |picked, _, cx| {
                desk_pick.update(cx, |this, cx| {
                    set(&mut this.settings, picked);
                    this.save_settings();
                    this.settings_took(cx);
                    cx.notify();
                });
            })
    }

    fn told_srow(
        &self,
        id: &'static str,
        name: &'static str,
        about: &'static str,
        of: fn(&mut crate::settings::NotifyOn) -> &mut bool,
        scoped: bool,
        cx: &mut Context<Self>,
    ) -> SRow {
        let mut copy = self.settings.notify_on.clone();
        let mut def_copy = Settings::default().notify_on;
        let now = *of(&mut copy);
        let def = *of(&mut def_copy);
        let off = self.settings.notify == Notify::Off;
        let desk_set = cx.entity();
        let desk_reset = cx.entity();
        self.srow(id, crate::tr(name), crate::tr(about))
            .disabled(scoped || off)
            .modified(now != def, move |_, cx| {
                desk_reset.update(cx, |this, cx| {
                    *of(&mut this.settings.notify_on) = def;
                    this.save_settings();
                    cx.notify();
                });
            })
            .switch(now, move |on, _, cx| {
                desk_set.update(cx, |this, cx| {
                    *of(&mut this.settings.notify_on) = on;
                    this.save_settings();
                    cx.notify();
                });
            })
    }

    fn asks_srow(
        &self,
        id: &'static str,
        name: &'static str,
        about: &'static str,
        of: fn(&mut crate::settings::Confirm) -> &mut bool,
        scoped: bool,
        cx: &mut Context<Self>,
    ) -> SRow {
        let mut copy = self.settings.confirm.clone();
        let mut def_copy = Settings::default().confirm;
        let now = *of(&mut copy);
        let def = *of(&mut def_copy);
        let desk_set = cx.entity();
        let desk_reset = cx.entity();
        self.srow(id, crate::tr(name), crate::tr(about))
            .disabled(scoped)
            .modified(now != def, move |_, cx| {
                desk_reset.update(cx, |this, cx| {
                    *of(&mut this.settings.confirm) = def;
                    this.save_settings();
                    cx.notify();
                });
            })
            .switch(now, move |on, _, cx| {
                desk_set.update(cx, |this, cx| {
                    *of(&mut this.settings.confirm) = on;
                    this.save_settings();
                    cx.notify();
                });
            })
    }

    fn editor_srow(&self, scoped: bool, cx: &mut Context<Self>) -> SRow {
        let mut choices: Vec<(SharedString, Option<String>)> =
            vec![(crate::tr("First found"), None)];
        for (command, name) in crate::desk::EDITORS {
            if self.on_path.contains(&command) || self.settings.editor.as_deref() == Some(command) {
                choices.push((name.into(), Some(command.to_owned())));
            }
        }
        let now = self.settings.editor.clone();
        let desk_pick = cx.entity();
        let desk_reset = cx.entity();
        self.srow(
            "editor",
            crate::tr("Files open in"),
            crate::tr("A file the agent named, an edit, or keybindings.json."),
        )
        .disabled(scoped)
        .modified(now.is_some(), move |_, cx| {
            desk_reset.update(cx, |this, cx| {
                this.settings.editor = None;
                this.save_settings();
                this.settings_took(cx);
                cx.notify();
            });
        })
        .select(choices, &now, move |picked, _, cx| {
            desk_pick.update(cx, |this, cx| {
                this.settings.editor = picked;
                this.save_settings();
                this.settings_took(cx);
                cx.notify();
            });
        })
    }

    fn worktree_location_srow(&self, scoped: bool, cx: &mut Context<Self>) -> SRow {
        let folder = &self.worktree_folders(Host::HERE)[0];
        let missing_err = self
            .settings
            .worktrees
            .as_ref()
            .filter(|p| !p.exists())
            .map(|p| {
                crate::tr_with(
                    "That folder is not there: %{folder}",
                    &[("folder", &Desk::shown(p))],
                )
            });
        let row = self
            .srow(
                "worktrees",
                crate::tr("Worktree location"),
                crate::tr_with(
                    "New worktrees go in %{folder}. Existing ones stay put.",
                    &[("folder", &Desk::shown(folder))],
                ),
            )
            .disabled(scoped)
            .error(missing_err);
        if self.settings.worktrees.is_none() {
            let desk_choose = cx.entity();
            row.button(
                crate::tr("Choose…"),
                ButtonVariant::Tertiary,
                move |_, cx| {
                    desk_choose.update(cx, |_, cx| {
                        Desk::ask_folder(
                            &crate::tr("Make worktrees here"),
                            |d, f| {
                                d.settings.worktrees = Some(f);
                                d.save_settings();
                            },
                            cx,
                        );
                    });
                },
            )
        } else {
            let desk_own = cx.entity();
            let desk_choose = cx.entity();
            let controls = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    CarbonButton::new("worktrees-own", crate::tr("u0's own"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk_own.update(cx, |this, cx| {
                                this.settings.worktrees = None;
                                this.save_settings();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    CarbonButton::new("worktrees-choose", crate::tr("Choose…"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            desk_choose.update(cx, |_, cx| {
                                Desk::ask_folder(
                                    &crate::tr("Make worktrees here"),
                                    |d, f| {
                                        d.settings.worktrees = Some(f);
                                        d.save_settings();
                                    },
                                    cx,
                                );
                            });
                        }),
                );
            row.control(controls)
        }
    }

    /// Pages of items (`Chat`, `Writing`, `About`,
    /// `SourceControl`).
    pub(super) fn own_tab(&self, tab: super::Tab, w: &WeakEntity<Desk>, _: &App) -> Option<Page> {
        use super::Tab;
        let (lede, groups) = match tab {
            Tab::Chat => (
                "The composer, and what the transcript shows.",
                vec![
                    group(
                        crate::tr("Composer"),
                        vec![
                            item(
                                w,
                                "Send shortcut",
                                "Where ↵ does not send, it adds a new line.",
                                |d, _, cx| {
                                    d.pick(
                                        "send",
                                        [
                                            ("↵ sends", crate::settings::SendKey::Return),
                                            (
                                                "⌘↵ once multiline",
                                                crate::settings::SendKey::Multiline,
                                            ),
                                            ("⌘↵ sends", crate::settings::SendKey::Command),
                                        ],
                                        d.settings.send,
                                        |s, v| s.send = v,
                                        cx,
                                    )
                                },
                            ),
                            item(
                                w,
                                "What you send waits, or steers",
                                "Waiting sends it once the turn ends; Steer says it now.",
                                |d, _, cx| {
                                    d.pick(
                                        "steer",
                                        [("Waits its turn", false), ("Steer", true)],
                                        d.settings.steer,
                                        |s, v| s.steer = v,
                                        cx,
                                    )
                                },
                            ),
                            item(
                                w,
                                "Check spelling as you type",
                                "Unknown words are underlined; right-click one for its guesses.",
                                |d, _, cx| {
                                    super::srow::flip(
                                        "spell-check",
                                        crate::tr("Check spelling as you type"),
                                        d.settings.spell_check,
                                        cx,
                                        |this, _, cx| {
                                            this.settings.spell_check = !this.settings.spell_check;
                                            this.save_settings();
                                            this.sync_spelling(cx);
                                            cx.notify();
                                        },
                                    )
                                    .into_any_element()
                                },
                            ),
                        ],
                    )
                    .description(crate::tr("How the message box behaves while you write.")),
                    group(
                        crate::tr("Transcript"),
                        vec![
                            item(
                                w,
                                "Transcript view (⌃O)",
                                "How much of the agent's work a thread shows; ⌃O goes round them.",
                                |d, _, cx| {
                                    d.pick(
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
                                },
                            ),
                            item(
                                w,
                                "An answer streams in",
                                "A paragraph at a time holds each back until it is whole.",
                                |d, _, cx| {
                                    d.pick(
                                        "paragraphs",
                                        [("Word by word", false), ("A paragraph at a time", true)],
                                        d.settings.paragraphs,
                                        |s, v| s.paragraphs = v,
                                        cx,
                                    )
                                },
                            ),
                            item(
                                w,
                                "Math",
                                "TeX in what an agent says, $…$ and $$…$$, set as formulas.",
                                |d, _, cx| {
                                    d.pick(
                                        "math",
                                        [("As written", false), ("Set as formulas", true)],
                                        d.settings.math,
                                        |s, v| s.math = v,
                                        cx,
                                    )
                                },
                            ),
                        ],
                    )
                    .description(crate::tr(
                        "What a thread shows between your messages and the replies.",
                    )),
                ],
            ),
            Tab::Writing => return Some(Self::writing_tab(w)),
            Tab::Browser => return Some(self.browser_tab(w)),
            Tab::About => return Some(self.about_u0(w)),
            Tab::SourceControl => return Some(self.source_control(w)),
            _ => return None,
        };
        Some(
            Page::new(tab.title())
                .description(crate::tr(lede))
                .groups(groups),
        )
    }

    /// The language of u0's window: as the system's, or one chosen.
    fn window_language_srow(&self, scoped: bool, cx: &mut Context<Self>) -> SRow {
        let mut choices = vec![(crate::tr("As the system"), None)];
        choices.extend(
            (crate::i18n::LANGUAGES.iter())
                .map(|(l, name)| ((*name).into(), Some((*l).to_owned()))),
        );
        let desk = cx.entity();
        self.srow(
            "language",
            crate::tr("Language"),
            crate::tr("The words of u0's window; agents' stay as they say them."),
        )
        .disabled(scoped)
        .select(choices, &self.settings.language, move |locale, _, cx| {
            desk.update(cx, |d, cx| {
                d.settings.language = locale;
                d.save_settings();
                d.settings_took(cx);
                cx.notify();
            });
        })
    }

    /// Carbon's content switcher over `choices`, or its select past three,
    /// each saved to the settings by `set` as it is picked.
    pub(super) fn pick<T: Copy + PartialEq + 'static, const N: usize>(
        &self,
        id: &str,
        choices: [(impl Into<SharedString>, T); N],
        now: T,
        set: fn(&mut Settings, T),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let choices = choices
            .into_iter()
            .map(|(label, value)| (crate::tr(&label.into()), value));
        let key = id.split_once('-').map_or(id, |(key, _)| key);
        let locked = crate::policy::locked(id).or_else(|| crate::policy::locked(key));
        let readonly = self.settings_readonly(Host::HERE) || locked.is_some();
        let desk = cx.entity();
        let picked = move |value, _: &mut Window, cx: &mut App| {
            desk.update(cx, |this, cx| {
                set(&mut this.settings, value);
                this.save_settings();
                this.settings_took(cx);
                cx.notify();
            });
        };
        let id = SharedString::from(id.to_owned());
        // Past three, the choices take a menu rather than a row of buttons.
        let group = if N > 3 {
            select(id.clone(), choices, &now, readonly, picked)
        } else {
            super::srow::segmented(id.clone(), choices, now, readonly, picked)
        };
        let id = id.as_ref();
        if let Some(by) = locked {
            managed(group, by)
        } else if let Some((by, _)) =
            crate::policy::default_of(id).or_else(|| crate::policy::default_of(key))
        {
            defaulted(group, by)
        } else {
            group
        }
    }

    fn about_u0(&self, w: &WeakEntity<Desk>) -> Page {
        let rows = vec![
            item(
                w,
                crate::tr("The u0 command"),
                crate::tr("u0 <folder> opens a folder as a project. Installed in ~/.local/bin."),
                |_, _, cx| {
                    super::srow::button(
                        "install-command",
                        crate::tr("Install command"),
                        Weight::Secondary,
                    )
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
                    super::srow::button("show-home", crate::tr("Show"), Weight::Secondary)
                        .on_click(move |_, _, cx| cx.open_with_system(&home))
                        .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("u0's log"),
                crate::tr("A file a day in logs/: send it with a bug report."),
                |d, _, _| {
                    let logs = d.home.join("logs");
                    super::srow::button("show-logs", crate::tr("Show"), Weight::Secondary)
                        .on_click(move |_, _, cx| cx.open_with_system(&logs))
                        .into_any_element()
                },
            ),
            item(
                w,
                crate::tr("Keep logs"),
                crate::tr("The days of log kept; older files go as u0 starts."),
                |d, _, cx| {
                    d.pick(
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
                },
            ),
            item(
                w,
                crate::tr("The agents come from"),
                crate::tr("The ACP registry, read again each hour."),
                |_, _, cx| {
                    super::srow::button("open-registry", crate::tr("Open"), Weight::Secondary)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.browse("https://agentclientprotocol.com/registry", cx);
                        }))
                        .into_any_element()
                },
            ),
        ];
        let channel = match self.settings.channel {
            crate::update::Channel::Stable => crate::tr("Stable"),
            crate::update::Channel::Nightly => crate::tr("Nightly"),
        };
        Page::new(super::Tab::About.title())
            .description(format!(
                "{} · {channel} · Apache-2.0",
                crate::update::this_build()
            ))
            .group(self.updates_group(w))
            .group(group(crate::tr("u0 on this Mac"), rows))
            .group(group(
                crate::tr("Get u0 elsewhere"),
                vec![whole(w, &["download", "linux"], |_, faces, cx| {
                    super::pairing::install_u0_elsewhere_card("about-install", faces, cx)
                        .into_any_element()
                })],
            ))
    }
}

pub(super) fn managed_info_banner(faces: &Faces) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .px(px(16.))
        .py(px(14.))
        .bg(theme::role(ColorRole::NotificationBackgroundInfo))
        .border_1()
        .border_color(theme::role(ColorRole::SupportInfo))
        .border_l(px(3.))
        .child(
            CarbonGlyph::carbon(CarbonIcon::InformationFilled)
                .size_length(px(18.))
                .color(theme::role(ColorRole::SupportInfo))
                .flex_none(),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(2.))
                .child(
                    ui::text(faces, crate::tr("Some settings are managed"))
                        .text_size(px(crate::style::BODY))
                        .weight(gpui::FontWeight::SEMIBOLD),
                )
                .child(
                    ui::text(
                        faces,
                        crate::tr("Locked controls are set by your organization."),
                    )
                    .text_size(px(crate::style::BODY))
                    .line_height(px(20.))
                    .text_color(theme::text2()),
                ),
        )
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

/// A control whose default comes from the policy, still editable, with who
/// set the default beside it.
pub(super) fn defaulted(control: impl IntoElement, by: &str) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(control)
        .child(
            div()
                .text_color(crate::theme::muted())
                .child(crate::tr_with(
                    "Default from %{by}’s policy",
                    &[("by", &by)],
                )),
        )
        .into_any_element()
}
