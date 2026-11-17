//! Going about the desk: searching the sessions, moving between them and
//! back and forward through them, showing or hiding the sidebar, and
//! opening a character from the left and closing her again.

use gpui::{ClipboardItem, Context, Focusable as _, Window};

use super::{Desk, Notice, Open, Page, Popup, Watched};
use crate::{
    daemons::Host,
    link,
    settings::{self, Session},
    thread,
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
        if let Some(&i) = settings::matching(&self.settings.sessions, query).first() {
            self.choose(i, cx);
        }
        self.close_search(window, cx);
    }

    pub fn new_session(&mut self, cx: &mut Context<Self>) {
        self.close_character(cx);
        self.page = Page::Thread;
        let was = self.session().id;
        // An untouched new session is reused rather than stacked.
        if let Some(i) = self
            .settings
            .sessions
            .iter()
            .position(|s| s.title.is_empty())
        {
            self.current = i;
        } else {
            self.settings.sessions.insert(0, Session::new());
            self.current = 0;
            self.settings.save();
        }
        self.visited(was);
        self.follow();
        cx.notify();
    }

    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        self.close_character(cx);
        self.page = Page::Thread;
        let was = self.session().id;
        self.current = index.min(self.settings.sessions.len().saturating_sub(1));
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
        let Some(session) = self.settings.sessions.iter().find(|s| s.id == id) else {
            return;
        };
        let title = session.title.clone();
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
        let name = name.trim();
        if let Some(id) = self.renaming.take()
            && !name.is_empty()
            && let Some(session) = self.settings.sessions.iter_mut().find(|s| s.id == id)
        {
            session.title = name.chars().take(80).collect();
            self.settings.save();
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
            abilities: None,
            expanded: &self.expanded,
            following: &self.solver.following,
            work: &self.solver.work,
            whom: crate::answer::Whom::Solver,
            scroll: None,
            rendering: thread::Rendering::Task,
        };
        let (text, lines) = thread::transcript(&view);
        if lines == 0 {
            self.notice = Some(Notice::said("Nothing has been said in that session yet."));
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.notify();
    }

    /// Take a session off the desk's list. What was said in it stays in
    /// Ghost's journal, which keeps everything; only the desk's way to it
    /// goes.
    pub(crate) fn delete_session(&mut self, id: &str, cx: &mut Context<Self>) {
        self.popup = Popup::Closed;
        let Some(i) = self.index_of(id) else { return };
        self.settings.sessions.remove(i);
        if self.settings.sessions.is_empty() {
            self.settings.sessions.push(Session::new());
        }
        if i < self.current || self.current >= self.settings.sessions.len() {
            self.current = self.current.saturating_sub(1);
        }
        self.settings.save();
        cx.notify();
    }

    /// Remember the session just left, if it was left, for back.
    fn visited(&mut self, was: String) {
        if was != self.session().id {
            self.back.push(was);
            self.forward.clear();
        }
        // Another thread opens at its end, where its latest words are.
        self.thread_scroll.scroll_to_bottom();
    }

    /// Where a session id is now.
    fn index_of(&self, id: &str) -> Option<usize> {
        self.settings.sessions.iter().position(|s| s.id == id)
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
            if let Some(i) = self.index_of(&id) {
                self.forward.push(self.session().id);
                self.current = i;
                cx.notify();
                return;
            }
        }
    }

    /// Forward again, after going back.
    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        self.close_character(cx);
        while let Some(id) = self.forward.pop() {
            if let Some(i) = self.index_of(&id) {
                self.back.push(self.session().id);
                self.current = i;
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
