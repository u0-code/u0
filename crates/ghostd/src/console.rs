//! The commands that connect to a character in a running daemon as a
//! console and print what she says.

use ghost_link::Console;

use crate::{Daemon, show, wire};

pub(crate) async fn watch(daemon: Daemon, last: u32) -> anyhow::Result<()> {
    let (mut console, welcome) = Console::connect(&daemon.url, daemon.token, daemon.ghost).await?;
    let status = console.status().await?;
    let tz: chrono_tz::Tz = status.timezone.parse().unwrap_or(chrono_tz::UTC);
    eprintln!(
        "watching {} in {} ({})",
        welcome.ghost.name, welcome.ghost.city, status.local_time
    );
    let print = |entry: ghost_protocol::Entry| {
        let (seq, at) = (entry.seq, entry.at);
        match wire::event(entry) {
            Ok(event) => println!("{}", show::event(seq, at, &event, tz)),
            Err(e) => eprintln!("(an entry this build cannot read: {e})"),
        }
    };
    if last > 0 {
        for entry in console.journal(1 - i64::from(last), last).await? {
            print(entry);
        }
    }
    console.subscribe().await?;
    while let Some(entry) = console.next_entry().await? {
        print(entry);
    }
    eprintln!("the daemon went away");
    Ok(())
}
