//! The settings that are u0's own, a tab each: `General` and `New threads`
//! rendered natively on Carbon `SettingsSection` + `SRow`, plus the shared
//! `item` / `whole` / `group` / `managed` / `pick` helpers used across
//! `pages/`.

use gpui::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui::component::{Disableable as _, Sizable as _, switch::Switch};
use gpui::{AnyElement, App, Context, Div, SharedString, WeakEntity, div, prelude::*, px};
use u0ui::{
    components::button::{Button as CarbonButton, ButtonSize, ButtonVariant},
    icons::{CarbonIcon, Icon as CarbonGlyph},
    tokens::ColorRole,
};

use super::srow::{SGroup, SRow, carbon_page};
use crate::{
    desk::Desk,
    fonts::{Faces, Weighted as _},
    hub::Host,
    settings::{Notify, Quit, Settings, Submodules},
    theme,
    ui::{self, Pick, Weight},
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
                    "notify",
                    "awake",
                    "editor",
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
                        crate::tr("While you are away"),
                        vec![
                            self.global_select_srow(
                                "notify",
                                "When a thread ends a turn or needs you",
                                "From macOS when u0 is behind; in u0 when another thread is shown.",
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
                    crate::tr("How u0 tells you things, and what it does while you are away."),
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

    /// Legacy `SettingPage` tabs (`Chat`, `Keybindings`, `Writing`, `About`,
    /// `SourceControl`).
    pub(super) fn own_tab(
        &self,
        tab: super::Tab,
        w: &WeakEntity<Desk>,
        banners: Option<SettingGroup>,
        _: &App,
    ) -> Option<SettingPage> {
        use super::Tab;
        let (lede, groups) = match tab {
            Tab::Chat => (
                "What you write, and what the agent did.",
                vec![
                    group(
                        crate::tr("While a turn goes on"),
                        vec![item(
                            w,
                            crate::tr("What you send waits, or steers"),
                            crate::tr("Steer interrupts the turn; waiting sends once it ends."),
                            |d, _, cx| {
                                d.pick(
                                    "steer",
                                    [
                                        (crate::tr("Wait its turn"), false),
                                        (crate::tr("Steer"), true),
                                    ],
                                    d.settings.steer,
                                    |s, v| s.steer = v,
                                    cx,
                                )
                            },
                        )],
                    ),
                    group(
                        crate::tr("Spelling"),
                        vec![item(
                            w,
                            crate::tr("Check spelling as you type"),
                            crate::tr("Unknown words are underlined; right-click for suggestions."),
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
                            crate::tr(
                                "If Return doesn't send, it breaks the line; the other key starts a thread beside it.",
                            ),
                            |d, _, cx| {
                                d.pick(
                                    "send",
                                    [
                                        (crate::tr("Return"), crate::settings::SendKey::Return),
                                        (
                                            crate::tr("⌘Return if multiline"),
                                            crate::settings::SendKey::Multiline,
                                        ),
                                        (crate::tr("⌘Return"), crate::settings::SendKey::Command),
                                    ],
                                    d.settings.send,
                                    |s, v| s.send = v,
                                    cx,
                                )
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
                                crate::tr(
                                    "From least to most shown: Answers, Quiet, Normal, Thinking, Verbose.",
                                ),
                                |d, _, cx| {
                                    d.pick(
                                        "view",
                                        [
                                            (crate::tr("Answers"), crate::settings::View::Zen),
                                            (crate::tr("Quiet"), crate::settings::View::Quiet),
                                            (crate::tr("Normal"), crate::settings::View::Normal),
                                            (
                                                crate::tr("Thinking"),
                                                crate::settings::View::Thinking,
                                            ),
                                            (crate::tr("Verbose"), crate::settings::View::Verbose),
                                        ],
                                        d.settings.view,
                                        |s, v| s.view = v,
                                        cx,
                                    )
                                },
                            ),
                            item(
                                w,
                                crate::tr("An answer streams in"),
                                crate::tr(
                                    "A paragraph at a time holds each back until it is whole.",
                                ),
                                |d, _, cx| {
                                    d.pick(
                                        "paragraphs",
                                        [
                                            (crate::tr("Word by word"), false),
                                            (crate::tr("A paragraph at a time"), true),
                                        ],
                                        d.settings.paragraphs,
                                        |s, v| s.paragraphs = v,
                                        cx,
                                    )
                                },
                            ),
                            Self::window_language(w),
                            item(
                                w,
                                crate::tr("Math"),
                                crate::tr(
                                    "TeX in what an agent says, $…$ and $$…$$, set as formulas.",
                                ),
                                |d, _, cx| {
                                    d.pick(
                                        "math",
                                        [
                                            (crate::tr("As written"), false),
                                            (crate::tr("Set as formulas"), true),
                                        ],
                                        d.settings.math,
                                        |s, v| s.math = v,
                                        cx,
                                    )
                                },
                            ),
                        ],
                    ),
                ],
            ),
            Tab::Writing => return Some(Self::writing_tab(w, banners)),
            Tab::Browser => return Some(self.browser_tab(w, banners)),
            Tab::About => return Some(self.about_u0(w, banners)),
            Tab::SourceControl => return Some(self.source_control(w, banners)),
            _ => return None,
        };
        let mut all = Vec::new();
        if let Some(b) = banners {
            all.push(b);
        }
        all.extend(groups);
        Some(
            SettingPage::new(tab.title())
                .description(crate::tr(lede))
                .groups(all),
        )
    }

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
        &self,
        id: &str,
        choices: [(impl Into<SharedString>, T); N],
        now: T,
        set: fn(&mut Settings, T),
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
        let readonly = self.settings_readonly(Host::HERE);
        let group = ui::segmented(id, choices).disabled(readonly);
        let key = id.split_once('-').map_or(id, |(key, _)| key);
        if let Some(by) = crate::policy::locked(id).or_else(|| crate::policy::locked(key)) {
            managed(group.disabled(true), by)
        } else if let Some((by, _)) =
            crate::policy::default_of(id).or_else(|| crate::policy::default_of(key))
        {
            defaulted(group, by)
        } else {
            group.into_any_element()
        }
    }

    fn about_u0(&self, w: &WeakEntity<Desk>, banners: Option<SettingGroup>) -> SettingPage {
        let rows = vec![
            item(
                w,
                crate::tr("The u0 command"),
                crate::tr("u0 <folder> opens a folder as a project. Installed in ~/.local/bin."),
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
                crate::tr("A file a day in logs/: send it with a bug report."),
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
                    ui::button("open-registry", crate::tr("Open"), Weight::Quiet)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.browse("https://agentclientprotocol.com/registry", cx);
                        }))
                        .into_any_element()
                },
            ),
        ];
        let mut page =
            SettingPage::new(super::Tab::About.title()).description(crate::update::this_build());
        if let Some(b) = banners {
            page = page.group(b);
        }
        page.group(SettingGroup::new().items(rows))
            .group(self.updates_group(w))
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

/// A setting as gpui-kit's Settings draws one: what it is, said plainly
/// (and found by its search), and its control, which the desk, `w`, draws.
pub(super) fn item(
    w: &WeakEntity<Desk>,
    name: impl Into<SharedString>,
    about: impl Into<SharedString>,
    control: impl Fn(&mut Desk, &Faces, &mut Context<Desk>) -> AnyElement + 'static,
) -> SettingItem {
    let w = w.clone();
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
    if about.is_empty() {
        item
    } else {
        item.description(about)
    }
}

/// What the desk, `w`, draws whole as one setting, found by `words`.
pub(crate) fn whole(
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

/// Settings under a caption.
pub(super) fn group(caption: impl Into<SharedString>, items: Vec<SettingItem>) -> SettingGroup {
    SettingGroup::new()
        .title(crate::tr(&caption.into()))
        .items(items)
}
