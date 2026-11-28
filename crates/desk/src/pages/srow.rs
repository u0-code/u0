//! Carbon settings row (`SRow` board), section (`SGroup`) and page container
//! (`carbon_page`), built on `u0ui::components` (`Switch`, `ToggleGroup`,
//! `Select`, `Button`, `Badge`), `u0ui::t3::shell::WorkspacePageHeader` and
//! `u0ui::t3::workspace::settings::SettingsSection`.

use std::rc::Rc;

use gpui::component::{
    Sizable as _, input::Input, input::InputState, scroll::ScrollableElement as _,
};
use gpui::{
    AnyElement, App, BoxShadow, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, point, prelude::FluentBuilder as _, px,
};
use u0ui::{
    components::{
        badge::{Badge, BadgeSize, BadgeVariant},
        button::{Button, ButtonSize, ButtonVariant},
        form::FieldSize,
        list_box::ListItem,
        select::Select,
        switch::{Switch, SwitchSize},
        toggle::ToggleSize,
        toggle_group::{ToggleGroup, ToggleGroupItem},
    },
    icons::{CarbonIcon, Icon as CarbonGlyph},
    t3::{shell::WorkspacePageHeader, workspace::settings::SettingsSection},
    theme::LayerLevel,
    tokens::ColorRole,
};

use crate::{
    fonts::{Faces, Weighted as _},
    theme, ui,
};

type ResetHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// One Carbon setting row (`SRow.dc.html`): 64px min-height, `16px 20px`
/// padding, `32px` gap, `14/20` title + `12/16` description on the left, and a
/// `240px` `field-02` control rail on the right. States (`overridden`,
/// `modified`, `managed` / `policy`, `error`, `unreadable`) are always spoken
/// in words with their icon, never by colour or a bare dot alone.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent SRow state: overridden, modified, managed, unreadable, disabled"
)]
#[derive(IntoElement)]
pub struct SRow {
    id: ElementId,
    title: SharedString,
    desc: SharedString,
    control: Option<AnyElement>,
    overridden: bool,
    overridden_by: Option<SharedString>,
    inherited: Option<SharedString>,
    modified: bool,
    managed: bool,
    managed_state: Option<SharedString>,
    policy: Option<SharedString>,
    error: Option<SharedString>,
    unreadable: bool,
    disabled: bool,
    on_reset: Option<ResetHandler>,
}

