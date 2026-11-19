//! What she did on a platform, and what she did inside herself, as the
//! thread reads it from her journal.
//!
//! In the console, where you talk with her, what she did out there shows
//! as steps: a box for what she said in a group or to someone, a row for
//! a reaction, a box with a thumbnail for an image she sent, the card of a
//! post. Her journal does not say why she spoke somewhere, so the console
//! shows all of it, in order of time, among what passed between you.
//!
//! In any place, what she did inside while she looked at it (kept, let go
//! of, felt, meant to, settled, put off, went quiet) shows as quiet rows,
//! each with a dot in her journal's colour for its kind; an image she sent
//! there is drawn as itself; and where the platform would not take what
//! she sent, a calm line says so, with the reason it gave.
//!
//! Everything is read from entries as the journal has them. The one thing
//! asked of her daemon is an image itself, by the name the journal gives
//! it: see `media.rs`.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use gpui::{
    AnyElement, Context, Div, Hsla, ObjectFit, SharedString, StyledImage, div, img, prelude::*, px,
};
use serde_json::Value;

use super::{
    View, calls, from_desk, s,
    style::{BOX_RADIUS, CARD_RADIUS, META, PROSE, PROSE_LINE, ROW, S2, S3, S4, meta},
};
use crate::{
    desk::Desk,
    fonts::Faces,
    media::{self, Shown, Source},
    model::Entry,
    theme, ui,
};

/// One thing she did, as a row or a box in a thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Did {
    /// It was done in the place the thread shows, and so is part of her
    /// answer there; otherwise it was done out there and the thread is a
    /// console's.
    pub here: bool,
    pub what: What,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum What {
    /// Something she said somewhere else, by its place's full name; and
    /// what she sent with it, if anything.
    Said {
        place: String,
        text: String,
        sent: Option<Sent>,
    },
    /// A reaction, and to whom when her journal has their line.
    Reacted {
        place: String,
        emoji: String,
        to: Option<String>,
    },
    /// A post, to no one in particular.
    Posted {
        platform: String,
        text: String,
        counts: Option<Counts>,
        /// Where the post can be read, when the platform's address for it
        /// is known.
        link: Option<String>,
    },
    /// An image she sent in this place: its name in `media/`, and where.
    Image { file: String, place: String },
    /// Something she did inside: kept, felt, meant to.
    Inner {
        kind: String,
        verb: &'static str,
        what: String,
    },
    /// The platform would not take what she sent: where, and why it said.
    Refused { place: String, reason: String },
}

/// What went with a line of hers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Sent {
    /// An image, by its name in `media/`.
    Image(String),
    /// A file she made, by its name: it was read off her machine, and
    /// nothing of it was kept.
    File(String),
}

/// How a post is doing, as the platform last counted it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Counts {
    pub views: u64,
    pub likes: u64,
    pub replies: u64,
    pub reposts: u64,
}

impl Counts {
    /// "212 views · 31 likes": likes always, the rest when there are any.
    pub fn line(self) -> String {
        let mut parts = Vec::new();
        let mut part = |n: u64, one: &str, many: &str, always: bool| {
            if n > 0 || always {
                parts.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        };
        part(self.views, "view", "views", false);
        part(self.likes, "like", "likes", true);
        part(self.replies, "reply", "replies", false);
        part(self.reposts, "repost", "reposts", false);
        parts.join(" · ")
    }
}

impl Did {
    /// Whether it is her answer in the place shown: something done there
    /// ends what you said there waiting on her, and something done out
    /// there does not.
    pub fn answers(&self) -> bool {
        self.here
            && !matches!(&self.what, What::Inner { kind, .. } if kind == "woke" || kind == "slept")
    }

