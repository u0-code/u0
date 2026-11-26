//! What agents say they used, read from the records: what a session cost
//! (ACP's `usage_update`), the tokens of each turn by model (the end of a
//! prompt), and the limits of an account (what an adapter adds under
//! `_meta`). Ghost counts and prices nothing itself: what no agent says is
//! not shown.

use std::time::{Duration, SystemTime};

use agent_client_protocol::{
    Error,
    v1::schema::{Meta, PromptResponse, SessionUpdate, Usage},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::Event;

/// One thing an agent said it used, at the time of the message before it
/// (seconds since the epoch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Used {
    pub at: u64,
    pub what: Use,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Use {
    /// What the session had cost so far: agents say the session's total,
    /// not the turn's.
    Cost { amount: f64, currency: String },
    /// What one turn took.
    Tokens(Tokens),
    /// Where the account stood against one of its limits.
    Limit(Limit),
}

/// The tokens a turn took of one model; no model where the agent did not
/// say which.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    #[serde(default)]
    pub model: Option<String>,
    pub usage: Usage,
}

impl Tokens {
    /// What the end of a prompt says it took: by model where the adapter
    /// adds them (`_meta.quota.model_usage`, which Claude's and Codex's
    /// both do), else the turn's whole (`usage`).
    pub fn of(ended: &PromptResponse) -> Vec<Self> {
        let rows = ended
            .meta
            .as_ref()
            .and_then(|m| m.get("quota")?.get("model_usage")?.as_array().cloned())
            .unwrap_or_default();
        let by_model: Vec<Self> = rows
            .iter()
            .filter_map(|row| {
                let n = |k: &str| row["token_count"][k].as_u64();
                Some(Self {
                    model: Some(row["model"].as_str()?.to_owned()),
                    usage: Usage::new(n("totalTokens")?, n("inputTokens")?, n("outputTokens")?)
                        .thought_tokens(n("reasoningOutputTokens").filter(|n| *n > 0))
                        .cached_read_tokens(n("cachedInputTokens"))
                        .cached_write_tokens(
                            n("cachedWriteTokens").or_else(|| n("cacheWriteInputTokens")),
                        ),
                })
            })
            .filter(|t| t.usage.total_tokens > 0)
            .collect();
        if !by_model.is_empty() {
            return by_model;
        }
        ended
            .usage
            .clone()
            .filter(|u| u.total_tokens > 0)
            .map(|usage| Self { model: None, usage })
            .into_iter()
            .collect()
    }
}

/// Where an account stood against one limit window, as Claude's adapter
/// says it (`_claude/rateLimit`, its CLI's `rate_limit_info`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    /// Which window: `five_hour`, `seven_day`, `seven_day_opus`, ...
    pub window: String,
    /// `allowed`, `allowed_warning` or `rejected`.
    pub status: String,
    /// When it resets, in seconds since the epoch.
    pub resets: Option<u64>,
    /// How much of it is used, 0 to 1.
    pub used: Option<f64>,
}

impl Limit {
    /// The limit said in `info`, a `rate_limit_info`.
    fn read(info: &Value) -> Option<Self> {
        Some(Self {
            window: info["rateLimitType"].as_str().unwrap_or("limit").to_owned(),
            status: info["status"].as_str()?.to_owned(),
            resets: info["resetsAt"].as_u64(),
            used: info["utilization"].as_f64(),
        })
    }

    /// The limit an update's `_meta` says.
    pub fn of(meta: &Meta) -> Option<Self> {
        Self::read(meta.get("_claude/rateLimit")?)
    }

    /// Whether the account may not go on until it resets.
    pub fn reached(&self) -> bool {
        self.status == "rejected"
    }

    /// The window as read: "5-hour", "Weekly", "Weekly · Opus".
    pub fn name(&self) -> String {
        match self.window.as_str() {
            "five_hour" => "5-hour".into(),
            "seven_day" => "Weekly".into(),
            w => match w.strip_prefix("seven_day_") {
                Some(model) => format!("Weekly · {}", capital(model)),
                None => capital(&w.replace('_', " ")),
            },
        }
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// When a turn refused for a limit may go on: when the agent says (the
/// error's `rateLimit`, from Claude's adapter), else when its words say
/// (`when::reset_at`, which reads Codex's "try again at 3:05 PM").
pub fn resets(refused: &Error, now: SystemTime) -> Option<SystemTime> {
    let said = (refused.data.as_ref())
        .and_then(|d| Limit::read(&d["rateLimit"]))
        .and_then(|l| l.resets)
        .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s))
        .filter(|t| *t > now);
    said.or_else(|| {
        refused
            .message
            .to_lowercase()
            .contains("limit")
            .then(|| crate::when::reset_at(&refused.message, now))?
    })
}