impl SRow {
    /// A row identified by `id` with translated `title`.
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            desc: SharedString::default(),
            control: None,
            overridden: false,
            overridden_by: None,
            inherited: None,
            modified: false,
            managed: false,
            managed_state: None,
            policy: None,
            error: None,
            unreadable: false,
            disabled: false,
            on_reset: None,
        }
    }

    /// Optional secondary description (`12/16`, `text-secondary`).
    pub fn description(mut self, desc: impl Into<SharedString>) -> Self {
        self.desc = desc.into();
        self
    }

    /// Check organization policy for `key` (`settings.json` top-level key, or
    /// prefix before `-`): when locked, marks the row `managed` and records who
    /// locked it.
    pub fn policy_key(mut self, key: &str) -> Self {
        let prefix = key.split_once('-').map_or(key, |(k, _)| k);
        if let Some(by) = crate::policy::locked(key).or_else(|| crate::policy::locked(prefix)) {
            self.managed = true;
            self.policy = Some(crate::tr_with("Managed by %{by}", &[("by", &by)]));
        }
        self
    }

    /// `settings.json` did not read: locks the control and shows the alert line
    /// so nothing is saved over a file u0 could not parse.
    pub fn unreadable(mut self, unreadable: bool) -> Self {
        self.unreadable = unreadable;
        self
    }

    /// Validation or filesystem error spoken under the row with an
    /// `ErrorFilled` glyph and a 1px inset `support-error` border; the control
    /// stays active unless `unreadable` or `managed` so the user can fix it.
    pub fn error(mut self, error: Option<SharedString>) -> Self {
        self.error = error;
        self
    }

    /// Disable interaction (for global-only rows while a project scope is
    /// selected atop the page).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Mark the row changed from its default value (`Changed from default ·
    /// Reset`).
    pub fn modified(
        mut self,
        modified: bool,
        on_reset: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.modified = modified;
        if modified && self.on_reset.is_none() {
            self.on_reset = Some(Rc::new(on_reset));
        }
        self
    }

    /// Mark the row overridden for a project (`Overridden` badge + `Set by … ·
    /// Global value: … · Reset`).
    pub fn project_override(
        mut self,
        overridden: bool,
        overridden_by: Option<SharedString>,
        inherited: SharedString,
        on_reset: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.overridden = overridden;
        self.overridden_by = overridden_by;
        self.inherited = Some(inherited);
        if overridden {
            self.on_reset = Some(Rc::new(on_reset));
        }
        self
    }

    /// Whether the control is locked (`managed`, `unreadable`, or `disabled`).
    pub fn is_locked(&self) -> bool {
        self.managed || self.unreadable || self.disabled
    }

    /// Attach an arbitrary right-rail control element.
    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    /// Carbon `Switch` (`sm` 32×16): when `managed`, speaks `"On"` / `"Off"`
    /// beside the disabled track per `SRow.dc.html`.
    pub fn switch(
        mut self,
        checked: bool,
        on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        if self.managed {
            self.managed_state = Some(crate::tr(if checked { "On" } else { "Off" }));
        }
        let locked = self.is_locked();
        let sw = Switch::new((self.id.clone(), "sw"))
            .label(self.title.clone())
            .hide_label(true)
            .size(SwitchSize::Sm)
            .checked(checked)
            .disabled(locked)
            .on_checked_change(on_change);
        self.control = Some(sw.into_any_element());
        self
    }

    /// Carbon `ContentSwitcher` (`ToggleGroup`, `sm` 32px) for 2–3 choices,
    /// each as wide as its label is in the language shown.
    pub fn segmented<T: Copy + PartialEq + 'static>(
        mut self,
        choices: impl IntoIterator<Item = (SharedString, T)>,
        current: T,
        on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
    ) -> Self {
        let locked = self.is_locked();
        let list: Vec<(SharedString, T)> = choices.into_iter().collect();
        let active_ix = list.iter().position(|(_, v)| *v == current).unwrap_or(0);
        let values: Vec<T> = list.iter().map(|(_, v)| *v).collect();
        let mut group = ToggleGroup::new((self.id.clone(), "seg"))
            .fit_content(true)
            .size(ToggleSize::Sm)
            .value([active_ix.to_string()]);
        for (ix, (label, _)) in list.into_iter().enumerate() {
            group = group.item(
                ToggleGroupItem::new(ix.to_string())
                    .disabled(locked)
                    .child(label),
            );
        }
        group = group.on_value_change(move |vals, window, cx| {
            if let Some(ix) = vals.first().and_then(|s| s.parse::<usize>().ok())
                && let Some(&picked) = values.get(ix)
            {
                on_pick(picked, window, cx);
            }
        });
        self.control = Some(group.into_any_element());
        self
    }

    /// Carbon `Select` (`sm` 32px on `field-02`, `240px..320px`).
    pub fn select<T: Clone + PartialEq + 'static>(
        mut self,
        choices: impl IntoIterator<Item = (SharedString, T)>,
        current: &T,
        on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
    ) -> Self {
        let locked = self.is_locked();
        let list: Vec<(SharedString, T)> = choices.into_iter().collect();
        let active_ix = list.iter().position(|(_, v)| v == current);
        let items: Vec<ListItem> = list
            .iter()
            .enumerate()
            .map(|(ix, (label, _))| ListItem::labeled(ix.to_string(), label.clone()))
            .collect();
        let values: Vec<T> = list.into_iter().map(|(_, v)| v).collect();
        let sel = Select::new((self.id.clone(), "sel"))
            .size(FieldSize::Sm)
            .layer(LayerLevel::Two)
            .disabled(locked)
            .items(items)
            .value(active_ix.map(|ix| ix.to_string()))
            .on_value_change(move |val, window, cx| {
                if let Some(ix) = val.and_then(|s| s.parse::<usize>().ok())
                    && let Some(picked) = values.get(ix).cloned()
                {
                    on_pick(picked, window, cx);
                }
            });
        self.control = Some(
            div()
                .min_w(px(240.))
                .max_w(px(320.))
                .child(sel)
                .into_any_element(),
        );
        self
    }

    /// `240px` `field-02` text input (`mono: true` for paths, prefixes and
    /// commands).
    pub fn text_input(mut self, state: &Entity<InputState>, faces: &Faces, mono: bool) -> Self {
        let field = field(Input::new(state), faces, mono, self.is_locked()).w(px(240.));
        self.control = Some(field.into_any_element());
        self
    }

    /// Carbon `sm` action button (`ButtonVariant::Tertiary` by default).
    pub fn button(
        mut self,
        label: impl Into<SharedString>,
        variant: ButtonVariant,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let locked = self.is_locked();
        let btn = Button::new((self.id.clone(), "btn"), label)
            .variant(variant)
            .size(ButtonSize::Sm)
            .disabled(locked)
            .on_click(move |_, w, cx| on_click(w, cx));
        self.control = Some(btn.into_any_element());
        self
    }

    /// Case-insensitive search match across title and description.
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        self.title.to_lowercase().contains(&q) || self.desc.to_lowercase().contains(&q)
    }
}

