//! The commands that connect to a character in a running daemon as a
//! console and print what she says.

use ghost_core::journal::Entry;
use ghost_link::Console;

use crate::{Daemon, show};

pub(crate) async fn watch(daemon: Daemon, last: u32) -> anyhow::Result<()> {
    let (mut console, welcome) = Console::connect(&daemon.url, daemon.token, daemon.ghost).await?;
    let status = console.status().await?;
    let tz: chrono_tz::Tz = status["timezone"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    eprintln!(
        "watching {} in {} ({})",
        welcome.ghost.name,
        welcome.ghost.city,
        status["local_time"].as_str().unwrap_or_default()
    );
    let print = |value: serde_json::Value| match serde_json::from_value::<Entry>(value) {
        Ok(entry) => println!("{}", show::entry(&entry, tz)),
        Err(e) => eprintln!("(an entry this build cannot read: {e})"),
    };
    if last > 0 {
        for value in console.journal(1 - i64::from(last), last).await? {
            print(value);
        }
    }
    console.subscribe().await?;
    while let Some(value) = console.next_entry().await? {
        print(value);
    }
    eprintln!("the daemon went away");
    Ok(())
}
