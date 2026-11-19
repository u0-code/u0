//! Printing her journal and the people in it for a terminal.

use chrono_tz::Tz;
use ghost_core::journal::{Entry, Event, Line};
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

/// One line of a journal on one line: an entry as [`entry`] shows it, or
/// one this build cannot read, by its kind.
pub fn line(l: &Line, tz: Tz) -> String {
    match l {
        Line::Entry(e) => entry(e, tz),
        Line::Unknown { seq, at, kind } => format!(
            "#{seq:<5} {}  {kind:<9} (a kind this version does not know: a newer one wrote it)",
            at.with_timezone(&tz).format("%m-%d %H:%M:%S"),
        ),
    }
}

fn describe(event: &Event, tz: Tz) -> String {
    let flat = |text: &str| text.replace('\n', " ⏎ ");
    match event {
        Event::Born { name, seal } => format!("{name} was born (seal {})", seal.short()),
        Event::Described { seal, .. } => {
            format!("her persona as it stands now (seal {})", seal.short())
        }
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
            images,
            ..
        } => format!(
            "[{channel}:{}] {}{}: {}{}",
            place.name.as_deref().unwrap_or(&place.id),
            from.name,
            if *addressed { "" } else { " (not to her)" },
            flat(text),
            match images.len() {
                0 => String::new(),
                1 => " (1 image)".to_owned(),
                n => format!(" ({n} images)"),
            }
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
        Event::Intended {
            due, what, report, ..
        } => format!(
            "by {}{}: {}",
            due.with_timezone(&tz).format("%m-%d %H:%M"),
            if *report { " (report)" } else { "" },
            flat(what)
        ),
        Event::Settled { intention, outcome } => format!("#{intention}: {}", flat(outcome)),
        Event::Left { person, why } => format!("{person}: {}", flat(why)),
        Event::Canon { fact, source } => format!("{} [{source}]", flat(fact)),
        Event::Undertook { id, brief, dir, .. } => match dir {
            Some(dir) => format!("[{id}] in {dir}: {}", flat(brief)),
            None => format!("[{id}] {}", flat(brief)),
        },
        Event::Asked { id, question } => format!("[{id}] {}", flat(question)),
        Event::Added { id, text, by } => match by {
            Some(by) => format!("[{id}] from {by}: {}", flat(text)),
            None => format!("[{id}] {}", flat(text)),
        },
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
        Event::Viewed {
            message,
            image,
            what,
            file,
        } => format!(
            "#{message} image {image}{}: {}",
            file.as_deref()
                .map_or_else(String::new, |f| format!(" (media/{f})")),
            flat(what)
        ),
        Event::Kept { file, note, from } => format!(
            "media/{file}{}: {}",
            from.as_deref()
                .map_or_else(String::new, |f| format!(" (from {f})")),
            flat(note)
        ),
        Event::LetGo { kept } => format!("let #{kept} go"),
        Event::Reported { id, text, files } => format!(
            "[{id}] {}{}",
            flat(text),
            if files.is_empty() {
                String::new()
            } else {
                format!(" ({})", files.join(", "))
            }
        ),
        Event::Opened {
            url, text, file, ..
        } => format!(
            "{url}{}: {}",
            file.as_deref()
                .map_or_else(String::new, |f| format!(" (media/{f})")),
            flat(&text.chars().take(200).collect::<String>())
        ),
        Event::Directed {
            message,
            from,
            text,
        } => format!("#{message} from {from}: {}", flat(text)),
        Event::Quieted { place, until } => format!(
            "quiet in {place} until {}",
            until.with_timezone(&tz).format("%m-%d %H:%M")
        ),
        Event::LookedUp { query, found, .. } => format!(
            "\"{query}\": {}",
            flat(&found.chars().take(200).collect::<String>())
        ),
        Event::Installed {
            name,
            version,
            source,
            tools,
            ..
        } => format!("{name} {version} ({source}): {}", tools.join(", ")),
        Event::Uninstalled { name, why } | Event::SkillUninstalled { name, why } => {
            format!("{name}: {}", flat(why))
        }
        Event::SkillInstalled {
            name, source, sha, ..
        } => format!(
            "{name} ({source} at {})",
            sha.chars().take(12).collect::<String>()
        ),
        Event::Stopped {
            place,
            by,
            looking,
            unsaid,
            works,
        } => {
            let mut what = Vec::new();
            if *looking {
                what.push("her look".to_owned());
            }
            if !unsaid.is_empty() {
                let quoted: Vec<String> =
                    unsaid.iter().map(|u| format!("\"{}\"", flat(u))).collect();
                what.push(format!("unsaid {}", quoted.join(" ")));
            }
            if !works.is_empty() {
                what.push(format!("work {}", works.join(", ")));
            }
            format!("[{place}] by {by}: {}", what.join("; "))
        }
        Event::Rewound {
            place,
            by,
            to,
            looking,
            unsaid,
        } => {
            let mut what = vec![format!("to before m{to}")];
            if *looking {
                what.push("her look".to_owned());
            }
            if !unsaid.is_empty() {
                let quoted: Vec<String> =
                    unsaid.iter().map(|u| format!("\"{}\"", flat(u))).collect();
                what.push(format!("unsaid {}", quoted.join(" ")));
            }
            format!("[{place}] by {by}: {}", what.join("; "))
        }
        Event::BrainChanged { think, was } => match was {
            Some(was) => format!("{think}, was {was}"),
            None => think.clone(),
        },
        Event::SessionNamed { place, by, title } => {
            format!("{place} called \"{}\" by {by}", flat(title))
        }
        Event::SessionSetAside { place, by } => format!("{place} set aside by {by}"),
        Event::Moved { to } => format!("moved to {to}"),
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