impl RenderOnce for SRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let faces = crate::fonts::faces(cx);
        let id = self.id.clone();
        let locked = self.is_locked();

        let mut title_row = div().flex().flex_wrap().items_center().gap(px(8.)).child(
            ui::text(faces, self.title.clone())
                .text_size(px(crate::style::BODY))
                .line_height(px(20.))
                .text_color(theme::text()),
        );
        if self.overridden {
            title_row = title_row.child(
                Badge::new(crate::tr("Overridden"))
                    .variant(BadgeVariant::Blue)
                    .size(BadgeSize::Sm),
            );
        }
        if self.managed {
            title_row = title_row.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        CarbonGlyph::carbon(CarbonIcon::Locked)
                            .size_length(px(14.))
                            .color(theme::role(ColorRole::IconSecondary)),
                    )
                    .child(
                        ui::text(faces, crate::tr("Managed"))
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(16.))
                            .text_color(theme::text2()),
                    ),
            );
        }

        let reset_link = |suffix: &'static str, f: ResetHandler| {
            div()
                .id((id.clone(), suffix))
                .cursor_pointer()
                .text_size(px(crate::style::CAPTION))
                .line_height(px(16.))
                .text_color(theme::link())
                .hover(gpui::Styled::underline)
                .on_click(move |_, w, cx| f(w, cx))
                .child(crate::tr("Reset"))
        };

        let overridden_note = self.overridden.then(|| {
            let mut note = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(4.))
                .text_size(px(crate::style::CAPTION))
                .line_height(px(16.))
                .text_color(theme::text2());
            if let Some(by) = self.overridden_by.clone() {
                note = note.child(format!(
                    "{} ·",
                    crate::tr_with("Set by %{by}", &[("by", &by)])
                ));
            }
            if let Some(inherited) = self.inherited.clone() {
                note = note.child(format!(
                    "{} ·",
                    crate::tr_with("Global value: %{value}", &[("value", &inherited)])
                ));
            }
            if let Some(f) = self.on_reset.clone() {
                note = note.child(reset_link("reset-over", f));
            }
            note
        });

        let modified_note = (self.modified && !self.overridden && !locked).then(|| {
            let mut note = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(4.))
                .text_size(px(crate::style::CAPTION))
                .line_height(px(16.))
                .text_color(theme::text2())
                .child(format!("{} ·", crate::tr("Changed from default")));
            if let Some(f) = self.on_reset.clone() {
                note = note.child(reset_link("reset-mod", f));
            }
            note
        });

        let policy_line = self.policy.clone().map(|p| {
            div()
                .flex()
                .items_start()
                .gap(px(4.))
                .child(
                    CarbonGlyph::carbon(CarbonIcon::Locked)
                        .size_length(px(14.))
                        .color(theme::role(ColorRole::IconSecondary))
                        .flex_none(),
                )
                .child(
                    ui::text(faces, p)
                        .text_size(px(crate::style::CAPTION))
                        .line_height(px(16.))
                        .text_color(theme::text2()),
                )
        });

        let err_msg = if self.unreadable {
            Some(self.error.clone().unwrap_or_else(|| {
                crate::tr(
                    "settings.json does not read; not saved. Fix or move it, then reopen Settings.",
                )
            }))
        } else {
            self.error.clone()
        };
        let has_error = err_msg.is_some();
        let error_line = err_msg.map(|msg| {
            div()
                .flex()
                .items_start()
                .gap(px(4.))
                .child(
                    CarbonGlyph::carbon(CarbonIcon::ErrorFilled)
                        .size_length(px(14.))
                        .color(theme::role(ColorRole::SupportError))
                        .flex_none(),
                )
                .child(
                    ui::text(faces, msg)
                        .text_size(px(crate::style::CAPTION))
                        .line_height(px(16.))
                        .text_color(theme::role(ColorRole::SupportError)),
                )
        });

        let right_ctl = div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .children(self.managed_state.map(|state| {
                ui::text(faces, state)
                    .text_size(px(crate::style::BODY))
                    .line_height(px(20.))
                    .text_color(theme::text2())
            }))
            .children(self.control)
            .when(locked, |d| {
                d.when(self.disabled && !self.managed, |d| d.opacity(0.55))
                    .child(div().absolute().inset_0().occlude())
            });

        div()
            .id(id)
            .flex()
            .w_full()
            .items_center()
            .gap(px(32.))
            .min_h(px(64.))
            .py(px(16.))
            .px(px(20.))
            .when(has_error, |d| {
                d.shadow(vec![BoxShadow {
                    color: theme::role(ColorRole::SupportError),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: true,
                }])
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(4.))
                    .child(title_row)
                    .when(!self.desc.is_empty(), |d| {
                        d.child(
                            ui::text(faces, self.desc)
                                .text_size(px(crate::style::CAPTION))
                                .line_height(px(16.))
                                .text_color(theme::text2()),
                        )
                    })
                    .children(overridden_note)
                    .children(policy_line)
                    .children(error_line)
                    .children(modified_note),
            )
            .child(right_ctl)
    }
}

