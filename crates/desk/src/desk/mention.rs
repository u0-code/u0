//! What the composer offers as you type, and what goes with what you say:
//! the agent's commands after `/`; after `@`, the project's files, its other
//! sessions and its open pull requests; images pasted or dropped in.
//!
//! A file named goes to the agent as a link to it, a session as what was
//! said in it, a pull request as `gh` shows it, an image as itself.

use std::path::{Path, PathBuf};

use agent_client_protocol::v1::schema::{
    ContentBlock, EmbeddedResource, EmbeddedResourceResource, ImageContent, ResourceLink,
    TextResourceContents,
};
use gpui::{AppContext as _, Context, Entity, Window};

use super::Desk;
use crate::{
    hub,
    session::{Item as Said, Session},
};
use gpui::component::input::{InputEvent, TextareaState};

/// The key context the composer's own keys (⌘S, ⇧⌘Return) apply under.
pub const CONTEXT: &str = "Composer";

/// The line you type into, and what goes with it: the offer Return or Tab
/// takes, what was named with `@` and images put in, waiting to be sent;
/// whether the draft on screen goes to every agent that can run once
/// sent; what you put aside
/// from it (see `store::Index`); and what was sent from it, the latest
/// last, with which of that is in the line while ↑ and ↓ go back through it;
/// what the writer thinks you will say next in a session, by its key.
pub struct Composer {
    pub input: Entity<TextareaState>,
    pub offer: usize,
    pub mentions: Vec<Mention>,
    pub pictures: Vec<Picture>,
    pub fanout: bool,
    pub stashed: Vec<String>,
    pub sent: Vec<String>,
    pub recalled: Option<usize>,
    pub suggested: Option<(u64, String)>,
}

impl Composer {
    pub(super) fn new(cmd_sends: bool, window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 12)
                .submit_on_enter(!cmd_sends)
                .placeholder("Ask for anything, or give a task")
        });
        cx.subscribe_in(&input, window, |this, input, e: &InputEvent, window, cx| {
            match e {
                InputEvent::Change => {
                    this.composer.offer = 0;
                    this.composer.recalled = None;
                    if input.read(cx).value().contains('@') {
                        this.want_offers();
                    }
                    cx.notify();
                }
                // Return sends, and ⌘Return sends aside; where Return
                // breaks the line, ⌘Return sends. ⇧Return is a new line.
                InputEvent::PressEnter {
                    secondary,
                    shift: false,
                } => match (this.settings.cmd_sends, secondary) {
                    (true, false) => {}
                    (false, true) => this.send_typed(true, window, cx),
                    _ => this.send_typed(false, window, cx),
                },
                _ => {}
            }
        })
        .detach();
        Self {
            input,
            offer: 0,
            mentions: Vec::new(),
            pictures: Vec::new(),
            fanout: false,
            stashed: Vec::new(),
            sent: Vec::new(),
            recalled: None,
            suggested: None,
        }
    }
}

impl Desk {
    /// What is typed in the composer, without the space around it.
    pub(crate) fn typed(&self, cx: &gpui::App) -> String {
        self.composer.input.read(cx).value().trim().to_owned()
    }

    /// Put `text` in the composer in place of what it held, the cursor at
    /// its end, as though it had been typed.
    pub(crate) fn put_line(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.input.update(cx, |i, cx| {
            i.set_value(text.to_owned(), window, cx);
            i.set_selected_range(text.len()..text.len(), cx);
        });
    }

    /// Hand over what is typed, if anything, and clear the line: said in
    /// the session, or (`aside`) said and left to go on while a new one is
    /// begun beside it.
    pub(crate) fn send_typed(&mut self, aside: bool, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.typed(cx);
        if text.is_empty() {
            return;
        }
        if self.composer.sent.last() != Some(&text) {
            self.composer.sent.push(text.clone());
        }
        self.put_line("", window, cx);
        if aside {
            self.send_aside(&text, window, cx);
        } else {
            self.send(&text, cx);
        }
        cx.notify();
    }