    /// Whether it is drawn as one quiet row, which stacks close under
    /// another, rather than as a box.
    pub fn is_row(&self) -> bool {
        matches!(
            self.what,
            What::Inner { .. } | What::Reacted { .. } | What::Refused { .. }
        )
    }
}

/// What a platform is called, written in full.
fn platform(channel: &str) -> String {
    match channel {
        "qq" => "QQ".to_owned(),
        "x" => "X".to_owned(),
        "telegram" => "Telegram".to_owned(),
        "discord" => "Discord".to_owned(),
        "term" => "Terminal".to_owned(),
        "console" => "Console".to_owned(),
        other => other.to_owned(),
    }
}

/// What her journal says of a place: what kind it is, and its name or,
/// for a private one, who it is with.
#[derive(Debug, Clone, Default)]
struct Spot {
    kind: String,
    name: Option<String>,
}

/// What the journal says of places and people, to name them; and, for
/// her inner entries, which place each was written in.
pub(super) struct Known<'a> {
    spots: HashMap<String, Spot>,
    /// Who sent a message, by its place's key and the channel's id for it.
    lines: HashMap<(String, &'a str), &'a str>,
    /// A person's name, by their key: `channel:id`.
    people: HashMap<String, &'a str>,
    /// The place each inner entry was written in, by its seq.
    homes: HashMap<i64, String>,
    /// How each post is doing, by channel and id: the latest count.
    counts: HashMap<(&'a str, &'a str), Counts>,
    /// What she kept, by the seq of its `kept` entry.
    kept: HashMap<i64, &'a Entry>,
}

/// The kinds of entry drawn as quiet rows.
const INNER: [&str; 8] = [
    "kept", "let_go", "felt", "intended", "canon", "quieted", "woke", "slept",
];

/// What a look's decision writes, all at once, before the `decided` entry
/// that ends it: one actor writes her journal, so nothing else comes
/// between them. An inner entry in such a run was written in the place the
/// `decided` is about.
const DECIDING: [&str; 11] = [
    "read",
    "reacted",
    "noted",
    "undertook",
    "left",
    "felt",
    "intended",
    "kept",
    "let_go",
    "canon",
    "quieted",
];

/// A place key: `channel:id`, not a link and not a work's id.
fn is_place(key: &str) -> bool {
    key.contains(':') && !key.contains("://")
}

impl<'a> Known<'a> {
    pub fn of(journal: &'a [Entry]) -> Self {
        let mut known = Known {
            spots: HashMap::new(),
            lines: HashMap::new(),
            people: HashMap::new(),
            homes: HashMap::new(),
            counts: HashMap::new(),
            kept: HashMap::new(),
        };
        let mut pending: Vec<i64> = Vec::new();
        for e in journal {
            let d = &e.data;
            match e.kind.as_str() {
                "heard" => {
                    let channel = s(d, "channel");
                    let (Some(place), Some(from)) = (d.get("place"), d.get("from")) else {
                        continue;
                    };
                    let key = format!("{channel}:{}", s(place, "id"));
                    let who = s(from, "name");
                    let spot = known.spots.entry(key.clone()).or_default();
                    s(place, "kind").clone_into(&mut spot.kind);
                    let name = place.get("name").and_then(Value::as_str);
                    match name {
                        Some(name) => spot.name = Some(name.to_owned()),
                        None if spot.kind == "direct" && !who.is_empty() => {
                            spot.name = Some(who.to_owned());
                        }
                        None => {}
                    }
                    if !who.is_empty() {
                        known
                            .people
                            .insert(format!("{channel}:{}", s(from, "id")), who);
                        known.lines.insert((key, s(d, "id")), who);
                    }
                }
                "measured" => {
                    let channel = s(d, "channel");
                    let posts = d.get("posts").and_then(Value::as_array);
                    for p in posts.into_iter().flatten() {
                        let n = |k| p.get(k).and_then(Value::as_u64).unwrap_or_default();
                        let counts = Counts {
                            views: n("views"),
                            likes: n("likes"),
                            replies: n("replies"),
                            reposts: n("reposts"),
                        };
                        known.counts.insert((channel, s(p, "id")), counts);
                    }
                }
                "kept" => {
                    known.kept.insert(e.seq, e);
                }
                _ => {}
            }
            // Which place each inner entry belongs to.
            match e.kind.as_str() {
                "decided" => {
                    let about = s(d, "about");
                    for seq in pending.drain(..) {
                        known.homes.insert(seq, about.to_owned());
                    }
                }
                // Something kept in a piece of work is not a look's.
                "kept" if !is_place(s(d, "from")) => pending.clear(),
                kind if DECIDING.contains(&kind) => {
                    if INNER.contains(&kind) {
                        pending.push(e.seq);
                    }
                }
                _ => pending.clear(),
            }
        }
        known
    }

    /// A place by its full name: "QQ 群 · 大荒兽医站", "QQ 私聊 · 阿鹿",
    /// "Telegram group · …"; its id where her journal does not name it.
    pub fn place(&self, channel: &str, id: &str) -> String {
        let spot = self.spots.get(&format!("{channel}:{id}"));
        let name = spot
            .and_then(|s| s.name.clone())
            .unwrap_or_else(|| id.to_owned());
        let kind = spot.map_or("", |s| s.kind.as_str());
        let word = match (channel, kind) {
            ("qq", "group") => " 群",
            ("qq", "direct") => " 私聊",
            (_, "group") => " group",
            (_, "direct") => " DM",
            _ => "",
        };
        format!("{}{word} · {name}", platform(channel))
    }

    /// A place by its key, `channel:id`.
    fn place_of(&self, key: &str) -> String {
        match key.split_once(':') {
            Some((channel, id)) => self.place(channel, id),
            None => key.to_owned(),
        }
    }