/// A Carbon `sm` text field on `field-02` (32px, a `border-strong-01` rule
/// under it): gpui-kit's `input` drawn bare inside it, as wide as its
/// caller sets; `mono` for paths, commands and names.
pub fn field(input: Input, faces: &Faces, mono: bool, locked: bool) -> gpui::Div {
    div()
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .bg(theme::role(ColorRole::Field02))
        .border_b_1()
        .border_color(if locked {
            theme::role(ColorRole::BorderSubtle01)
        } else {
            theme::role(ColorRole::BorderStrong01)
        })
        .when(mono, |d| d.font_family(faces.mono.clone()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(input.appearance(false).small().disabled(locked)),
        )
}

/// A titled Carbon settings section holding `SRow`s on `layer-01`.
pub struct SGroup {
    title: SharedString,
    desc: Option<SharedString>,
    rows: Vec<SRow>,
}

impl SGroup {
    pub fn new(title: impl Into<SharedString>, rows: Vec<SRow>) -> Self {
        Self {
            title: title.into(),
            desc: None,
            rows,
        }
    }

    pub fn description(mut self, desc: impl Into<SharedString>) -> Self {
        self.desc = Some(desc.into());
        self
    }

    fn into_section(self, query: &str) -> Option<SettingsSection> {
        let group_hit = !query.is_empty()
            && (self.title.to_lowercase().contains(&query.to_lowercase())
                || self
                    .desc
                    .as_ref()
                    .is_some_and(|d| d.to_lowercase().contains(&query.to_lowercase())));
        let rows: Vec<SRow> = if group_hit || query.is_empty() {
            self.rows
        } else {
            self.rows.into_iter().filter(|r| r.matches(query)).collect()
        };
        if rows.is_empty() {
            return None;
        }
        let mut sec = SettingsSection::new(self.title);
        if let Some(d) = self.desc {
            sec = sec.description(d);
        }
        sec.extend(rows.into_iter().map(IntoElement::into_any_element));
        Some(sec)
    }
}

/// Scrollable 720px-centered Carbon settings page (`SetGeneral` / `SetAppearance`
/// layout): `WorkspacePageHeader`, optional top banner, and `SettingsSection`s
/// 32px apart.
pub fn carbon_page(
    id: &'static str,
    title: SharedString,
    lede: SharedString,
    banner: Option<AnyElement>,
    groups: Vec<SGroup>,
    query: &str,
    faces: &Faces,
) -> AnyElement {
    carbon_page_with_tail(id, title, lede, banner, groups, None, query, faces)
}

/// Like [`carbon_page`], with an optional custom `tail` section rendered after
/// the `SettingsSection`s inside the same 720px column (`SetAppearance` `Themes`).
#[allow(
    clippy::too_many_arguments,
    reason = "a Carbon settings page takes its id, header copy, optional banner, groups, optional tail, query and faces"
)]
pub fn carbon_page_with_tail(
    id: &'static str,
    title: SharedString,
    lede: SharedString,
    banner: Option<AnyElement>,
    groups: Vec<SGroup>,
    tail: Option<AnyElement>,
    query: &str,
    faces: &Faces,
) -> AnyElement {
    let sections: Vec<SettingsSection> = groups
        .into_iter()
        .filter_map(|g| g.into_section(query))
        .collect();
    let empty = sections.is_empty() && tail.is_none() && !query.is_empty();
    div()
        .id(id)
        .flex_1()
        .min_h(px(0.))
        .pt(px(24.))
        .px(px(32.))
        .pb(px(48.))
        .child(
            div()
                .w_full()
                .max_w(px(720.))
                .mx_auto()
                .flex()
                .flex_col()
                .gap(px(32.))
                .child(
                    WorkspacePageHeader::new((ElementId::from(id), "head"), title)
                        .description(lede),
                )
                .children(banner)
                .children(sections)
                .children(tail)
                .when(empty, |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .py(px(24.))
                            .child(
                                ui::text(faces, crate::tr("No matching settings"))
                                    .text_size(px(crate::style::BODY))
                                    .weight(gpui::FontWeight::SEMIBOLD),
                            )
                            .child(
                                ui::text(
                                    faces,
                                    crate::tr("Try another word, or clear the search."),
                                )
                                .text_size(px(crate::style::CAPTION))
                                .text_color(theme::text2()),
                            ),
                    )
                }),
        )
        // gpui-kit's scroll area, as the settings' page list (see there).
        .overflow_y_scrollbar()
        .into_any_element()
}
