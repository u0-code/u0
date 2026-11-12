//! The desktop shell of the Ghost console.
//!
//! The console itself is a web client of the Ghost Protocol: it opens its own
//! WebSocket to a daemon and needs nothing from this process but a window.
//! Keeping it that way means the same console runs in a browser, and that
//! this shell can never do more to a ghost than the protocol allows.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("the console window could not be opened");
}
