//! Sidebar search results (`SidebarStates` board): `"Titles"`, `"What was
//! said"` (with inline `<mark>` snippet highlight), and `"Branches and PRs"`.

use std::fmt::Write as _;
use std::time::SystemTime;

use gpui::component::menu::ContextMenuExt as _;
use gpui::{
    AnyElement, Context, Entity, HighlightStyle, SharedString, StyledText, div, prelude::*, px, rgb,
};
use u0ui::tokens::ColorRole;

use super::list::{Row, tagged};
use crate::{
    desk::{Desk, Place, rank},
    fonts::{Faces, Weighted as _},
    session::{Item, Session},
    theme, ui, when,
};

impl Desk {
    pub(super) fn render_search_results(
        &self,
        faces: &Faces,
        typed: &str,
        query: &rank::Query,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let desk = cx.entity();
        let now = SystemTime::now();
        let needle = typed.trim_start_matches('#');
        let mut title_hits = Vec::new();
        let mut said_hits = Vec::new();
        let mut branch_hits = Vec::new();

        for s in &self.sessions.all {
            if s.parent.is_some() || s.beside.is_some() || s.meta.flags.archived {
                continue;
            }
            if !self.filter.is_empty() && !self.filter.contains(&Place::project_of(s)) {
                continue;
            }
            if !self.matches_tags(s) || !self.answers(s, query, now) {
                continue;
            }
            let title_and_tags = tagged(&s.meta.title, &s.meta.tags);
            if contains_ci(&title_and_tags, needle) {
                title_hits.push(s);
            } else if let Some(snippet) = match_snippet(s, needle) {
                said_hits.push((s, snippet));
            } else if self.search.found.contains(&s.key) {
                said_hits.push((s, s.meta.title.clone()));
            } else {
                branch_hits.push(s);
            }
        }

        let mut out: Vec<AnyElement> = Vec::new();
        let head = |label: SharedString, count: usize, top: f32| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .h(px(28.))
                .mt(px(top))
                .px(px(12.))
                .child(
                    ui::text(faces, label)
                        .flex_1()
                        .text_size(px(crate::style::CAPTION))
                        .weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::text()),
                )
                .child(
                    ui::text(faces, count.to_string())
                        .text_size(px(crate::style::CAPTION))
                        .text_color(theme::text2()),
                )
                .into_any_element()
        };

        if !title_hits.is_empty() {
            out.push(head(crate::tr("Titles"), title_hits.len(), 0.));
            for s in title_hits {
                let where_line = format!(
                    "{} · {}",
                    self.project_on(&Place::project_of(s)),
                    self.name_of(&s.meta.agent)
                );
                out.push(self.search_hit_row(
                    faces,
                    s.key,
                    &tagged(s.shown_title(), &s.meta.tags),
                    Some(&where_line),
                    None,
                    needle,
                    &desk,
                ));
            }
        }
        if !said_hits.is_empty() {
            let top = if out.is_empty() { 0. } else { 12. };
            out.push(head(crate::tr("What was said"), said_hits.len(), top));
            for (s, snippet) in said_hits {
                let where_line = format!(
                    "{} · {}",
                    self.name_of(&s.meta.agent),
                    when::ago(s.meta.updated, now)
                );
                out.push(self.search_hit_row(
                    faces,
                    s.key,
                    s.shown_title(),
                    Some(&snippet),
                    Some(&where_line),
                    needle,
                    &desk,
                ));
            }
        }
        if !branch_hits.is_empty() {
            let top = if out.is_empty() { 0. } else { 12. };
            out.push(head(crate::tr("Branches and PRs"), branch_hits.len(), top));
            for s in branch_hits {
                let mut line = s.branch.clone().unwrap_or_default();
                if let Some(pr) = &s.git.pr {
                    if !line.is_empty() {
                        line.push_str(" · ");
                    }
                    let _ = write!(line, "PR #{} {}", pr.number, pr.state);
                }
                out.push(self.search_hit_row(
                    faces,
                    s.key,
                    s.shown_title(),
                    Some(&line),
                    None,
                    needle,
                    &desk,
                ));
            }
        }
        out
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a search hit carries title, snippet, location, query needle and desk entity"
    )]
    fn search_hit_row(
        &self,
        faces: &Faces,
        key: u64,
        title: &str,
        sub: Option<&str>,
        where_line: Option<&str>,
        needle: &str,
        desk: &Entity<Self>,
    ) -> AnyElement {
        let active = self.current == Some(key);
        let click = desk.clone();
        let menu_desk = desk.clone();
        let row_id = Row::Session(key).id();
        div()
            .id(SharedString::from(format!("ss-hit-{key}")))
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(2.))
            .pl(px(36.))
            .pr(px(12.))
            .py(px(6.))
            .cursor_pointer()
            .when(active, |d| {
                d.bg(theme::role(ColorRole::LayerSelected01)).child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.))
                        .bg(theme::role(ColorRole::BorderInteractive)),
                )
            })
            .when(!active, |d| {
                d.hover(|st| st.bg(theme::role(ColorRole::LayerHover01)))
            })
            .on_click(move |e, window, cx| {
                click.update(cx, |this, cx| this.side_click(key, e, window, cx));
            })
            .context_menu(move |menu, window, cx| {
                Desk::side_menu(&menu_desk, &row_id, menu, window, cx)
            })
            .child(
                div()
                    .font_family(faces.ui.clone())
                    .text_size(px(crate::style::BODY))
                    .line_height(px(20.))
                    .text_color(theme::text())
                    .truncate()
                    .child(marked_text(title, needle)),
            )
            .children(sub.filter(|s| !s.is_empty()).map(|s| {
                div()
                    .font_family(faces.ui.clone())
                    .text_size(px(crate::style::CAPTION))
                    .line_height(px(16.))
                    .text_color(theme::text2())
                    .truncate()
                    .child(marked_text(s, needle))
            }))
            .children(where_line.filter(|s| !s.is_empty()).map(|w| {
                ui::text(faces, w.to_owned())
                    .truncate()
                    .text_size(px(crate::style::CAPTION))
                    .line_height(px(16.))
                    .text_color(theme::text2())
            }))
            .into_any_element()
    }
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    !needle.is_empty() && haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn match_snippet(s: &Session, needle: &str) -> Option<String> {
    if needle.is_empty() {
        return None;
    }
    let lower_needle = needle.to_lowercase();
    for item in s.items.iter().rev() {
        let text = match item {
            Item::You(t) | Item::Agent(t) => t.as_str(),
            _ => continue,
        };
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.to_lowercase().contains(&lower_needle) {
                return Some(ui::fit(trimmed, 80));
            }
        }
    }
    None
}

/// Inline `<mark>` highlight per `SidebarStates.dc.html`: `#d0e2ff` under
/// `text-primary` in light themes, `#0043ce` under `#ffffff` in dark themes.
fn marked_text(text: &str, needle: &str) -> StyledText {
    let styled = StyledText::new(text.to_owned());
    if needle.is_empty() {
        return styled;
    }
    let lower_t = text.to_lowercase();
    let lower_n = needle.to_lowercase();
    if lower_t.len() != text.len() || lower_n.len() != needle.len() {
        return styled;
    }
    let Some(pos) = lower_t.find(&lower_n) else {
        return styled;
    };
    let end = (pos + lower_n.len()).min(text.len());
    if !text.is_char_boundary(pos) || !text.is_char_boundary(end) {
        return styled;
    }
    let (bg, fg) = if theme::is_light() {
        (rgb(0x00d0_e2ff).into(), theme::text())
    } else {
        (rgb(0x0000_43ce).into(), gpui::white())
    };
    styled.with_highlights([(
        pos..end,
        HighlightStyle {
            background_color: Some(bg),
            color: Some(fg),
            ..Default::default()
        },
    )])
}
