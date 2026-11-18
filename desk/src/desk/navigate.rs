//! Going about the desk: searching the sessions, moving between them and
//! back and forward through them, showing or hiding the sidebar, and
//! opening a character from the left and closing her again.

use gpui::{ClipboardItem, Context, Focusable as _, Window};

use super::{Desk, Notice, Open, Page, Popup, Watched};
use crate::{
    daemons::Host,
    link::{self, Order},
    sessions, thread,
};

impl Desk {
    /// Turn the Search row into a field and type into it.
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.searching = true;
        self.search_typed = false;
        self.search.focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Close the search, show every session again, and go back to the line
    /// you type into.
    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.searching = false;
        self.search_typed = false;
        self.search.update(cx, |search, cx| search.set_text("", cx));
        self.input.focus_handle(cx).focus(window);
        cx.notify();
    }

    pub(super) fn search_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let empty = self.search.read(cx).is_empty();
        if empty && self.search_typed {
            self.close_search(window, cx);
            return;
        }
        self.search_typed = !empty;
        cx.notify();
    }

    /// Return in the search opens the first session it found.
    pub(super) fn open_first_match(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let all = self.sessions.all();
        if let Some(&i) = sessions::matching(&all, query).first() {
            self.choose(&all[i].id, cx);
        }
        self.close_search(window, cx);
    }

    pub fn new_session(&mut self, cx: &mut Context<Self>) {
        self.close_character(cx);
        self.page = Page::Thread;
        let was = self.session().id;
        // An untouched new session is reused rather than stacked.
        self.sessions.begin();
        self.visited(was);
        self.follow();
        cx.notify();
    }

    pub fn choose(&mut self, id: &str, cx: &mut Context<Self>) {
        self.close_character(cx);
        self.page = Page::Thread;
        let was = self.session().id;
        self.sessions.choose(id);
        self.visited(was);
        self.follow();
        cx.notify();
    }

    /// Open a session's menu, or close it when it is the one open.
    pub(crate) fn toggle_session_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        self.popup = if self.popup == Popup::Session(id.to_owned()) {
            Popup::Closed
        } else {
            Popup::Session(id.to_owned())
        };
        cx.notify();
    }

    /// Open the menu under the session's name in the top bar, or close it.
    pub(crate) fn toggle_title_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        self.popup = if self.popup == Popup::Title(id.to_owned()) {
            Popup::Closed
        } else {
            Popup::Title(id.to_owned())
        };
        cx.notify();
    }

    /// Turn a session's row into a field with its name in it, selected.
    pub(crate) fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.all().into_iter().find(|s| s.id == id) else {
            return;
        };
        let title = session.title;
        self.popup = Popup::Closed;
        self.renaming = Some(id.to_owned());
        self.rename.update(cx, |field, cx| {
            field.set_text(&title, cx);
            field.select_everything(cx);
        });
        self.rename.focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Give the session being renamed the name typed, unless it is blank,
    /// and put the row back.
    pub(crate) fn finish_rename(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The name goes to the core, which keeps it for every desk; the
        // list shows it at once.
        if let Some(id) = self.renaming.take()
            && let Some(title) = self.sessions.rename(&id, name)
        {
            self.solver.order(Order::Rename { place: id, title });
        }
        self.input.focus_handle(cx).focus(window);
        cx.notify();
    }

    pub(crate) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = None;
        self.input.focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Put one message on the clipboard. Quietly: a copy needs no line
    /// under the box to say it happened.
    pub(crate) fn copy_text(text: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    }

    /// Put a session's thread on the clipboard as text: who said what, and
    /// when, from Ghost's journal.
    pub(crate) fn copy_session(&mut self, id: &str, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let place = thread::place_key(id);
        let view = thread::View {
            journal: &self.solver.journal,
            pending: &self.solver.pending,
            stops: &self.solver.stops,
            place: &place,
            name: self.solver.name(),
            who: self.settings.who.clone(),
            tz: self.solver.tz(),
            connected: true,
            expanded: &self.expanded,
            following: &self.solver.following,
            work: &self.solver.work,
            whom: crate::answer::Whom::Solver,
            scroll: None,
            rendering: thread::Rendering::Task,
            column: thread::COLUMN,
            media: self.solver.media(),
            status: self.solver.status.as_ref(),
        };
        let (text, lines) = thread::transcript(&view);
        if lines == 0 {
            self.notice = Some(Notice::said("Nothing has been said in that session yet."));
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.notify();
    }

    /// Take a session off the list: its core sets it aside, for every
    /// desk. What was said in it stays in Ghost's journal, which keeps
    /// everything; only the way to it goes.
    pub(crate) fn delete_session(&mut self, id: &str, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let was = self.session().id;
        if self.sessions.remove(id) {
            self.solver.order(Order::SetAside {
                place: id.to_owned(),
            });
        }
        if self.session().id != was {
            self.remember_open();
            self.follow();
        }
        cx.notify();
    }

    /// Remember the session just left, if it was left, for back; and the
    /// one on screen, for next time.
    fn visited(&mut self, was: String) {
        if was != self.session().id {
            self.back.push(was);
            self.forward.clear();
            self.remember_open();
        }
        // Another thread opens at its end, where its latest words are.
        self.thread_scroll.scroll_to_bottom();
    }

    /// Keep which session is on screen in `desk.json`, to open on it
    /// next time.
    fn remember_open(&mut self) {
        let id = self.session().id;
        if self.settings.open.as_deref() != Some(id.as_str()) {
            self.settings.open = Some(id);
            self.settings.save();
        }
    }

    /// Back to the session before, skipping any that are gone. From a
    /// character, back is the session behind her.
    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.attending() {
            self.page = Page::Thread;
            cx.notify();
            return;
        }
        if self.open.is_some() {
            self.close_character(cx);
            return;
        }
        while let Some(id) = self.back.pop() {
            let here = self.session().id;
            if self.sessions.choose(&id) {
                self.forward.push(here);
                self.remember_open();
                self.follow();
                cx.notify();
                return;
            }
        }
    }

    /// Forward again, after going back.
    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        self.close_character(cx);
        while let Some(id) = self.forward.pop() {
            let here = self.session().id;
            if self.sessions.choose(&id) {
                self.back.push(here);
                self.remember_open();
                self.follow();
                cx.notify();
                return;
            }
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.panes.left = !self.panes.left;
        cx.notify();
    }

    pub fn toggle_aside(&mut self, cx: &mut Context<Self>) {
        self.panes.right = !self.panes.right;
        cx.notify();
    }

    /// Open the character `id` of the daemon `host`: follow her on a link
    /// of her own, and show her.
    pub(crate) fn open_character(&mut self, host: &Host, id: &str, cx: &mut Context<Self>) {
        self.page = Page::Thread;
        if self
            .open
            .as_ref()
            .is_some_and(|o| o.host == *host && o.id == id)
        {
            cx.notify();
            return;
        }
        let Some(daemon) = self.daemons.iter().find(|d| d.host == *host) else {
            return;
        };
        let Ok(token) = daemon.token.clone() else {
            return;
        };
        let name = daemon
            .cast
            .ghosts
            .iter()
            .find(|g| g.id == id)
            .map_or_else(|| id.to_owned(), |g| g.card.name.clone());
        let her = Watched::start(
            link::Config {
                url: daemon.url.clone(),
                token,
                ghost: Some(id.to_owned()),
                who: self.settings.who.clone(),
                sessions: false,
            },
            &name,
        );
        self.open = Some(Open {
            host: host.clone(),
            id: id.to_owned(),
            url: daemon.url.clone(),
            her,
            place: None,
            focus: None,
        });
        self.popup = Popup::Closed;
        let placeholder = self.placeholder();
        self.input
            .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
        self.follow();
        cx.notify();
    }

    /// Back to the solver: stop following the character on screen.
    pub(crate) fn close_character(&mut self, cx: &mut Context<Self>) {
        if self.open.take().is_some() {
            self.popup = Popup::Closed;
            let placeholder = self.placeholder();
            self.input
                .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
            cx.notify();
        }
    }

    /// Show the place `key` in her thread.
    pub(crate) fn choose_place(&mut self, key: String, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.open {
            open.place = Some(key);
        }
        self.popup = Popup::Closed;
        self.follow();
        cx.notify();
    }
}
