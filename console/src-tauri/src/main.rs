// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Read;

use serde_json::{Value, json};

fn main() {
    // `ghost-console keeper` answers one keeper request on stdin and exits.
    // The development server uses it so the window can be worked on in a
    // browser against the same code the desktop app runs.
    if std::env::args().nth(1).as_deref() == Some("keeper") {
        let mut input = String::new();
        let _ = std::io::stdin().read_to_string(&mut input);
        let request: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
        let cmd = request["cmd"].as_str().unwrap_or_default();
        let answer = match ghost_console::keeper::dispatch(cmd, &request["args"]) {
            Ok(value) => json!({"ok": value}),
            Err(why) => json!({"err": why}),
        };
        println!("{answer}");
        return;
    }
    ghost_console::run();
}
