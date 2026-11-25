//! Times as a person reads them in a sidebar: how long something took, how
//! long ago, the hour of the day in this computer's own time zone.

use std::time::{Duration, SystemTime};

use jiff::{Timestamp, ToSpan, ZonedDateTime, civil};

/// A duration: "14s", "2m 14s", "1h 05m".
pub fn took(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// A duration under a minute with its tenths, as a command's time: "3.2s".
pub fn took_short(d: Duration) -> String {
    if d.as_secs() < 60 {
        format!("{:.1}s", d.as_secs_f32())
    } else {
        took(d)
    }
}

/// `t` in this computer's time zone.
fn local(t: SystemTime) -> ZonedDateTime {
    ZonedDateTime::try_from(t).unwrap_or_default()
}

/// `t` as a name for a folder, in this computer's time zone:
/// "2026-11-25 0142".
pub fn stamp(t: SystemTime) -> String {
    local(t).strftime("%Y-%m-%d %H%M").to_string()
}

/// `days` days after `t`, at `hour` o'clock in this computer's time zone.
pub fn at_hour(t: SystemTime, days: i32, hour: i8) -> SystemTime {
    local(t)
        .with()
        .time(civil::time(hour, 0, 0, 0))
        .build()
        .and_then(|at| at.checked_add(days.days()))
        .and_then(SystemTime::try_from)
        .unwrap_or(t)
}

/// Days from `t` to the next Monday (1 to 7).
pub fn to_monday(t: SystemTime) -> i32 {
    8 - i32::from(local(t).weekday().to_monday_one_offset())
}

/// The time of day of `t`: "10:42", or "10:42 am".
pub fn clock(t: SystemTime) -> String {
    let twelve = TWELVE.load(std::sync::atomic::Ordering::Relaxed);
    local(t)
        .strftime(if twelve { "%-I:%M %P" } else { "%H:%M" })
        .to_string()
}

/// Whether times are told on a twelve-hour clock, as the settings say.
static TWELVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_twelve(on: bool) {
    TWELVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// When a message was said: its time of day if today ("10:42"), else its
/// date too ("2026-11-24 10:42").
pub fn said_at(t: SystemTime, now: SystemTime) -> String {
    let day = local(t).date();
    if day == local(now).date() {
        clock(t)
    } else {
        format!("{day} {}", clock(t))
    }
}

/// A while said shortly: "45m", "3h", "2d", "1w"; none if it does not read.
pub fn span(text: &str) -> Option<Duration> {
    let text = text.trim();
    let (n, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = n.parse().ok().filter(|n| *n > 0)?;
    let secs = match unit.trim() {
        "m" | "min" | "mins" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86_400,
        "w" | "week" | "weeks" => 604_800,
        _ => return None,
    };
    Some(Duration::from_secs(n * secs))
}

/// How long ago `t` was, the way a list of sessions says it: "now", "12m",
/// "2h", and past a day the weekday ("Mon"), past a week the date.
pub fn ago(t: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(t).map_or(0, |d| d.as_secs());
    match secs {
        0..60 => "now".into(),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        86_400..604_800 => local(t).strftime("%a").to_string(),
        _ => local(t).strftime("%b %-d").to_string(),
    }
}

/// A time written as RFC 3339 says ("2026-11-24T10:42:07.123Z", or with
/// an offset such as "+08:00"), as agents date their sessions.
pub fn parse(text: &str) -> Option<SystemTime> {
    SystemTime::try_from(text.parse::<Timestamp>().ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_told_on_the_clock_chosen() {
        let t = at_hour(SystemTime::now(), 0, 15);
        assert_eq!(clock(t), "15:00");
        set_twelve(true);
        assert_eq!(clock(t), "3:00 pm");
        set_twelve(false);
    }

    #[test]
    fn a_while_said_shortly_is_read() {
        assert_eq!(span("45m"), Some(Duration::from_secs(2700)));
        assert_eq!(span("3h"), Some(Duration::from_secs(10_800)));
        assert_eq!(span("2 days"), Some(Duration::from_secs(172_800)));
        assert_eq!(span("soon"), None);
        assert_eq!(span("0h"), None);
    }

    #[test]
    fn a_snooze_lands_on_the_hour_asked_and_next_monday_is_ahead() {
        let now = SystemTime::now();
        let t = at_hour(now, 1, 9);
        assert!(t > now);
        assert_eq!((local(t).hour(), local(t).minute()), (9, 0));
        let days = to_monday(now);
        assert!((1..=7).contains(&days));
        assert_eq!(
            local(at_hour(now, days, 9)).weekday(),
            civil::Weekday::Monday
        );
    }

    #[test]
    fn an_rfc_3339_time_is_read_with_its_fraction_and_offset() {
        let t = |s| {
            parse(s)
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        };
        assert_eq!(t("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(t("2026-11-24T10:42:07.123Z"), Some(1_795_516_927));
        assert_eq!(t("2026-11-24T18:42:07+08:00"), Some(1_795_516_927));
        assert_eq!(t("yesterday"), None);
    }

    #[test]
    fn a_duration_reads_in_seconds_then_minutes_then_hours() {
        assert_eq!(took(Duration::from_secs(14)), "14s");
        assert_eq!(took(Duration::from_secs(134)), "2m 14s");
        assert_eq!(took(Duration::from_secs(3900)), "1h 05m");
        assert_eq!(took_short(Duration::from_millis(3200)), "3.2s");
    }

    #[test]
    fn ago_is_now_then_minutes_hours_and_a_weekday() {
        let now = SystemTime::now();
        assert_eq!(ago(now, now), "now");
        assert_eq!(ago(now - Duration::from_secs(720), now), "12m");
        assert_eq!(ago(now - Duration::from_secs(7300), now), "2h");
        assert_eq!(ago(now - Duration::from_secs(2 * 86_400), now).len(), 3);
        let long = ago(now - Duration::from_secs(40 * 86_400), now);
        let (month, day) = long.split_once(' ').unwrap();
        assert!(month.len() == 3 && !day.starts_with('0'), "{long}");
    }
}
