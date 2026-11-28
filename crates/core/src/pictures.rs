//! The pictures a pull request or an issue shows: found in what was
//! written there, as gpui-kit's view reads it (the `markdown` crate, its
//! own parser), and fetched by the server for the desk to draw. Only what
//! the item's own text shows is fetched, only so many and so large, and
//! only from the public internet (`http::outward`).

use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    proto::{Bytes, Event},
    server::Send as Tell,
};

use crate::github::Detail;

/// The most pictures one item shows.
pub const MOST: usize = 50;

/// The most bytes one picture may have: well under what the link keeps
/// for a desk that comes back (`link::KEPT`, 16 MiB), as it is sent there
/// in base64 (a third more).
const MOST_BYTES: u64 = 8 << 20;

/// The widest and tallest a picture may say it is, read from its first
/// bytes before anything decodes it.
const MOST_SIDE: usize = 8192;

/// The most frames an animated picture may have, and the most pixels its
/// frames may cover together, each drawn whole (512 MiB of them, what
/// `image` lets one picture take).
const MOST_FRAMES: u64 = 300;
const MOST_PIXELS: u64 = 1 << 27;

/// How long a picture fetched is answered from what came to whoever asks
/// for it again: a desk that asks over and over has it fetched once.
const AGAIN: Duration = Duration::from_secs(60);

/// The most bytes of pictures kept to answer again.
const KEPT: usize = 64 << 20;

/// What came for a picture: its bytes, or why not.
type Answer = Result<Bytes, String>;

/// The pictures fetched lately, each answered again from what came; and
/// those being fetched, with who waits for each.
#[derive(Default)]
pub(crate) struct Fetched {
    came: VecDeque<(String, Instant, Answer)>,
    going: HashMap<String, Vec<Tell>>,
}

impl Fetched {
    /// Of `urls`, the first `MOST` that `shown` holds (the pictures the
    /// item was found to show): each that came lately told to `tell` now,
    /// each being fetched told it as it comes, and the rest, to fetch,
    /// returned, `tell` waiting for them too.
    pub(crate) fn ask(&mut self, urls: &[String], shown: &[String], tell: &Tell) -> Vec<String> {
        self.forget();
        let mut fetch = Vec::new();
        for url in urls.iter().filter(|u| shown.contains(u)).take(MOST) {
            if let Some((_, _, answer)) = self.came.iter().find(|(u, ..)| u == url) {
                tell(picture_of(url, answer));
            } else if let Some(waiting) = self.going.get_mut(url) {
                waiting.push(tell.clone());
            } else {
                self.going.insert(url.clone(), vec![tell.clone()]);
                fetch.push(url.clone());
            }
        }
        fetch
    }

    /// What came for `url`: told to each who waits for it, and kept.
    pub(crate) fn came(&mut self, url: String, answer: Result<Vec<u8>, String>) {
        let answer = answer.map(Bytes);
        for tell in self.going.remove(&url).unwrap_or_default() {
            tell(picture_of(&url, &answer));
        }
        self.came.push_back((url, Instant::now(), answer));
        self.forget();
    }

    /// What came longer ago than `AGAIN` forgotten, and the oldest past
    /// `KEPT` bytes.
    fn forget(&mut self) {
        let size =
            |(_, _, answer): &(String, Instant, Answer)| answer.as_ref().map_or(0, |b| b.0.len());
        self.came.retain(|(_, at, _)| at.elapsed() < AGAIN);
        let mut kept: usize = self.came.iter().map(size).sum();
        while kept > KEPT
            && let Some(old) = self.came.pop_front()
        {
            kept -= size(&old);
        }
    }
}

fn picture_of(url: &str, answer: &Answer) -> Event {
    Event::Picture {
        url: url.to_owned(),
        picture: answer.clone(),
    }
}