    /// Put what is typed aside (⌘S), and clear the line.
    pub(crate) fn stash_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.typed(cx);
        if text.is_empty() {
            return;
        }
        self.put_line("", window, cx);
        self.composer.stashed.push(text);
        self.save_stash();
        self.done(
            "Put aside: take it back from the stack under the line",
            None,
        );
        cx.notify();
    }

    /// ↑ (-1) or ↓ (1) on the composer's first or last line: back through
    /// what was sent before, while the line is empty or holds one of
    /// those. Whether the key went to it.
    pub(crate) fn recall(
        &mut self,
        by: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let input = self.composer.input.read(cx);
        let (text, at) = (input.value(), input.cursor());
        let edge = if by < 0 {
            !text[..at].contains('\n')
        } else {
            !text[at..].contains('\n')
        };
        let sent = &self.composer.sent;
        if !edge || sent.is_empty() || (!text.is_empty() && self.composer.recalled.is_none()) {
            return false;
        }
        let at = match (self.composer.recalled, by < 0) {
            (None, true) => Some(sent.len() - 1),
            (Some(n), true) => Some(n.saturating_sub(1)),
            (Some(n), false) => (n + 1 < sent.len()).then_some(n + 1),
            (None, false) => return false,
        };
        let text = at.map(|n| sent[n].clone()).unwrap_or_default();
        self.put_line(&text, window, cx);
        self.composer.recalled = at;
        true
    }

    /// A new line in the composer, where what is typed is a list: its next
    /// item begun, or after an item left empty, the list ended there.
    /// Whether it was a list's.
    pub(crate) fn continue_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let input = self.composer.input.read(cx);
        let text = input.value();
        let (at, end) = (input.selected_range().start, input.selected_range().end);
        let begins = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let Some(item) = next_item(&text[begins..at]) else {
            return false;
        };
        self.composer.input.update(cx, |i, cx| match item {
            Item::Next(mark) => i.replace(format!("\n{mark}"), window, cx),
            Item::End => {
                i.set_selected_range(begins..end, cx);
                i.replace("", window, cx);
            }
        });
        true
    }
}

/// What a new line after a list's item does.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    /// Begins the next item with this.
    Next(String),
    /// The item was left empty: its mark goes, and the list ends.
    End,
}

/// What a new line after `line` does, where it is a list's item: the same
/// mark ("- ", "* ", "> ") or the next number ("3. "), under the same
/// indent; none where it is not a list.
fn next_item(line: &str) -> Option<Item> {
    let rest = line.trim_start();
    let indent = &line[..line.len() - rest.len()];
    let (mark, after) = if let Some(found) = ["- ", "* ", "> "]
        .iter()
        .find_map(|m| rest.strip_prefix(m).map(|a| ((*m).to_owned(), a)))
    {
        found
    } else {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let after = rest[digits..].strip_prefix(". ")?;
        let n: u64 = rest[..digits].parse().ok()?;
        (format!("{}. ", n + 1), after)
    };
    Some(if after.trim().is_empty() {
        Item::End
    } else {
        Item::Next(format!("{indent}{mark}"))
    })
}

