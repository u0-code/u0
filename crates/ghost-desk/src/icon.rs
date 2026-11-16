//! The app's own icon: the ghost, on the body macOS app icons sit on.
//!
//! A bundle built with `bundle.sh` carries it as `Ghost.icns`, and Finder,
//! Launchpad and the Dock read it from there. Run straight from cargo there
//! is no bundle, so the desk hands `AppKit` the same picture as it starts,
//! and the Dock shows the ghost either way. The picture is
//! `assets/app/icon.png`, rendered from `assets/app/icon.svg`.

/// The 1024-pixel picture, built into the binary.
#[cfg(target_os = "macos")]
const PNG: &[u8] = include_bytes!("../assets/app/icon.png");

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

/// Elsewhere the window manager's own icon stands.
#[cfg(not(target_os = "macos"))]
pub fn set() {}
