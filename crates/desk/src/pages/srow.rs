//! Carbon settings row (`SRow` board), section (`SGroup`) and page container
//! (`carbon_page`), built on `u0ui::components` (`Switch`, `ToggleGroup`,
//! `Select`, `Button`, `Badge`), `u0ui::t3::shell::WorkspacePageHeader` and
//! `u0ui::t3::workspace::settings::SettingsSection`.

use std::rc::Rc;

use gpui::component::{
    Sizable as _, input::Input, input::InputState, scroll::ScrollableElement as _,
};
use gpui::{
    AnyElement, App, BoxShadow, Context, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window, div, point, prelude::FluentBuilder as _, px,
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
    desk::Desk,
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
        self.control = Some(switch(
            (self.id.clone(), "sw"),
            self.title.clone(),
            checked,
            locked,
            on_change,
        ));
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
        self.control = Some(segmented(
            (self.id.clone(), "seg"),
            choices,
            current,
            locked,
            on_pick,
        ));
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
        self.control = Some(select(
            (self.id.clone(), "sel"),
            choices,
            current,
            locked,
            on_pick,
        ));
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
                    ui::fine_said(faces, (id.clone(), "error"), msg)
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

/// Carbon's `Switch` (`sm` 32×16), named `label` for a screen reader.
pub fn switch(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    locked: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    Switch::new(id)
        .label(label)
        .hide_label(true)
        .size(SwitchSize::Sm)
        .checked(checked)
        .disabled(locked)
        .on_checked_change(on_change)
        .into_any_element()
}

/// Carbon's `sm` button on a settings page: `Secondary` is its tertiary,
/// `Quiet` its ghost.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    weight: ui::Weight,
) -> Button {
    Button::new(id, label)
        .size(ButtonSize::Sm)
        .variant(match weight {
            ui::Weight::Primary => ButtonVariant::Primary,
            ui::Weight::Secondary => ButtonVariant::Tertiary,
            ui::Weight::Quiet => ButtonVariant::Ghost,
        })
}

/// Carbon's `Switch` (`sm`), named `label` for a screen reader, that
/// calls `flipped` on the desk as it is flipped.
pub fn flip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    cx: &Context<Desk>,
    flipped: impl Fn(&mut Desk, &mut Window, &mut Context<Desk>) + 'static,
) -> Switch {
    let desk = cx.entity().downgrade();
    Switch::new(id)
        .label(label)
        .hide_label(true)
        .size(SwitchSize::Sm)
        .checked(checked)
        .on_checked_change(move |_, window, cx| {
            let _ = desk.update(cx, |d, cx| flipped(d, window, cx));
        })
}

/// Carbon's `ContentSwitcher` (`ToggleGroup`, `sm` 32px) for a few
/// choices, each as wide as its label is in the language shown.
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: impl Into<ElementId>,
    choices: impl IntoIterator<Item = (SharedString, T)>,
    current: T,
    locked: bool,
    on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let list: Vec<(SharedString, T)> = choices.into_iter().collect();
    let active_ix = list.iter().position(|(_, v)| *v == current).unwrap_or(0);
    let values: Vec<T> = list.iter().map(|(_, v)| *v).collect();
    let mut group = ToggleGroup::new(id)
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
    group
        .on_value_change(move |vals, window, cx| {
            if let Some(ix) = vals.first().and_then(|s| s.parse::<usize>().ok())
                && let Some(&picked) = values.get(ix)
            {
                on_pick(picked, window, cx);
            }
        })
        .into_any_element()
}

/// Carbon's `ContentSwitcher` over choices that each carry what picking
/// it does, the one now chosen marked.
pub fn picks(
    id: impl Into<SharedString>,
    choices: Vec<(impl Into<SharedString>, bool, ui::Pick)>,
) -> AnyElement {
    let id: SharedString = id.into();
    let mut labels = Vec::new();
    let mut picks: Vec<ui::Pick> = Vec::new();
    let mut now = 0;
    for (n, (label, on, pick)) in choices.into_iter().enumerate() {
        labels.push((label.into(), n));
        picks.push(pick);
        if on {
            now = n;
        }
    }
    segmented(id, labels, now, false, move |n, window, cx| {
        picks[n](&vec![n], window, cx);
    })
}

