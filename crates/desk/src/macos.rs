//! What u0 asks of macOS: its icon, the Dock's badge, dictation, a
//! notification's sound, a Terminal window.
//!
//! The app's own icon is its mark, on the body macOS app icons sit on. A
//! bundle built with `bundle.sh` carries it as `u0.icns`, and Finder,
//! Launchpad and the Dock read it from there. Run straight from cargo there
//! is no bundle, so the desk hands `AppKit` the same picture as it starts,
//! and the Dock shows the mark either way. The picture is
//! `assets/app/icon.png`, rendered from `assets/app/icon.svg`.

/// The 1024-pixel picture, built into the binary.
#[cfg(target_os = "macos")]
const PNG: &[u8] = include_bytes!("../../../assets/app/icon.png");

/// Give the running app its icon. Call once the app has started.
#[cfg(target_os = "macos")]
pub fn set() {
    use cocoa::{
        appkit::{NSApp, NSApplication, NSImage},
        base::{id, nil},
        foundation::NSData,
    };

    // SAFETY: called on the main thread after GPUI has made the shared
    // NSApplication; the data and image are plain Foundation objects made
    // here, and AppKit retains the image it is given.
    unsafe {
        let data: id = NSData::dataWithBytes_length_(
            nil,
            PNG.as_ptr().cast(),
            u64::try_from(PNG.len()).unwrap_or(0),
        );
        let image: id = NSImage::initWithData_(NSImage::alloc(nil), data);
        if image != nil {
            NSApp().setApplicationIconImage_(image);
        }
    }
}

/// Begin macOS dictation into the field that has the focus: what is said
/// is typed there, as by the keyboard.
#[cfg(target_os = "macos")]
pub fn dictate() {
    use cocoa::{
        appkit::NSApp,
        base::{id, nil},
    };
    use objc::{msg_send, sel, sel_impl};
    // SAFETY: called on the main thread; `startDictation:` is a standard
    // NSApplication action that takes a sender, which may be nil.
    unsafe {
        let app: id = NSApp();
        let _: () = msg_send![app, startDictation: nil];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn dictate() {}

/// Whether the event being handled is a key held down (macOS repeating
/// it), or pressed; none when it is not a key at all (a menu clicked).
#[cfg(target_os = "macos")]
pub fn key_held() -> Option<bool> {
    use cocoa::{
        appkit::{NSApp, NSEvent, NSEventType},
        base::{id, nil},
    };
    use objc::{msg_send, sel, sel_impl};
    // SAFETY: called on the main thread while AppKit dispatches an event;
    // `currentEvent` is that event or nil, and is only read here.
    unsafe {
        let event: id = msg_send![NSApp(), currentEvent];
        if event == nil || event.eventType() != NSEventType::NSKeyDown {
            return None;
        }
        let held: bool = msg_send![event, isARepeat];
        Some(held)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn key_held() -> Option<bool> {
    Some(false)
}

/// Say on the Dock's icon how many things wait for you; nothing at none.
#[cfg(target_os = "macos")]
pub fn badge(n: usize) {
    use cocoa::{
        appkit::NSApp,
        base::{id, nil},
        foundation::NSString,
    };
    use objc::{msg_send, sel, sel_impl};
    // SAFETY: called on the main thread; the dock tile is the shared
    // NSApplication's, and the label a Foundation string made here (nil
    // clears it).
    unsafe {
        let tile: id = msg_send![NSApp(), dockTile];
        let label: id = if n == 0 {
            nil
        } else {
            NSString::alloc(nil).init_str(&n.to_string())
        };
        let _: () = msg_send![tile, setBadgeLabel: label];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn badge(_: usize) {}

/// Elsewhere the window manager's own icon stands.
#[cfg(not(target_os = "macos"))]
pub fn set() {}

/// `text` as it goes between the quotes of a string in a script for
/// `osascript`.
fn apple_quoted(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Play macOS's Glass, the sound a notification is asked to make.
#[cfg(target_os = "macos")]
pub fn chime() {
    use cocoa::{
        base::{id, nil},
        foundation::NSString,
    };
    use objc::{class, msg_send, sel, sel_impl};
    // SAFETY: called on the main thread; NSSound's named sounds are the
    // system's, and the name a Foundation string made here.
    unsafe {
        let name = NSString::alloc(nil).init_str("Glass");
        let sound: id = msg_send![class!(NSSound), soundNamed: name];
        if sound != nil {
            let _: bool = msg_send![sound, play];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn chime() {}

/// Run `program` with `args` and `env` in a Terminal window, for a sign-in
/// that is done by typing: the agent's own login, which it says how to run.
pub fn in_terminal(
    program: &std::path::Path,
    args: &[String],
    env: &[(String, String)],
) -> std::io::Result<()> {
    // A key goes into the line as it is: one that is not a name would be
    // a command of its own, and a remote agent's come from that machine.
    let name = |k: &str| {
        k.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if let Some((k, _)) = env.iter().find(|(k, _)| !name(k)) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            crate::tr_with(
                "the agent asks to set %{name}, which is not a name a shell takes",
                &[("name", &format!("{k:?}"))],
            )
            .to_string(),
        ));
    }
    let mut parts: Vec<String> = env
        .iter()
        .map(|(k, v)| format!("{k}={}", shell_words::quote(v)))
        .collect();
    parts.push(shell_words::quote(&program.to_string_lossy()).into_owned());
    parts.extend(args.iter().map(|a| shell_words::quote(a).into_owned()));
    let line = parts.join(" ");
    let script = format!(
        "tell application \"Terminal\" to do script \"{}\"\ntell application \"Terminal\" to activate",
        apple_quoted(&line)
    );
    std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .spawn()
        .map(|_| ())
}
