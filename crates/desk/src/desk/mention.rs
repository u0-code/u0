//! What the composer offers as you type, and what goes with what you say:
//! the agent's commands after `/`; after `@`, the project's files, its other
//! sessions and its open pull requests; what is attached: images and files
//! pasted, dropped or chosen, and a paste too long for the line.
//!
//! A file named or attached goes to the agent as a link to it, a session as
//! what was said in it, a pull request as `gh` shows it, an image as itself,
//! a long paste as its text.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use agent_client_protocol::v1::schema::{
    BlobResourceContents, ContentBlock, EmbeddedResource, EmbeddedResourceResource, ImageContent,
    ResourceLink, TextResourceContents,
};
use gpui::{AppContext as _, Context, Entity, Window};

use super::Desk;
use crate::{
    hub,
    session::{Item as Said, Session, State},
};
use gpui::component::input::{InputEvent, InputState, SpellChecker, TextareaState};

/// The system's spell checker (macOS's; none elsewhere), as gpui-kit's
/// input asks for one.
struct System;

impl SpellChecker for System {
    fn misspelled_words(&self, text: &str, cx: &gpui::App) -> Vec<std::ops::Range<usize>> {
        cx.misspelled_words(text)
    }

    fn guesses(&self, word: &str, cx: &gpui::App) -> Vec<gpui::SharedString> {
        (cx.spelling_guesses(word).into_iter())
            .map(Into::into)
            .collect()
    }

    fn learn(&self, word: &str, cx: &mut gpui::App) {
        cx.learn_spelling(word);
    }
}

/// The composer's spell checker: the system's when `on`.
fn spelling(on: bool) -> Option<std::rc::Rc<dyn SpellChecker>> {
    on.then(|| std::rc::Rc::new(System) as std::rc::Rc<dyn SpellChecker>)
}

/// The key context the composer's own keys (⌘S, ⇧⌘Return) apply under.
pub const CONTEXT: &str = "Composer";

/// The line you type into, and what goes with it: the offer Return or Tab
/// takes, what was named with `@` and images put in, waiting to be sent;
/// how many pictures are still being read for it, and whether what is
/// typed goes once they are (and how: `Some(true)` the other way, see
/// `Desk::send_typed`); the draft of the session on screen; what you put
/// aside from it; and what was sent from it, the latest last, with which
/// of that is in the line while ↑ and ↓ go back through it; what the
/// writer thinks you will say next in a session, by its key; the chip
/// whose note is being written, and the field for it; and each other
/// session's draft, by its key, while another is on screen.
pub struct Composer {
    pub input: Entity<TextareaState>,
    pub offer: usize,
    pub mentions: Vec<Mention>,
    pub draft: Draft,
    pub reading: usize,
    pub held: Option<bool>,
    /// The queued message being edited in place in the composer: its
    /// session, and what it said when taken up.
    pub editing_queued: Option<(u64, String)>,
    /// Attached excerpt chip index currently showing its preview card.
    pub previewing_chip: Option<usize>,
    pub stashed: Vec<Stashed>,
    pub recalled: Option<usize>,
    pub suggested: Option<(u64, String)>,
    pub noting: Option<usize>,
    pub note: Entity<InputState>,
    pub drafts: HashMap<u64, Draft>,
}

/// What goes with a session's draft besides its text, kept with that
/// session alone: what is attached; the other `(agent, model)` it also
/// starts on once sent ("Also run on"); whether it starts in the
/// background, the draft page staying; and whether its text came from a
/// `u0://` link.
#[derive(Default)]
pub struct Draft {
    pub attached: Vec<Attached>,
    pub also_run: Vec<(String, Option<String>)>,
    pub in_background: bool,
    pub from_link: bool,
}

impl Draft {
    /// The other agents kept only where each gets a worktree of its own
    /// (`worktrees`): several agents never work in one folder at once.
    pub fn keep_apart(&mut self, worktrees: bool) {
        if !worktrees {
            self.also_run.clear();
        }
    }

    /// `agent` runs it now, first: it is no longer one it also runs on.
    pub fn leads(&mut self, agent: &str) {
        self.also_run.retain(|(a, _)| a != agent);
    }