    /// The place an inner entry belongs to, by its key: where she was
    /// looking when she wrote it, or else the place it names itself.
    fn home(&self, e: &Entry) -> Option<String> {
        if let Some(home) = self.homes.get(&e.seq) {
            return Some(home.clone());
        }
        let d = &e.data;
        let own = match e.kind.as_str() {
            "intended" | "quieted" => s(d, "place"),
            "kept" => s(d, "from"),
            "canon" => s(d, "source").strip_prefix("said in ").unwrap_or_default(),
            _ => "",
        };
        is_place(own).then(|| own.to_owned())
    }
}

/// Her words with what she handed over taken off their front: the journal
/// writes `[image: …]` or `[file: name]` before the words of a line that
/// carried something.
pub(super) fn words(text: &str) -> &str {
    let t = text.trim_start();
    if (t.starts_with("[image") || t.starts_with("[file:"))
        && let Some(end) = t.find(']')
    {
        return t[end + 1..].trim();
    }
    t
}

/// The name of a file she made, from the `[file: name]` her line begins
/// with.
fn made(text: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix("[file:")?;
    let name = rest[..rest.find(']')?].trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// What the entry `e` shows as in the thread `thread` (a thread key), if
/// anything.
pub(super) fn did(e: &Entry, thread: &str, known: &Known<'_>) -> Option<Did> {
    let console = thread.starts_with("console:");
    let d = &e.data;
    let here_or_out = |channel: &str, id: &str| -> Option<bool> {
        let here = format!("{channel}:{id}") == thread;
        (here || (console && !from_desk(channel))).then_some(here)
    };
    let what = match e.kind.as_str() {
        "said" => {
            let (channel, id) = (s(d, "channel"), s(d, "place"));
            let here = here_or_out(channel, id)?;
            let file = d.get("file").and_then(Value::as_str).map(str::to_owned);
            let place = known.place(channel, id);
            if here {
                // Her words here are a line of hers in the thread already;
                // only the image she sent with them is drawn here.
                return Some(Did {
                    here,
                    what: What::Image { file: file?, place },
                });
            }
            let text = s(d, "text");
            What::Said {
                place,
                text: words(text).to_owned(),
                sent: file.map(Sent::Image).or_else(|| made(text).map(Sent::File)),
            }
        }
        "reacted" => {
            let (channel, id) = (s(d, "channel"), s(d, "place"));
            let here = here_or_out(channel, id)?;
            let key = format!("{channel}:{id}");
            return Some(Did {
                here,
                what: What::Reacted {
                    place: known.place(channel, id),
                    emoji: s(d, "emoji").to_owned(),
                    to: known.lines.get(&(key, s(d, "to"))).map(|n| (*n).to_owned()),
                },
            });
        }
        "posted" if console => {
            let channel = s(d, "channel");
            let id = d.get("id").and_then(Value::as_str);
            What::Posted {
                platform: platform(channel),
                text: s(d, "text").to_owned(),
                counts: id.and_then(|id| known.counts.get(&(channel, id)).copied()),
                link: id
                    .filter(|_| channel == "x")
                    .map(|id| format!("x.com/i/status/{id}")),
            }
        }
        "decided" if s(d, "decision") == "could not send" => {
            let about = s(d, "about");
            let (channel, id) = about.split_once(':')?;
            let here = here_or_out(channel, id)?;
            return Some(Did {
                here,
                what: What::Refused {
                    place: known.place_of(about),
                    reason: calls::one_line(s(d, "reason"), 200),
                },
            });
        }
        "woke" | "slept" if console => inner(e, known)?,
        kind if INNER.contains(&kind) => {
            let home = known.home(e)?;
            if home != thread {
                return None;
            }
            return Some(Did {
                here: true,
                what: inner(e, known)?,
            });
        }
        _ => return None,
    };
    Some(Did { here: false, what })
}

/// An inner entry as a quiet row says it: a verb, and what it was about.
fn inner(e: &Entry, known: &Known<'_>) -> Option<What> {
    let d = &e.data;
    let (verb, what) = match e.kind.as_str() {
        "kept" => ("Kept", s(d, "file").to_owned()),
        "let_go" => {
            let kept = d.get("kept").and_then(Value::as_i64);
            let thing = kept.and_then(|k| known.kept.get(&k)).map(|k| {
                let note = s(&k.data, "note");
                if note.is_empty() {
                    s(&k.data, "file")
                } else {
                    note
                }
                .to_owned()
            });
            ("Let go of", thing.unwrap_or_default())
        }
        "felt" => {
            let delta = d.get("delta").and_then(Value::as_i64).unwrap_or_default();
            let toward = s(d, "toward");
            let name = known.people.get(toward).map_or_else(
                || toward.split_once(':').map_or(toward, |(_, id)| id),
                |n| *n,
            );
            let how = if delta < 0 { "cooler" } else { "warmer" };
            ("Felt", format!("{how} toward {name}"))
        }
        "intended" => {
            let what = s(d, "what");
            // Putting a conversation off is an intention to come back to it.
            match what.strip_prefix("get back to this conversation:") {
                Some(why) => ("Put it off", why.trim().to_owned()),
                None => ("Meant to", what.to_owned()),
            }
        }
        "canon" => ("Settled", s(d, "fact").to_owned()),
        "quieted" => ("Went quiet", String::new()),
        "slept" => ("Slept", String::new()),
        "woke" => ("Woke", String::new()),
        _ => return None,
    };
    Some(What::Inner {
        kind: e.kind.clone(),
        verb,
        what,
    })
}

/// The colour of a kind of journal entry, the same on every page. Only the
/// dot beside a row has it; the words keep theirs.
pub(super) fn colour(kind: &str) -> Hsla {
    match kind {
        "kept" | "let_go" => theme::amber2(),
        "directed" => theme::purple(),
        "canon" | "described" => theme::coral(),
        "woke" | "slept" | "quieted" => theme::green(),
        "undertook" | "worked" | "reported" | "asked" | "added" | "concluded" => theme::blue(),
        _ => theme::muted(),
    }
}

/// When, as the rows of a thread say it: the time today, the day and time
/// before.
fn stamp(at: DateTime<Utc>, tz: Tz) -> String {
    let local = at.with_timezone(&tz);
    if local.date_naive() == Utc::now().with_timezone(&tz).date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%-d %b %H:%M").to_string()
    }
}

/// The widest an image she sent is drawn, and the tallest.
const IMAGE_WIDE: f32 = 320.;
const IMAGE_TALL: f32 = 360.;
/// The side of the thumbnail in a box saying she sent an image elsewhere.
const THUMB: f32 = 160.;
/// The height of the place where an image would be, when it cannot be.
const GONE_TALL: f32 = 96.;

/// The size to draw an image of `w` by `h` pixels at, within `wide` by
/// `tall`: its own shape, never made larger than it is.
pub(super) fn fitted((w, h): (u32, u32), wide: f32, tall: f32) -> (f32, f32) {
    if w == 0 || h == 0 {
        return (wide, tall);
    }
    let (w, h) = (w as f32, h as f32);
    let scale = (wide / w).min(tall / h).min(1.);
    ((w * scale).round().max(1.), (h * scale).round().max(1.))
}

/// The size of an image in pixels, read from the head of its file: PNG,
/// GIF, WebP and JPEG, which are what platforms send.
pub(super) fn dimensions(file: &mut (impl Read + Seek)) -> Option<(u32, u32)> {
    let mut head = [0u8; 30];
    let n = file.read(&mut head).ok()?;
    let head = &head[..n];
    let be16 = |b: &[u8], i: usize| u32::from(u16::from_be_bytes([b[i], b[i + 1]]));
    let le16 = |b: &[u8], i: usize| u32::from(u16::from_le_bytes([b[i], b[i + 1]]));
    let le24 =
        |b: &[u8], i: usize| u32::from(b[i]) | u32::from(b[i + 1]) << 8 | u32::from(b[i + 2]) << 16;
    if head.starts_with(b"\x89PNG\r\n\x1a\n") && n >= 24 {
        let be32 = |i: usize| u32::from_be_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
        return Some((be32(16), be32(20)));
    }
    if head.starts_with(b"GIF8") && n >= 10 {
        return Some((le16(head, 6), le16(head, 8)));
    }
    if head.starts_with(b"RIFF") && n >= 30 && &head[8..12] == b"WEBP" {
        return match &head[12..16] {
            b"VP8 " => Some((le16(head, 26) & 0x3fff, le16(head, 28) & 0x3fff)),
            b"VP8L" => {
                let b = &head[21..25];
                let w = 1 + (u32::from(b[1] & 0x3f) << 8 | u32::from(b[0]));
                let h = 1
                    + (u32::from(b[3] & 0x0f) << 10
                        | u32::from(b[2]) << 2
                        | u32::from(b[1] & 0xc0) >> 6);
                Some((w, h))
            }
            b"VP8X" => Some((1 + le24(head, 24), 1 + le24(head, 27))),
            _ => None,
        };
    }
    if head.starts_with(&[0xff, 0xd8]) {
        // A JPEG says its size in its frame header, after whatever
        // segments come first: walk them by their lengths.
        file.seek(SeekFrom::Start(2)).ok()?;
        let mut byte = [0u8; 1];
        for _ in 0..512 {
            file.read_exact(&mut byte).ok()?;
            if byte[0] != 0xff {
                return None;
            }
            let mut marker = byte[0];
            while marker == 0xff {
                file.read_exact(&mut byte).ok()?;
                marker = byte[0];
            }
            if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
                continue;
            }
            let mut len = [0u8; 2];
            file.read_exact(&mut len).ok()?;
            let len = be16(&len, 0);
            let frame = (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
            if frame {
                let mut sof = [0u8; 5];
                file.read_exact(&mut sof).ok()?;
                return Some((be16(&sof, 3), be16(&sof, 1)));
            }
            file.seek(SeekFrom::Current(i64::from(len) - 2)).ok()?;
        }
    }
    None
}

/// The size of the image at `path`, read once: the desk draws the thread
/// several times a second while something goes on.
fn size_of(path: &Path) -> Option<(u32, u32)> {
    type Sizes = HashMap<PathBuf, Option<(u32, u32)>>;
    static SIZES: LazyLock<Mutex<Sizes>> = LazyLock::new(Mutex::default);
    let mut sizes = SIZES.lock().ok()?;
    if let Some(size) = sizes.get(path) {
        return *size;
    }
    let size = File::open(path).ok().and_then(|mut f| dimensions(&mut f));
    sizes.insert(path.to_owned(), size);
    size
}

/// An image she sent, at most `wide` by `tall` in its own shape, in a
/// hairline of `edge`, as her core hands it out: while it is on its way, a
/// quiet frame of the width it will have; where it cannot be shown, a
/// dashed one, saying why. A click opens the copy kept here.
fn picture(
    faces: &Faces,
    id: String,
    file: &str,
    media: Result<&Source, &str>,
    (wide, tall): (f32, f32),
    edge: Hsla,
    cx: &mut Context<Desk>,
) -> AnyElement {
    let gone = |why: String| {
        div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(S2))
            .w(px(wide))
            .h(px(GONE_TALL.min(tall)))
            .px(px(S3))
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_dashed()
            .border_color(theme::tag_line())
            .child(
                ui::icon("image-off", 16.)
                    .flex_none()
                    .text_color(theme::faint()),
            )
            .child(
                ui::text(faces, why)
                    .text_size(px(SMALL_PRINT))
                    .line_height(px(17.))
                    .text_color(theme::faint())
                    .text_center(),
            )
            .into_any_element()
    };
    let path = match media.map(|source| media::shown(source, file)) {
        Ok(Shown::Here(path)) => path,
        Ok(Shown::Loading) => {
            return div()
                .w(px(wide))
                .h(px(GONE_TALL.min(tall)))
                .rounded(px(CARD_RADIUS))
                .border_1()
                .border_color(edge)
                .into_any_element();
        }
        Ok(Shown::Failed(why)) => return gone(why),
        Err(why) => return gone(why.to_owned()),
    };
    let (w, h) = size_of(&path).map_or((wide, tall.min(wide)), |size| fitted(size, wide, tall));
    let unread = format!("{file} could not be read as an image");
    let open = path.clone();
    div()
        .id(SharedString::from(id))
        .flex_none()
        .cursor_pointer()
        .on_click(cx.listener(move |_, _, _, cx| cx.open_with_system(&open)))
        .child(
            img(path)
                .w(px(w))
                .h(px(h))
                .object_fit(ObjectFit::Contain)
                .rounded(px(CARD_RADIUS))
                .border_1()
                .border_color(edge)
                .with_fallback(move || {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(SMALL_PRINT))
                        .text_color(theme::faint())
                        .child(unread.clone())
                        .into_any_element()
                }),
        )
        .into_any_element()
}