/// The addresses of the pictures `detail` shows, over HTTPS: in its
/// description, what was said and its conversations on lines, each once,
/// the first `MOST` of them.
pub fn of(detail: &Detail) -> Vec<String> {
    let said = (detail.threads.iter())
        .flat_map(|t| &t.said)
        .chain(&detail.said)
        .map(|s| s.body.as_str());
    let mut urls: Vec<String> = Vec::new();
    for url in std::iter::once(detail.body.as_str())
        .chain(said)
        .flat_map(found)
    {
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls.truncate(MOST);
    urls
}

/// The addresses of the pictures `text` shows over HTTPS, as the view
/// reads it: each image (`![alt](…)`, `![alt][ref]`) and each `<img>`'s
/// `src`, once each.
pub fn found(text: &str) -> Vec<String> {
    use markdown::mdast::Node;
    fn walk(node: &Node, found: &mut Vec<String>, refs: &mut Vec<String>) {
        match node {
            Node::Image(image) => found.push(image.url.clone()),
            Node::ImageReference(image) => refs.push(image.identifier.clone()),
            Node::Html(html) => found.extend(sources(&html.value)),
            _ => {}
        }
        for child in node.children().into_iter().flatten() {
            walk(child, found, refs);
        }
    }
    fn defined(node: &Node, id: &str) -> Option<String> {
        match node {
            Node::Definition(d) if d.identifier == id => Some(d.url.clone()),
            _ => node.children()?.iter().find_map(|c| defined(c, id)),
        }
    }
    let Ok(tree) = markdown::to_mdast(text, &markdown::ParseOptions::gfm()) else {
        return Vec::new();
    };
    let (mut found, mut refs) = (Vec::new(), Vec::new());
    walk(&tree, &mut found, &mut refs);
    found.extend(refs.iter().filter_map(|id| defined(&tree, id)));
    let mut urls: Vec<String> = Vec::new();
    for url in found {
        if url.starts_with("https://") && !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

/// The `src` of each `<img>` in `html`: all of a tag that is read.
fn sources(html: &str) -> impl Iterator<Item = String> + '_ {
    html.match_indices('<').filter_map(|(at, _)| {
        let tag = &html[at + 1..];
        let rest = tag
            .get(..3)
            .filter(|name| name.eq_ignore_ascii_case("img"))
            .map(|_| &tag[3..])?;
        if !rest.starts_with(|c: char| c.is_whitespace() || c == '/') {
            return None;
        }
        attribute(&rest[..rest.find('>').unwrap_or(rest.len())], "src")
    })
}

/// The value of the attribute `name` among a tag's `attrs`, its character
/// references read.
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return None;
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '=' || c == '/')
            .unwrap_or(rest.len());
        let key = &rest[..end];
        rest = rest[end..].trim_start();
        let mut value = "";
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            (value, rest) = match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let inner = &after[1..];
                    let close = inner.find(quote).unwrap_or(inner.len());
                    (&inner[..close], inner.get(close + 1..).unwrap_or(""))
                }
                _ => after.split_at(after.find(char::is_whitespace).unwrap_or(after.len())),
            };
        }
        if key.eq_ignore_ascii_case(name) {
            return Some(unescape(value));
        }
    }
}

/// `value` with its character references read: `&amp;`, `&quot;`,
/// `&apos;`, `&lt;`, `&gt;` and numbered ones (`&#38;`, `&#x26;`); any
/// other `&` is kept as it is.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let read = rest.find(';').and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => '&',
                "quot" => '"',
                "apos" => '\'',
                "lt" => '<',
                "gt" => '>',
                _ => {
                    let n = name.strip_prefix('#')?;
                    let n = match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => n.parse().ok()?,
                    };
                    char::from_u32(n)?
                }
            };
            Some((c, end + 1))
        });
        let (c, len) = read.unwrap_or(('&', 1));
        out.push(c);
        rest = &rest[len..];
    }
    out + rest
}