    fn is_empty(&self) -> bool {
        self.attached.is_empty()
            && self.also_run.is_empty()
            && !self.in_background
            && !self.from_link
    }
}

/// Whether the draft `s` holds anything of yours: `text` typed in it,
/// what `draft` attached, or what it was begun about (a pull request or
/// an issue linked, what its worktree begins from).
pub fn holds(s: &Session, text: &str, draft: Option<&Draft>) -> bool {
    !text.trim().is_empty()
        || draft.is_some_and(|d| !d.attached.is_empty())
        || !s.attached.is_empty()
        || !s.meta.links.is_empty()
        || s.begin.is_some()
}

/// The draft of the session left (`from`; none for the composer drawn
/// with no session on screen, kept as session 0) put away in `drafts`,
/// and that of the session `to` brought back into `on`: a session that
/// has none gets an empty one.
fn swap_draft(on: &mut Draft, drafts: &mut HashMap<u64, Draft>, from: Option<u64>, to: u64) {
    let left = std::mem::take(on);
    if !left.is_empty() {
        drafts.insert(from.unwrap_or(0), left);
    }
    *on = drafts.remove(&to).unwrap_or_default();
}

/// What was put aside (⌘S): its text, kept between runs (see
/// `store::Index`), and what was attached to it, kept while u0 runs.
#[derive(Debug, Clone)]
pub struct Stashed {
    pub text: String,
    pub attached: Vec<Attached>,
}

/// The most a message may hold, in characters; one longer stays in the
/// line, to be cut or split.
pub const LIMIT: usize = 120_000;

