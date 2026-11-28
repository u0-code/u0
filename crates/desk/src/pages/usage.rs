//! The Usage workspace page (`Usage` board, ⌘U): tokens, turns and
//! subscription limits strictly as each agent reported them. u0 adds no
//! figures of its own and puts no price on them.

use std::{collections::BTreeMap, fmt::Write as _, path::PathBuf, rc::Rc, time::SystemTime};

use gpui::component::scroll::ScrollableElement as _;
use gpui::{Context, Div, IntoElement, SharedString, canvas, div, prelude::*, px};
use u0ui::components::{
    Select, ToggleGroup, ToggleGroupItem,
    data_table::{DataTable, DataTableColumn, DataTableRow, TableSize},
    form::FieldSize,
    list_box::ListItem,
    toggle::ToggleSize,
};
use u0ui::prelude::*;
use u0ui::t3::workspace::{StatBlock, StatsGrid, UsageTexts, UsageWindowBar};

use crate::{
    desk::{Desk, Page, Place},
    hub::Host,
    pages::Tab,
    store, theme,
    usage::{Limit, Use, Used},
    when,
};

/// The periods the page reads over, in days (`None` = all history).
pub const PERIODS: [(&str, &str, Option<u64>); 5] = [
    ("24h", "24 hours", Some(1)),
    ("7d", "7 days", Some(7)),
    ("30d", "30 days", Some(30)),
    ("90d", "90 days", Some(90)),
    ("all", "All", None),
];

/// Tokens reported across turns: input, output, cache read, cache write.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Took {
    input: u64,
    output: u64,
    read: u64,
    wrote: u64,
    has_read: bool,
    has_wrote: bool,
}

impl Took {
    fn add(&mut self, o: Self) {
        self.input += o.input;
        self.output += o.output;
        self.read += o.read;
        self.wrote += o.wrote;
        self.has_read |= o.has_read;
        self.has_wrote |= o.has_wrote;
    }

    fn total(self) -> u64 {
        self.input + self.output + self.read + self.wrote
    }
}

/// Format a token count cleanly: `0`, `950`, `12.4k`, `4.3M`, `1.2B`.
fn count(n: u64) -> String {
    #[allow(
        clippy::cast_precision_loss,
        reason = "token counts formatted to one decimal"
    )]
    let f = n as f64;
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", f / 1e3),
        1_000_000_000.. => format!("{:.1}B", f / 1e9),
        _ => format!("{:.1}M", f / 1e6),
    }
}

/// Format a cell value for `By model`: an em-dash where none was reported.
fn cell_count(n: u64, reported: bool) -> String {
    if !reported && n == 0 {
        "\u{2014}".into()
    } else {
        count(n)
    }
}

/// Choose a clean chart ceiling above `peak` so the top and mid ticks read cleanly.
fn nice_ceiling(peak: u64) -> u64 {
    if peak == 0 {
        return 10_000_000;
    }
    let exp = 10u64.pow(peak.ilog10());
    for mult in [1u64, 2, 5, 10] {
        let cand = mult.saturating_mul(exp);
        if cand >= peak {
            return cand;
        }
    }
    peak
}

/// Format a Y-axis tick in the same unit as `ceiling` (`0M`, `5M`, `10M`).
fn axis_count(val: u64, ceiling: u64) -> String {
    let (div, suffix) = if ceiling >= 1_000_000_000 {
        (1_000_000_000u64, "B")
    } else if ceiling >= 1_000_000 {
        (1_000_000u64, "M")
    } else if ceiling >= 1_000 {
        (1_000u64, "k")
    } else {
        return val.to_string();
    };
    if val.is_multiple_of(div) {
        format!("{}{suffix}", val / div)
    } else {
        #[allow(clippy::cast_precision_loss, reason = "half-step axis tick")]
        let scaled = val as f64 / div as f64;
        format!("{scaled:.1}{suffix}")
    }
}