/// Carbon's `Select` (`sm` 32px on `field-02`, `240px..320px`).
pub fn select<T: Clone + PartialEq + 'static>(
    id: impl Into<ElementId>,
    choices: impl IntoIterator<Item = (SharedString, T)>,
    current: &T,
    locked: bool,
    on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let list: Vec<(SharedString, T)> = choices.into_iter().collect();
    let active_ix = list.iter().position(|(_, v)| v == current);
    let items: Vec<ListItem> = list
        .iter()
        .enumerate()
        .map(|(ix, (label, _))| ListItem::labeled(ix.to_string(), label.clone()))
        .collect();
    let values: Vec<T> = list.into_iter().map(|(_, v)| v).collect();
    let sel = Select::new(id)
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
    div()
        .min_w(px(240.))
        .max_w(px(320.))
        .child(sel)
        .into_any_element()
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

/// A titled Carbon settings section holding rows on `layer-01`: `SRow`s, or
/// what a page draws whole, each with the words a search finds it by.
pub struct SGroup {
    title: SharedString,
    desc: Option<SharedString>,
    rows: Vec<(SharedString, AnyElement)>,
}

impl SGroup {
    pub fn new(title: impl Into<SharedString>, rows: Vec<SRow>) -> Self {
        let rows = rows
            .into_iter()
            .map(|r| {
                (
                    format!("{} {}", r.title, r.desc).into(),
                    r.into_any_element(),
                )
            })
            .collect();
        Self::of(title, rows)
    }