/// Something offered as you type.
/// Ghost's own commands after `/`, beside the agent's: done at once.
const OWN: [(&str, &str); 4] = [
    ("clear", "Begin afresh: a new session here"),
    ("model", "Choose the model"),
    ("branch", "Branch off a copy of this session"),
    ("copy", "Copy the conversation"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Command {
        name: String,
        about: String,
    },
    /// One of Ghost's own commands (see `OWN`).
    Own {
        name: &'static str,
        about: &'static str,
    },
    /// A model to switch to, by its option and value.
    Model {
        config: String,
        value: String,
        label: String,
    },
    File(String),
    Session {
        key: u64,
        title: String,
    },
    Pr {
        number: u64,
        title: String,
    },
}

/// Something named with `@`, kept until it is sent: the words it put in
/// the line, and what goes with them.
#[derive(Debug, Clone)]
pub struct Mention {
    pub token: String,
    pub what: Offer,
    /// A pull request's text, once `gh` has shown it.
    pub text: Option<String>,
}

/// An image pasted or dropped in, waiting to go with what is said.
#[derive(Debug, Clone)]
pub struct Picture {
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// At most this many offers show.
const OFFERS: usize = 8;

impl Desk {
    /// What to offer for what is typed: where the word being completed
    /// starts, and the offers for it.
    pub(crate) fn offers(&self, text: &str) -> Option<(usize, Vec<Offer>)> {
        let s = self.session()?;
        if let Some(name) = text.strip_prefix('/')
            && !name.contains(char::is_whitespace)
        {
            let commands = if s.commands.is_empty() {
                self.slot(&s.meta.agent)
                    .map(|x| x.commands.clone())
                    .unwrap_or_default()
            } else {
                s.commands.clone()
            };
            let mut offers: Vec<Offer> = commands
                .into_iter()
                .filter(|c| c.name.starts_with(name))
                .take(OFFERS)
                .map(|c| Offer::Command {
                    name: c.name,
                    about: c.description,
                })
                .collect();
            offers.extend(
                OWN.iter()
                    .filter(|(own, _)| own.starts_with(name))
                    .map(|&(name, about)| Offer::Own { name, about }),
            );
            return (!offers.is_empty()).then_some((0, offers));
        }
        let start = text.rfind(char::is_whitespace).map_or(0, |i| {
            i + text[i..].chars().next().map_or(1, char::len_utf8)
        });
        let word = &text[start..];
        let query = word.strip_prefix('@')?.to_lowercase();
        let mut offers = Vec::new();
        for (n, title) in self.prs.get(&s.meta.project).into_iter().flatten() {
            if query.is_empty()
                || title.to_lowercase().contains(&query)
                || n.to_string().starts_with(&query)
            {
                offers.push(Offer::Pr {
                    number: *n,
                    title: title.clone(),
                });
            }
        }
        // The models the agent offers: @ switches to one.
        if let Some(option) = s.options.iter().find(|o| {
            o.category
                == Some(agent_client_protocol::v1::schema::SessionConfigOptionCategory::Model)
        }) && let Some((_, choices)) = crate::thread::select_choices(&option.kind)
        {
            for (value, label, on) in choices {
                if !on
                    && !query.is_empty()
                    && (label.to_lowercase().contains(&query)
                        || value.to_lowercase().contains(&query))
                {
                    offers.push(Offer::Model {
                        config: option.id.0.to_string(),
                        value,
                        label,
                    });
                }
            }
        }
        for other in self.sessions.all.iter().filter(|o| {
            o.meta.project == s.meta.project
                && o.key != s.key
                && (o.recorded || !o.items.is_empty())
        }) {
            if query.is_empty() || other.meta.title.to_lowercase().contains(&query) {
                offers.push(Offer::Session {
                    key: other.key,
                    title: other.meta.title.clone(),
                });
            }
        }
        offers.truncate(3);
        offers.extend(
            ranked(self.files.get(&s.meta.folder), &query)
                .into_iter()
                .take(OFFERS)
                .map(|f| Offer::File(f.clone())),
        );
        offers.truncate(OFFERS);
        Some((start, offers))
    }

    /// Read what `@` can name in the session on screen, once.
    pub(crate) fn want_offers(&mut self) {
        let Some(s) = self.session() else { return };
        let (folder, project) = (s.meta.folder.clone(), s.meta.project.clone());
        if !self.files.contains_key(&folder) {
            self.files.insert(folder.clone(), Vec::new());
            self.transport
                .send(crate::proto::Call::ListFiles { folder });
        }
        if !self.prs.contains_key(&project) {
            self.prs.insert(project.clone(), Vec::new());
            self.transport
                .send(crate::proto::Call::ListPrs { folder: project });
        }
    }

    /// Take the offer `n` for what is typed: it replaces the word.
    pub fn take_offer(&mut self, n: usize, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let text = self.typed(cx);
        let Some((start, offers)) = self.offers(&text) else {
            return;
        };
        let Some(offer) = offers.get(n).cloned() else {
            return;
        };
        // A model is switched to, and its word taken out.
        if let Offer::Model { config, value, .. } = &offer {
            let line = text[..start].to_owned();
            self.put_line(&line, window, cx);
            self.set_option(config.clone(), value.clone(), cx);
            return;
        }
        // One of Ghost's own is done, not written.
        if let Offer::Own { name, .. } = offer {
            self.put_line("", window, cx);
            let key = self.current;
            match name {
                "clear" => self.new_default(window, cx),
                "model" => self.open_model(window, cx),
                "branch" => {
                    if let Some(key) = key {
                        self.fork(key, cx);
                    }
                }
                _ => {
                    if let Some(key) = key {
                        self.copy_thread(key, cx);
                    }
                }
            }
            return;
        }
        let token = match &offer {
            Offer::Own { name, .. } => format!("/{name}"),
            Offer::Model { label, .. } => format!("@{label}"),
            Offer::Command { name, .. } => format!("/{name}"),
            Offer::File(f) => format!("@{f}"),
            Offer::Session { title, .. } => {
                format!("@{}", crate::ui::fit(title, 32).replace(' ', "-"))
            }
            Offer::Pr { number, .. } => format!("@pr#{number}"),
        };
        let line = format!("{}{token} ", &text[..start]);
        self.put_line(&line, window, cx);
        if !matches!(offer, Offer::Command { .. }) {
            if let Offer::Pr { number, .. } = &offer
                && let Some(s) = self.session()
            {
                self.transport.send(crate::proto::Call::PrText {
                    dir: s.meta.project.clone(),
                    number: *number,
                });
            }
            self.composer.mentions.push(Mention {
                token,
                what: offer,
                text: None,
            });
        }
        cx.notify();
    }

    /// An image pasted in the composer, of a kind an agent reads.
    pub fn paste_picture(&mut self, image: &gpui::Image, cx: &mut Context<Self>) {
        let mime = match image.format {
            gpui::ImageFormat::Png => "image/png",
            gpui::ImageFormat::Jpeg => "image/jpeg",
            gpui::ImageFormat::Gif => "image/gif",
            gpui::ImageFormat::Webp => "image/webp",
            _ => return,
        };
        self.add_picture(
            Picture {
                mime: mime.into(),
                bytes: image.bytes.clone(),
            },
            cx,
        );
    }

    /// An image pasted or dropped in, unless the agent said, when it last
    /// started, that it takes none.
    pub fn add_picture(&mut self, picture: Picture, cx: &mut Context<Self>) {
        let refused = self
            .session()
            .and_then(|s| self.slot(&s.meta.agent))
            .and_then(crate::hub::Slot::caps)
            .is_some_and(|known| !known.prompt_capabilities.image);
        if refused {
            let name = self
                .session()
                .map(|s| self.name_of(&s.meta.agent))
                .unwrap_or_default();
            self.trouble(format!("{name} does not take images"), None, None);
        } else {
            self.composer.pictures.push(picture);
        }
        cx.notify();
    }

    /// Quote `text` (something the agent said) in what you are writing,
    /// to answer it.
    pub fn quote(&mut self, text: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let quoted = text.trim().lines().fold(String::new(), |mut q, l| {
            q.push_str("> ");
            q.push_str(l);
            q.push('\n');
            q
        });
        let typed = self.typed(cx);
        let line = format!("{quoted}\n{typed}");
        self.put_line(&line, window, cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    pub fn drop_picture(&mut self, n: usize, cx: &mut Context<Self>) {
        if n < self.composer.pictures.len() {
            self.composer.pictures.remove(n);
        }
        cx.notify();
    }

    /// Images dropped on the composer: each file that is one.
    pub fn drop_files(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        for p in paths {
            let mime = match p
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .as_deref()
            {
                Some("png") => "image/png",
                Some("jpg" | "jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                // Any other file, or a folder, is named, as @ names one.
                _ => {
                    self.name_dropped(p, window, cx);
                    continue;
                }
            };
            hub::read_picture(p.clone(), mime, self.tx.clone());
        }
    }

    /// Let you choose a window to capture and go with what you write.
    pub fn capture(&mut self, cx: &mut Context<Self>) {
        hub::capture_window(self.tx.clone());
        cx.notify();
    }

    /// Name the file or folder `p` in what you are writing, as `@` would:
    /// from the session's folder where it is in it, else whole.
    fn name_dropped(&mut self, p: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let named = self
            .session()
            .and_then(|s| p.strip_prefix(&s.meta.folder).ok())
            .map_or_else(|| p.display().to_string(), |r| r.display().to_string());
        let token = format!("@{named}");
        let typed = self.typed(cx);
        let line = if typed.is_empty() {
            format!("{token} ")
        } else {
            format!("{typed} {token} ")
        };
        self.put_line(&line, window, cx);
        self.composer.mentions.push(Mention {
            token,
            what: Offer::File(named),
            text: None,
        });
        cx.notify();
    }

    /// What goes with `text` said in the session `key`: the images waiting,
    /// and each file, session and pull request it names.
    pub(crate) fn attach(&mut self, key: u64, text: &str) -> Vec<ContentBlock> {
        let mut blocks: Vec<ContentBlock> = self
            .composer
            .pictures
            .drain(..)
            .map(|p| {
                ContentBlock::Image(ImageContent::new(
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &p.bytes),
                    p.mime,
                ))
            })
            .collect();
        let Some(s) = self.sessions.at(key) else {
            return blocks;
        };
        let folder = s.meta.folder.clone();
        let mentions = std::mem::take(&mut self.composer.mentions);
        for m in mentions.iter().filter(|m| text.contains(&m.token)) {
            match &m.what {
                Offer::File(f) => blocks.extend(link(&folder, f)),
                Offer::Session { key, title } => {
                    if let Some(other) = self.sessions.at(*key) {
                        blocks.push(text_resource(
                            &format!("ghost://session/{key}"),
                            &format!("The session “{title}”:\n\n{}", said_in(other)),
                        ));
                    }
                }
                Offer::Pr { number, .. } => {
                    if let Some(t) = &m.text {
                        blocks.push(text_resource(&format!("ghost://pr/{number}"), t));
                    }
                }
                Offer::Command { .. } | Offer::Own { .. } | Offer::Model { .. } => {}
            }
        }
        // A path typed by hand after `@` that names a file goes too.
        for word in text.split_whitespace() {
            if let Some(f) = word.strip_prefix('@')
                && !mentions.iter().any(|m| m.token == word)
                && folder.join(f).is_file()
            {
                blocks.extend(link(&folder, f));
            }
        }
        blocks
    }
}

/// The files whose paths hold `query` (in lower case), a name that starts
/// with it first, then the shortest paths.
pub(super) fn ranked<'a>(files: Option<&'a Vec<String>>, query: &str) -> Vec<&'a String> {
    let mut found: Vec<&String> = files
        .into_iter()
        .flatten()
        .filter(|f| f.to_lowercase().contains(query))
        .collect();
    found.sort_by_key(|f| {
        let name = f.rsplit('/').next().unwrap_or_default().to_lowercase();
        (!name.starts_with(query), f.len())
    });
    found
}

/// A link to the file `f` in `folder`, by a `file:` URI with what needs
/// escaping in the path escaped; none where `folder` is not absolute.
fn link(folder: &Path, f: &str) -> Option<ContentBlock> {
    let uri = url::Url::from_file_path(folder.join(f)).ok()?;
    Some(ContentBlock::ResourceLink(ResourceLink::new(
        f.to_owned(),
        uri.to_string(),
    )))
}

fn text_resource(uri: &str, text: &str) -> ContentBlock {
    ContentBlock::Resource(EmbeddedResource::new(
        EmbeddedResourceResource::TextResourceContents(TextResourceContents::new(text, uri)),
    ))
}

/// What was said in `s`, you and the agent in turn, the last of it.
fn said_in(s: &Session) -> String {
    let out = said(s, "Agent");
    let from = out.len().saturating_sub(30);
    out[from..].join("\n\n")
}

/// What was said in `s`, yours and the agent's (named `name`), in turn.
fn said(s: &Session, name: &str) -> Vec<String> {
    s.items
        .iter()
        .filter_map(|item| match item {
            Said::You(t) => Some(format!("You: {t}")),
            Said::Agent(t) => Some(format!("{name}: {t}")),
            _ => None,
        })
        .collect()
}

impl Desk {
    /// Copy all that was said in the session `key`.
    pub fn copy_thread(&mut self, key: u64, cx: &mut Context<Self>) {
        self.popup = crate::desk::Popup::Closed;
        if let Some(s) = self.sessions.at(key) {
            let text = said(s, &self.name_of(&s.meta.agent)).join("\n\n");
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            self.done("The conversation is copied", None);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_goes_on_to_its_next_item_and_ends_at_an_empty_one() {
        let next = |m: &str| Some(Item::Next(m.into()));
        assert_eq!(next_item("- milk"), next("- "));
        assert_eq!(next_item("  * eggs"), next("  * "));
        assert_eq!(next_item("9. ten"), next("10. "));
        assert_eq!(next_item("> said"), next("> "));
        assert_eq!(next_item("- "), Some(Item::End));
        assert_eq!(next_item("3. "), Some(Item::End));
        assert_eq!(next_item("plain"), None);
        assert_eq!(next_item("-not a list"), None);
        assert_eq!(next_item(". no number"), None);
    }

    #[test]
    fn a_file_whose_name_starts_with_what_is_typed_comes_before_a_shorter_path() {
        let files = [
            "a/ma/b.rs",
            "app/domain/maps.rs",
            "docs/Main.md",
            "src/main.rs",
            "x.rs",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            ranked(Some(&files), "ma"),
            [
                "src/main.rs",
                "docs/Main.md",
                "app/domain/maps.rs",
                "a/ma/b.rs"
            ]
        );
    }

    #[test]
    fn a_file_named_with_a_space_and_a_hash_is_linked_by_a_uri_that_reads_back() {
        let Some(ContentBlock::ResourceLink(l)) = link(Path::new("/tmp/my project"), "a #1.rs")
        else {
            panic!("not a link");
        };
        assert_eq!(l.uri, "file:///tmp/my%20project/a%20%231.rs");
        let back = url::Url::parse(&l.uri).unwrap().to_file_path().unwrap();
        assert_eq!(back, Path::new("/tmp/my project/a #1.rs"));
    }
}