/// What the record `events` says was used, each at the time of the
/// message before it.
pub fn used(events: &[Event]) -> Vec<Used> {
    let mut at = 0;
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::At(t) => at = *t,
            Event::Update(u) => {
                if let SessionUpdate::UsageUpdate(u) = &**u {
                    if let Some(c) = &u.cost {
                        out.push(Used {
                            at,
                            what: Use::Cost {
                                amount: c.amount,
                                currency: c.currency.clone(),
                            },
                        });
                    }
                    if let Some(l) = u.meta.as_ref().and_then(Limit::of) {
                        out.push(Used {
                            at,
                            what: Use::Limit(l),
                        });
                    }
                }
            }
            Event::Ended { tokens, .. } => out.extend(tokens.iter().map(|t| Used {
                at,
                what: Use::Tokens(t.clone()),
            })),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::v1::schema::{Cost, StopReason, UsageUpdate};
    use serde_json::json;

    #[test]
    fn a_turn_s_tokens_are_read_by_model_where_the_adapter_says_them() {
        let ended: PromptResponse = serde_json::from_value(json!({
            "stopReason": "end_turn",
            "usage": {"totalTokens": 30, "inputTokens": 10, "outputTokens": 20},
            "_meta": {"quota": {"token_count": {}, "model_usage": [
                {"model": "opus", "token_count": {"totalTokens": 25, "inputTokens": 5,
                    "cachedInputTokens": 100, "cachedWriteTokens": 3, "outputTokens": 20,
                    "reasoningOutputTokens": 0}},
                {"model": "haiku", "token_count": {"totalTokens": 5, "inputTokens": 5,
                    "cachedInputTokens": 0, "cacheWriteInputTokens": 0, "outputTokens": 0,
                    "reasoningOutputTokens": 0}}
            ]}}
        }))
        .unwrap();
        let tokens = Tokens::of(&ended);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].model.as_deref(), Some("opus"));
        assert_eq!(tokens[0].usage.cached_read_tokens, Some(100));
        assert_eq!(tokens[0].usage.cached_write_tokens, Some(3));
        let bare = PromptResponse::new(StopReason::EndTurn);
        assert!(
            Tokens::of(&bare).is_empty(),
            "nothing said, nothing counted"
        );
    }

    #[test]
    fn what_a_session_cost_is_read_from_its_record_at_the_time_said() {
        let usage = |amount: f64| {
            Event::Update(Box::new(SessionUpdate::UsageUpdate(
                UsageUpdate::new(1, 2).cost(Cost::new(amount, "USD")),
            )))
        };
        let events = [Event::At(100), usage(0.5), Event::At(200), usage(1.25)];
        let costs: Vec<(u64, f64)> = used(&events)
            .into_iter()
            .filter_map(|u| match u.what {
                Use::Cost { amount, .. } => Some((u.at, amount)),
                _ => None,
            })
            .collect();
        assert_eq!(costs, [(100, 0.5), (200, 1.25)]);
    }

    #[test]
    fn claude_s_rate_limit_is_read_from_a_usage_update() {
        let mut meta = Meta::new();
        meta.insert(
            "_claude/rateLimit".into(),
            json!({"status": "rejected", "resetsAt": 1_800_000_000u64,
                "rateLimitType": "seven_day_opus", "utilization": 1.0}),
        );
        let update = UsageUpdate::new(1, 2).meta(meta);
        let events = [
            Event::At(7),
            Event::Update(Box::new(SessionUpdate::UsageUpdate(update))),
        ];
        let used = used(&events);
        let Use::Limit(l) = &used[0].what else {
            panic!("a limit")
        };
        assert_eq!(used[0].at, 7);
        assert!(l.reached());
        assert_eq!(l.resets, Some(1_800_000_000));
        assert_eq!(l.name(), "Weekly · Opus");
    }

    #[test]
    fn a_refused_turn_resumes_when_the_agent_says_else_when_its_words_say() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let said = Error::new(-32603, "Claude usage limit reached").data(json!({
            "errorKind": "rate_limit",
            "rateLimit": {"status": "rejected", "resetsAt": 1_700_003_600u64}
        }));
        assert_eq!(resets(&said, now), Some(now + Duration::from_secs(3600)));
        let other = Error::new(-32603, "Something else broke");
        assert_eq!(resets(&other, now), None);
    }
}
