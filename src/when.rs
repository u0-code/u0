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

/// The time of day of `t`: "10:42".
pub fn clock(t: SystemTime) -> String {
    let tm = local(t);
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
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

#[cfg(test)]
mod tests {
    use super::*;

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
