//! What a page you open, and the addresses around it, may reach, each
//! rule stated once: what the Preview tab shows, where a page goes, and
//! what is handed to your browser. A dev server serves code an agent may
//! have written, and a repository's `u0.json` names its `preview`, so
//! nothing of theirs reaches macOS's other handlers of addresses (an app, a
//! script, a share, u0's own links).

#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

use url::Url;

/// `address` as a page the Preview tab shows: a web address; a file of
/// yours only when `yours` (typed in its bar, or opened lately), never
/// a file a repository or a page names.
#[cfg(target_os = "macos")]
pub fn shown(address: &str, yours: bool) -> Option<Url> {
    Url::parse(address).ok().filter(|u| match u.scheme() {
        "http" | "https" => true,
        "file" => yours,
        _ => false,
    })
}

/// `address` as what your browser is given ("Open in your browser", a
/// link, a window or download a page asks for): a web address, and
/// nothing else, on every system.
pub fn for_browser(address: &str) -> Option<Url> {
    Url::parse(address)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
}

/// Give `address` to your browser if it is a web address
/// ([`for_browser`]), and say whether it was: with [`yours`], the only way
/// u0 hands an address to the system's handlers (`App::open_url`).
pub fn to_browser(address: &str, cx: &mut gpui::App) -> bool {
    let Some(url) = for_browser(address) else {
        return false;
    };
    cx.open_url(url.as_str());
    true
}

/// `address` as what your mail app is given: a `mailto:` address, and
/// nothing else.
pub fn for_mail(address: &str) -> Option<Url> {
    Url::parse(address).ok().filter(|u| u.scheme() == "mailto")
}

/// Give `address` to your mail app if it is a `mailto:` address
/// ([`for_mail`]), and say whether it was: [`to_browser`]'s sibling, for a
/// link in a reply.
pub fn to_mail(address: &str, cx: &mut gpui::App) -> bool {
    let Some(url) = for_mail(address) else {
        return false;
    };
    cx.open_url(url.as_str());
    true
}

/// Whether the system's app for `path` only shows it: a web page (a
/// browser), audio or video (a player), known by its name's extension. An
/// executable, a package, a script or anything else the system would run
/// or install is not, whoever wrote it.
pub fn only_shown(path: &std::path::Path) -> bool {
    mime_guess::from_path(path).first().is_some_and(|kind| {
        kind.essence_str() == "text/html" || matches!(kind.type_().as_str(), "audio" | "video")
    })
}

/// Whether `path` is a plain file, what its name says it is: not a link
/// (symbolic, or a Finder alias, which begins `book`) to something else,
/// not a folder or a package, not executable. The system resolves a link
/// before it picks an app, so `clip.mp4 -> tool` would run `tool`.
pub fn plain(path: &std::path::Path) -> bool {
    use std::io::Read as _;
    // What it is, from its entry alone: nothing but a regular file is
    // opened to be read (a FIFO would wait for a writer, and the window
    // with it).
    let Some(about) = std::fs::symlink_metadata(path)
        .ok()
        .filter(|a| a.file_type().is_file())
    else {
        return false;
    };
    #[cfg(unix)]
    let runs = std::os::unix::fs::PermissionsExt::mode(&about.permissions()) & 0o111 != 0;
    #[cfg(not(unix))]
    let runs = false;
    if runs {
        return false;
    }
    let mut head = [0u8; 4];
    let alias = std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && &head == b"book";
    !alias
}

/// Open `path`, a file the viewer shows as a web page or as audio or
/// video ([`only_shown`]) and a plain one ([`plain`]), with the app the
/// system has for its name's kind (on macOS chosen by its extension, so
/// what the file really is cannot choose), by a button that says so: the
/// one other way u0 hands a file to the system. Anything else is refused:
/// an agent may have written the file.
pub fn yours(path: &std::path::Path, cx: &mut gpui::App) {
    if !only_shown(path) || !plain(path) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = cx;
        let Some(app) = path
            .extension()
            .and_then(|e| crate::macos::app_for_extension(&e.to_string_lossy()))
        else {
            return;
        };
        if let Ok(mut opening) = std::process::Command::new("/usr/bin/open")
            .arg("-a")
            .arg(app)
            .arg(path)
            .spawn()
        {
            std::thread::spawn(move || opening.wait());
        }
    }
    #[cfg(not(target_os = "macos"))]
    if let Ok(url) = Url::from_file_path(path) {
        cx.open_url(url.as_str());
    }
}