/// Latest reported limit per `(agent, window)`.
fn latest_limits<'a>(
    said: impl Iterator<Item = (&'a str, &'a Used)>,
) -> BTreeMap<(String, String), (u64, Limit)> {
    let mut out: BTreeMap<(String, String), (u64, Limit)> = BTreeMap::new();
    for (agent, u) in said {
        if let Use::Limit(l) = &u.what {
            let key = (agent.to_owned(), l.window.clone());
            if out.get(&key).is_none_or(|(at, _)| u.at >= *at) {
                out.insert(key, (u.at, l.clone()));
            }
        }
    }
    out
}

/// Draw a dashed 1px rectangle around `Today`'s stacked bar.
fn dashed_frame(stroke: gpui::Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let dash = 3.0f32;
            let gap = 2.0f32;
            let x0 = f32::from(bounds.origin.x);
            let y0 = f32::from(bounds.origin.y);
            let w = f32::from(bounds.size.width);
            let h = f32::from(bounds.size.height);
            let mut x = 0.0f32;
            while x < w {
                let seg = dash.min(w - x);
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(gpui::point(px(x0 + x), px(y0)), gpui::size(px(seg), px(1.))),
                    stroke,
                ));
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(
                        gpui::point(px(x0 + x), px(y0 + h - 1.)),
                        gpui::size(px(seg), px(1.)),
                    ),
                    stroke,
                ));
                x += dash + gap;
            }
            let mut y = 0.0f32;
            while y < h {
                let seg = dash.min(h - y);
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(gpui::point(px(x0), px(y0 + y)), gpui::size(px(1.), px(seg))),
                    stroke,
                ));
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(
                        gpui::point(px(x0 + w - 1.), px(y0 + y)),
                        gpui::size(px(1.), px(seg)),
                    ),
                    stroke,
                ));
                y += dash + gap;
            }
        },
    )
    .absolute()
    .top(px(-3.))
    .bottom(px(-3.))
    .left(px(-3.))
    .right(px(-3.))
}

/// Draw a horizontal dashed line for the chart's mid-tick.
fn dashed_hline(stroke: gpui::Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let x0 = f32::from(bounds.origin.x);
            let y0 = f32::from(bounds.origin.y);
            let w = f32::from(bounds.size.width);
            let mut x = 0.0f32;
            while x < w {
                let seg = 4.0f32.min(w - x);
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(gpui::point(px(x0 + x), px(y0)), gpui::size(px(seg), px(1.))),
                    stroke,
                ));
                x += 7.0;
            }
        },
    )
    .w_full()
    .h(px(1.))
}

impl Desk {
    /// ⇧⌘1 to ⇧⌘5 on the Usage workspace page select the 5 periods.
    pub(crate) fn usage_keys(&mut self, k: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let m = &k.modifiers;
        let n = ["1", "2", "3", "4", "5"]
            .iter()
            .zip(["!", "@", "#", "$", "%"])
            .position(|(d, s)| (k.key == *d && m.shift) || k.key == s)
            .map(|n| n + 1);
        match n {
            Some(n) if !self.pages.open && self.page == Page::Usage && m.platform => {
                self.pages.period = PERIODS[n - 1].2;
                cx.notify();
                true
            }
            _ => false,
        }
    }

