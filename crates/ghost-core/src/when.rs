//! When something happened, in the words she would remember it by.
//!
//! The journal keeps every instant to the microsecond, and nothing here
//! changes that. This is how an instant is put into words when it is shown
//! to her: the further back it is, the less exactly she knows when. Fourteen
//! minutes ago is "14 minutes ago"; a week ago is "last Friday afternoon";
//! March is "in March".
//!
//! Precision is not a function of distance alone. At the same distance one
//! memory is "last Friday around 5pm", another "last Friday afternoon",
//! another "last Friday". Which one a memory gets is drawn from its own
//! instant, so it is always the same for the same memory, and it only ever
//! gets coarser as time passes: a memory never becomes sharper with age.
//!
//! Days are her waking days, not calendar days: something at one in the
//! morning before she went to sleep was "last night", not "this morning".

use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc, Weekday};

use crate::life::Clock;

/// How exactly an instant is said, from finest to coarsest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Grain {
    Minute,
    Hour,
    Part,
    Day,
    Week,
    Month,
    Year,
}

const GRAINS: [Grain; 7] = [
    Grain::Minute,
    Grain::Hour,
    Grain::Part,
    Grain::Day,
    Grain::Week,
    Grain::Month,
    Grain::Year,
];

/// When `at` was, seen from `now`, as she remembers it.
pub fn when(at: DateTime<Utc>, now: DateTime<Utc>, clock: &Clock) -> String {
    let d = now - at;
    if d < Duration::minutes(2) {
        return "just now".into();
    }
    if d < Duration::hours(1) {
        return format!("{} minutes ago", d.num_minutes());
    }
    let days = waking_days(at, now, clock);
    say(grain(at, d, days), at, now, days, clock)
}

/// The waking day `at` belongs to, as a header for a stretch of
/// conversation: "today", "yesterday", "Tuesday", "last Friday",
/// "October 28".
pub fn day(at: DateTime<Utc>, now: DateTime<Utc>, clock: &Clock) -> String {
    match day_ref(at, now, clock) {
        DayRef::Today => "today".into(),
        DayRef::Yesterday => "yesterday".into(),
        DayRef::ThisWeek(w) => weekday(w).into(),
        DayRef::LastWeek(w) => format!("last {}", weekday(w)),
        DayRef::Date(date) => date_words(date, clock.day_of(now)),
    }
}

/// How many waking days back `at` is from `now`.
pub fn waking_days(at: DateTime<Utc>, now: DateTime<Utc>, clock: &Clock) -> i64 {
    (clock.day_of(now) - clock.day_of(at)).num_days()
}

/// The range of grains that fit a distance. Both ends only ever move
/// towards coarser as the distance grows.
fn band(d: Duration, days: i64) -> (usize, usize) {
    match days {
        _ if d < Duration::hours(1) => (0, 0),
        0 if d < Duration::hours(6) => (0, 1),
        0 => (1, 2),
        1..=6 => (1, 3),
        7..=13 => (1, 4),
        14..=60 => (3, 5),
        61..=365 => (5, 6),
        _ => (6, 6),
    }
}

/// The grain one memory is said at: the same place within the band for
/// every distance, drawn from the instant itself.
fn grain(at: DateTime<Utc>, d: Duration, days: i64) -> Grain {
    let (lo, hi) = band(d, days);
    let width = hi - lo + 1;
    let u = draw(at);
    // `u` is in [0, 1), so this is below `width`; `min` guards rounding.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let step = ((u * width as f64) as usize).min(width - 1);
    GRAINS[lo + step]
}