/// The size of what is written in a placeholder.
const SMALL_PRINT: f32 = 13.;

/// About how many points a column of the interface's face takes at 14,
/// a Chinese character counting two.
const POINTS_PER_COL: f32 = 7.;

/// Draw what she did, `seq` being its entry's, at `at`.
pub(super) fn render(
    faces: &Faces,
    did: &Did,
    seq: i64,
    at: DateTime<Utc>,
    view: &View<'_>,
    cx: &mut Context<Desk>,
) -> AnyElement {
    let when = stamp(at, view.tz);
    let media = view.media.as_ref().map_err(|why| *why);
    // Room for words in a label or a row: the column less its insets, the
    // verb before them and the time after.
    let cols = |less: f32| ((view.column - less).max(60.) / POINTS_PER_COL) as usize;
    match &did.what {
        What::Said { place, text, sent } => {
            let mut head = label();
            head = match sent {
                Some(Sent::Image(file) | Sent::File(file)) => head
                    .child(muted(faces, "Sent"))
                    .child(white(faces, calls::fit(file, cols(420.).max(12))))
                    .child(muted(faces, "to"))
                    .child(white(faces, calls::fit(place, cols(360.).max(12)))),
                None => head
                    .child(muted(faces, "Said in"))
                    .child(white(faces, calls::fit(place, cols(160.)))),
            };
            let mut b = boxed().child(head.child(right(faces, when)));
            if let Some(Sent::Image(file)) = sent {
                b = b.child(div().px(px(10.)).child(picture(
                    faces,
                    format!("sent-{seq}"),
                    file,
                    media,
                    (THUMB, THUMB),
                    theme::tag_line(),
                    cx,
                )));
            }
            if !text.is_empty() {
                b = b.child(
                    ui::serif(faces, text.clone())
                        .px(px(10.))
                        .text_size(px(PROSE))
                        .line_height(px(PROSE_LINE)),
                );
            }
            b.into_any_element()
        }
        What::Reacted { place, emoji, to } => {
            let right_words = if did.here {
                when
            } else {
                format!("{} · {when}", calls::fit(place, cols(300.).max(12)))
            };
            row()
                .child(muted(faces, "Reacted"))
                .child(white(faces, emoji.clone()))
                .when_some(to.clone(), |d, to| {
                    d.child(muted(faces, "to"))
                        .child(white(faces, calls::fit(&to, 24)))
                })
                .child(div().flex_1())
                .child(meta(faces, right_words).flex_none().pl(px(S4)))
                .into_any_element()
        }
        What::Posted {
            platform,
            text,
            counts,
            link,
        } => {
            let head = label()
                .child(muted(faces, "Posted on"))
                .child(white(faces, platform.clone()))
                .child(right(faces, when));
            let foot = div()
                .flex()
                .items_center()
                .gap(px(S4))
                .when_some(*counts, |d, c| d.child(meta(faces, c.line())))
                .when_some(link.clone(), |d, link| {
                    let url = format!("https://{link}");
                    d.child(div().flex_1()).child(
                        div()
                            .id(SharedString::from(format!("post-{seq}")))
                            .cursor_pointer()
                            .child(
                                ui::text(faces, calls::fit(&link, 40))
                                    .text_size(px(META))
                                    .text_color(theme::link()),
                            )
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                    )
                });
            let card = div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .mx(px(10.))
                .px(px(14.))
                .py(px(S3))
                .rounded(px(CARD_RADIUS))
                .border_1()
                .border_color(theme::border())
                .child(
                    ui::text(faces, view.name.clone())
                        .text_size(px(13.5))
                        .line_height(px(18.)),
                )
                .child(
                    ui::text(faces, text.clone())
                        .text_size(px(15.))
                        .line_height(px(21.)),
                )
                .when(counts.is_some() || link.is_some(), |d| d.child(foot));
            boxed()
                .gap(px(10.))
                .child(head)
                .child(card)
                .into_any_element()
        }
        What::Image { file, place } => div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(S2))
            .child(picture(
                faces,
                format!("image-{seq}"),
                file,
                media,
                (IMAGE_WIDE.min(view.column), IMAGE_TALL),
                theme::border(),
                cx,
            ))
            .child(meta(
                faces,
                calls::fit(&format!("{file} · sent to {place} · {when}"), cols(0.)),
            ))
            .into_any_element(),
        What::Inner { kind, verb, what } => row()
            .child(ui::dot(colour(kind)))
            .child(muted(faces, *verb))
            .when(!what.is_empty(), |d| {
                d.child(white(faces, calls::fit(what, cols(170.))).min_w(px(0.)))
            })
            .child(div().flex_1())
            .child(meta(faces, when).flex_none().pl(px(S4)))
            .into_any_element(),
        What::Refused { place, reason } => {
            let reason = reason.trim_end_matches('.');
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .px(px(10.))
                .child(
                    div()
                        .pt(px(3.))
                        .flex_none()
                        .child(ui::icon("alert", 16.).text_color(theme::coral())),
                )
                .child(
                    ui::text(
                        faces,
                        format!("Not sent to {place}: {reason}. She did not try again."),
                    )
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(ROW - 1.))
                    .line_height(px(22.))
                    .text_color(theme::muted()),
                )
                .child(meta(faces, when).flex_none().pt(px(2.)))
                .into_any_element()
        }
    }
}

