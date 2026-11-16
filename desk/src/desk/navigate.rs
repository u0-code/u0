//! Going about the desk: searching the sessions, moving between them and
//! back and forward through them, showing or hiding the sidebar, and
//! opening a character from the left and closing her again.

use gpui::{Context, Focusable as _, Window};

use super::{Desk, Open, Page, Popup, Watched};
use crate::{
    daemons::Host,
    link,
    settings::{self, Session},
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
        cx.notify();
    }

    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        self.close_character(cx);
        self.page = Page::Thread;
        let was = self.session().id;
        self.current = index.min(self.settings.sessions.len().saturating_sub(1));
        self.visited(was);
        cx.notify();
    }

    /// Remember the session just left, if it was left, for back.
    fn visited(&mut self, was: String) {
        if was != self.session().id {
            self.back.push(was);
            self.forward.clear();
        }
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
        self.sidebar = !self.sidebar;
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
        cx.notify();
    }
}