/// A number in [0, 1) that belongs to an instant and to nothing else.
fn draw(at: DateTime<Utc>) -> f64 {
    #[allow(clippy::cast_sign_loss)]
    let mut x = at.timestamp_micros() as u64;
    // splitmix64: neighbouring instants land far apart.
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    #[allow(clippy::cast_precision_loss)]
    let u = (x >> 11) as f64 / (1u64 << 53) as f64;
    u
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DayRef {
    Today,
    Yesterday,
    /// Earlier this week.
    ThisWeek(Weekday),
    LastWeek(Weekday),
    Date(NaiveDate),
}

fn day_ref(at: DateTime<Utc>, now: DateTime<Utc>, clock: &Clock) -> DayRef {
    let then = clock.day_of(at);
    let today = clock.day_of(now);
    match (today - then).num_days() {
        i64::MIN..=0 => DayRef::Today,
        1 => DayRef::Yesterday,
        2..=13 => match (monday(today) - monday(then)).num_days() / 7 {
            0 => DayRef::ThisWeek(then.weekday()),
            1 => DayRef::LastWeek(then.weekday()),
            _ => DayRef::Date(then),
        },
        _ => DayRef::Date(then),
    }
}

fn monday(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// Where in her day an instant fell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Morning,
    Midday,
    Afternoon,
    Evening,
    Night,
}

fn part(at: DateTime<Utc>, clock: &Clock) -> Part {
    let local = at.with_timezone(&clock.tz());
    // Past midnight but before she slept: still the night of the day before.
    if local.date_naive() != clock.day_of(at) {
        return Part::Night;
    }
    match local.hour() {
        5..=11 => Part::Morning,
        12..=13 => Part::Midday,
        14..=17 => Part::Afternoon,
        18..=21 => Part::Evening,
        _ => Part::Night,
    }
}

fn say(grain: Grain, at: DateTime<Utc>, now: DateTime<Utc>, days: i64, clock: &Clock) -> String {
    let d = now - at;
    match grain {
        Grain::Minute => format!(
            "{} at {}",
            on_day(at, now, clock, true),
            clock_time(at, clock)
        ),
        Grain::Hour if days == 0 && d < Duration::hours(6) => match d.num_hours() {
            1 => "about an hour ago".into(),
            n => format!("about {} hours ago", number(n)),
        },
        Grain::Hour => format!(
            "{} around {}",
            on_day(at, now, clock, true),
            hour(at, clock)
        ),
        Grain::Part => part_of_day(at, now, clock),
        Grain::Day => match day_ref(at, now, clock) {
            DayRef::Today => "earlier today".into(),
            DayRef::Yesterday => "yesterday".into(),
            DayRef::ThisWeek(w) => format!("on {}", weekday(w)),
            DayRef::LastWeek(w) => format!("last {}", weekday(w)),
            DayRef::Date(date) => format!("on {}", date_words(date, clock.day_of(now))),
        },
        Grain::Week => {
            let weeks = (monday(clock.day_of(now)) - monday(clock.day_of(at))).num_days() / 7;
            match weeks {
                i64::MIN..=0 => "earlier this week".into(),
                1 => "last week".into(),
                2 | 3 => format!("{} weeks ago", number(weeks)),
                4..=6 => "about a month ago".into(),
                _ => "about two months ago".into(),
            }
        }
        Grain::Month => {
            let (now, at) = (
                now.with_timezone(&clock.tz()),
                at.with_timezone(&clock.tz()),
            );
            let months =
                (now.year() * 12 + month0(now.month())) - (at.year() * 12 + month0(at.month()));
            let name = month_name(at.month());
            match months {
                i32::MIN..=0 => "earlier this month".into(),
                1 => "last month".into(),
                _ if at.year() == now.year() => format!("in {name}"),
                _ if at.year() + 1 == now.year() => format!("in {name} last year"),
                _ => format!("in {name} {}", at.year()),
            }
        }
        Grain::Year => {
            let years = i64::from(
                now.with_timezone(&clock.tz()).year() - at.with_timezone(&clock.tz()).year(),
            );
            match years {
                i64::MIN..=0 => "earlier this year".into(),
                1 => "last year".into(),
                n => format!("{} years ago", number(n)),
            }
        }
    }
}

/// The day, with the part of it where that reads naturally, for a time
/// to follow: "yesterday afternoon", "last night", "on Tuesday". Around
/// midday the part is left out, since the hour says it.
fn on_day(at: DateTime<Utc>, now: DateTime<Utc>, clock: &Clock, with_part: bool) -> String {
    let p = part(at, clock);
    if with_part && p != Part::Midday {
        return part_of_day(at, now, clock);
    }
    match day_ref(at, now, clock) {
        DayRef::Today => "today".into(),
        DayRef::Yesterday => "yesterday".into(),
        DayRef::ThisWeek(w) => format!("on {}", weekday(w)),
        DayRef::LastWeek(w) => format!("last {}", weekday(w)),
        DayRef::Date(date) => format!("on {}", date_words(date, clock.day_of(now))),
    }
}

fn part_of_day(at: DateTime<Utc>, now: DateTime<Utc>, clock: &Clock) -> String {
    let p = part(at, clock);
    let word = match p {
        Part::Morning => "morning",
        Part::Midday => "midday",
        Part::Afternoon => "afternoon",
        Part::Evening => "evening",
        Part::Night => "night",
    };
    match (day_ref(at, now, clock), p) {
        (DayRef::Today, Part::Night) => "earlier tonight".into(),
        (DayRef::Today, Part::Midday) => "around midday".into(),
        (DayRef::Today, _) => format!("this {word}"),
        (DayRef::Yesterday, Part::Night) => "last night".into(),
        (DayRef::Yesterday, Part::Midday) => "yesterday around midday".into(),
        (DayRef::Yesterday, _) => format!("yesterday {word}"),
        (DayRef::ThisWeek(w), Part::Midday) => format!("on {} around midday", weekday(w)),
        (DayRef::ThisWeek(w), _) => format!("on {} {word}", weekday(w)),
        (DayRef::LastWeek(w), Part::Midday) => format!("last {} around midday", weekday(w)),
        (DayRef::LastWeek(w), _) => format!("last {} {word}", weekday(w)),
        (DayRef::Date(date), Part::Midday) => {
            format!("around midday on {}", date_words(date, clock.day_of(now)))
        }
        (DayRef::Date(date), _) => {
            format!("on the {word} of {}", date_words(date, clock.day_of(now)))
        }
    }
}

fn clock_time(at: DateTime<Utc>, clock: &Clock) -> String {
    let local = at.with_timezone(&clock.tz());
    let (pm, h) = local.hour12();
    format!("{h}:{:02}{}", local.minute(), if pm { "pm" } else { "am" })
}

fn hour(at: DateTime<Utc>, clock: &Clock) -> String {
    let local = at.with_timezone(&clock.tz()) + Duration::minutes(30);
    match local.hour() {
        0 => "midnight".into(),
        12 => "noon".into(),
        h if h < 12 => format!("{h}am"),
        h => format!("{}pm", h - 12),
    }
}

fn date_words(date: NaiveDate, today: NaiveDate) -> String {
    if date.year() == today.year() {
        format!("{} {}", month_name(date.month()), date.day())
    } else {
        format!(
            "{} {}, {}",
            month_name(date.month()),
            date.day(),
            date.year()
        )
    }
}

fn weekday(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

fn month_name(m: u32) -> &'static str {
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ][(m.clamp(1, 12) - 1) as usize]
}