/// A box for what she said or posted out there: a hairline frame, as an
/// opened step's.
fn boxed() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(S2))
        .px(px(S3))
        .pt(px(14.))
        .pb(px(S4))
        .rounded(px(BOX_RADIUS))
        .border_1()
        .border_color(theme::border())
}

/// The line a box is headed with: what was done, and where.
fn label() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(S2))
        .px(px(10.))
        .min_w(px(0.))
        .h(px(20.))
}

/// A quiet row, the size and inset of a step's.
fn row() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(30.))
        .px(px(10.))
        .min_w(px(0.))
}

fn muted(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .flex_none()
        .whitespace_nowrap()
        .text_size(px(ROW - 1.))
        .text_color(theme::muted())
}

fn white(faces: &Faces, text: impl Into<SharedString>) -> Div {
    ui::text(faces, text)
        .whitespace_nowrap()
        .overflow_hidden()
        .text_size(px(ROW - 1.))
        .text_color(theme::text())
}

/// The time, at a label's right end.
fn right(faces: &Faces, when: String) -> Div {
    div()
        .flex_1()
        .flex()
        .justify_end()
        .child(meta(faces, when).flex_none().pl(px(S4)))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::*;

    fn entry(seq: i64, kind: &str, data: Value) -> Entry {
        Entry {
            seq,
            at: Utc::now(),
            kind: kind.to_owned(),
            data,
        }
    }

    fn heard(seq: i64, place: &Value, from: (&str, &str), id: &str) -> Entry {
        entry(
            seq,
            "heard",
            json!({"channel": "qq", "place": place, "from": {"id": from.0, "name": from.1},
                   "id": id, "text": "hi"}),
        )
    }

    /// A group and a private place on QQ, each with a line heard.
    fn journal() -> Vec<Entry> {
        vec![
            heard(
                1,
                &json!({"id": "group:g7", "kind": "group", "name": "大荒兽医站"}),
                ("u1", "阿鹿"),
                "m-1",
            ),
            heard(
                2,
                &json!({"id": "c2c:u1", "kind": "direct"}),
                ("u1", "阿鹿"),
                "m-2",
            ),
        ]
    }

    #[test]
    fn a_place_is_named_in_full_with_its_platform_its_kind_and_its_name() {
        let j = journal();
        let known = Known::of(&j);
        assert_eq!(known.place("qq", "group:g7"), "QQ 群 · 大荒兽医站");
        assert_eq!(known.place("qq", "c2c:u1"), "QQ 私聊 · 阿鹿");
        assert_eq!(
            known.place("telegram", "42"),
            "Telegram · 42",
            "unknown: its id"
        );
    }

    #[test]
    fn what_she_did_out_there_shows_in_the_console_and_not_in_another_place() {
        let mut j = journal();
        j.push(entry(
            3,
            "said",
            json!({"channel": "qq", "place": "group:g7", "text": "霁今天吃药了"}),
        ));
        j.push(entry(
            4,
            "reacted",
            json!({"channel": "qq", "place": "group:g7", "to": "m-1", "emoji": "👍"}),
        ));
        j.push(entry(
            5,
            "posted",
            json!({"channel": "x", "id": "77", "text": "大荒第七日"}),
        ));
        j.push(entry(
            6,
            "measured",
            json!({"channel": "x", "posts": [{"id": "77", "likes": 31, "views": 212}]}),
        ));
        let known = Known::of(&j);
        let console: Vec<What> = j
            .iter()
            .filter_map(|e| did(e, "console:fox", &known))
            .map(|d| d.what)
            .collect();
        assert_eq!(
            console,
            [
                What::Said {
                    place: "QQ 群 · 大荒兽医站".into(),
                    text: "霁今天吃药了".into(),
                    sent: None,
                },
                What::Reacted {
                    place: "QQ 群 · 大荒兽医站".into(),
                    emoji: "👍".into(),
                    to: Some("阿鹿".into()),
                },
                What::Posted {
                    platform: "X".into(),
                    text: "大荒第七日".into(),
                    counts: Some(Counts {
                        views: 212,
                        likes: 31,
                        ..Counts::default()
                    }),
                    link: Some("x.com/i/status/77".into()),
                },
            ]
        );
        let elsewhere: Vec<Did> = j
            .iter()
            .filter_map(|e| did(e, "qq:c2c:u1", &known))
            .collect();
        assert!(elsewhere.is_empty(), "{elsewhere:?}");
        // In its own place her line is the thread's already; her reaction
        // is a row there, and part of her answer.
        let home: Vec<Did> = j
            .iter()
            .filter_map(|e| did(e, "qq:group:g7", &known))
            .collect();
        assert_eq!(home.len(), 1);
        assert!(home[0].here && home[0].answers());
    }

    #[test]
    fn what_she_said_on_the_console_is_not_shown_again_as_done_out_there() {
        let j = vec![entry(
            1,
            "said",
            json!({"channel": "console", "place": "ann", "text": "hi"}),
        )];
        let known = Known::of(&j);
        assert!(j.iter().all(|e| did(e, "console:fox", &known).is_none()));
    }

    #[test]
    fn an_image_she_sent_is_drawn_in_its_place_and_boxed_in_the_console() {
        let mut j = journal();
        j.push(entry(
            3,
            "said",
            json!({"channel": "qq", "place": "c2c:u1", "file": "meme-03.jpg",
                   "text": "[image: a cat in the rain] 给你"}),
        ));
        let known = Known::of(&j);
        let here = did(&j[2], "qq:c2c:u1", &known).unwrap();
        assert_eq!(
            here.what,
            What::Image {
                file: "meme-03.jpg".into(),
                place: "QQ 私聊 · 阿鹿".into()
            }
        );
        assert!(here.answers());
        let out = did(&j[2], "console:fox", &known).unwrap();
        assert_eq!(
            out.what,
            What::Said {
                place: "QQ 私聊 · 阿鹿".into(),
                text: "给你".into(),
                sent: Some(Sent::Image("meme-03.jpg".into())),
            }
        );
        assert!(!out.answers(), "done out there, it does not answer you");
        assert_eq!(words("[file: notes.md] here"), "here");
        assert_eq!(made("[file: notes.md] here").as_deref(), Some("notes.md"));
        assert_eq!(words("no image"), "no image");
    }

    #[test]
    fn an_inner_entry_belongs_to_the_place_whose_look_wrote_it() {
        let mut j = journal();
        j.extend([
            entry(3, "read", json!({"channel": "qq", "place": "group:g7", "upto": 1})),
            entry(4, "felt", json!({"toward": "qq:u1", "delta": 2, "why": "kind"})),
            entry(
                5,
                "kept",
                json!({"file": "meme-03.jpg", "note": "一张重复的图", "from": "qq:group:g7"}),
            ),
            entry(
                6,
                "intended",
                json!({"due": "2026-11-18T20:00:00Z", "what": "get back to this conversation: 群里在吵",
                       "place": "qq:group:g7"}),
            ),
            entry(7, "canon", json!({"fact": "霁是未定品", "source": "said in qq:group:g7"})),
            entry(8, "decided", json!({"about": "qq:group:g7", "decision": "felt +2", "reason": ""})),
            // A feeling with no look after it has no place to be shown in.
            entry(9, "felt", json!({"toward": "qq:u1", "delta": -1, "why": ""})),
            heard(10, &json!({"id": "c2c:u1", "kind": "direct"}), ("u1", "阿鹿"), "m-3"),
            entry(11, "let_go", json!({"kept": 5})),
            entry(12, "decided", json!({"about": "qq:c2c:u1", "decision": "let go", "reason": ""})),
        ]);
        let known = Known::of(&j);
        let rows = |thread: &str| -> Vec<(&'static str, String)> {
            j.iter()
                .filter_map(|e| did(e, thread, &known))
                .filter_map(|d| match d.what {
                    What::Inner { verb, what, .. } => Some((verb, what)),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            rows("qq:group:g7"),
            [
                ("Felt", "warmer toward 阿鹿".to_owned()),
                ("Kept", "meme-03.jpg".to_owned()),
                ("Put it off", "群里在吵".to_owned()),
                ("Settled", "霁是未定品".to_owned()),
            ]
        );
        assert_eq!(
            rows("qq:c2c:u1"),
            [("Let go of", "一张重复的图".to_owned())]
        );
    }

    #[test]
    fn a_send_the_platform_refused_shows_in_its_place_and_in_the_console() {
        let mut j = journal();
        j.push(entry(
            3,
            "decided",
            json!({"about": "qq:group:g7", "decision": "could not send",
                   "reason": "qq said 400 (40054005): reply window closed\nmore"}),
        ));
        let known = Known::of(&j);
        let want = What::Refused {
            place: "QQ 群 · 大荒兽医站".into(),
            reason: "qq said 400 (40054005): reply window closed more".into(),
        };
        assert_eq!(
            did(&j[2], "qq:group:g7", &known).map(|d| d.what),
            Some(want.clone())
        );
        assert_eq!(
            did(&j[2], "console:fox", &known).map(|d| d.what),
            Some(want)
        );
        assert_eq!(did(&j[2], "qq:c2c:u1", &known), None);
    }

    #[test]
    fn each_kind_has_its_journal_colour_and_the_rest_are_grey() {
        assert_eq!(colour("kept"), theme::amber2());
        assert_eq!(colour("let_go"), theme::amber2());
        assert_eq!(colour("directed"), theme::purple());
        assert_eq!(colour("canon"), theme::coral());
        assert_eq!(colour("slept"), theme::green());
        assert_eq!(colour("worked"), theme::blue());
        assert_eq!(colour("felt"), theme::muted());
        assert_eq!(colour("intended"), theme::muted());
    }

    #[test]
    fn an_image_keeps_its_shape_within_its_bounds_and_is_never_enlarged() {
        assert_eq!(fitted((1080, 720), 320., 360.), (320., 213.));
        assert_eq!(fitted((600, 1200), 320., 360.), (180., 360.));
        assert_eq!(fitted((120, 90), 320., 360.), (120., 90.));
        assert_eq!(fitted((0, 0), 160., 160.), (160., 160.));
    }

    #[test]
    fn an_image_size_is_read_from_the_head_of_its_file() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        assert_eq!(dimensions(&mut Cursor::new(png)), Some((640, 480)));

        let mut gif = b"GIF89a".to_vec();
        gif.extend(200u16.to_le_bytes());
        gif.extend(100u16.to_le_bytes());
        assert_eq!(dimensions(&mut Cursor::new(gif)), Some((200, 100)));

        // A JPEG with an APP0 segment before its frame header.
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend([0xff, 0xc0, 0x00, 0x11, 0x08]);
        jpeg.extend(300u16.to_be_bytes());
        jpeg.extend(400u16.to_be_bytes());
        jpeg.extend([0; 20]);
        assert_eq!(dimensions(&mut Cursor::new(jpeg)), Some((400, 300)));

        let mut webp = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        webp.extend([0; 8]);
        webp.extend([0x3f, 0x01, 0x00, 0xef, 0x00, 0x00]);
        assert_eq!(dimensions(&mut Cursor::new(webp)), Some((320, 240)));

        assert_eq!(dimensions(&mut Cursor::new(b"not an image".to_vec())), None);
    }
}