    /// Top bar of the Usage workspace page (`.r2-top`).
    pub(crate) fn usage_topbar(&self, left: Div, right: Div, cx: &mut Context<Self>) -> (Div, Div) {
        let now = SystemTime::now();
        let desk = cx.entity();
        let line = div()
            .type_style(TypeStyle::Label01)
            .text_color(theme::role(ColorRole::TextSecondary))
            .child(when::header_date(now, self.settings.twelve));
        (
            left.child(line),
            right.child(
                Button::new("usage-export-csv", crate::tr("Export CSV"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .icon(CarbonIcon::Download)
                    .on_click(move |_, _, cx| {
                        desk.update(cx, |this, cx| this.export_usage_csv(cx));
                    }),
            ),
        )
    }

    fn export_usage_csv(&self, cx: &mut Context<Self>) {
        let now = SystemTime::now();
        let since = self
            .pages
            .period
            .map_or(0, |days| store::secs(now).saturating_sub(days * 86_400));
        let mut by_model: BTreeMap<(String, String), Took> = BTreeMap::new();
        for s in &self.sessions.all {
            if self.pages.usage_host.is_some_and(|h| s.host != h)
                || self
                    .pages
                    .usage_project
                    .as_ref()
                    .is_some_and(|p| &s.meta.project != p)
            {
                continue;
            }
            let Some(used) = self.used.get(&s.key) else {
                continue;
            };
            let agent = self.name_of(&s.meta.agent);
            for u in used.iter().filter(|u| u.at >= since) {
                if let Use::Tokens(t) = &u.what {
                    let model = t.model.clone().unwrap_or_else(|| "Unspecified".into());
                    by_model
                        .entry((model, agent.clone()))
                        .or_default()
                        .add(Took {
                            input: t.usage.input_tokens,
                            output: t.usage.output_tokens,
                            read: t.usage.cached_read_tokens.unwrap_or(0),
                            wrote: t.usage.cached_write_tokens.unwrap_or(0),
                            has_read: t.usage.cached_read_tokens.is_some(),
                            has_wrote: t.usage.cached_write_tokens.is_some(),
                        });
                }
            }
        }
        let mut csv = String::from("model,agent,input,cache_write,cache_read,output\n");
        for ((model, agent), t) in by_model {
            let _ = writeln!(
                csv,
                "{:?},{:?},{},{},{},{}",
                model, agent, t.input, t.wrote, t.read, t.output
            );
        }
        let home = Self::home_path().unwrap_or_else(|| self.home.clone());
        let ask = cx.prompt_for_new_path(&home, Some("u0-usage.csv"));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = ask.await {
                let _ = this.update(cx, |d, _| d.write(path, csv));
            }
        })
        .detach();
    }

    /// Full Usage workspace page (`Usage` board).
    pub(crate) fn usage_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let now = SystemTime::now();
        let now_secs = store::secs(now);
        let since = self
            .pages
            .period
            .map_or(0, |days| now_secs.saturating_sub(days * 86_400));

        // Filter sessions by selected project & machine.
        let matching_sessions: Vec<_> = self
            .sessions
            .all
            .iter()
            .filter(|s| {
                self.pages.usage_host.is_none_or(|h| s.host == h)
                    && self
                        .pages
                        .usage_project
                        .as_ref()
                        .is_none_or(|p| &s.meta.project == p)
            })
            .collect();
        let total_threads = matching_sessions.len();

        let mut total_took = Took::default();
        let mut active_threads = 0usize;
        let mut total_turns = 0usize;
        let mut limit_pauses = 0usize;
        let mut by_agent: BTreeMap<String, u64> = BTreeMap::new();
        let mut by_model: BTreeMap<(String, String), Took> = BTreeMap::new();

        // 7-day daily buckets ending today (local midnight boundaries).
        let today_start = store::secs(when::start_of_day(now));
        let day_starts: [u64; 7] = std::array::from_fn(|i| {
            let back = 6_i32 - i32::try_from(i).unwrap_or(0);
            store::secs(when::at_minute(now, -back, 0))
        });
        let mut day_by_agent: [BTreeMap<String, u64>; 7] = Default::default();

