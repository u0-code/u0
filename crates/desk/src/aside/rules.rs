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
/// window or download a page asks for): a web address, and nothing else,
/// on every system.
pub fn for_browser(address: &str) -> Option<Url> {
    Url::parse(address)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
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
