//! The commands that work on one ghost directory by itself: `birth`, which
//! makes her, and `status`, `journal` and `verify`, which read her without
//! disturbing a daemon that is running her.

use std::{fmt::Write as _, path::Path};

use anyhow::Context;
use chrono::Utc;
use ghost_core::{
    Ghost,
    life::{Life as Day, Phase},
    memory::Memory,
};

use crate::show;

pub(crate) fn birth(dir: &Path) -> anyhow::Result<()> {
    let ghost = Ghost::birth(dir).with_context(|| format!("birth in {}", dir.display()))?;
    let id = ghost.identity();
    println!(
        "{} is born in {}, {}. seal {}",
        id.name(),
        id.seed().home.city,
        id.seed().home.country,
        id.seal().short()
    );
    Ok(())
}

pub(crate) fn status(dir: &Path) -> anyhow::Result<()> {
    let peek = Ghost::peek(dir)?;
    let id = &peek.identity;
    let life = Day::new(id);
    let now = Utc::now();
    let local = now.with_timezone(&life.clock().tz());
    let moment = life.moment(&peek.journal, now)?;
    println!("name      {} ({})", id.name(), id.seed().pronouns);
    println!(
        "home      {}, {} [{}]",
        id.seed().home.city,
        id.seed().home.country,
        id.seed().home.timezone
    );
    println!("born      {}", id.seed().born);
    println!(
        "seal      {}{}",
        id.seal(),
        if peek.revised {
            " (ghost.toml revised; recorded and in effect at the next run)"
        } else {
            ""
        }
    );
    println!("local     {}", local.format("%Y-%m-%d %H:%M %Z"));
    println!(
        "now       {}",
        match (&moment.phase, &moment.doing) {
            (Phase::Asleep, _) => "asleep".to_owned(),
            (Phase::Awake, Some(doing)) =>
                format!("{} ({:?})", doing.what, doing.reach).to_lowercase(),
            (Phase::Awake, None) => "awake, nothing planned right now".to_owned(),
        }
    );
    println!("journal   {} entries", peek.entries);
    if let Some(last) = peek.journal.last()? {
        println!("last      {}", show::entry(&last, life.clock().tz()));
    }
    // What a running body keeps on disk, read without disturbing it. What
    // only the live process knows, such as which sitting is running this
    // second, is not here; the time of each piece of work's last step is
    // the nearest thing.
    let Some(memory) = Memory::glance(dir)? else {
        println!("memory    not built yet, or by another version; it is built when she runs");
        return Ok(());
    };
    let unread = memory.places_unread()?;
    if unread.is_empty() {
        println!("unread    nothing");
    }
    for (i, place) in unread.iter().enumerate() {
        let mut detail = format!("{} unread", place.unread);
        if place.unread_addressed > 0 {
            let _ = write!(detail, ", {} to her", place.unread_addressed);
        }
        if place.unread_directed > 0 {
            let _ = write!(detail, ", {} from an operator", place.unread_directed);
        }
        let name = place
            .name
            .as_ref()
            .map_or_else(|| place.key.clone(), |n| format!("{n} ({})", place.key));
        println!("{:<9} {name}: {detail}", if i == 0 { "unread" } else { "" });
    }
    let open = memory.undertakings(true, 50)?;
    let db = dir.join(ghost_work::WORK_DB);
    let seen = if db.exists() {
        ghost_work::undertaking::glance(&db).with_context(|| format!("read {}", db.display()))?
    } else {
        Vec::new()
    };
    println!("work      {} open", open.len());
    for work in &open {
        let brief: String = work.brief.split_whitespace().collect::<Vec<_>>().join(" ");
        let brief: String = brief.chars().take(70).collect();
        println!("          #{} {brief}", work.seq);
        let mut detail = format!("{} sittings", work.sittings);
        if let Some(seen) = seen.iter().find(|s| s.id == work.id) {
            let _ = write!(detail, ", {} steps", seen.steps);
            if let Some(at) = seen.last_step {
                let at = at.with_timezone(&life.clock().tz());
                let _ = write!(detail, ", last step {}", at.format("%m-%d %H:%M"));
            }
            if seen.parts_open > 0 {
                let _ = write!(detail, ", {} parts unfinished", seen.parts_open);
            }
        }
        if let Some(question) = &work.waiting {
            let question: String = question.chars().take(60).collect();
            let _ = write!(detail, ", waiting to hear: {question}");
        }
        println!("            {detail}");
    }
    Ok(())
}

pub(crate) fn journal(dir: &Path, last: usize) -> anyhow::Result<()> {
    let peek = Ghost::peek(dir)?;
    let tz = peek.identity.tz();
    for entry in peek.journal.tail(last)? {
        println!("{}", show::entry(&entry, tz));
    }
    Ok(())
}

pub(crate) fn verify(dir: &Path) -> anyhow::Result<()> {
    let peek = Ghost::peek(dir)?;
    println!("chain ok, {} entries", peek.entries);
    Ok(())
}