        for s in &matching_sessions {
            let Some(used) = self.used.get(&s.key) else {
                continue;
            };
            let agent_name = self.name_of(&s.meta.agent);
            let mut had_turn = false;
            let mut session_turns = 0usize;
            let mut token_events = 0usize;
            for u in used {
                // Daily chart always shows the 7 local days ending today when within `since`.
                if u.at >= day_starts[0]
                    && u.at >= since
                    && let Use::Tokens(t) = &u.what
                {
                    let idx = day_starts.iter().rposition(|&ds| u.at >= ds).unwrap_or(0);
                    let sum = t.usage.input_tokens
                        + t.usage.output_tokens
                        + t.usage.cached_read_tokens.unwrap_or(0)
                        + t.usage.cached_write_tokens.unwrap_or(0);
                    *day_by_agent[idx].entry(agent_name.clone()).or_default() += sum;
                }
                if u.at < since {
                    continue;
                }
                match &u.what {
                    Use::Tokens(t) => {
                        had_turn = true;
                        token_events += 1;
                        let tk = Took {
                            input: t.usage.input_tokens,
                            output: t.usage.output_tokens,
                            read: t.usage.cached_read_tokens.unwrap_or(0),
                            wrote: t.usage.cached_write_tokens.unwrap_or(0),
                            has_read: t.usage.cached_read_tokens.is_some(),
                            has_wrote: t.usage.cached_write_tokens.is_some(),
                        };
                        total_took.add(tk);
                        *by_agent.entry(agent_name.clone()).or_default() += tk.total();
                        let model = t.model.clone().unwrap_or_else(|| "Unspecified".into());
                        by_model
                            .entry((model, agent_name.clone()))
                            .or_default()
                            .add(tk);
                    }
                    Use::Turn => {
                        had_turn = true;
                        session_turns += 1;
                    }
                    Use::LimitPaused => {
                        limit_pauses += 1;
                    }
                    Use::Cost { .. } | Use::Limit(_) => {}
                }
            }
            if had_turn {
                active_threads += 1;
                total_turns += session_turns.max(token_events);
            }
        }

        // Top 3 agents by token share for the stacked daily chart.
        let mut agent_rank: Vec<(String, u64)> = by_agent.into_iter().collect();
        agent_rank.sort_by_key(|(_, v)| std::cmp::Reverse(*v));
        let chart_roles = [ColorRole::Charts1, ColorRole::Charts2, ColorRole::Charts3];
        let top_agents: Vec<(String, u64, ColorRole)> = agent_rank
            .iter()
            .take(3)
            .enumerate()
            .map(|(i, (name, tok))| (name.clone(), *tok, chart_roles[i]))
            .collect();

        // Period switcher + Project + Machine filters.
        let active_period = PERIODS
            .iter()
            .find(|(_, _, d)| *d == self.pages.period)
            .map_or("7d", |(k, _, _)| *k);
        let desk_p = cx.entity();
        let mut switcher = ToggleGroup::new("usage-period-sw")
            .size(ToggleSize::Sm)
            .value([active_period])
            .on_value_change(move |vals, _, cx| {
                if let Some(first) = vals.first()
                    && let Some((_, _, days)) =
                        PERIODS.iter().find(|(k, _, _)| *k == first.as_ref())
                {
                    desk_p.update(cx, |this, cx| {
                        this.pages.period = *days;
                        cx.notify();
                    });
                }
            });
        for (key, label, _) in PERIODS {
            switcher = switcher.item(ToggleGroupItem::new(key).child(crate::tr(label)));
        }

        let mut project_items: Vec<ListItem> =
            vec![ListItem::labeled("__all__", crate::tr("All projects"))];
        let mut seen_folders: Vec<PathBuf> = Vec::new();
        for p in self.projects() {
            if !seen_folders.contains(&p.folder) {
                seen_folders.push(p.folder.clone());
                project_items.push(ListItem::labeled(
                    p.folder.to_string_lossy().into_owned(),
                    self.project_name(&Place::new(p.host, p.folder)),
                ));
            }
        }
        let proj_val: SharedString = self.pages.usage_project.as_ref().map_or_else(
            || "__all__".into(),
            |p| p.to_string_lossy().into_owned().into(),
        );
        let desk_proj = cx.entity();
        let project_select = Select::new("usage-project-filter")
            .size(FieldSize::Sm)
            .items(project_items)
            .value(Some(proj_val))
            .on_value_change(move |val, _, cx| {
                let picked = val
                    .map(SharedString::as_ref)
                    .filter(|v| *v != "__all__")
                    .map(PathBuf::from);
                desk_proj.update(cx, |this, cx| {
                    this.pages.usage_project = picked;
                    cx.notify();
                });
            });

