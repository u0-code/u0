//! What a page in the Preview tab may reach, each rule stated once: what
//! is shown, where its links go, where a window it asks for opens, and
//! what becomes of a download. A page is a dev server's, serving code an
//! agent may have written, so nothing it asks for reaches macOS's other
//! handlers of addresses (an app, a share, u0's own links).

use url::Url;

/// `address` as a page u0 shows: a web address, or a file of yours.
pub fn shown(address: &str) -> Option<Url> {
    Url::parse(address)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https" | "file"))
}

/// `address`, asked for by a page (a window of its own, a download), as
/// what your browser is given: a web address, and nothing else.
pub fn for_browser(address: &str) -> Option<Url> {
    Url::parse(address)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
}

/// Whether a page opened at `opened` may go to `to`, in its own frame or
/// one inside it: a web address; what the engine makes inside a page
/// (`about:blank`, a `blob:` or `data:` frame, which the engine itself keeps
/// from the top); a file only when it is the one opened.
pub fn may_go(opened: &str, to: &str) -> bool {
    let Ok(to) = Url::parse(to) else {
        return false;
    };
    match to.scheme() {
        "http" | "https" | "blob" | "data" => true,
        "about" => matches!(to.path(), "blank" | "srcdoc"),
        "file" => Url::parse(opened)
            .is_ok_and(|opened| opened.scheme() == "file" && opened.path() == to.path()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_shows_a_web_address_or_a_file_and_nothing_else() {
        for ok in [
            "http://127.0.0.1:5173/",
            "https://example.com/",
            "file:///tmp/a.html",
        ] {
            assert!(shown(ok).is_some(), "{ok}");
        }
        for refused in [
            "u0://new?prompt=hi",
            "smb://server/share",
            "javascript:alert(1)",
            "data:text/html,<p>",
            "vscode://file/etc/passwd",
            "localhost:5173",
        ] {
            assert!(shown(refused).is_none(), "{refused}");
        }
    }

    #[test]
    fn what_a_page_asks_to_open_elsewhere_or_download_goes_to_the_browser_only_if_it_is_web() {
        assert!(for_browser("https://example.com/a.zip").is_some());
        assert!(for_browser("http://127.0.0.1:5173/docs").is_some());
        for refused in [
            "file:///Applications/Calculator.app",
            "smb://server/share",
            "u0://new?prompt=rm",
            "x-apple.systempreferences:",
            "mailto:a@b.c",
        ] {
            assert!(for_browser(refused).is_none(), "{refused}");
        }
    }

    #[test]
    fn a_page_goes_to_web_addresses_and_a_file_page_only_to_itself() {
        let web = "http://127.0.0.1:5173/";
        assert!(may_go(web, "http://127.0.0.1:5173/two"));
        assert!(may_go(web, "https://example.com/"));
        assert!(may_go(web, "about:blank"));
        assert!(may_go(web, "blob:http://127.0.0.1:5173/1f2e"));
        assert!(!may_go(web, "file:///etc/passwd"));
        assert!(!may_go(web, "u0://new"));
        assert!(!may_go(web, "about:preferences"));
        let file = "file:///tmp/site/index.html";
        assert!(may_go(file, "file:///tmp/site/index.html#part"));
        assert!(!may_go(file, "file:///tmp/site/other.html"));
        assert!(may_go(file, "https://example.com/"));
    }
}
