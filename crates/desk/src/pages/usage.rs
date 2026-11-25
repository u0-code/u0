//! The usage page (⌘U): what sessions cost over a period, as their agents
//! said it (ACP's `usage_update`, read back from the records), by agent and
//! by session; and the context each open one holds (what runs is in
//! Diagnostics). ⇧⌘1 to ⇧⌘5 choose the period. Ghost prices nothing
//! itself: where an agent says no cost, none is shown.

use std::collections::BTreeMap;

use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::{
    card,
    prefs::{page, setting},
};
use crate::{
    desk::Desk,
    fonts::Faces,
    store::Spend,
    theme,
    ui::{self, Weight},
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
fn cost_since(spent: &[Spend], since: u64) -> BTreeMap<String, f64> {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    let currencies: std::collections::BTreeSet<&str> =
        spent.iter().map(|s| s.currency.as_str()).collect();
    for c in currencies {
        let of: Vec<&Spend> = spent.iter().filter(|s| s.currency == c).collect();
        let before = of
            .iter()
            .filter(|s| s.at < since)
            .map(|s| s.amount)
            .next_back();
        let last = of
            .iter()
            .filter(|s| s.at >= since)
            .map(|s| s.amount)
            .next_back();
        if let Some(last) = last {
            let cost = last - before.unwrap_or(0.);
            if cost > 0. {
                out.insert(c.to_owned(), cost);
            }
        }
    }
    out
}

/// Amounts by currency, as read: "1.20 USD · 0.40 EUR".
fn amounts(by: &BTreeMap<String, f64>) -> String {
    by.iter()
        .map(|(c, a)| format!("{a:.2} {c}"))
        .collect::<Vec<_>>()
        .join(" · ")
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

    /// The usage page.
    pub(super) fn usage(&self, faces: &Faces, cx: &mut Context<Self>) -> Div {
        let since = self.pages.period.map_or(0, |days| {
            crate::store::secs(std::time::SystemTime::now()).saturating_sub(days * 86_400)
        });
        // By agent, then by session, the most spent first.
        let mut by_agent: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
        let mut rows: Vec<(u64, BTreeMap<String, f64>)> = Vec::new();
        for (key, spent) in &self.spent {
            let cost = cost_since(spent, since);
            let Some(s) = self.sessions.at(*key).filter(|_| !cost.is_empty()) else {
                continue;
            };
            let agent = by_agent.entry(s.meta.agent.clone()).or_default();
            for (c, a) in &cost {
                *agent.entry(c.clone()).or_default() += a;
            }
            rows.push((*key, cost));
        }
        rows.sort_by(|a, b| {
            let total = |r: &BTreeMap<String, f64>| r.values().sum::<f64>();
            total(&b.1).total_cmp(&total(&a.1))
        });
        let periods = ui::segmented(
            "usage-period",
            PERIODS
                .iter()
                .map(|&(label, days)| {
                    let pick: ui::Pick = Box::new(cx.listener(move |this, _, _, cx| {
                        this.pages.period = days;
                        cx.notify();
                    }));
                    (label, self.pages.period == days, pick)
                })
                .collect(),
        );
        let mut agents = card().overflow_hidden();
        for (n, (agent, cost)) in by_agent.iter().enumerate() {
            let sessions = rows
                .iter()
                .filter(|(k, _)| self.sessions.at(*k).is_some_and(|s| s.meta.agent == *agent))
                .count();
            agents = agents.child(
                setting(
                    faces,
                    &self.name_of(agent),
                    &format!("{sessions} session{}", if sessions == 1 { "" } else { "s" }),
                    ui::text(faces, amounts(cost)).text_size(px(13.5)),
                )
                .when(n > 0, |d| d.border_t_1().border_color(theme::border())),
            );
        }
        if by_agent.is_empty() {
            agents = agents.child(div().px(px(16.)).py(px(14.)).child(ui::fine(
                faces,
                "No agent said what a session cost in this time. Not every agent says; Ghost does not guess.",
            )));
        }
        let mut sessions = card().overflow_hidden();
        for (n, (key, cost)) in rows.iter().enumerate() {
            let Some(s) = self.sessions.at(*key) else {
                continue;
            };
            let context = s
                .usage
                .as_ref()
                .filter(|u| u.size > 0)
                .map(|u| format!(" · {}k of {}k in context", u.used / 1000, u.size / 1000))
                .unwrap_or_default();
            let key = *key;
            sessions = sessions.child(
                setting(
                    faces,
                    &ui::fit(&s.meta.title, 48),
                    &format!("{}{context}", self.name_of(&s.meta.agent)),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            ui::text(faces, amounts(cost))
                                .text_size(px(12.5))
                                .text_color(theme::text2()),
                        )
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
                        ),
                )
                .when(n > 0, |d| d.border_t_1().border_color(theme::border())),
            );
        }
        let section = |label: &str, body: Div| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(ui::label(faces, label))
                .child(body)
        };
        let mut out = page(
            faces,
            super::Tab::Usage.title(),
            "What sessions cost, as their agents said.",
        )
        .child(div().flex().child(periods))
        .child(section("By agent", agents));
        if !rows.is_empty() {
            out = out.child(section("By session", sessions));
        }
        out.child(ui::fine(
            faces,
            "Limits and their reset times are not shown: no agent says them over ACP yet.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_s_cost_in_a_period_is_its_total_less_what_came_before() {
        let paid = |at, amount| Spend {
            at,
            amount,
            currency: "USD".into(),
        };
        let spent = [paid(10, 1.0), paid(20, 1.5), paid(30, 2.25)];
        assert_eq!(cost_since(&spent, 15).get("USD"), Some(&1.25));
        assert_eq!(cost_since(&spent, 0).get("USD"), Some(&2.25));
        assert!(cost_since(&spent, 40).is_empty());
    }
}