/// Fetch each of `urls` (what `Fetched::ask` gave, for a project in
/// `dir`), one after another, handing `got` each as it comes, or why not.
/// Your GitHub token (the one you gave, else gh's, asked once) goes only
/// to a GitHub attachment.
pub(crate) fn fetch(
    dir: &Path,
    urls: &[String],
    mut got: impl FnMut(String, Result<Vec<u8>, String>),
) {
    let mut token = None;
    for url in urls {
        let Ok(parsed) = url::Url::parse(url) else {
            got(url.clone(), Err("it is not an address".into()));
            continue;
        };
        let auth = attachment(&parsed)
            .then(|| {
                token
                    .get_or_insert_with(|| {
                        crate::forge::Host::GitHub
                            .token()
                            .or_else(|| crate::git::yours(dir, "gh", &["auth", "token"]).ok())
                    })
                    .as_ref()
                    .map(|t| format!("Bearer {t}"))
            })
            .flatten();
        got(url.clone(), picture(&parsed, auth.as_deref()));
    }
}

/// The picture at `url`, sent `auth`: over HTTPS from the public internet
/// alone, no larger than `MOST_BYTES` nor wider or taller than
/// `MOST_SIDE`, and a picture the desk draws.
fn picture(url: &url::Url, auth: Option<&str>) -> Result<Vec<u8>, String> {
    if url.scheme() != "https" {
        return Err("it is not over HTTPS".into());
    }
    let headers: Vec<_> = auth.iter().map(|a| ("Authorization", *a)).collect();
    let bytes = crate::http::outward(url.as_str(), &headers, MOST_BYTES)?;
    checked(bytes)
}

/// `bytes`, where they are a picture the desk draws, of a size it may.
fn checked(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    use crate::session::Format;
    let Some(format) = Format::sniff(&bytes) else {
        return Err("it is not a picture".into());
    };
    let size = imagesize::blob_size(&bytes).map_err(|_| "it is not a picture".to_owned())?;
    if size.width > MOST_SIDE || size.height > MOST_SIDE {
        return Err("it is too large".into());
    }
    if format == Format::Gif && !few_frames(&bytes) {
        return Err("it has too many frames".into());
    }
    Ok(bytes)
}

/// Whether the GIF `bytes` has no more than `MOST_FRAMES`, covering no
/// more than `MOST_PIXELS` together: counted from each frame's
/// description, none decoded.
fn few_frames(bytes: &[u8]) -> bool {
    let mut options = gif::DecodeOptions::new();
    options.skip_frame_decoding(true);
    let Ok(mut gif) = options.read_info(bytes) else {
        return false;
    };
    let canvas = u64::from(gif.width()) * u64::from(gif.height());
    let mut frames = 0;
    loop {
        match gif.next_frame_info() {
            Ok(None) => return true,
            Ok(Some(_)) => frames += 1,
            Err(_) => return false,
        }
        if frames > MOST_FRAMES || frames * canvas > MOST_PIXELS {
            return false;
        }
    }
}

