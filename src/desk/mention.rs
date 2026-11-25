//! What the composer offers as you type, and what goes with what you say:
//! the agent's commands after `/`; after `@`, the project's files, its other
//! sessions and its open pull requests; images pasted or dropped in.
//!
//! A file named goes to the agent as a link to it, a session as what was
//! said in it, a pull request as `gh` shows it, an image as itself.

use std::path::{Path, PathBuf};

use agent_client_protocol_schema::v1::{
    ContentBlock, EmbeddedResource, EmbeddedResourceResource, ImageContent, ResourceLink,
    TextResourceContents,
};
use gpui::Context;

use super::Desk;
use crate::{
    hub,
    session::{Item, Session},
};

/// Something offered as you type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Command { name: String, about: String },
    File(String),
    Session { key: u64, title: String },
    Pr { number: u64, title: String },
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
                self.slot(&s.agent)
                    .map(|x| x.commands.clone())
                    .unwrap_or_default()
            } else {
                s.commands.clone()
            };
            let offers: Vec<Offer> = commands
                .into_iter()
                .filter(|c| c.name.starts_with(name))
                .take(OFFERS)
                .map(|c| Offer::Command {
                    name: c.name,
                    about: c.description,
                })
                .collect();
            return (!offers.is_empty()).then_some((0, offers));
        }
        let start = text.rfind(char::is_whitespace).map_or(0, |i| {
            i + text[i..].chars().next().map_or(1, char::len_utf8)
        });
        let word = &text[start..];
        let query = word.strip_prefix('@')?.to_lowercase();
        let mut offers = Vec::new();
        for (n, title) in self.prs.get(&s.project).into_iter().flatten() {
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
        for other in self.sessions.iter().filter(|o| {
            o.project == s.project && o.key != s.key && (o.recorded || !o.items.is_empty())
        }) {
            if query.is_empty() || other.title.to_lowercase().contains(&query) {
                offers.push(Offer::Session {
                    key: other.key,
                    title: other.title.clone(),
                });
            }
        }
        offers.truncate(3);
        let files = self.files.get(&s.folder).into_iter().flatten();
        let mut named: Vec<&String> = files
            .filter(|f| query.is_empty() || f.to_lowercase().contains(&query))
            .collect();
        // A name that starts with what is typed comes first, then the
        // shortest paths.
        named.sort_by_key(|f| {
            let name = Path::new(f.as_str())
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            (!name.starts_with(&query), f.len())
        });
        offers.extend(
            named
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
        let (folder, project) = (s.folder.clone(), s.project.clone());
        if !self.files.contains_key(&folder) {
            self.files.insert(folder.clone(), Vec::new());
            hub::list_files(folder, self.tx.clone());
        }
        if !self.prs.contains_key(&project) {
            self.prs.insert(project.clone(), Vec::new());
            hub::list_prs(project, self.tx.clone());
        }
    }

    /// Take the offer `n` for what is typed: it replaces the word.
    pub fn take_offer(&mut self, n: usize, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text();
        let Some((start, offers)) = self.offers(&text) else {
            return;
        };
        let Some(offer) = offers.get(n).cloned() else {
            return;
        };
        let token = match &offer {
            Offer::Command { name, .. } => format!("/{name}"),
            Offer::File(f) => format!("@{f}"),
            Offer::Session { title, .. } => {
                format!("@{}", crate::ui::fit(title, 32).replace(' ', "-"))
            }
            Offer::Pr { number, .. } => format!("@pr#{number}"),
        };
        let line = format!("{}{token} ", &text[..start]);
        self.input.update(cx, |i, cx| i.set_text(&line, cx));
        if !matches!(offer, Offer::Command { .. }) {
            if let Offer::Pr { number, .. } = &offer
                && let Some(s) = self.session()
            {
                let dir = s.project.clone();
                let (n, tx) = (*number, self.tx.clone());
                crate::acp::runtime().spawn_blocking(move || {
                    let text = crate::git::pull_request_text(&dir, n);
                    let _ = tx.send(hub::Msg::PrText { number: n, text });
                });
            }
            self.mentions.push(Mention {
                token,
                what: offer,
                text: None,
            });
        }
        cx.notify();
    }

    /// An image pasted or dropped in, unless the agent said, when it last
    /// started, that it takes none.
    pub fn add_picture(&mut self, picture: Picture, cx: &mut Context<Self>) {
        let refused = self
            .session()
            .and_then(|s| self.slot(&s.agent))
            .is_some_and(|slot| slot.images == Some(false));
        if !refused {
            self.pictures.push(picture);
        } else if let Some(key) = self.current
            && let Some(s) = self.session_mut(key)
        {
            s.items.push(Item::Note {
                text: "This agent does not take images.".into(),
                trouble: true,
            });
        }
        cx.notify();
    }

    pub fn drop_picture(&mut self, n: usize, cx: &mut Context<Self>) {
        if n < self.pictures.len() {
            self.pictures.remove(n);
        }
        cx.notify();
    }

    /// Images dropped on the composer: each file that is one.
    pub fn drop_files(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
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
                _ => continue,
            };
            if let Ok(bytes) = std::fs::read(p) {
                self.add_picture(
                    Picture {
                        mime: mime.into(),
                        bytes,
                    },
                    cx,
                );
            }
        }
    }

    /// What goes with `text` said in the session `key`: the images waiting,
    /// and each file, session and pull request it names.
    pub(crate) fn attach(&mut self, key: u64, text: &str) -> Vec<ContentBlock> {
        let mut blocks: Vec<ContentBlock> = self
            .pictures
            .drain(..)
            .map(|p| {
                ContentBlock::Image(ImageContent::new(crate::base64::encode(&p.bytes), p.mime))
            })
            .collect();
        let Some(s) = self.sessions.iter().find(|s| s.key == key) else {
            return blocks;
        };
        let folder = s.folder.clone();
        let mentions = std::mem::take(&mut self.mentions);
        for m in mentions.iter().filter(|m| text.contains(&m.token)) {
            match &m.what {
                Offer::File(f) => blocks.push(link(&folder, f)),
                Offer::Session { key, title } => {
                    if let Some(other) = self.sessions.iter().find(|o| o.key == *key) {
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
                Offer::Command { .. } => {}
            }
        }
        // A path typed by hand after `@` that names a file goes too.
        for word in text.split_whitespace() {
            if let Some(f) = word.strip_prefix('@')
                && !mentions.iter().any(|m| m.token == word)
                && folder.join(f).is_file()
            {
                blocks.push(link(&folder, f));
            }
        }
        blocks
    }
}

/// A link to the file `f` in `folder`.
fn link(folder: &Path, f: &str) -> ContentBlock {
    let path = folder.join(f);
    ContentBlock::ResourceLink(ResourceLink::new(
        f.to_owned(),
        format!("file://{}", path.display()),
    ))
}

fn text_resource(uri: &str, text: &str) -> ContentBlock {
    ContentBlock::Resource(EmbeddedResource::new(
        EmbeddedResourceResource::TextResourceContents(TextResourceContents::new(text, uri)),
    ))
}

/// What was said in `s`, you and the agent in turn, the last of it.
fn said_in(s: &Session) -> String {
    let mut out = Vec::new();
    for item in &s.items {
        match item {
            Item::You(t) => out.push(format!("You: {t}")),
            Item::Agent(t) => out.push(format!("Agent: {t}")),
            _ => {}
        }
    }
    let from = out.len().saturating_sub(30);
    out[from..].join("\n\n")
}