    /// A section of rows already drawn, each with the words it is found by.
    pub fn of(title: impl Into<SharedString>, rows: Vec<(SharedString, AnyElement)>) -> Self {
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

    fn into_section(self, query: &str) -> Option<AnyElement> {
        let q = query.to_lowercase();
        let group_hit = !q.is_empty()
            && (self.title.to_lowercase().contains(&q)
                || self
                    .desc
                    .as_ref()
                    .is_some_and(|d| d.to_lowercase().contains(&q)));
        let rows: Vec<AnyElement> = self
            .rows
            .into_iter()
            .filter(|(words, _)| group_hit || q.is_empty() || words.to_lowercase().contains(&q))
            .map(|(_, row)| row)
            .collect();
        if rows.is_empty() {
            return None;
        }
        if self.title.is_empty() {
            // A section with no heading is its rows alone, on the same layer.
            return Some(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .bg(theme::role(ColorRole::Layer01))
                    .children(rows)
                    .into_any_element(),
            );
        }
        let mut sec = SettingsSection::new(self.title);
        if let Some(d) = self.desc {
            sec = sec.description(d);
        }
        sec.extend(rows);
        Some(sec.into_any_element())
    }
}

/// Scrollable 720px-centered Carbon settings page (`SetGeneral` / `SetAppearance`
/// layout): `WorkspacePageHeader`, optional top banner, and `SettingsSection`s
/// 32px apart.
pub fn carbon_page(
    id: impl Into<SharedString>,
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
    id: impl Into<SharedString>,
    title: SharedString,
    lede: SharedString,
    banner: Option<AnyElement>,
    groups: Vec<SGroup>,
    tail: Option<AnyElement>,
    query: &str,
    faces: &Faces,
) -> AnyElement {
    let sections: Vec<AnyElement> = groups
        .into_iter()
        .filter_map(|g| g.into_section(query))
        .collect();
    let empty = sections.is_empty() && tail.is_none() && !query.is_empty();
    let id: SharedString = id.into();
    div()
        .id(id.clone())
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

/// How the desk draws a control or a row, as the page is drawn.
type Draw = dyn Fn(&mut Desk, &Faces, &mut Context<Desk>) -> AnyElement;

/// A control the desk, `w`, draws once its page is laid out: the desk is
/// not borrowed then, as it is while it builds the page.
#[derive(IntoElement)]
struct Drawn {
    w: WeakEntity<Desk>,
    draw: Rc<Draw>,
}

impl RenderOnce for Drawn {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.w
            .update(cx, |d, cx| {
                let faces = crate::fonts::faces(cx).clone();
                (self.draw)(d, &faces, cx)
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

/// A setting on a page: an `SRow` of its name, what it does and its
/// control, or something the page draws whole.
pub struct Item {
    name: SharedString,
    about: SharedString,
    words: Vec<SharedString>,
    drawn: Drawn,
    whole: bool,
    stacked: bool,
    disabled: bool,
}

impl Item {
    /// The words it is found by besides its name and what it does.
    pub fn keywords(mut self, words: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.words.extend(words.into_iter().map(Into::into));
        self
    }

    /// Its control under its name, as wide as the row: a field to write in.
    pub fn stacked(mut self) -> Self {
        self.stacked = true;
        self
    }

    /// Its control shown, but not to be changed.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn into_row(self, id: SharedString, faces: &Faces) -> (SharedString, AnyElement) {
        let words = format!("{} {} {}", self.name, self.about, self.words.join(" ")).into();
        let row = if self.stacked {
            div()
                .id(id)
                .flex()
                .flex_col()
                .gap(px(8.))
                .w_full()
                .px(px(20.))
                .py(px(16.))
                .child(
                    ui::text(faces, self.name)
                        .text_size(px(crate::style::BODY))
                        .line_height(px(20.))
                        .text_color(theme::text()),
                )
                .when(!self.about.is_empty(), |d| {
                    d.child(
                        ui::text(faces, self.about)
                            .text_size(px(crate::style::CAPTION))
                            .line_height(px(16.))
                            .text_color(theme::text2()),
                    )
                })
                .child(self.drawn)
                .into_any_element()
        } else if self.whole {
            div()
                .id(id)
                .w_full()
                .px(px(20.))
                .py(px(12.))
                .child(self.drawn)
                .into_any_element()
        } else {
            SRow::new(id, self.name)
                .description(self.about)
                .disabled(self.disabled)
                .control(self.drawn)
                .into_any_element()
        };
        (words, row)
    }
}

/// A setting: what it is, said plainly (and found by the search), and its
/// control, which the desk, `w`, draws.
pub(crate) fn item(
    w: &WeakEntity<Desk>,
    name: impl Into<SharedString>,
    about: impl Into<SharedString>,
    control: impl Fn(&mut Desk, &Faces, &mut Context<Desk>) -> AnyElement + 'static,
) -> Item {
    Item {
        name: crate::tr(&name.into()),
        about: crate::tr(&about.into()),
        words: Vec::new(),
        drawn: Drawn {
            w: w.clone(),
            draw: Rc::new(control),
        },
        whole: false,
        stacked: false,
        disabled: false,
    }
}

/// What the desk, `w`, draws whole as one setting, found by `words`.
pub(crate) fn whole(
    w: &WeakEntity<Desk>,
    words: &[&'static str],
    draw: impl Fn(&mut Desk, &Faces, &mut Context<Desk>) -> AnyElement + 'static,
) -> Item {
    Item {
        name: SharedString::default(),
        about: SharedString::default(),
        words: words.iter().copied().map(SharedString::from).collect(),
        drawn: Drawn {
            w: w.clone(),
            draw: Rc::new(draw),
        },
        whole: true,
        stacked: false,
        disabled: false,
    }
}

/// Settings under a heading, or under none.
#[derive(Default)]
pub struct Group {
    title: SharedString,
    about: Option<SharedString>,
    items: Vec<Item>,
}

impl Group {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    pub fn description(mut self, about: impl Into<SharedString>) -> Self {
        self.about = Some(about.into());
        self
    }

    pub fn item(mut self, item: Item) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = Item>) -> Self {
        self.items.extend(items);
        self
    }
}

/// Settings under `caption`.
pub(crate) fn group(caption: impl Into<SharedString>, items: Vec<Item>) -> Group {
    Group::new().title(crate::tr(&caption.into())).items(items)
}

/// A settings page: its title, what it is for, and its groups, drawn as
/// every page is (`carbon_page`).
pub struct Page {
    title: SharedString,
    lede: SharedString,
    groups: Vec<Group>,
}

impl Page {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            lede: SharedString::default(),
            groups: Vec::new(),
        }
    }

    pub fn description(mut self, lede: impl Into<SharedString>) -> Self {
        self.lede = lede.into();
        self
    }

    pub fn group(mut self, group: Group) -> Self {
        self.groups.push(group);
        self
    }

    pub fn groups(mut self, groups: impl IntoIterator<Item = Group>) -> Self {
        self.groups.extend(groups);
        self
    }

    /// The page drawn, `banner` over its groups, showing what `query` finds.
    pub fn draw(
        self,
        id: &str,
        banner: Option<AnyElement>,
        query: &str,
        faces: &Faces,
    ) -> AnyElement {
        let groups = (self.groups.into_iter().enumerate())
            .map(|(g, group)| {
                let rows = (group.items.into_iter().enumerate())
                    .map(|(i, item)| item.into_row(format!("{id}-{g}-{i}").into(), faces))
                    .collect();
                let section = SGroup::of(group.title, rows);
                match group.about {
                    Some(about) => section.description(about),
                    None => section,
                }
            })
            .collect();
        carbon_page(
            SharedString::from(id.to_owned()),
            self.title,
            self.lede,
            banner,
            groups,
            query,
            faces,
        )
    }
}
