//! Printing her journal and the people in it for a terminal.

use chrono_tz::Tz;
use ghost_core::journal::{Entry, Event};
use serde_json::Value;

/// One journal entry on one line, in her local time.
pub fn entry(e: &Entry, tz: Tz) -> String {
    format!(
        "#{:<5} {}  {:<9} {}",
        e.seq,
        e.at.with_timezone(&tz).format("%m-%d %H:%M:%S"),
        e.event.kind(),
        describe(&e.event, tz)
    )
}

fn describe(event: &Event, tz: Tz) -> String {
    let flat = |text: &str| text.replace('\n', " ⏎ ");
    match event {
        Event::Born { name, seal } => format!("{name} was born (seal {})", seal.short()),
        Event::Dormant { since } => format!(
            "nothing since {}",
            since.with_timezone(&tz).format("%m-%d %H:%M")
        ),
        Event::Woke => "woke up".into(),
        Event::Slept => "went to sleep".into(),
        Event::Planned {
            day,
            weather,
            blocks,
        } => {
            let plan = blocks
                .iter()
                .map(|b| format!("{}–{} {} [{:?}]", b.from, b.to, b.what, b.reach).to_lowercase())
                .collect::<Vec<_>>()
                .join(" · ");
            match weather {
                Some(w) => format!("{day} ({w}): {plan}"),
                None => format!("{day}: {plan}"),
            }
        }
        Event::Lived { text, .. }
        | Event::Reflected {
            text,
            channel: None,
            ..
        } => flat(text),
        Event::Reflected {
            text,
            channel: Some(channel),
            ..
        } => format!("[{channel}] {}", flat(text)),
        Event::Heard {
            channel,
            place,
            from,
            text,
            addressed,
            ..
        } => format!(
            "[{channel}:{}] {}{}: {}",
            place.name.as_deref().unwrap_or(&place.id),
            from.name,
            if *addressed { "" } else { " (not to her)" },
            flat(text)
        ),
        Event::Read {
            channel,
            place,
            upto,
        } => format!("[{channel}:{place}] up to #{upto}"),
        Event::Said {
            channel,
            place,
            text,
            ..
        } => format!("[{channel}:{place}] {}", flat(text)),
        Event::Reacted {
            channel,
            place,
            emoji,
            ..
        } => format!("[{channel}:{place}] {emoji}"),
        Event::Posted { channel, text, .. } => format!("[{channel}] {}", flat(text)),
        Event::Saw { channel, text } => format!("[{channel}] {}", flat(text)),
        Event::Decided {
            about,
            decision,
            reason,
        } => {
            if reason.is_empty() {
                format!("[{about}] {decision}")
            } else {
                format!("[{about}] {decision} — {}", flat(reason))
            }
        }
        Event::Felt { toward, delta, why } => format!("{delta:+} toward {toward}: {}", flat(why)),
        Event::Noted { about, text, .. } => format!("{about}: {}", flat(text)),
        Event::Intended { due, what, .. } => format!(
            "by {}: {}",
            due.with_timezone(&tz).format("%m-%d %H:%M"),
            flat(what)
        ),
        Event::Settled { intention, outcome } => format!("#{intention}: {}", flat(outcome)),
        Event::Left { person, why } => format!("{person}: {}", flat(why)),
        Event::Canon { fact, source } => format!("{} [{source}]", flat(fact)),
        Event::Undertook { id, brief, .. } => format!("[{id}] {}", flat(brief)),
        Event::Asked { id, question } => format!("[{id}] {}", flat(question)),
        Event::Added { id, text } => format!("[{id}] {}", flat(text)),
        Event::Worked { id, summary, steps } => {
            format!("[{id}] {steps} steps: {}", flat(summary))
        }
        Event::Concluded {
            id,
            outcome,
            summary,
        } => format!("[{id}] {outcome}: {}", flat(summary)),
        Event::Measured {
            channel,
            followers,
            posts,
        } => format!(
            "[{channel}] {} posts measured{}",
            posts.len(),
            followers.map_or_else(String::new, |f| format!(", {f} followers"))
        ),
        Event::Learned { lesson, .. } | Event::Practised { lesson, .. } => flat(lesson),
        Event::Pursued { what, .. } if what.trim().is_empty() => "let it go".to_owned(),
        Event::Pursued { what, .. } => flat(what),
        Event::Challenged {
            claimant,
            claimed,
            word,
        } => format!("{claimant} says they are {claimed}; waiting for \"{word}\""),
        Event::Linked { one, other } => format!("{one} and {other} are one person"),
        Event::Digested { place, text, .. } => format!("[{place}] {}", flat(text)),
        Event::Profiled { person, shared, .. } => format!("{person}: {}", flat(shared)),
        Event::LookedUp { query, found, .. } => format!(
            "\"{query}\": {}",
            flat(&found.chars().take(200).collect::<String>())
        ),
    }
}

/// The people she knows, from a console's `memory.people` answer.
pub fn people(value: &Value) {
    let Some(people) = value.as_array().filter(|p| !p.is_empty()) else {
        println!("she does not know anyone yet");
        return;
    };
    for entry in people {
        let (person, standing) = (&entry["person"], &entry["standing"]);
        println!(
            "{:<28} {:<13} warmth {:>+3}  {} day(s)  heard {}",
            format!(
                "{} ({})",
                person["name"].as_str().unwrap_or("?"),
                person["key"].as_str().unwrap_or("?")
            ),
            standing["stage"].as_str().unwrap_or("?"),
            standing["warmth"].as_i64().unwrap_or(0),
            standing["active_days"].as_i64().unwrap_or(0),
            person["heard"].as_i64().unwrap_or(0),
        );
        for note in entry["notes"].as_array().into_iter().flatten() {
            println!("    · {}", note["text"].as_str().unwrap_or_default());
        }
    }
}