/// The file a link names (`src/a.rs`, `a.rs:12`, `/abs/a.rs`,
/// `file:///abs/a.rs`), as a path within `folder`, the thread's: none for
/// one outside it, or a link that names no file. Read as written, `..`
/// and all, never by the file system: a link within the folder to a file
/// elsewhere is followed by the viewer's read, which only shows it to you
/// (the folder may be on another machine, whose files the desk cannot
/// resolve).
pub fn within(folder: &std::path::Path, link: &str) -> Option<std::path::PathBuf> {
    use std::path::{Component, PathBuf};
    let path = match Url::parse(link) {
        Ok(url) if url.scheme() == "file" => url.to_file_path().ok()?,
        // A scheme other than a drive letter's is not a file.
        Ok(url) if url.scheme().len() > 1 => return None,
        _ => PathBuf::from(link.split(':').next().unwrap_or(link)),
    };
    let mut whole = PathBuf::new();
    for part in folder.join(path).components() {
        match part {
            Component::ParentDir => {
                whole.pop();
            }
            Component::CurDir => {}
            other => whole.push(other),
        }
    }
    whole
        .strip_prefix(folder)
        .ok()
        .map(std::path::Path::to_path_buf)
}

/// Whether a page opened at `opened` may go to `to`, in its own frame or
/// one inside it: a web address; a page's own frames (`about:blank`,
/// `about:srcdoc`, `blob:`, `data:`), as powerless as it; a file only when
/// it is the one opened (its host and path).
#[cfg(target_os = "macos")]
pub fn may_go(opened: &str, to: &str) -> bool {
    let Ok(to) = Url::parse(to) else {
        return false;
    };
    match to.scheme() {
        "http" | "https" | "blob" | "data" => true,
        "about" => matches!(to.path(), "blank" | "srcdoc"),
        "file" => Url::parse(opened).is_ok_and(|opened| {
            opened.scheme() == "file" && opened.host() == to.host() && opened.path() == to.path()
        }),
        _ => false,
    }
}

/// How long after one a page's next window or download is handed to your
/// browser: a page may ask for them in a loop, with no click of yours.
#[cfg(target_os = "macos")]
pub const HANDED_EVERY: Duration = Duration::from_secs(5);