fn month0(m: u32) -> i32 {
    i32::try_from(m).unwrap_or(1) - 1
}

fn number(n: i64) -> String {
    match n {
        2 => "two".into(),
        3 => "three".into(),
        4 => "four".into(),
        5 => "five".into(),
        n => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveTime, TimeZone};

    use super::*;

    fn clock() -> Clock {
        Clock::new(
            chrono_tz::Asia::Shanghai,
            NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(1, 0, 0).unwrap(),
        )
    }

    /// A local time in Shanghai.
    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        chrono_tz::Asia::Shanghai
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Friday 2026-11-13, 16:00 local.
    fn now() -> DateTime<Utc> {
        at(2026, 11, 13, 16, 0)
    }

    #[test]
    fn the_last_hour_is_kept_to_the_minute() {
        let c = clock();
        assert_eq!(when(now() - Duration::seconds(40), now(), &c), "just now");
        assert_eq!(
            when(now() - Duration::minutes(14), now(), &c),
            "14 minutes ago"
        );
        assert_eq!(
            when(now() - Duration::minutes(59), now(), &c),
            "59 minutes ago"
        );
    }

    #[test]
    fn a_memory_never_gets_sharper_with_age() {
        let c = clock();
        // Many memories, each watched from the moment it happened until
        // two years later.
        for k in 0..300 {
            let then = now() - Duration::minutes(k * 37 + 3);
            let mut last = Grain::Minute;
            for h in 0..(8 * 800) {
                let later = then + Duration::hours(h * 3);
                let d = later - then;
                let g = grain(then, d, waking_days(then, later, &c));
                assert!(g >= last, "{then} seen at {later}: {g:?} after {last:?}");
                last = g;
            }
        }
    }

    #[test]
    fn the_same_memory_is_always_put_the_same_way() {
        let c = clock();
        let then = at(2026, 11, 6, 17, 4);
        assert_eq!(when(then, now(), &c), when(then, now(), &c));
    }

    #[test]
    fn memories_at_the_same_distance_are_not_all_equally_sharp() {
        let c = clock();
        // Last Friday, between five and six in the afternoon.
        let said: std::collections::BTreeSet<String> = (0..60)
            .map(|s| {
                when(
                    at(2026, 11, 6, 17, 0) + Duration::seconds(s * 59),
                    now(),
                    &c,
                )
            })
            .collect();
        assert!(said.contains("last Friday"), "{said:?}");
        assert!(said.contains("last Friday afternoon"), "{said:?}");
        assert!(
            said.iter()
                .any(|s| s.starts_with("last Friday afternoon around ")),
            "{said:?}"
        );
    }

    #[test]
    fn each_grain_reads_naturally() {
        let c = clock();
        let n = now();
        let friday = at(2026, 11, 6, 17, 4);
        let days = waking_days(friday, n, &c);
        assert_eq!(days, 7);
        assert_eq!(
            say(Grain::Hour, friday, n, days, &c),
            "last Friday afternoon around 5pm"
        );
        assert_eq!(
            say(Grain::Part, friday, n, days, &c),
            "last Friday afternoon"
        );
        assert_eq!(say(Grain::Day, friday, n, days, &c), "last Friday");
        assert_eq!(say(Grain::Week, friday, n, days, &c), "last week");

        let tuesday = at(2026, 11, 10, 12, 40);
        let days = waking_days(tuesday, n, &c);
        assert_eq!(
            say(Grain::Minute, tuesday, n, days, &c),
            "on Tuesday at 12:40pm"
        );
        assert_eq!(
            say(Grain::Part, tuesday, n, days, &c),
            "on Tuesday around midday"
        );

        let morning = at(2026, 11, 13, 11, 10);
        assert_eq!(say(Grain::Hour, morning, n, 0, &c), "about four hours ago");
        assert_eq!(say(Grain::Part, morning, n, 0, &c), "this morning");

        let march = at(2026, 3, 2, 9, 0);
        let days = waking_days(march, n, &c);
        assert_eq!(say(Grain::Month, march, n, days, &c), "in March");
        assert_eq!(say(Grain::Year, march, n, days, &c), "earlier this year");
        assert_eq!(
            say(Grain::Day, at(2025, 3, 2, 9, 0), n, 400, &c),
            "on March 2, 2025"
        );
    }

    #[test]
    fn a_day_is_her_waking_day_not_the_calendar_one() {
        let c = clock();
        // Half past midnight on Friday, before she slept at one: Thursday night.
        let late = at(2026, 11, 13, 0, 30);
        assert_eq!(waking_days(late, now(), &c), 1);
        assert_eq!(part_of_day(late, now(), &c), "last night");
        assert_eq!(day(late, now(), &c), "yesterday");
        // Seen from twenty past midnight the same night, it is still tonight.
        let soon = at(2026, 11, 13, 0, 20);
        let midnight = at(2026, 11, 12, 23, 50);
        assert_eq!(part_of_day(midnight, soon, &c), "earlier tonight");
    }

    #[test]
    fn days_are_named_by_the_week_they_fall_in() {
        let c = clock();
        assert_eq!(day(at(2026, 11, 13, 9, 0), now(), &c), "today");
        assert_eq!(day(at(2026, 11, 10, 9, 0), now(), &c), "Tuesday");
        assert_eq!(day(at(2026, 11, 7, 9, 0), now(), &c), "last Saturday");
        assert_eq!(day(at(2026, 10, 28, 9, 0), now(), &c), "October 28");
    }
}