/// Whether `url` is a file attached to what was written on GitHub
/// (`https://github.com/user-attachments/assets/<id>`, or a repository's
/// `https://github.com/<owner>/<repo>/assets/<user>/<id>`): the one
/// address your GitHub token is sent to for a picture. It is the address
/// as parsed that is checked and then sent.
fn attachment(url: &url::Url) -> bool {
    let path: Vec<_> = url
        .path_segments()
        .map(Iterator::collect)
        .unwrap_or_default();
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && match path.as_slice() {
            ["user-attachments", "assets", id] => !id.is_empty(),
            [owner, repo, "assets", user, id] => {
                [owner, repo, user, id].iter().all(|s| !s.is_empty())
            }
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    #[test]
    fn the_pictures_a_body_shows_are_its_images_and_img_tags_over_https() {
        let body = concat!(
            "### What happened\n\n",
            "<img width=\"1973\" height=\"1162\" alt=\"image\" ",
            "src=\"https://github.com/user-attachments/assets/aad7\" />\n\n",
            "![shot](https://example.com/a.png) and ![again](https://example.com/a.png)\n\n",
            "<p align=center><IMG SRC='https://example.com/b.png?x=1&amp;y=2'></p>\n\n",
            "![ref][r]\n\n[r]: https://example.com/c.png\n\n",
            "![plain](http://example.com/d.png) ![here](a.png) ",
            "![data](data:image/png;base64,AAAA) <imgx src=\"https://e.com/x\">\n\n",
            "`![code](https://example.com/e.png)`\n\n",
            "```\n<img src=\"https://example.com/f.png\">\n```\n",
        );
        assert_eq!(
            found(body),
            [
                "https://github.com/user-attachments/assets/aad7",
                "https://example.com/a.png",
                "https://example.com/b.png?x=1&y=2",
                "https://example.com/c.png",
            ]
        );
    }

    #[test]
    fn an_img_tag_is_read_for_its_src_alone_its_references_read() {
        assert_eq!(
            attribute(r#" alt="a src=x" data-src="no" src = "yes" "#, "src").as_deref(),
            Some("yes")
        );
        assert_eq!(attribute(" alt=a/", "src"), None);
        assert_eq!(
            attribute(" src=https://e.com/a.png width=3", "src").as_deref(),
            Some("https://e.com/a.png")
        );
        assert_eq!(unescape("a&amp;b&#38;c&#x26;d&lt;&q;&"), "a&b&c&d<&q;&");
    }

    #[test]
    fn an_item_shows_each_picture_once_and_no_more_than_the_most() {
        let body = (0..MOST + 10).fold(String::new(), |mut body, n| {
            let _ = write!(body, "![{n}](https://example.com/{n}.png)\n\n");
            body
        });
        let detail = Detail {
            body: body.clone(),
            said: vec![crate::github::Said {
                body,
                ..Default::default()
            }],
            ..Default::default()
        };
        let urls = of(&detail);
        assert_eq!(urls.len(), MOST);
        assert_eq!(urls[0], "https://example.com/0.png");
    }

    /// The pictures a `Tell` was told, kept to read back.
    type Heard = std::sync::Arc<std::sync::Mutex<Vec<(String, Answer)>>>;
    fn told() -> (Tell, Heard) {
        let heard = Heard::default();
        let into = std::sync::Arc::clone(&heard);
        let tell: Tell = std::sync::Arc::new(move |e| {
            if let Event::Picture { url, picture } = e {
                into.lock().unwrap().push((url, picture));
            }
        });
        (tell, heard)
    }

    #[test]
    fn only_what_the_item_shows_is_fetched() {
        let (tell, _) = told();
        let shown = ["http://example.com/shown.png".to_owned()];
        let urls = [shown[0].clone(), "https://10.0.0.1/x".to_owned()];
        let fetch = Fetched::default().ask(&urls, &shown, &tell);
        assert_eq!(fetch, shown);
        let mut asked = Vec::new();
        super::fetch(&std::env::temp_dir(), &fetch, |url, picture| {
            asked.push((url, picture));
        });
        assert_eq!(
            asked,
            [(urls[0].clone(), Err("it is not over HTTPS".to_owned()))]
        );
    }

    #[test]
    fn a_picture_asked_for_again_is_answered_from_what_came_until_a_minute_is_out() {
        let url = "https://example.com/a.png".to_owned();
        let shown = [url.clone()];
        let mut fetched = Fetched::default();
        let (first, heard_first) = told();
        let (second, heard_second) = told();
        // Asked twice as it is fetched: fetched once, both told as it comes.
        assert_eq!(fetched.ask(&shown, &shown, &first), shown);
        assert!(fetched.ask(&shown, &shown, &second).is_empty());
        fetched.came(url.clone(), Ok(b"png".to_vec()));
        let picture = (url.clone(), Ok(Bytes(b"png".to_vec())));
        assert_eq!(*heard_first.lock().unwrap(), std::slice::from_ref(&picture));
        assert_eq!(
            *heard_second.lock().unwrap(),
            std::slice::from_ref(&picture)
        );
        // Asked again within the minute: answered at once, nothing fetched.
        assert!(fetched.ask(&shown, &shown, &first).is_empty());
        assert_eq!(*heard_first.lock().unwrap(), [picture.clone(), picture]);
        // A minute on, it is fetched again.
        fetched.came[0].1 -= AGAIN;
        assert_eq!(fetched.ask(&shown, &shown, &first), shown);
    }

    #[test]
    fn a_picture_on_this_machine_or_its_network_is_never_reached() {
        for near in [
            "https://127.0.0.1/a.png",
            "https://[::1]:8443/a.png",
            "https://169.254.169.254/latest",
            "https://10.0.0.1/a.png",
        ] {
            let url = url::Url::parse(near).unwrap();
            assert_eq!(
                picture(&url, None),
                Err("it could not be read".into()),
                "{near}"
            );
        }
    }

    #[test]
    fn only_a_picture_of_a_size_the_desk_may_draw_comes_back() {
        let png = |w: u32, h: u32| {
            let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
            b.extend(w.to_be_bytes());
            b.extend(h.to_be_bytes());
            b.extend(b"\x08\x06\0\0\0");
            b
        };
        assert!(checked(png(1973, 1162)).is_ok());
        assert_eq!(checked(png(8193, 10)), Err("it is too large".into()));
        assert_eq!(checked(png(10, 100_000)), Err("it is too large".into()));
        assert_eq!(
            checked(b"<svg/>".to_vec()),
            Err("it is not a picture".into())
        );
    }

    #[test]
    fn an_animated_picture_of_too_many_frames_or_pixels_is_refused() {
        let gif = |side: u16, frames: usize| {
            let mut b = b"GIF89a".to_vec();
            b.extend(side.to_le_bytes());
            b.extend(side.to_le_bytes());
            b.extend([0x80, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff]);
            for _ in 0..frames {
                b.push(0x2c);
                b.extend([0, 0, 0, 0]);
                b.extend(side.to_le_bytes());
                b.extend(side.to_le_bytes());
                b.extend([0, 2, 2, 0x44, 0x01, 0]);
            }
            b.push(0x3b);
            b
        };
        assert!(checked(gif(10, 1)).is_ok());
        assert!(checked(gif(10, 300)).is_ok());
        assert_eq!(checked(gif(10, 301)), Err("it has too many frames".into()));
        assert!(checked(gif(4096, 8)).is_ok());
        assert_eq!(checked(gif(4096, 9)), Err("it has too many frames".into()));
    }

    #[test]
    fn a_token_goes_only_to_a_github_attachment() {
        for sent in [
            "https://github.com/user-attachments/assets/aad7d8b6-9546-4c02-8ec1-5645b4e1c29e",
            "https://github.com/u0-code/u0/assets/252857002/aad7d8b6",
        ] {
            assert!(attachment(&url::Url::parse(sent).unwrap()), "{sent}");
        }
        for kept in [
            "http://github.com/user-attachments/assets/a",
            "https://github.com.evil.example/user-attachments/assets/a",
            "https://evil.example/?https://github.com/user-attachments/assets/a",
            "https://user@github.com/user-attachments/assets/a",
            "https://github.com:8443/user-attachments/assets/a",
            "https://raw.githubusercontent.com/o/r/main/a.png",
            "https://github.com/o/r/blob/main/a.png",
            "https://github.com/o/r/assets/1/a/more",
            "https://github.com/user-attachments/files/1/a.zip",
            "https://github.com/settings/tokens",
            "https://github.com/user-attachments/assets/a?next=1",
        ] {
            assert!(!attachment(&url::Url::parse(kept).unwrap()), "{kept}");
        }
        // What is checked is the address as parsed, and that is what is
        // sent: its dot segments read away.
        let dotted = url::Url::parse("https://github.com/settings/../user-attachments/assets/a");
        assert_eq!(
            dotted.unwrap().as_str(),
            "https://github.com/user-attachments/assets/a"
        );
    }
}