/// Whether what a page asks for now is handed on, the last having been
/// at `last`: one in [`HANDED_EVERY`], the rest dropped.
#[cfg(target_os = "macos")]
pub fn may_hand(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|last| now.duration_since(last) >= HANDED_EVERY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn a_page_shows_a_web_address_and_a_file_only_of_yours() {
        for ok in ["http://127.0.0.1:5173/", "https://example.com/"] {
            assert!(shown(ok, false).is_some(), "{ok}");
        }
        assert!(shown("file:///tmp/a.html", true).is_some());
        assert!(
            shown("file:///Users/me/proj/x.command", false).is_none(),
            "a file a repository's u0.json names is not shown"
        );
        for refused in [
            "u0://new?prompt=hi",
            "smb://server/share",
            "javascript:alert(1)",
            "data:text/html,<p>",
            "vscode://file/etc/passwd",
            "localhost:5173",
        ] {
            assert!(shown(refused, true).is_none(), "{refused}");
        }
    }

    #[test]
    fn a_file_link_names_a_file_of_the_thread_s_folder_and_nothing_else() {
        use std::path::Path;
        let folder = Path::new("/Users/me/proj");
        assert_eq!(within(folder, "src/a.rs:12"), Some("src/a.rs".into()));
        assert_eq!(within(folder, "./src/../b.rs"), Some("b.rs".into()));
        assert_eq!(
            within(folder, "file:///Users/me/proj/x.html"),
            Some("x.html".into())
        );
        assert_eq!(
            within(folder, "/Users/me/proj/README.md"),
            Some("README.md".into())
        );
        for outside in [
            "../other/x.command",
            "/Applications/Calculator.app",
            "file:///Users/me/Downloads/x.command",
            "x-app://run",
            "smb://server/share",
            "javascript:go()",
        ] {
            assert_eq!(within(folder, outside), None, "{outside}");
        }
    }

    #[test]
    fn a_file_goes_to_the_system_only_to_be_shown_never_run() {
        use std::path::Path;
        for shown in [
            "site/index.html",
            "a.htm",
            "clip.mp4",
            "song.mp3",
            "voice.m4a",
        ] {
            assert!(only_shown(Path::new(shown)), "{shown}");
        }
        for runs in [
            "tool",
            "x.command",
            "setup.pkg",
            "Evil.app",
            "run.sh",
            "a.dmg",
            "b.terminal",
            "notes.txt",
        ] {
            assert!(!only_shown(Path::new(runs)), "{runs}");
        }
    }

    #[test]
    fn a_file_is_handed_on_only_as_what_its_name_says() {
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip.mp4");
        std::fs::write(&clip, b"\0\0\0 ftypisom").unwrap();
        assert!(plain(&clip), "a movie is a movie");
        let tool = dir.path().join("tool");
        std::fs::write(&tool, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(!plain(&tool), "an executable is not");
            let posing = dir.path().join("posing.mp4");
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::os::unix::fs::symlink(&tool, &posing).unwrap();
            assert!(!plain(&posing), "a link named as a movie is not");
            let marked = dir.path().join("marked.mp4");
            std::fs::write(&marked, b"x").unwrap();
            std::fs::set_permissions(&marked, std::fs::Permissions::from_mode(0o744)).unwrap();
            assert!(!plain(&marked), "nor a movie one may run");
        }
        let alias = dir.path().join("page.html");
        std::fs::write(&alias, b"book\0\0\0\0mark\0\0\0\0").unwrap();
        assert!(!plain(&alias), "a Finder alias is not");
        let package = dir.path().join("Setup.mp4");
        std::fs::create_dir(&package).unwrap();
        assert!(!plain(&package), "a folder or package is not");
        assert!(
            !plain(&dir.path().join("gone.mp4")),
            "nor what is not there"
        );
        #[cfg(unix)]
        {
            // Opened, a FIFO would wait for a writer: it is refused unopened.
            let pipe = dir.path().join("pipe.mp4");
            let made = std::process::Command::new("mkfifo").arg(&pipe).status();
            assert!(made.is_ok_and(|m| m.success()), "mkfifo");
            assert!(!plain(&pipe), "a FIFO is not, and is not waited on");
        }
    }

    #[test]
    fn only_a_mailto_address_goes_to_mail() {
        assert!(for_mail("mailto:a@b.c").is_some());
        for refused in [
            "https://example.com",
            "file:///x",
            "smb://s/x",
            "x-mail://a",
        ] {
            assert!(for_mail(refused).is_none(), "{refused}");
        }
    }

    #[test]
    fn an_attachment_is_opened_only_within_the_thread_s_folder() {
        use std::path::Path;
        let folder = Path::new("/Users/me/proj");
        assert_eq!(
            within(folder, "file:///Users/me/proj/shot.png"),
            Some("shot.png".into())
        );
        for elsewhere in [
            "file:///usr/local/bin/tool",
            "file:///Users/me/Downloads/setup.pkg",
            "file:///Users/me/proj/../.ssh/id_rsa",
        ] {
            assert_eq!(within(folder, elsewhere), None, "{elsewhere}");
        }
    }

    #[test]
    fn only_a_web_address_is_handed_to_the_browser() {
        assert!(for_browser("https://example.com/a.zip").is_some());
        assert!(for_browser("http://127.0.0.1:5173/docs").is_some());
        for refused in [
            "file:///Applications/Calculator.app",
            "file:///Users/me/proj/x.command",
            "smb://server/share",
            "u0://new?prompt=rm",
            "x-apple.systempreferences:",
            "mailto:a@b.c",
            "not an address",
        ] {
            assert!(for_browser(refused).is_none(), "{refused}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_page_goes_to_web_addresses_its_own_frames_and_a_file_page_only_to_itself() {
        let web = "http://127.0.0.1:5173/";
        for ok in [
            "http://127.0.0.1:5173/two",
            "https://example.com/",
            "about:blank",
            "about:srcdoc",
            "blob:http://127.0.0.1:5173/1f2e",
            "data:text/html,<p>hi</p>",
        ] {
            assert!(may_go(web, ok), "{ok}");
        }
        for refused in [
            "file:///etc/passwd",
            "u0://new",
            "about:preferences",
            "smb://s/x",
        ] {
            assert!(!may_go(web, refused), "{refused}");
        }
        let file = "file:///tmp/site/index.html";
        assert!(may_go(file, "file:///tmp/site/index.html#part"));
        assert!(!may_go(file, "file:///tmp/site/other.html"));
        assert!(!may_go(file, "file://elsewhere/tmp/site/index.html"));
        assert!(may_go(file, "https://example.com/"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_page_s_windows_and_downloads_go_on_one_in_five_seconds() {
        let now = Instant::now();
        assert!(may_hand(None, now));
        assert!(!may_hand(Some(now), now + Duration::from_secs(1)));
        assert!(may_hand(Some(now), now + HANDED_EVERY));
    }
}
