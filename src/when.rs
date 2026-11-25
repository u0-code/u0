//! Times as a person reads them in a sidebar: how long something took, how
//! long ago, the hour of the day in this computer's own time zone.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// The local broken-down time of `t`.
fn local(t: SystemTime) -> libc::tm {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let secs = libc::time_t::try_from(secs).unwrap_or_default();
    // SAFETY: localtime_r writes only into the `tm` it is given.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&raw const secs, &raw mut tm);
        tm
    }
}

/// `t` as a name for a folder, in this computer's time zone:
/// "2026-11-25 0142".
pub fn stamp(t: SystemTime) -> String {
    let tm = local(t);
    format!(
        "{}-{:02}-{:02} {:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

/// `days` days after `t`, at `hour` o'clock in this computer's time zone.
pub fn at_hour(t: SystemTime, days: i32, hour: i32) -> SystemTime {
    let mut tm = local(t);
    tm.tm_mday += days;
    tm.tm_hour = hour;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    // SAFETY: mktime reads and normalizes only the `tm` it is given.
    let secs = unsafe { libc::mktime(&raw mut tm) };
    u64::try_from(secs).map_or(t, |s| UNIX_EPOCH + Duration::from_secs(s))
}

/// Days from `t` to the next Monday (1 to 7).
pub fn to_monday(t: SystemTime) -> i32 {
    let wday = local(t).tm_wday;
    (8 - wday) % 7 + if wday == 1 { 7 } else { 0 }
}

/// The time of day of `t`: "10:42".
pub fn clock(t: SystemTime) -> String {
    let tm = local(t);
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

/// When a message was said: its time of day if today ("10:42"), else its
/// date too ("2026-11-24 10:42").
pub fn said_at(t: SystemTime, now: SystemTime) -> String {
    let (a, b) = (local(t), local(now));
    if (a.tm_year, a.tm_yday) == (b.tm_year, b.tm_yday) {
        clock(t)
    } else {
        format!(
            "{}-{:02}-{:02} {}",
            a.tm_year + 1900,
            a.tm_mon + 1,
            a.tm_mday,
            clock(t)
        )
    }
}

/// How long ago `t` was, the way a list of sessions says it: "now", "12m",
/// "2h", and past a day the weekday ("Mon"), past a week the date.
pub fn ago(t: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(t).map_or(0, |d| d.as_secs());
    match secs {
        0..60 => "now".into(),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        86_400..604_800 => {
            const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            DAYS[usize::try_from(local(t).tm_wday).unwrap_or(0) % 7].into()
        }
        _ => {
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let tm = local(t);
            format!(
                "{} {}",
                MONTHS[usize::try_from(tm.tm_mon).unwrap_or(0) % 12],
                tm.tm_mday
            )
        }
    }
}

/// A time written as RFC 3339 says ("2026-11-24T10:42:07.123Z", or with
/// an offset such as "+08:00"), as agents date their sessions.
pub fn parse(text: &str) -> Option<SystemTime> {
    let bytes = text.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<i64> { text.get(r)?.parse().ok() };
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't' | b' ')
    {
        return None;
    }
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let clock = num(11..13)? * 3600 + num(14..16)? * 60 + num(17..19)?;
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        rest = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest.as_bytes().first() {
        Some(b'Z' | b'z') | None => 0,
        Some(sign @ (b'+' | b'-')) => {
            let east: i64 = rest.get(1..3)?.parse::<i64>().ok()? * 3600
                + rest.get(4..6)?.parse::<i64>().ok()? * 60;
            if *sign == b'+' { east } else { -east }
        }
        _ => return None,
    };
    // Days from the civil date, as Howard Hinnant counts them.
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = shifted.div_euclid(400);
    let of_era = shifted - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = era * 146_097 + of_cycle - 719_468;
    let secs = days * 86_400 + clock - offset;
    Some(UNIX_EPOCH + Duration::from_secs(u64::try_from(secs).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snooze_lands_on_the_hour_asked_and_next_monday_is_ahead() {
        let now = SystemTime::now();
        let t = at_hour(now, 1, 9);
        assert!(t > now);
        let tm = local(t);
        assert_eq!((tm.tm_hour, tm.tm_min), (9, 0));
        let days = to_monday(now);
        assert!((1..=7).contains(&days));
        assert_eq!(local(at_hour(now, days, 9)).tm_wday, 1);
    }

    #[test]
    fn an_rfc_3339_time_is_read_with_its_fraction_and_offset() {
        let t = |s| {
            parse(s)
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
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
    }
}
