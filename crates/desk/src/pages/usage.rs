//! The usage page (⌘U): what sessions used over a period, as their agents
//! said it and read back from the records (`usage::used`): what they cost
//! and the tokens they took, by agent, by model and by session; where each
//! account stands against its limits; and the context each open one holds
//! (what runs is in Diagnostics). ⇧⌘1 to ⇧⌘5 choose the period. Ghost
//! prices and counts nothing itself: what an agent does not say is not
//! shown.

use std::{collections::BTreeMap, time::SystemTime};

use gpui::component::{
    Sizable as _,
    progress::Progress,
    setting::{SettingGroup, SettingItem},
};
use gpui::{Context, IntoElement, SharedString, WeakEntity, div, prelude::*, px};

use super::prefs::{group, item};
use crate::{
    desk::Desk,
    store, theme,
    ui::{self, Weight},
    usage::{Limit, Use, Used},
    when,
};

/// The periods the page reads over, in days (none, all of it), as T3's.
pub const PERIODS: [(&str, Option<u64>); 5] = [
    ("24 hours", Some(1)),
    ("7 days", Some(7)),
    ("30 days", Some(30)),
    ("90 days", Some(90)),
    ("All", None),
];

/// What a session cost from `since` on, by currency: what its agent last
/// said, less what it had said before then (agents say the session's
/// total so far).
fn cost_since(used: &[Used], since: u64) -> BTreeMap<String, f64> {
    let spent: Vec<(u64, &str, f64)> = (used.iter())
        .filter_map(|u| match &u.what {
            Use::Cost { amount, currency } => Some((u.at, currency.as_str(), *amount)),
            _ => None,
        })
        .collect();
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    let currencies: std::collections::BTreeSet<&str> = spent.iter().map(|s| s.1).collect();
    for c in currencies {
        let of = || spent.iter().filter(move |s| s.1 == c);
        let before = of().filter(|s| s.0 < since).map(|s| s.2).next_back();
        let last = of().filter(|s| s.0 >= since).map(|s| s.2).next_back();
        if let Some(last) = last {
            let cost = last - before.unwrap_or(0.);
            if cost > 0. {
                out.insert(c.to_owned(), cost);
            }
        }
    }
    out
}

/// The tokens taken from `since` on: input (cache aside), output, read
/// from the cache and written to it.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Took {
    input: u64,
    output: u64,
    read: u64,
    wrote: u64,
}

impl Took {
    fn add(&mut self, o: Self) {
        self.input += o.input;
        self.output += o.output;
        self.read += o.read;
        self.wrote += o.wrote;
    }

    fn total(self) -> u64 {
        self.input + self.output + self.read + self.wrote
    }

    /// "12.4k in · 3.1k out · 210k cached"; none when nothing was said.
    fn said(self) -> Option<String> {
        if self.total() == 0 {
            return None;
        }
        let mut out = vec![
            format!("{} in", count(self.input)),
            format!("{} out", count(self.output)),
        ];
        if self.read + self.wrote > 0 {
            out.push(format!("{} cached", count(self.read + self.wrote)));
        }
        Some(out.join(" · "))
    }
}

/// The tokens a session took from `since` on, by model ("" where its
/// agent did not say which).
fn tokens_since(used: &[Used], since: u64) -> BTreeMap<String, Took> {
    let mut out: BTreeMap<String, Took> = BTreeMap::new();
    for u in used.iter().filter(|u| u.at >= since) {
        if let Use::Tokens(t) = &u.what {
            out.entry(t.model.clone().unwrap_or_default())
                .or_default()
                .add(Took {
                    input: t.usage.input_tokens,
                    output: t.usage.output_tokens,
                    read: t.usage.cached_read_tokens.unwrap_or(0),
                    wrote: t.usage.cached_write_tokens.unwrap_or(0),
                });
        }
    }
    out
}

/// A count of tokens as read: 950, 12.4k, 3.2M.
fn count(n: u64) -> String {
    #[allow(clippy::cast_precision_loss, reason = "a count shown to a tenth")]
    let f = n as f64;
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", f / 1e3),
        _ => format!("{:.1}M", f / 1e6),
    }
}