        let mut machine_items: Vec<ListItem> =
            vec![ListItem::labeled("__all__", crate::tr("All machines"))];
        for (idx, m) in self.machines.iter().enumerate() {
            machine_items.push(ListItem::labeled(idx.to_string(), m.name.clone()));
        }
        let mach_val: SharedString = self
            .pages
            .usage_host
            .and_then(|h| self.machines.iter().position(|m| m.host == h))
            .map_or_else(|| "__all__".into(), |idx| idx.to_string().into());
        let desk_mach = cx.entity();
        let machine_select = Select::new("usage-machine-filter")
            .size(FieldSize::Sm)
            .items(machine_items)
            .value(Some(mach_val))
            .on_value_change(move |val, _, cx| {
                let idx = val
                    .map(SharedString::as_ref)
                    .filter(|v| *v != "__all__")
                    .and_then(|v| v.parse::<usize>().ok());
                desk_mach.update(cx, |this, cx| {
                    this.pages.usage_host = idx.and_then(|i| this.machines.get(i).map(|m| m.host));
                    cx.notify();
                });
            });

        // 4 StatBlocks.
        let grand_total = total_took.total();
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "cache percentage rounded to whole number"
        )]
        let cache_sub = if grand_total > 0 && total_took.read > 0 {
            let pct = ((total_took.read as f64 / grand_total as f64) * 100.0).round() as i64;
            crate::tr_with("%{pct}% read from cache, as reported", &[("pct", &pct)])
        } else {
            crate::tr("No cache reads reported")
        };
        let stats = StatsGrid::new("usage-stats")
            .child(StatBlock::new(crate::tr("Tokens"), count(grand_total)).sub(cache_sub))
            .child(
                StatBlock::new(crate::tr("Threads"), active_threads.to_string()).sub(
                    crate::tr_with(
                        "of your %{total} threads had a turn",
                        &[("total", &total_threads)],
                    ),
                ),
            )
            .child(
                StatBlock::new(crate::tr("Turns"), total_turns.to_string()).sub(crate::tr_with(
                    "across %{active} of your %{total} threads",
                    &[("active", &active_threads), ("total", &total_threads)],
                )),
            )
            .child(
                StatBlock::new(crate::tr("Paused by limits"), limit_pauses.to_string()).sub(
                    if limit_pauses == 0 {
                        crate::tr("None paused in this period")
                    } else {
                        crate::tr_with(
                            "%{n} paused when a limit was reached",
                            &[("n", &limit_pauses)],
                        )
                    },
                ),
            );

        // Left card: Tokens per day stacked bar chart.
        let day_totals: [u64; 7] =
            std::array::from_fn(|i| day_by_agent[i].values().copied().sum::<u64>());
        let max_day = day_totals.into_iter().max().unwrap_or(0);
        let ceiling = nice_ceiling(max_day);
        let mid = ceiling / 2;
        let plot_h = 130.0f32;

        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "whole-percent legend shares"
        )]
        let legend = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(16.))
            .children(top_agents.iter().map(|(name, tok, role)| {
                let share = if grand_total > 0 {
                    ((*tok as f64 / grand_total as f64) * 100.0).round() as i64
                } else {
                    0
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(div().size(px(10.)).bg(theme::role(*role)))
                    .child(format!("{name} {share}%"))
            }));

        let tick_row = |y: f32, label: String, kind: u8| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(y))
                .h(px(1.))
                .when(kind == 0, |d| d.bg(theme::role(ColorRole::BorderStrong01)))
                .when(kind == 2, |d| d.bg(theme::role(ColorRole::BorderSubtle01)))
                .when(kind == 1, |d| {
                    d.child(dashed_hline(theme::role(ColorRole::BorderSubtle01)))
                })
                .child(
                    div()
                        .absolute()
                        .left(px(-44.))
                        .top(px(-8.))
                        .w(px(36.))
                        .whitespace_nowrap()
                        .text_right()
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(ColorRole::TextHelper))
                        .child(label),
                )
        };

        let bars = (0..7).map(|i| {
            let is_today = day_starts[i] == today_start;
            let label = if is_today {
                crate::tr("Today").to_string()
            } else {
                when::short_day(store::time(day_starts[i]))
            };
            let day_sum = day_totals[i];
            #[allow(clippy::cast_precision_loss, reason = "bar height ratio")]
            let scale = |v: u64| -> f32 {
                if ceiling == 0 || v == 0 {
                    0.
                } else {
                    ((v as f64 / ceiling as f64) as f32 * plot_h).clamp(1., plot_h)
                }
            };
            let mut used_tok = 0u64;
            let mut segs: Vec<(f32, gpui::Hsla)> = Vec::new();
            for (name, _, role) in &top_agents {
                let v = day_by_agent[i].get(name).copied().unwrap_or(0);
                used_tok += v;
                if v > 0 {
                    segs.push((scale(v), theme::role(*role)));
                }
            }
            let rest = day_sum.saturating_sub(used_tok);
            if rest > 0 {
                segs.push((scale(rest), theme::role(ColorRole::Charts4)));
            }
            let stack_h: f32 = segs.iter().map(|(h, _)| *h).sum();
            div()
                .relative()
                .flex()
                .flex_1()
                .flex_col()
                .justify_end()
                .items_center()
                .h_full()
                .when(day_sum > 0, |d| {
                    d.child(
                        div()
                            .absolute()
                            .bottom(px(stack_h + 4.))
                            .type_style(TypeStyle::Label01)
                            .text_color(theme::role(ColorRole::TextSecondary))
                            .child(count(day_sum)),
                    )
                })
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_col_reverse()
                        .w(px(28.))
                        .children(segs.into_iter().enumerate().map(|(si, (h, fill))| {
                            div().w_full().h(px(h)).bg(fill).when(si > 0, |s| {
                                s.border_b_1().border_color(theme::role(ColorRole::Layer01))
                            })
                        }))
                        .when(is_today && stack_h > 0., |s| {
                            s.child(dashed_frame(theme::role(ColorRole::BorderStrong01)))
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(-22.))
                        .whitespace_nowrap()
                        .type_style(TypeStyle::Label01)
                        .text_color(theme::role(if is_today {
                            ColorRole::TextSecondary
                        } else {
                            ColorRole::TextHelper
                        }))
                        .child(label),
                )
        });

        let daily_card = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(12.))
            .py(px(16.))
            .px(px(20.))
            .bg(theme::role(ColorRole::Layer01))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap_x(px(12.))
                            .gap_y(px(4.))
                            .child(
                                div()
                                    .type_style(TypeStyle::HeadingCompact01)
                                    .text_color(theme::role(ColorRole::TextPrimary))
                                    .child(crate::tr("Tokens per day")),
                            )
                            .child(
                                div()
                                    .type_style(TypeStyle::Label01)
                                    .text_color(theme::role(ColorRole::TextSecondary))
                                    .child(crate::tr(
                                        "as each agent reported them · dashed: today so far",
                                    )),
                            ),
                    )
                    .when(!top_agents.is_empty(), |d| d.child(legend)),
            )
            .child(
                div()
                    .relative()
                    .h(px(plot_h))
                    .mt(px(12.))
                    .ml(px(44.))
                    .mb(px(18.))
                    .child(tick_row(0., axis_count(0, ceiling), 0))
                    .child(tick_row(plot_h * 0.5, axis_count(mid, ceiling), 1))
                    .child(tick_row(plot_h, axis_count(ceiling, ceiling), 2))
                    .child(div().absolute().inset_0().flex().children(bars)),
            );

        // Right card: Subscription limits (480px).
        let limits = latest_limits(matching_sessions.iter().flat_map(|s| {
            self.used
                .get(&s.key)
                .into_iter()
                .flat_map(move |u| u.iter().map(move |x| (s.meta.agent.as_str(), x)))
        }));

        let usage_texts = UsageTexts {
            used: Rc::new(|n| crate::tr_with("%{n}% used", &[("n", &n)])),
            resets: Rc::new(|w| crate::tr_with("Resets %{when}", &[("when", &w)])),
            pace: [
                crate::tr("Ahead of pace"),
                crate::tr("On pace"),
                crate::tr("Under pace"),
            ],
            over: Rc::new(|t| crate::tr_with("Over pace · limit ~%{t}", &[("t", &t)])),
            ..UsageTexts::default()
        };

        let desk_accts = cx.entity();
        let mut limits_card = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(480.))
            .gap(px(10.))
            .py(px(16.))
            .px(px(20.))
            .bg(theme::role(ColorRole::Layer01))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .child(
                        div()
                            .flex_1()
                            .type_style(TypeStyle::HeadingCompact01)
                            .text_color(theme::role(ColorRole::TextPrimary))
                            .child(crate::tr("Subscription limits")),
                    )
                    .child(
                        div()
                            .id("usage-manage-accounts")
                            .cursor_pointer()
                            .type_style(TypeStyle::BodyCompact01)
                            .text_color(theme::role(ColorRole::LinkPrimary))
                            .hover(|s| s.text_color(theme::role(ColorRole::LinkPrimaryHover)))
                            .child(crate::tr("Manage accounts"))
                            .on_click(move |_, window, cx| {
                                desk_accts
                                    .update(cx, |this, cx| this.open_tab(Tab::Agents, window, cx));
                            }),
                    ),
            );

        if limits.is_empty() {
            limits_card = limits_card.child(
                div()
                    .py(px(12.))
                    .type_style(TypeStyle::Label01)
                    .text_color(theme::role(ColorRole::TextSecondary))
                    .child(crate::tr("No agent has reported subscription limits yet.")),
            );
        } else {
            for (idx, ((agent, _), (_, l))) in limits.iter().enumerate() {
                let reset = l.resets.map(store::time);
                let past = reset.is_some_and(|t| t <= now);
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "percentage clamped to 0..=100"
                )]
                let used_pct = if past {
                    0
                } else {
                    l.used
                        .map_or(0, |u| (u * 100.0).round().clamp(0.0, 100.0) as u32)
                };
                let elapsed = u32::from(l.elapsed_pct(now_secs).unwrap_or(50));
                let resets_label = match reset {
                    Some(t) if !past => when::until_reset(t, now, self.settings.twelve),
                    Some(_) => crate::tr("Reset since it was said").to_string(),
                    None => crate::tr("No reset time said").to_string(),
                };
                limits_card = limits_card.child(
                    UsageWindowBar::new(
                        SharedString::from(format!("uwb-{idx}")),
                        format!("{} · {}", self.name_of(agent), l.name()),
                    )
                    .used(used_pct)
                    .elapsed(elapsed)
                    .resets(resets_label)
                    .texts(usage_texts.clone()),
                );
            }
        }
        limits_card = limits_card.child(
            div()
                .mt_auto()
                .pt(px(2.))
                .type_style(TypeStyle::Label01)
                .text_color(theme::role(ColorRole::TextHelper))
                .child(crate::tr(
                    "The tick shows how much of each window has passed.",
                )),
        );

        // By model DataTable.
        let mut model_rows: Vec<((String, String), Took)> = by_model.into_iter().collect();
        model_rows.sort_by_key(|(_, t)| std::cmp::Reverse(t.total()));
        let rows: Vec<DataTableRow> = model_rows
            .iter()
            .enumerate()
            .map(|(i, ((model, agent), t))| {
                DataTableRow::new(format!("m-{i}"))
                    .cell("model", model.clone())
                    .cell("agent", agent.clone())
                    .cell("input", count(t.input))
                    .cell("cwrite", cell_count(t.wrote, t.has_wrote))
                    .cell("cache", cell_count(t.read, t.has_read))
                    .cell("output", count(t.output))
            })
            .collect();

        let footer = DataTableRow::new("all-models")
            .cell("model", crate::tr("All models"))
            .cell("agent", "")
            .cell("input", count(total_took.input))
            .cell("cwrite", cell_count(total_took.wrote, total_took.has_wrote))
            .cell("cache", cell_count(total_took.read, total_took.has_read))
            .cell("output", count(total_took.output));

        let nothing = rows.is_empty();
        let table = DataTable::new("usage-by-model")
            .size(TableSize::Xs)
            .sortable(false)
            .search(false)
            .page_size(25)
            .columns(vec![
                DataTableColumn::new("model", crate::tr("Model")),
                DataTableColumn::new("agent", crate::tr("Agent")),
                DataTableColumn::new("input", crate::tr("Input")).align_end(),
                DataTableColumn::new("cwrite", crate::tr("Cache write")).align_end(),
                DataTableColumn::new("cache", crate::tr("Cache read")).align_end(),
                DataTableColumn::new("output", crate::tr("Output")).align_end(),
            ])
            .rows(rows)
            .footer_row(footer);

        let _ = Host::HERE;
        div()
            .id("usage-workspace")
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .w_full()
                    .max_w(px(1080.))
                    .pt(px(28.))
                    .pb(px(32.))
                    .px(px(40.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .type_style(TypeStyle::Heading04)
                                    .text_color(theme::role(ColorRole::TextPrimary))
                                    .child(crate::tr("Usage")),
                            )
                            .child(
                                div()
                                    .type_style(TypeStyle::Body01)
                                    .text_color(theme::role(ColorRole::TextSecondary))
                                    .child(crate::tr(
                                        "As each agent reported them; u0 adds no figures or prices.",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(445.)).flex_none().child(switcher))
                            .child(div().w(px(168.)).flex_none().child(project_select))
                            .child(div().w(px(168.)).flex_none().child(machine_select)),
                    )
                    .child(stats)
                    .child(
                        div()
                            .flex()
                            .items_stretch()
                            .gap(px(24.))
                            .child(daily_card)
                            .child(limits_card),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .type_style(TypeStyle::HeadingCompact01)
                                            .text_color(theme::role(ColorRole::TextPrimary))
                                            .child(crate::tr("By model")),
                                    )
                                    .child(
                                        div()
                                            .type_style(TypeStyle::Label01)
                                            .text_color(theme::role(ColorRole::TextSecondary))
                                            .child(crate::tr(
                                                "Tokens each agent reported; a dash where it reports none",
                                            )),
                                    ),
                            )
                            .child(if nothing {
                                self.usage_nothing(cx).into_any_element()
                            } else {
                                table.into_any_element()
                            }),
                    ),
            )
    }

    /// No agent said anything in the period: said so, and from a day,
    /// the week a click away.
    fn usage_nothing(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use u0ui::components::empty::{
            Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle,
        };
        Empty::new()
            .child(
                EmptyHeader::new()
                    .child(EmptyTitle::new(crate::tr("Nothing said")))
                    .child(EmptyDescription::new(crate::tr(
                        "No agent said what a thread cost or took in this time.",
                    ))),
            )
            .when(self.pages.period == Some(1), |e| {
                e.child(
                    EmptyContent::new().child(
                        Button::new("usage-7-days", crate::tr("Last 7 days (⇧⌘2)"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pages.period = Some(7);
                                cx.notify();
                            })),
                    ),
                )
            })
    }
}
