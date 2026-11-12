//! The desktop shell of the Ghost console.
//!
//! The console is a web client of the Ghost Protocol: it opens its own
//! WebSocket to a daemon, and everything it knows about a living ghost comes
//! over that. This process adds a window and one thing a web page cannot do:
//! keep ghosts on this machine. See [`keeper`].

pub mod keeper;

use serde_json::Value;

/// The one command the window may call. It reaches the keeper and nothing
/// else; the keeper cannot change a ghost once she is born.
#[tauri::command(async)]
fn keep(cmd: String, args: Value) -> Result<Value, String> {
    keeper::dispatch(&cmd, &args)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![keep])
        .run(tauri::generate_context!())
        .expect("the console window could not be opened");
}