/// Amounts by currency, as read: "1.20 USD · 0.40 EUR".
fn amounts(by: &BTreeMap<String, f64>) -> String {
    by.iter()
        .map(|(c, a)| format!("{a:.2} {c}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The last each window of a limit was said, by agent and window, with
/// when: limits are an account's, so the latest any session heard holds.
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

impl Desk {
    /// ⇧⌘1 to ⇧⌘5 on the usage page: its periods, as T3's keys have them
    /// (macOS keeps ⇧⌘3 to ⇧⌘5 for its screenshots). Whether the keystroke
    /// was one.
    pub(crate) fn usage_keys(&mut self, k: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let m = &k.modifiers;
        // ⇧1 comes as what the key types with ⇧ ("!"), without ⇧ said.
        let n = ["1", "2", "3", "4", "5"]
            .iter()
            .zip(["!", "@", "#", "$", "%"])
            .position(|(d, s)| (k.key == *d && m.shift) || k.key == s)
            .map(|n| n + 1);
        match n {
            Some(n) if self.pages.open && self.pages.tab == super::Tab::Usage && m.platform => {
                self.pages.period = PERIODS[n - 1].1;
                cx.notify();
                true
            }
            _ => false,
        }
    }

    /// The usage page: where each limit stands; then, over its period,
    /// what each agent, model and session cost and took.
    pub(super) fn usage(&self, w: &WeakEntity<Desk>) -> Vec<SettingGroup> {
        let now = SystemTime::now();
        let since = self
            .pages
            .period
            .map_or(0, |days| store::secs(now).saturating_sub(days * 86_400));
        // By agent, by model, then by session, the most used first.
        let mut by_agent: BTreeMap<String, (BTreeMap<String, f64>, Took, usize)> = BTreeMap::new();
        let mut by_model: BTreeMap<String, Took> = BTreeMap::new();
        let mut rows: Vec<(u64, BTreeMap<String, f64>, Took)> = Vec::new();
        for (key, used) in &self.used {
            let (cost, tokens) = (cost_since(used, since), tokens_since(used, since));
            let Some(s) = self
                .sessions
                .at(*key)
                .filter(|_| !cost.is_empty() || !tokens.is_empty())
            else {
                continue;
            };
            let mut took = Took::default();
            for (model, t) in tokens {
                took.add(t);
                by_model.entry(model).or_default().add(t);
            }
            let agent = by_agent.entry(s.meta.agent.clone()).or_default();
            for (c, a) in &cost {
                *agent.0.entry(c.clone()).or_default() += a;
            }
            agent.1.add(took);
            agent.2 += 1;
            rows.push((*key, cost, took));
        }
        rows.sort_by(|a, b| {
            let total = |r: &BTreeMap<String, f64>| r.values().sum::<f64>();
            (total(&b.1).total_cmp(&total(&a.1))).then(b.2.total().cmp(&a.2.total()))
        });
        let limits = latest_limits(self.used.iter().flat_map(|(key, used)| {
            let agent = self.sessions.at(*key).map(|s| s.meta.agent.as_str());
            agent
                .into_iter()
                .flat_map(move |a| used.iter().map(move |u| (a, u)))
        }));
        let quiet = |faces: &crate::fonts::Faces, text: String| {
            ui::text(faces, text)
                .text_size(px(crate::style::CAPTION))
                .text_color(theme::text2())
        };
        let mut windows: Vec<SettingItem> = Vec::new();
        for ((agent, _), (_, l)) in &limits {
            let reset = l.resets.map(store::time);
            let past = reset.is_some_and(|t| t <= now);
            let about = match reset {
                Some(t) if !past => {
                    format!("Resets {}", when::said_at(t, now, self.settings.twelve))
                }
                Some(_) => "Reset since it was said".to_owned(),
                None => "No reset time said".to_owned(),
            };
            let used = l.used.filter(|_| !past);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a share shown as a whole percentage"
            )]
            let end = match (l.reached() && !past, used) {
                (true, _) => "Reached".to_owned(),
                (false, Some(u)) => format!("{}% used", (u * 100.).round() as i64),
                (false, None) => String::new(),
            };
            let (id, reached) = (format!("limit-{agent}-{}", l.window), l.reached());
            windows.push(item(
                w,
                format!("{} · {}", self.name_of(agent), l.name()),
                about,
                move |_, faces, _| {
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "a bar's fill needs no more than f32"
                    )]
                    let bar = used.map(|u| {
                        div().w(px(120.)).child(
                            Progress::new(SharedString::from(id.clone()))
                                .xsmall()
                                .value((u * 100.) as f32)
                                .color(if reached {
                                    theme::gone()
                                } else {
                                    theme::muted()
                                }),
                        )
                    });
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .children(bar)
                        .child(quiet(faces, end.clone()))
                        .into_any_element()
                },
            ));
        }
        let period = item(w, "Period", "⇧⌘1 to ⇧⌘5 choose it.", |d, _, cx| {
            let picks = PERIODS
                .iter()
                .map(|&(label, days)| {
                    let pick: ui::Pick = Box::new(cx.listener(move |this, _, _, cx| {
                        this.pages.period = days;
                        cx.notify();
                    }));
                    (label, d.pages.period == days, pick)
                })
                .collect();
            ui::segmented("usage-period", picks).into_any_element()
        });
        let mut agents: Vec<SettingItem> = Vec::new();
        for (agent, (cost, took, sessions)) in &by_agent {
            let about = [
                Some(format!(
                    "{sessions} session{}",
                    if *sessions == 1 { "" } else { "s" }
                )),
                took.said(),
            ];
            let said = amounts(cost);
            agents.push(item(
                w,
                self.name_of(agent),
                about.into_iter().flatten().collect::<Vec<_>>().join(" · "),
                move |_, faces, _| {
                    ui::text(faces, said.clone())
                        .text_size(px(crate::style::LABEL))
                        .into_any_element()
                },
            ));
        }
        if by_agent.is_empty() {
            agents.push(item(
                w,
                "Nothing said",
                "No agent said what a session cost or took in this time. Not every agent says; Ghost does not guess.",
                |_, _, _| div().into_any_element(),
            ));
        }
        let mut ranked: Vec<(&String, &Took)> = by_model.iter().collect();
        ranked.sort_by_key(|r| std::cmp::Reverse(r.1.total()));
        let models: Vec<SettingItem> = ranked
            .into_iter()
            .map(|(model, took)| {
                let name = if model.is_empty() {
                    "Model not said".to_owned()
                } else {
                    model.clone()
                };
                let total = format!("{} tokens", count(took.total()));
                item(
                    w,
                    name,
                    took.said().unwrap_or_default(),
                    move |_, faces, _| quiet(faces, total.clone()).into_any_element(),
                )
            })
            .collect();
        let mut sessions: Vec<SettingItem> = Vec::new();
        for (key, cost, took) in &rows {
            let Some(s) = self.sessions.at(*key) else {
                continue;
            };
            let context = s
                .usage
                .as_ref()
                .filter(|u| u.size > 0)
                .map(|u| format!("{}k of {}k in context", u.used / 1000, u.size / 1000));
            let about = [Some(self.name_of(&s.meta.agent)), took.said(), context];
            let (key, said) = (*key, amounts(cost));
            sessions.push(item(
                w,
                ui::fit(&s.meta.title, 48),
                about.into_iter().flatten().collect::<Vec<_>>().join(" · "),
                move |_, faces, cx| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(quiet(faces, said.clone()))
                        .child(
                            ui::button(
                                SharedString::from(format!("usage-open-{key}")),
                                "Open",
                                Weight::Quiet,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.close_settings(window, cx);
                                    this.show(key, window, cx);
                                },
                            )),
                        )
                        .into_any_element()
                },
            ));
        }
        let mut groups = Vec::new();
        if !windows.is_empty() {
            groups.push(group("Limits", windows).description(
                "As an agent says them while you work: Claude's adapter does; Codex's says them only in answer to /status.",
            ));
        }
        groups.push(SettingGroup::new().item(period));
        groups.push(
            group("By agent", agents).description("Costs and tokens are each agent's own count."),
        );
        if !models.is_empty() {
            groups.push(group("By model", models));
        }
        if !sessions.is_empty() {
            groups.push(group("By session", sessions));
        }
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_s_cost_in_a_period_is_its_total_less_what_came_before() {
        let paid = |at, amount| Used {
            at,
            what: Use::Cost {
                amount,
                currency: "USD".into(),
            },
        };
        let spent = [paid(10, 1.0), paid(20, 1.5), paid(30, 2.25)];
        assert_eq!(cost_since(&spent, 15).get("USD"), Some(&1.25));
        assert_eq!(cost_since(&spent, 0).get("USD"), Some(&2.25));
        assert!(cost_since(&spent, 40).is_empty());
    }

    #[test]
    fn the_latest_said_of_each_window_of_an_agent_s_limits_holds() {
        let limit = |at, used| Used {
            at,
            what: Use::Limit(Limit {
                window: "five_hour".into(),
                status: "allowed".into(),
                resets: None,
                used: Some(used),
            }),
        };
        let (old, new) = (limit(20, 0.9), limit(10, 0.2));
        let latest = latest_limits([("a", &old), ("a", &new)].into_iter());
        assert_eq!(latest.len(), 1);
        assert_eq!(latest.values().next().unwrap().1.used, Some(0.9));
    }
}