impl Composer {
    pub(super) fn new(
        return_sends: bool,
        spell_check: bool,
        window: &mut Window,
        cx: &mut Context<Desk>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx)
                .auto_grow(1, 12)
                .submit_on_enter(return_sends)
                .placeholder(crate::tr("Ask for anything, or give a task"));
            input.set_spell_checker(spelling(spell_check), cx);
            input
        });
        cx.subscribe_in(&input, window, |this, input, e: &InputEvent, window, cx| {
            match e {
                InputEvent::Change => {
                    // The start page's composer, first written in, is begun.
                    if !input.read(cx).value().is_empty() {
                        this.begin_unbegun(window, cx);
                    }
                    this.composer.offer = 0;
                    this.composer.recalled = None;
                    // Editing what waits to go keeps it back.
                    this.composer.held = None;
                    if input.read(cx).value().contains('@') {
                        this.want_offers();
                    }
                    this.sync_send_key(cx);
                    cx.notify();
                }
                // The key that sends sends; the other one (⌘Return where
                // Return sends, else ⇧⌘Return) sends the other way.
                // ⇧Return is a new line.
                InputEvent::PressEnter {
                    secondary,
                    shift: false,
                } => {
                    let sends = this.return_sends(cx);
                    if sends || *secondary {
                        this.send_typed(sends && *secondary, window, cx);
                    }
                }
                _ => {}
            }
        })
        .detach();
        let note =
            cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("Your note on it")));
        cx.subscribe_in(&note, window, |this, _, e: &InputEvent, window, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                this.keep_note(window, cx);
            }
        })
        .detach();
        Self {
            input,
            offer: 0,
            mentions: Vec::new(),
            draft: Draft::default(),
            reading: 0,
            held: None,
            editing_queued: None,
            previewing_chip: None,
            stashed: Vec::new(),
            recalled: None,
            suggested: None,
            noting: None,
            note,
            drafts: HashMap::new(),
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

    /// Whether Return sends what is typed now.
    pub(crate) fn return_sends(&self, cx: &gpui::App) -> bool {
        (self.settings.send).return_sends(&self.composer.input.read(cx).value())
    }

    /// Check the composer's spelling, or not, as the settings say.
    pub(crate) fn sync_spelling(&self, cx: &mut Context<Self>) {
        let checker = spelling(self.settings.spell_check);
        (self.composer.input).update(cx, |i, cx| i.set_spell_checker(checker, cx));
    }

    /// Have the line send on Return or break there, as the setting says
    /// for what is typed now.
    pub(crate) fn sync_send_key(&self, cx: &mut Context<Self>) {
        let sends = self.return_sends(cx);
        self.composer
            .input
            .update(cx, |i, cx| i.set_submit_on_enter(sends, cx));
    }

    /// Hand over what is typed, if anything, and clear the line: said in
    /// the session, or (`other`) the other way: while a turn goes on,
    /// steering it where it would wait and waiting where it would steer;
    /// else said and left to go on while a new one is begun beside it.
    /// While a picture is still being read for it, it waits for that.
    pub(crate) fn send_typed(&mut self, other: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.begin_unbegun(window, cx);
        let text = self.typed(cx);
        if text.is_empty() {
            return;
        }
        if text.chars().count() > LIMIT {
            self.trouble(
                crate::tr_with(
                    "Too long to send: at most %{limit} characters",
                    &[("limit", &LIMIT)],
                ),
                Some(crate::tr("Cut it, or send it in parts").into()),
                None,
            );
            return;
        }
        if let Some((key, was)) = self.composer.editing_queued.take() {
            if self.save_queued(key, &was, &text, cx) {
                self.put_line("", window, cx);
            } else {
                self.trouble(
                    crate::tr("It left the queue before your edit was saved"),
                    Some(crate::tr("What you wrote is still in the line").into()),
                    None,
                );
            }
            return;
        }
        if self.composer.reading > 0 {
            self.composer.held = Some(other);
            cx.notify();
            return;
        }
        self.composer.held = None;
        self.composer.draft.from_link = false;
        self.put_line("", window, cx);
        let draft = self.session().is_some_and(Session::is_draft);
        let working = self
            .session()
            .is_some_and(|s| matches!(s.state, State::Working | State::Opening));
        let aside = other || (draft && self.composer.draft.in_background);
        match (aside, working) {
            (true, true) => {
                if let Some(key) = self.current {
                    self.follow_up(key, &text, !self.settings.steer, cx);
                }
            }
            (true, false) => self.send_aside(&text, window, cx),
            (false, _) => self.send(&text, cx),
        }
        cx.notify();
    }

    /// Turn an over-limit draft in the composer into a pasted-text file
    /// attachment and clear the input line.
    pub fn attach_typed_as_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.input.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.put_line("", window, cx);
        self.add_attached(
            Attached::Text(Chip::new(crate::session::Context::Paste, &text)),
            cx,
        );
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
    }

    /// Put what is typed aside (⌘S), with what is attached, and clear the
    /// line; with nothing typed, take back what was put aside: the one
    /// there is, or the list to choose from.
    pub(crate) fn stash_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.typed(cx);
        if text.is_empty() && self.composer.draft.attached.is_empty() {
            match self.composer.stashed.len() {
                0 => {}
                1 => self.take_stashed(0, window, cx),
                _ => self.popup = crate::desk::Popup::Stash,
            }
            cx.notify();
            return;
        }
        self.put_line("", window, cx);
        let attached = std::mem::take(&mut self.composer.draft.attached);
        self.composer.stashed.push(Stashed { text, attached });
        self.save_stash();
        self.done(
            crate::tr("Put aside: ⌘S on an empty line takes it back"),
            None,
        );
        cx.notify();
    }

    /// ↑ (-1) or ↓ (1) on the composer's first or last line: back through
    /// what you said in the session on screen, while the line is empty or
    /// holds one of those. Whether the key went to it.
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
        let mut sent: Vec<&str> = (self.session().into_iter())
            .flat_map(|s| &s.items)
            .filter_map(|i| match i {
                Said::You(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        sent.dedup();
        if !edge || sent.is_empty() || (!text.is_empty() && self.composer.recalled.is_none()) {
            return false;
        }
        let at = match (self.composer.recalled, by < 0) {
            (None, true) => Some(sent.len() - 1),
            (Some(n), true) => Some(n.saturating_sub(1)),
            (Some(n), false) => (n + 1 < sent.len()).then_some(n + 1),
            (None, false) => return false,
        };
        let text = at.map(|n| sent[n].to_owned()).unwrap_or_default();
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
/// u0's own commands after `/`, beside the agent's: done at once.
const OWN: [(&str, &str); 5] = [
    ("clear", "Begin afresh: a new thread here"),
    ("usage-limits", "What threads cost, as agents said"),
    ("model", "Choose the model"),
    ("branch", "Branch off a copy of this thread"),
    ("copy", "Copy the conversation"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Command {
        name: String,
        about: String,
    },
    /// One of u0's own commands (see `OWN`).
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
    Issue {
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

/// What is attached, waiting to go with what is said: an image, sent as
/// itself; a file of yours, sent as a link to it; a paste too long for the
/// line, sent as its text.
#[derive(Debug, Clone)]
pub enum Attached {
    Image {
        mime: String,
        bytes: Vec<u8>,
    },
    File(PathBuf),
    /// Text, named by where it came from.
    Text(Chip),
}

/// Text beside what you write: what kind it is, the text, your note on
/// it, and where in a thread it was quoted from (the session and the
/// item), to go back to.
#[derive(Debug, Clone)]
pub struct Chip {
    pub kind: crate::session::Context,
    pub text: String,
    pub note: String,
    pub from: Option<(u64, usize)>,
}

impl Chip {
    pub fn new(kind: crate::session::Context, text: &str) -> Self {
        Self {
            kind,
            text: text.to_owned(),
            note: String::new(),
            from: None,
        }
    }

    /// What goes to the agent: the text, and your note on it after.
    fn said(&self) -> String {
        if self.note.trim().is_empty() {
            return self.text.clone();
        }
        format!("{}\n\nMy note on this: {}", self.text, self.note.trim())
    }
}

/// Chips copied, as the clipboard carries them beside their text: each
/// one's kind (by its address), text and note.
type Copied = Vec<(String, String, String)>;

/// A paste this long or longer is attached, not put in the line.
pub const LONG_PASTE: usize = 32 * 1024;

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
                self.slot(s.host, &s.meta.agent)
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
        let typed = super::rank::Query::new(&query);
        let found = |text: &str| super::rank::terms(&typed, text, &[], true).is_some();
        let mut offers = Vec::new();
        let project = super::Place::project_of(s);
        for (n, title) in self.prs.get(&project).into_iter().flatten() {
            if query.is_empty() || found(title) || n.to_string().starts_with(&query) {
                offers.push(Offer::Pr {
                    number: *n,
                    title: title.clone(),
                });
            }
        }
        for (n, title) in self.issues.get(&project).into_iter().flatten() {
            if query.is_empty() || found(title) || n.to_string().starts_with(&query) {
                offers.push(Offer::Issue {
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
                if !on && !query.is_empty() && (found(&label) || found(&value)) {
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
            if query.is_empty() || found(&other.meta.title) {
                offers.push(Offer::Session {
                    key: other.key,
                    title: other.meta.title.clone(),
                });
            }
        }
        offers.truncate(3);
        offers.extend(
            crate::repo::paths(
                (self.files.get(&super::Place::folder_of(s)))
                    .into_iter()
                    .flatten(),
                &query,
            )
            .into_iter()
            .take(OFFERS)
            .map(|(f, _)| Offer::File(f.clone())),
        );
        offers.truncate(OFFERS);
        Some((start, offers))
    }

    /// Read what `@` can name in the session on screen, once.
    pub(crate) fn want_offers(&mut self) {
        let Some(s) = self.session() else { return };
        let (folder, project) = (super::Place::folder_of(s), super::Place::project_of(s));
        if !self.files.contains_key(&folder) {
            self.files.insert(folder.clone(), Vec::new());
            self.list_files(&folder);
        }
        if !self.prs.contains_key(&project) {
            self.prs.insert(project.clone(), Vec::new());
            let call = crate::proto::Call::ListPrs {
                folder: project.folder,
            };
            self.call(project.host, call);
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
        // One of u0's own is done, not written.
        if let Offer::Own { name, .. } = offer {
            self.put_line("", window, cx);
            let key = self.current;
            match name {
                "clear" => self.new_default(window, cx),
                "model" => self.open_model(window, cx),
                "usage-limits" => self.open_tab(crate::pages::Tab::Usage, window, cx),
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
            Offer::Session { title, .. } => token_of(title),
            Offer::Pr { number, .. } => format!("@pr#{number}"),
            Offer::Issue { number, .. } => format!("@issue#{number}"),
        };
        let line = format!("{}{token} ", &text[..start]);
        self.put_line(&line, window, cx);
        if !matches!(offer, Offer::Command { .. }) {
            if let Offer::Pr { number, .. } = &offer
                && let Some(s) = self.session()
            {
                let call = crate::proto::Call::PrText {
                    dir: s.meta.project.clone(),
                    number: *number,
                };
                self.call(s.host, call);
            }
            self.composer.mentions.push(Mention {
                token,
                what: offer,
                text: None,
            });
        }
        cx.notify();
    }

    /// What is pasted in the composer, where it is not put in the line: an
    /// image, or text too long for it; whether it was taken.
    pub fn paste(
        &mut self,
        item: &gpui::ClipboardItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.begin_unbegun(window, cx);
        for e in item.entries() {
            match e {
                gpui::ClipboardEntry::ExternalPaths(p) => {
                    self.drop_files(p.paths(), window, cx);
                    return true;
                }
                gpui::ClipboardEntry::Image(i) => {
                    self.paste_picture(i, cx);
                    return true;
                }
                // Too long for the line, or for a message with what is in it.
                // Chips copied from a draft are chips again.
                gpui::ClipboardEntry::String(s) if s.metadata_json::<Copied>().is_some() => {
                    for (kind, text, note) in s.metadata_json::<Copied>().unwrap_or_default() {
                        if let Some(kind) = crate::session::Context::of(&kind) {
                            let mut chip = Chip::new(kind, &text);
                            chip.note = note;
                            self.add_attached(Attached::Text(chip), cx);
                        }
                    }
                    return true;
                }
                gpui::ClipboardEntry::String(s)
                    if s.text().len() >= LONG_PASTE
                        || s.text().chars().count() + self.typed(cx).chars().count() > LIMIT =>
                {
                    self.add_attached(
                        Attached::Text(Chip::new(crate::session::Context::Paste, s.text())),
                        cx,
                    );
                    return true;
                }
                gpui::ClipboardEntry::String(_) => {}
            }
        }
        false
    }

    /// An image pasted in the composer, of a kind an agent reads.
    fn paste_picture(&mut self, image: &gpui::Image, cx: &mut Context<Self>) {
        let Some(mime) = hub::agents_read(image.format) else {
            return;
        };
        self.add_attached(
            Attached::Image {
                mime: mime.into(),
                bytes: image.bytes.clone(),
            },
            cx,
        );
    }

    /// Something attached, unless it is an image and the agent said, when
    /// it last started, that it takes none; a file attached already is
    /// not attached twice.
    pub fn add_attached(&mut self, attached: Attached, cx: &mut Context<Self>) {
        let refused = matches!(attached, Attached::Image { .. })
            && self
                .session()
                .and_then(|s| self.slot(s.host, &s.meta.agent))
                .and_then(crate::hub::Slot::caps)
                .is_some_and(|known| !known.prompt_capabilities.image);
        let twice = matches!(&attached, Attached::File(p)
            if self.composer.draft.attached.iter().any(|a| matches!(a, Attached::File(q) if q == p)));
        if refused {
            let name = self
                .session()
                .map(|s| self.name_of(&s.meta.agent))
                .unwrap_or_default();
            self.trouble(
                crate::tr_with("%{name} does not take images", &[("name", &name)]),
                None,
                None,
            );
        } else if !twice {
            self.composer.draft.attached.push(attached);
        }
        cx.notify();
    }

    /// Quote `text` (something the agent said; the item `from` of a
    /// session's thread) in what you are writing, to answer it.
    pub fn quote(
        &mut self,
        text: &str,
        from: Option<(u64, usize)>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let mut chip = Chip::new(crate::session::Context::Quote, text.trim());
        chip.from = from;
        self.add_chip(chip, window, cx);
    }

    /// Put `text` beside what you are writing, as a chip named `kind`, and
    /// go to the line to say what to do with it.
    pub fn add_context(
        &mut self,
        kind: crate::session::Context,
        text: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.add_chip(Chip::new(kind, text), window, cx);
    }

    /// Put `chip` beside what you are writing, and go to the line.
    fn add_chip(&mut self, chip: Chip, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.add_attached(Attached::Text(chip), cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    /// Take out what was attached `n`th.
    pub fn detach(&mut self, n: usize, cx: &mut Context<Self>) {
        if n < self.composer.draft.attached.len() {
            self.composer.draft.attached.remove(n);
        }
        self.composer.noting = None;
        self.composer.previewing_chip = None;
        cx.notify();
    }

    /// Toggle the excerpt preview card for the attached chip `n`.
    pub fn toggle_chip_preview(&mut self, n: usize, cx: &mut Context<Self>) {
        if self.composer.previewing_chip == Some(n) {
            self.composer.previewing_chip = None;
        } else if matches!(self.composer.draft.attached.get(n), Some(Attached::Text(_))) {
            self.composer.previewing_chip = Some(n);
        }
        cx.notify();
    }

    /// Begin writing your note on the chip attached `n`th, or stop.
    pub fn begin_note(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Attached::Text(chip)) = self.composer.draft.attached.get(n) else {
            return;
        };
        if self.composer.noting == Some(n) {
            self.composer.noting = None;
        } else {
            let note = chip.note.clone();
            self.composer.noting = Some(n);
            self.composer.note.update(cx, |i, cx| {
                i.set_value(note, window, cx);
                window.focus(&gpui::Focusable::focus_handle(&*i, cx), cx);
            });
        }
        cx.notify();
    }

    /// Keep the note written on the chip being noted, and go back to the
    /// line.
    pub(crate) fn keep_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let note = self.composer.note.read(cx).value().trim().to_owned();
        if let Some(n) = self.composer.noting.take()
            && let Some(Attached::Text(chip)) = self.composer.draft.attached.get_mut(n)
        {
            chip.note = note;
        }
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    /// Go back to where in its thread the chip `n` was quoted from.
    pub fn go_back(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Attached::Text(Chip {
            from: Some((key, item)),
            ..
        })) = self.composer.draft.attached.get(n)
        else {
            return;
        };
        let (key, item) = (*key, *item);
        self.bring(key, window, cx);
        self.reveal_item(item, cx);
    }

    /// Copy the chips of the draft on screen, to paste in another.
    pub fn copy_chips(&mut self, cx: &mut Context<Self>) {
        let chips: Copied = (self.composer.draft.attached.iter())
            .filter_map(|a| match a {
                Attached::Text(c) => Some((c.kind.uri(0), c.text.clone(), c.note.clone())),
                _ => None,
            })
            .collect();
        let text = (chips.iter().map(|c| c.1.as_str()))
            .collect::<Vec<_>>()
            .join("\n\n");
        cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_json_metadata(
            text, chips,
        ));
        self.done(crate::tr("Chips copied: paste them in another draft"), None);
        cx.notify();
    }

    /// The draft of the session left goes with it (what is attached, the
    /// agents it also runs on), and the session shown brings back its own.
    pub(crate) fn swap_drafts(&mut self, from: Option<u64>, to: u64) {
        let c = &mut self.composer;
        swap_draft(&mut c.draft, &mut c.drafts, from, to);
        c.noting = None;
        c.previewing_chip = None;
    }

    /// A session dragged onto the composer: named in what you write, as
    /// `@` names one, so what was said in it goes with it.
    pub fn drop_session(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.sessions.at(key).map(|s| s.meta.title.clone()) else {
            return;
        };
        self.name_in_line(token_of(&title), Offer::Session { key, title }, window, cx);
        window.focus(
            &gpui::Focusable::focus_handle(self.composer.input.read(cx), cx),
            cx,
        );
    }

    /// Files dropped on the composer or chosen for it: an image is read
    /// (a photo in HEIC, or one too large, made a JPEG that fits), any
    /// other file is attached, a folder is named as @ names one.
    pub fn drop_files(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        self.begin_unbegun(window, cx);
        for p in paths {
            if p.is_dir() {
                self.name_dropped(p, window, cx);
            } else if hub::is_picture(p) {
                self.composer.reading += 1;
                hub::read_picture(p.clone(), self.tx.clone());
            } else {
                self.add_attached(Attached::File(p.clone()), cx);
            }
        }
    }

    /// Capture window `id` (or let you choose one interactively when `None`)
    /// to go with what you write.
    pub fn capture(&mut self, id: Option<u32>, cx: &mut Context<Self>) {
        self.popup = crate::desk::Popup::Closed;
        self.composer.reading += 1;
        hub::capture(
            id.map_or(hub::Shot::Chosen, hub::Shot::Window),
            self.tx.clone(),
        );
        cx.notify();
    }

    /// Name the file or folder `p` in what you are writing, as `@` would:
    /// from the session's folder where it is in it, else whole.
    fn name_dropped(&mut self, p: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let named = self
            .session()
            .and_then(|s| p.strip_prefix(&s.meta.folder).ok())
            .map_or_else(|| p.display().to_string(), |r| r.display().to_string());
        self.name_in_line(format!("@{named}"), Offer::File(named), window, cx);
    }

    /// Name `what` in what you write by `token`, after what is typed, as
    /// `@` names it.
    fn name_in_line(
        &mut self,
        token: String,
        what: Offer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let typed = self.typed(cx);
        let line = if typed.is_empty() {
            format!("{token} ")
        } else {
            format!("{typed} {token} ")
        };
        self.put_line(&line, window, cx);
        self.composer.mentions.push(Mention {
            token,
            what,
            text: None,
        });
        cx.notify();
    }

    /// What goes with `text` said in the session `key`: what is attached,
    /// and each file, session and pull request it names.
    pub(crate) fn attach(&mut self, key: u64, text: &str) -> Vec<ContentBlock> {
        // An agent on another server cannot read this Mac's files: what
        // they hold goes with what is said.
        let here = self
            .sessions
            .at(key)
            .is_none_or(|s| s.host == hub::Host::HERE);
        let mut blocks: Vec<ContentBlock> = (self.composer.draft.attached.drain(..).enumerate())
            .filter_map(|(n, a)| match a {
                Attached::Image { mime, bytes } => Some(ContentBlock::Image(ImageContent::new(
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
                    mime,
                ))),
                Attached::File(path) if !here => embedded(&path),
                Attached::File(path) => {
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    let uri = url::Url::from_file_path(&path).ok()?;
                    Some(ContentBlock::ResourceLink(ResourceLink::new(
                        name,
                        uri.to_string(),
                    )))
                }
                Attached::Text(chip) => Some(text_resource(&chip.kind.uri(n + 1), &chip.said())),
            })
            .collect();
        let Some(s) = self.sessions.at(key) else {
            return blocks;
        };
        let folder = s.meta.folder.clone();
        // What `@` names on another server is what it listed there.
        let listed = self.files.get(&super::Place::folder_of(s));
        let named = |f: &str| {
            if here {
                folder.join(f).is_file()
            } else {
                listed.is_some_and(|l| l.iter().any(|x| x == f))
            }
        };
        let mentions = std::mem::take(&mut self.composer.mentions);
        for m in mentions.iter().filter(|m| text.contains(&m.token)) {
            match &m.what {
                Offer::File(f) => blocks.extend(link(&folder, f)),
                Offer::Session { key, title } => {
                    if let Some(other) = self.sessions.at(*key) {
                        blocks.push(text_resource(
                            &format!("u0://session/{key}"),
                            &format!("The thread “{title}”:\n\n{}", said_in(other)),
                        ));
                    }
                }
                Offer::Pr { number, .. } => {
                    if let Some(t) = &m.text {
                        blocks.push(text_resource(&format!("u0://pr/{number}"), t));
                    }
                }
                Offer::Issue { number, title } => {
                    blocks.push(text_resource(
                        &format!("u0://issue/{number}"),
                        &format!("Issue #{number}: {title}"),
                    ));
                }
                Offer::Command { .. } | Offer::Own { .. } | Offer::Model { .. } => {}
            }
        }
        // A path typed by hand after `@` that names a file goes too.
        for word in text.split_whitespace() {
            if let Some(f) = word.strip_prefix('@')
                && !mentions.iter().any(|m| m.token == word)
                && named(f)
            {
                blocks.extend(link(&folder, f));
            }
        }
        blocks
    }
}

/// How a session titled `title` is named in what you write, after `@`.
pub(crate) fn token_of(title: &str) -> String {
    format!("@{}", crate::ui::fit(title, 32).replace(' ', "-"))
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

/// The most of a file of this Mac's sent whole with what is said to an
/// agent on another server.
const EMBEDDED: u64 = 10 * 1024 * 1024;

/// The file at `path`, whole, as a resource of its own: text as text, the
/// rest in base64; none where it does not read or is larger than
/// `EMBEDDED`.
fn embedded(path: &Path) -> Option<ContentBlock> {
    if std::fs::metadata(path).ok()?.len() > EMBEDDED {
        tracing::warn!("{} is too large to send to another server", path.display());
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let uri = url::Url::from_file_path(path).ok()?.to_string();
    Some(match String::from_utf8(bytes) {
        Ok(text) => text_resource(&uri, &text),
        Err(e) => {
            let blob =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, e.as_bytes());
            ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::BlobResourceContents(BlobResourceContents::new(
                    blob, uri,
                )),
            ))
        }
    })
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
            self.done(crate::tr("The conversation is copied"), None);
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
    fn a_file_named_with_a_space_and_a_hash_is_linked_by_a_uri_that_reads_back() {
        let Some(ContentBlock::ResourceLink(l)) = link(Path::new("/tmp/my project"), "a #1.rs")
        else {
            panic!("not a link");
        };
        assert_eq!(l.uri, "file:///tmp/my%20project/a%20%231.rs");
        let back = url::Url::parse(&l.uri).unwrap().to_file_path().unwrap();
        assert_eq!(back, Path::new("/tmp/my project/a #1.rs"));
    }

    #[test]
    fn a_draft_without_worktrees_runs_on_its_own_agent_alone() {
        let mut draft = Draft {
            also_run: vec![("codex".into(), None)],
            ..Draft::default()
        };
        draft.keep_apart(true);
        assert_eq!(draft.also_run.len(), 1);
        draft.keep_apart(false);
        assert!(draft.also_run.is_empty());
    }

    #[test]
    fn a_draft_holds_what_was_typed_attached_or_begun_about_and_not_its_choices() {
        let mut s = Session::new(1, "a", "/p".into());
        let mut d = Draft {
            also_run: vec![("b".into(), None)],
            in_background: true,
            ..Draft::default()
        };
        assert!(!holds(&s, "  \n", Some(&d)), "choices alone are not lost");
        assert!(holds(&s, "half written", None));
        d.attached.push(Attached::Text(Chip::new(
            crate::session::Context::Paste,
            "pasted",
        )));
        assert!(holds(&s, "", Some(&d)));
        s.begin = Some(crate::session::Begin::Pr(2, "p".into()));
        assert!(holds(&s, "", None));
    }

    #[test]
    fn the_agent_a_draft_is_switched_to_is_no_longer_one_it_also_runs_on() {
        let mut d = Draft {
            also_run: vec![("b".into(), None), ("c".into(), Some("m".into()))],
            ..Draft::default()
        };
        d.leads("b");
        assert_eq!(d.also_run, vec![("c".into(), Some("m".into()))]);
    }

    #[test]
    fn a_new_draft_in_another_project_runs_on_no_agent_chosen_for_the_last() {
        let (mut on, mut drafts) = (Draft::default(), HashMap::new());
        // Draft 1, in one project: also run on two more agents.
        swap_draft(&mut on, &mut drafts, None, 1);
        on.also_run = vec![
            ("codex".into(), None),
            ("gemini".into(), Some("pro".into())),
        ];
        on.in_background = true;
        // A new draft (2), in another project: it starts on its own agent alone.
        swap_draft(&mut on, &mut drafts, Some(1), 2);
        assert!(on.also_run.is_empty());
        assert!(!on.in_background);
        // Back on draft 1, its choice is as it was left.
        swap_draft(&mut on, &mut drafts, Some(2), 1);
        assert_eq!(on.also_run.len(), 2);
        assert!(on.in_background);
    }
}
