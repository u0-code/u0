//! `Customize <theme>`: a theme of yours in a panel over the settings (board
//! `SetAppearance`), u0ui's `Sheet` holding its `ThemeEditor` laid out to
//! customize: the main colours beside a picker and the contrast checks, a
//! preview, and every colour by hex. Each edit is saved as it is made.

use gpui::{AnyElement, Context, SharedString, div, prelude::*};
use u0ui::{
    components::{
        Sheet,
        button::{Button as CarbonButton, ButtonSize, ButtonVariant},
        dropdown_menu::{DropdownMenu, DropdownMenuKind, DropdownMenuSize},
        menu::MenuItem,
    },
    icons::CarbonIcon,
    u0::settings::{ThemeEditor, ThemeEditorLayout, ThemeEditorTexts, ThemeToken},
};

use crate::{desk::Desk, themes};

impl Desk {
    /// The panel the theme open to customize is edited in, over the
    /// settings; none while no theme is open.
    pub(super) fn theme_editor_drawer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let name = self.pages.editing_theme.clone()?;
        let config = themes::find(cx, &name)?;
        let tokens =
            themes::role_values(&config).map(|(role, value)| ThemeToken::new(role.name(), value));
        let primary = themes::PRIMARY_ROLES.map(|p| {
            (themes::EDITOR_ROLES.iter())
                .position(|(role, _)| *role == p)
                .expect("a primary role is edited")
        });
        let texts = ThemeEditorTexts {
            import: crate::tr("Import a VS Code theme…"),
            export: crate::tr("Export"),
            preview_title: crate::tr("Preview"),
            preview_body: crate::tr("Secondary text on layer."),
            preview_primary: crate::tr("Primary"),
            contrast: crate::tr("Contrast"),
            contrast_text: crate::tr("Text on background"),
            contrast_secondary: crate::tr("Secondary text on layer"),
            contrast_button: crate::tr("Button text on interactive"),
            tokens_title: crate::tr("Tokens"),
            tokens_description: crate::tr(
                "Each colour of the theme, as gpui-kit’s theme file names it. Pick a swatch or type a hex.",
            ),
            picker: [
                crate::tr("Saturation and brightness"),
                crate::tr("Hue"),
                crate::tr("Hex colour"),
                crate::tr("Swatches"),
            ],
        };
        let desk = cx.entity();
        let editor = ThemeEditor::new(
            SharedString::from(format!("theme-editor-{}", crate::git::slug(&name))),
            tokens,
        )
        .layout(ThemeEditorLayout::Customize)
        .name(name.clone())
        .primary(primary)
        .texts(texts)
        .on_change(move |n, value, _, cx| {
            if let Some((role, _)) = themes::EDITOR_ROLES.get(n) {
                desk.update(cx, |this, cx| this.set_edited_theme_role(*role, value, cx));
            }
        });
        let about = crate::tr(if config.mode.is_dark() {
            "Your dark theme. Edits save as you go."
        } else {
            "Your light theme. Edits save as you go."
        });
        let (close, dup, file, reload, reset, remove) = (
            cx.entity(),
            cx.entity(),
            cx.entity(),
            cx.entity(),
            cx.entity(),
            cx.entity(),
        );
        let (dup_name, file_name, rm_name) = (name.clone(), name.clone(), name.clone());
        let more = DropdownMenu::new("drawer-overflow")
            .kind(DropdownMenuKind::Ghost)
            .size(DropdownMenuSize::Sm)
            .overflow_icon(CarbonIcon::OverflowMenuHorizontal)
            .overflow_label(crate::tr("Export as a theme file, or reset"))
            .item(
                MenuItem::item(crate::tr("Open file in editor")).on_select(move |_, cx| {
                    file.update(cx, |this, cx| this.open_edited_theme_file(&file_name, cx));
                }),
            )
            .item(
                MenuItem::item(crate::tr("Read themes again"))
                    .on_select(move |_, cx| reload.update(cx, Desk::read_themes)),
            )
            .item(MenuItem::divider())
            .item(
                MenuItem::item(crate::tr("Reset to Carbon defaults")).on_select(move |_, cx| {
                    reset.update(cx, Desk::reset_edited_theme);
                }),
            );
        Some(
            Sheet::new("theme-drawer")
                .contained(true)
                .open(true)
                .width(600.)
                .footer_row(true)
                .title(crate::tr_with("Customize %{name}", &[("name", &name)]))
                .description(about)
                .close_label(crate::tr("Close"))
                .on_open_change(move |open, _, cx| {
                    if !open {
                        close.update(cx, Desk::close_theme_editor);
                    }
                })
                .footer(
                    CarbonButton::new("drawer-duplicate", crate::tr("Duplicate"))
                        .variant(ButtonVariant::Tertiary)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            dup.update(cx, |this, cx| {
                                this.duplicate_edited_theme(&dup_name, cx);
                            });
                        }),
                )
                .footer(more)
                .footer(div().flex_1())
                .footer(
                    CarbonButton::new("drawer-remove", crate::tr("Remove theme"))
                        .variant(ButtonVariant::DangerGhost)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            remove.update(cx, |this, cx| this.remove_theme(&rm_name, cx));
                        }),
                )
                .child(editor)
                .into_any_element(),
        )
    }
}
