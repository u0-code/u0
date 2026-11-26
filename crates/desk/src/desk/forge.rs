//! The PRs and Issues pages, as t3code has them: a project's pull requests
//! or issues, or every project's in one list, listed by state, by your
//! part in them and by GitHub's search;
//! one read whole beside the list; and what is done to one, as `gh` does
//! it (`core::github`).

use gpui::component::{
    IndexPath,
    diff::{DiffFile, DiffState},
    input::{InputEvent, InputState, TextareaState},
    list::{ListDelegate, ListItem, ListState},
};
use gpui::{
    App, AppContext as _, Context, Entity, ParentElement as _, Styled as _, WeakEntity, Window, px,
};

use super::{Desk, Page};
use crate::{
    github::{Act, Answer, Ask, Detail, Item, Kind},
    proto::Call,
    session::State,
    store::Link,
};

/// A page's tab for the one read: what it is, what was said, its changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Summary,
    Timeline,
    Code,
    Threads,
    Guide,
}

/// Your part in them, as GitHub's search says it.
pub const PARTS: [(&str, &str); 5] = [
    ("All", ""),
    ("Mine", "author:@me"),
    ("Reviewing", "review-requested:@me"),
    ("Assigned", "assignee:@me"),
    ("Mentions", "mentions:@me"),
];

/// A page of pull requests or issues: which, of which project, in which
/// state and by your part in them; the line to search with; what was
/// listed; the one read, its tab, what was said of it oldest first or
/// not, and its changes; the lines to say something, to name a label or a
/// person, to edit its title and words or write a new issue; and what is
/// being done to it.
pub struct Forge {
    pub kind: Kind,
    /// The project of the one read, and in one project's list its own.
    pub folder: Option<super::Place>,
    /// Where every project's are listed, not only `folder`'s: what each
    /// project was answered.
    pub every: Option<Vec<(super::Place, Listed)>>,
    pub state: &'static str,
    pub part: usize,
    pub search: Entity<InputState>,
    /// What was listed, drawn as gpui-kit's List.
    pub rows: Entity<ListState<Rows>>,
    pub chosen: Option<u64>,
    pub detail: Option<Result<Box<Detail>, String>>,
    pub tab: Tab,
    pub newest: bool,
    pub diff: Entity<DiffState>,
    /// The patch of the one read, held until it is read: what you viewed
    /// of it is known by its address.
    pub patch: Option<String>,
    pub line: Entity<TextareaState>,
    pub who: Entity<InputState>,
    pub title: Entity<InputState>,
    pub body: Entity<TextareaState>,
    pub editing: bool,
    pub creating: bool,
    pub doing: Option<String>,
    /// The Linear team the project's issues are read from, as typed.
    pub team: Entity<InputState>,
    /// The project whose labels and people are offered, and those offered
    /// under the line to name one, as gpui-kit's List.
    pub candidates: Option<super::Place>,
    pub offered: Entity<ListState<Offered>>,
    /// What a failed run of the checks printed, by the run's id.
    pub log: Option<(String, Result<String, String>)>,
    /// The repository's templates for a new issue: name and body.
    pub templates: Vec<(String, String)>,
    /// The pull request read, as it is reviewed (`desk::pr`).
    pub pr: super::pr::Review,
}

/// What was listed: none while it is read.
pub type Listed = Option<Result<Vec<Item>, String>>;

/// The list of pull requests or issues, as gpui-kit's List asks for it:
/// what was listed (none while it is read), and the desk it is chosen on.
pub struct Rows {
    pub list: Listed,
    /// In the list of every project: each project, its name and how many
    /// of the list are its, in the list's order. Empty for one project's.
    pub groups: Vec<(super::Place, String, usize)>,
    desk: WeakEntity<Desk>,
    selected: Option<usize>,
}

impl Rows {
    /// Those listed, none while they are read.
    pub fn items(&self) -> &[Item] {
        match &self.list {
            Some(Ok(items)) => items,
            _ => &[],
        }
    }

    /// The project of the row `row` and its name (none in one project's
    /// list), and the rows of that project.
    pub fn kin(&self, row: usize) -> (Option<(&super::Place, &str)>, &[Item]) {
        let items = self.items();
        let mut start = 0;
        for (place, name, len) in &self.groups {
            let end = (start + len).min(items.len());
            if row < end {
                return (Some((place, name)), &items[start..end]);
            }
            start = end;
        }
        (None, items)
    }

    /// Those of the project `place`: all of them in one project's list.
    pub fn of(&self, place: Option<&super::Place>) -> &[Item] {
        let items = self.items();
        if self.groups.is_empty() {
            return items;
        }
        let mut start = 0;
        for (p, _, len) in &self.groups {
            let end = (start + len).min(items.len());
            if Some(p) == place {
                return &items[start..end];
            }
            start = end;
        }
        &[]
    }
}

/// The list of every project from what each was answered, `every`: the
/// pull requests or issues of those that answered, a project's together,
/// and how many are each's; none while none has answered, and the first
/// trouble where every one failed (a folder that is not a repository on a
/// forge fails, and is left out).
pub fn gather<P: Clone>(every: &[(P, Listed)]) -> (Listed, Vec<(P, usize)>) {
    let answered: Vec<_> = (every.iter())
        .filter_map(|(p, a)| a.as_ref().map(|a| (p, a)))
        .collect();
    let Some((_, first)) = answered.first() else {
        return (None, Vec::new());
    };
    let listed: Vec<_> = (answered.iter())
        .filter_map(|(p, a)| a.as_ref().ok().map(|items| (*p, items)))
        .collect();
    if listed.is_empty() && answered.len() == every.len() {
        return (Some((*first).clone()), Vec::new());
    }
    let items = listed.iter().flat_map(|(_, items)| items.iter().cloned());
    let groups = (listed.iter())
        .filter(|(_, items)| !items.is_empty())
        .map(|(p, items)| ((*p).clone(), items.len()))
        .collect();
    (Some(Ok(items.collect())), groups)
}

impl ListDelegate for Rows {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.items().len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let item = self.items().get(ix.row)?;
        let (project, kin) = self.kin(ix.row);
        let (under, _) = crate::pages::pr::stack(item, kin);
        Some(
            // A virtual list measures a row at its narrowest, where words
            // wrap: its height is said instead.
            ListItem::new(ix.row)
                .h(px(68.))
                .child(crate::pages::forge::row(
                    crate::fonts::faces(cx),
                    item,
                    under,
                    project.map(|(_, name)| name),
                )),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|ix| ix.row);
    }

    fn confirm(&mut self, _: bool, _: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(row) = self.selected else {
            return;
        };
        let Some(n) = self.items().get(row).map(|i| i.number) else {
            return;
        };
        let place = self.kin(row).0.map(|(p, _)| p.clone());
        if let Some(desk) = self.desk.upgrade() {
            cx.defer(move |cx| {
                desk.update(cx, |d, cx| {
                    if let Some(place) = place {
                        d.forge.folder = Some(place);
                    }
                    d.read_forge(n, cx);
                });
            });
        }
    }

    fn loading(&self, _: &App) -> bool {
        self.list.is_none()
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> impl gpui::IntoElement {
        use gpui::ParentElement as _;
        use gpui::component::empty::{Empty, EmptyDescription, EmptyHeader};
        let said = match &self.list {
            Some(Err(e)) => crate::tr_with("Couldn't list them: %{e}", &[("e", e)]),
            _ => crate::tr("None here."),
        };
        Empty::new().header(EmptyHeader::new().description(EmptyDescription::new().child(said)))
    }
}

/// The project's labels and the people who can review or be assigned (each
/// true for a label), offered under the line to name one as it is typed.
pub struct Offered {
    all: Vec<(bool, String)>,
    shown: Vec<usize>,
    selected: Option<usize>,
    desk: WeakEntity<Desk>,
}

impl ListDelegate for Offered {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> gpui::Task<()> {
        let typed = query.trim().trim_start_matches('@').to_lowercase();
        self.shown = (0..self.all.len())
            .filter(|n| !typed.is_empty() && self.all[*n].1.to_lowercase().contains(&typed))
            .take(CANDIDATES)
            .collect();
        gpui::Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.shown.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let (label, name) = self.all.get(*self.shown.get(ix.row)?)?;
        let word = if *label {
            crate::tr_with("Label %{name}", &[("name", name)]).into()
        } else {
            format!("@{name}")
        };
        Some(ListItem::new(ix.row).child(word))
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|ix| ix.row);
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some((label, name)) = (self.selected)
            .and_then(|ix| self.shown.get(ix))
            .and_then(|n| self.all.get(*n))
            .cloned()
        else {
            return;
        };
        let Some(desk) = self.desk.upgrade() else {
            return;
        };
        window.defer(cx, move |window, cx| {
            desk.update(cx, |d, cx| {
                let who = d.forge.who.clone();
                who.update(cx, |i, cx| i.set_value(name.clone(), window, cx));
                d.add_person_or_label(usize::from(!label), window, cx);
            });
        });
    }
}

/// How many labels and people are offered at once.
const CANDIDATES: usize = 12;

impl Forge {
    pub(super) fn new(home: &std::path::Path, window: &mut Window, cx: &mut Context<Desk>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::tr("Search: label:bug, author:someone, words"))
        });
        cx.subscribe(&search, |this: &mut Desk, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                this.list_forge(cx);
            }
        })
        .detach();
        let who = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::tr("A label, or someone's login"))
        });
        let offered = Offered {
            all: Vec::new(),
            shown: Vec::new(),
            selected: None,
            desk: cx.entity().downgrade(),
        };
        let offered = cx.new(|cx| ListState::new(offered, window, cx));
        let team = cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("Linear team")));

        cx.subscribe(&team, |this: &mut Desk, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                this.bind_linear(cx);
            }
        })
        .detach();
        let desk = cx.entity().downgrade();
        let rows = cx.new(|cx| {
            let rows = Rows {
                list: None,
                groups: Vec::new(),
                desk,
                selected: None,
            };
            ListState::new(rows, window, cx)
        });
        let pr = super::pr::Review::new(home, window, cx);
        Self {
            kind: Kind::Pr,
            folder: None,
            every: None,
            state: "open",
            part: 0,
            search,
            rows,
            chosen: None,
            detail: None,
            tab: Tab::Summary,
            newest: false,
            diff: cx.new(|cx| DiffState::new([], cx)),
            patch: None,
            line: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 10)
                    .placeholder(crate::tr("Say something"))
            }),
            who,
            title: cx.new(|cx| InputState::new(window, cx).placeholder(crate::tr("Title"))),
            body: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(4, 16)
                    .placeholder(crate::tr("What it is about"))
            }),
            editing: false,
            creating: false,
            doing: None,
            team,
            candidates: None,
            offered,
            log: None,
            templates: Vec::new(),
            pr,
        }
    }
}

impl Desk {
    /// Open the page of `kind`, on the project of the session on screen,
    /// or on every project's where it lists every project's or there is
    /// no session.
    pub fn open_forge(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        let all = self.forge.every.is_some();
        if self.forge.kind != kind || (self.forge.folder.is_none() && !all) {
            self.forge.kind = kind;
            if !all {
                self.forge.folder = self.chosen_project();
                self.forge.every = self.forge.folder.is_none().then(Vec::new);
            }
            self.forge.state = "open";
            self.forge.chosen = None;
            self.forge.detail = None;
            self.show_team(window, cx);
            self.list_forge(cx);
        }
        self.open_page(Page::Forge(kind), window, cx);
    }

    /// Read what the failed run `run` of the checks printed.
    pub(crate) fn read_log(&mut self, run: String, cx: &mut Context<Self>) {
        self.forge.log = Some((run.clone(), Ok(crate::tr("Reading…").into())));
        self.ask_forge(self.forge.kind, Ask::Log(run));
        cx.notify();
    }

    /// Begin the new issue's words with the template `n`.
    pub(crate) fn use_template(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, body)) = self.forge.templates.get(n) {
            let body = body.clone();
            self.forge
                .body
                .update(cx, |b, cx| b.set_value(body, window, cx));
        }
    }

    /// Ask `ask` of the page's project's pull requests or issues (`kind`),
    /// of its Linear team where it is bound to one.
    fn ask_forge(&self, kind: Kind, ask: Ask) {
        if let Some(p) = self.forge.folder.clone() {
            self.ask_forge_of(p, kind, ask);
        }
    }

    /// Ask `ask` of the project `p`'s pull requests or issues (`kind`).
    fn ask_forge_of(&self, p: super::Place, kind: Kind, ask: Ask) {
        let linear = self.prefs(&p).linear;
        let call = Call::Forge {
            folder: p.folder,
            kind,
            ask,
            linear,
        };
        self.call(p.host, call);
    }

    /// Ask for the list the page's choices pick.
    pub(crate) fn list_forge(&mut self, cx: &mut Context<Self>) {
        let words = self.forge.search.read(cx).value().trim().to_owned();
        let search = format!("{} {words}", PARTS[self.forge.part].1)
            .trim()
            .to_owned();
        self.forge.rows.update(cx, |r, cx| {
            r.delegate_mut().list = None;
            r.delegate_mut().groups.clear();
            cx.notify();
        });
        let state = self.forge.state.to_owned();
        let ask = Ask::List { state, search };
        if self.forge.every.is_some() {
            let projects = self.projects();
            self.forge.every = Some(projects.iter().map(|p| (p.clone(), None)).collect());
            for p in projects {
                self.ask_forge_of(p, self.forge.kind, ask.clone());
            }
        } else {
            self.ask_forge(self.forge.kind, ask);
        }
        cx.notify();
    }

    /// List every project's (`all`), or those of `folder`.
    pub(crate) fn list_projects(
        &mut self,
        all: bool,
        folder: Option<super::Place>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.forge.every = all.then(Vec::new);
        self.forge.folder = folder;
        self.forge.chosen = None;
        self.forge.detail = None;
        self.show_team(window, cx);
        self.list_forge(cx);
    }

    /// Show the view `v` saved.
    pub(crate) fn show_view(
        &mut self,
        v: &crate::settings::SavedView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let f = &mut self.forge;
        f.state = ["open", "closed", "merged", "all"]
            .into_iter()
            .find(|s| *s == v.state)
            .unwrap_or("open");
        f.part = PARTS.iter().position(|p| p.1 == v.part).unwrap_or(0);
        let search = v.search.clone();
        f.search.update(cx, |i, cx| i.set_value(search, window, cx));
        self.list_forge(cx);
    }

    /// Save the view shown, named by what it shows: its state, your part,
    /// and the words searched.
    pub(crate) fn save_view(&mut self, cx: &mut Context<Self>) {
        let f = &self.forge;
        let search = f.search.read(cx).value().trim().to_owned();
        let (part_name, part) = PARTS[f.part];
        let mut name = vec![crate::pages::forge::state_word(f.state).to_owned()];
        if !part.is_empty() {
            name.push(part_name.to_owned());
        }
        if !search.is_empty() {
            name.push(search.clone());
        }
        let view = crate::settings::SavedView {
            name: name.join(" · "),
            kind: f.kind,
            state: f.state.to_owned(),
            part: part.to_owned(),
            search,
        };
        let views = &mut self.settings.views;
        views.retain(|v| !(v.kind == view.kind && v.name == view.name));
        views.push(view);
        self.save_settings_on(crate::hub::Host::HERE);
        cx.notify();
    }

    pub(crate) fn remove_view(&mut self, name: &str, cx: &mut Context<Self>) {
        let kind = self.forge.kind;
        (self.settings.views).retain(|v| !(v.kind == kind && v.name == name));
        self.save_settings_on(crate::hub::Host::HERE);
        cx.notify();
    }

    /// Bind the page's project's issues to the Linear team typed (its key),
    /// or, with none, to its repository's host; and list them again.
    pub(crate) fn bind_linear(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.forge.folder.clone() else {
            return;
        };
        let team = self.forge.team.read(cx).value().trim().to_uppercase();
        (self.settings_on_mut(p.host).prefs)
            .entry(p.folder.clone())
            .or_default()
            .linear = (!team.is_empty()).then_some(team);
        self.save_settings_on(p.host);
        self.list_forge(cx);
    }

    /// Show the Linear team the page's project is bound to in its field.
    pub(crate) fn show_team(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let team = (self.forge.folder.as_ref()).and_then(|p| self.prefs(p).linear);
        self.forge.team.update(cx, |i, cx| {
            i.set_value(team.unwrap_or_default(), window, cx);
        });
    }

    /// Read the one numbered `n` whole, and a pull request's changes.
    pub fn read_forge(&mut self, n: u64, cx: &mut Context<Self>) {
        if self.forge.chosen != Some(n) {
            self.pr_changed();
        }
        self.forge.chosen = Some(n);
        self.forge.log = None;
        if self.forge.candidates.as_ref() != self.forge.folder.as_ref() {
            self.ask_forge(self.forge.kind, Ask::Candidates);
        }
        self.forge.patch = None;
        self.forge.detail = None;
        self.forge.editing = false;
        self.forge.creating = false;
        self.forge.diff.update(cx, |d, cx| d.set_files([], cx));
        let kind = self.forge.kind;
        self.ask_forge(kind, Ask::View(n));
        if kind == Kind::Pr {
            self.ask_forge(kind, Ask::Diff(n));
        }
        cx.notify();
    }

    /// Do `act` to the one read.
    pub fn act_forge(&mut self, act: Act, cx: &mut Context<Self>) {
        if let Some(n) = self.forge.chosen {
            self.act_on(n, act, cx);
        }
    }

    /// Do `act` to the one numbered `n`.
    pub fn act_on(&mut self, n: u64, act: Act, cx: &mut Context<Self>) {
        let name = format!("{act:?}");
        let name = name.split(['(', ' ', '{']).next().unwrap_or("");
        self.forge.doing = Some(crate::tr(name).into());
        self.ask_forge(self.forge.kind, Ask::Act(n, act));
        cx.notify();
    }

    /// Say what is on the line about the one read: a comment, or with a
    /// verdict a review.
    pub fn say_forge(
        &mut self,
        verdict: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let body = self.forge.line.read(cx).value().trim().to_owned();
        let lines = std::mem::take(&mut self.forge.pr.lines);
        if body.is_empty() && lines.is_empty() && verdict != Some("approve") {
            return;
        }
        self.forge
            .line
            .update(cx, |l, cx| l.set_value("", window, cx));
        // Comments on lines go out as a review, with the words beside them.
        let act = match (verdict, lines.is_empty()) {
            (None, true) => Act::Comment(body),
            (v, _) => Act::Review {
                verdict: v.unwrap_or("comment").to_owned(),
                body,
                lines,
            },
        };
        self.act_forge(act, cx);
    }

    /// Begin editing the one read's title and words, or a new issue.
    pub fn edit_forge(&mut self, new: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (title, body) = match (&self.forge.detail, new) {
            (Some(Ok(d)), false) => (d.item.title.clone(), d.body.clone()),
            _ => (String::new(), String::new()),
        };
        self.forge
            .title
            .update(cx, |i, cx| i.set_value(&title, window, cx));
        self.forge
            .body
            .update(cx, |i, cx| i.set_value(&body, window, cx));
        self.forge.editing = !new;
        self.forge.creating = new;
        if new {
            self.ask_forge(Kind::Issue, Ask::Templates);
        }
        cx.notify();
    }

    /// Save the title and words edited, or make the new issue.
    pub fn save_forge(&mut self, cx: &mut Context<Self>) {
        let title = self.forge.title.read(cx).value().trim().to_owned();
        let body = self.forge.body.read(cx).value().to_string();
        if title.is_empty() {
            return;
        }
        if self.forge.creating {
            self.forge.creating = false;
            self.forge.doing = Some(crate::tr("Create").into());
            self.ask_forge(Kind::Issue, Ask::Create { title, body });
        } else {
            self.forge.editing = false;
            self.act_forge(Act::Edit { title, body }, cx);
        }
        cx.notify();
    }

    /// Begin a session on the pull request `n`: on its own branch in a
    /// worktree, or (`here`) checked out where the project is.
    pub fn start_from_pr(
        &mut self,
        folder: super::Place,
        n: u64,
        title: String,
        here: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if here {
            self.forge.folder = Some(folder.clone());
            self.forge.chosen = Some(n);
            self.act_forge(Act::Checkout, cx);
        }
        self.new_in(folder, window, cx);
        if let Some(key) = self.current
            && let Some(s) = self.sessions.at_mut(key)
            && s.state == State::Draft
        {
            s.meta.links = vec![Link {
                kind: Kind::Pr,
                number: n,
                title: title.clone(),
            }];
            if !here {
                s.wants_worktree = true;
                s.begin = Some(crate::session::Begin::Pr(n, title));
            }
        }
    }

    /// Begin a session about the issue `n`: its number and title, and with
    /// `triage` what is asked of it, begin what you write.
    pub fn start_from_issue(
        &mut self,
        folder: super::Place,
        n: u64,
        title: &str,
        triage: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_in(folder, window, cx);
        if let Some(s) = self.current.and_then(|key| self.sessions.at_mut(key)) {
            s.meta.links = vec![Link {
                kind: Kind::Issue,
                number: n,
                title: title.to_owned(),
            }];
        }
        let line = if triage {
            format!(
                "Triage issue #{n}: {title}\n\nRead it (gh issue view {n} --comments), find where in the code it comes from and why, and say how to fix it and how big the fix is. Change nothing yet."
            )
        } else {
            format!("Issue #{n}: {title}\n\n")
        };
        self.put_line(&line, window, cx);
    }

    /// Link `link` to the session `key`, or unlink it where it was.
    pub fn link(&mut self, key: u64, link: Link) {
        let Some(s) = self.sessions.at_mut(key) else {
            return;
        };
        if let Some(at) = s.meta.links.iter().position(|l| *l == link) {
            s.meta.links.remove(at);
        } else {
            s.meta.links.push(link);
        }
        self.save_index();
    }

    /// Show the pull request `n` of the session's project on the PRs page,
    /// when the session works in a repository: true when it does.
    pub fn show_pr(&mut self, n: u64, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.show_item(Kind::Pr, n, window, cx)
    }

    /// Show the pull request or issue `n` of the session's project on its
    /// page, when the session works in a repository: true when it does.
    pub fn show_item(
        &mut self,
        kind: Kind,
        n: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(folder) = self
            .session()
            .filter(|s| s.repo)
            .map(super::Place::project_of)
        else {
            return false;
        };
        self.forge.folder = Some(folder);
        self.forge.every = None;
        self.forge.kind = kind;
        self.forge.state = "all";
        self.list_forge(cx);
        self.read_forge(n, cx);
        self.open_page(Page::Forge(kind), window, cx);
        true
    }

    /// What came back of a project's pull requests or issues.
    pub(super) fn forge_answered(
        &mut self,
        folder: &super::Place,
        kind: Kind,
        answer: Answer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.forge.kind != kind {
            return;
        }
        if let (true, Answer::Listed(list)) = (self.forge.every.is_some(), &answer) {
            return self.every_answered(folder, list.clone(), window, cx);
        }
        if self.forge.folder.as_ref() != Some(folder) {
            return;
        }
        match answer {
            Answer::Listed(list) => {
                let at = (list.as_ref().ok())
                    .and_then(|l| l.iter().position(|i| Some(i.number) == self.forge.chosen));
                self.forge.rows.update(cx, |r, cx| {
                    r.delegate_mut().list = Some(list);
                    r.set_selected_index(at.map(|row| IndexPath::default().row(row)), window, cx);
                    cx.notify();
                });
            }
            Answer::Viewed(n, detail) if self.forge.chosen == Some(n) => {
                self.forge.detail = Some(detail);
                if let Some(patch) = self.forge.patch.take() {
                    self.pr_patch(&patch, cx);
                }
            }
            Answer::Diffed(n, Ok(patch)) if self.forge.chosen == Some(n) => {
                let parsed = DiffFile::parse(&patch).unwrap_or_default();
                self.forge.diff.update(cx, |d, cx| d.set_files(parsed, cx));
                // What you viewed is known by the pull request's address,
                // once it is read.
                if matches!(self.forge.detail, Some(Ok(_))) {
                    self.pr_patch(&patch, cx);
                } else {
                    self.forge.patch = Some(patch);
                }
            }
            Answer::Candidates(Ok((labels, people))) => {
                self.forge.candidates = Some(folder.clone());
                let all = (labels.into_iter().map(|l| (true, l)))
                    .chain(people.into_iter().map(|p| (false, p)))
                    .collect();
                self.forge
                    .offered
                    .update(cx, |o, _| o.delegate_mut().all = all);
            }
            Answer::Logged(run, log) => self.forge.log = Some((run, log)),
            Answer::Templates(templates) => self.forge.templates = templates,
            Answer::Acted(n, result) => {
                let what = self.forge.doing.take().unwrap_or_default();
                match result {
                    Ok(said) => self.done(
                        match said.lines().last() {
                            Some(l) => crate::tr_with(
                                "%{what}: done — %{said}",
                                &[("what", &what), ("said", &l)],
                            ),
                            None => crate::tr_with("%{what}: done", &[("what", &what)]),
                        },
                        None,
                    ),
                    Err(e) => self.trouble(
                        crate::tr_with("%{what} didn't go through", &[("what", &what)]),
                        Some(e),
                        None,
                    ),
                }
                // Read again what it did.
                self.list_forge(cx);
                if let Some(n) = n.or(self.forge.chosen) {
                    self.read_forge(n, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

impl Desk {
    /// What `folder` answered in the list of every project.
    fn every_answered(
        &mut self,
        folder: &super::Place,
        list: Result<Vec<Item>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(every) = self.forge.every.as_mut() else {
            return;
        };
        let Some(at) = every.iter().position(|(p, _)| p == folder) else {
            return;
        };
        every[at].1 = Some(list);
        let (list, groups) = gather(every);
        let groups: Vec<_> = (groups.into_iter())
            .map(|(p, len)| {
                let name = self.project_on(&p);
                (p, name, len)
            })
            .collect();
        // The one read stays chosen where it is still listed.
        let chosen = (self.forge.folder.clone()).zip(self.forge.chosen);
        let mut start = 0;
        let mut at = None;
        for (p, _, len) in &groups {
            if let (Some((folder, n)), Some(Ok(items))) = (&chosen, &list)
                && folder == p
            {
                at = (items[start..start + len].iter())
                    .position(|i| i.number == *n)
                    .map(|row| start + row);
            }
            start += len;
        }
        self.forge.rows.update(cx, |r, cx| {
            r.delegate_mut().list = list;
            r.delegate_mut().groups = groups;
            r.set_selected_index(at.map(|row| IndexPath::default().row(row)), window, cx);
            cx.notify();
        });
        cx.notify();
    }

    /// Keep `token` for `host`, or, with none, take it out of the Keychain.
    pub(crate) fn set_token(
        &mut self,
        host: crate::forge::Host,
        token: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(seen) = self.pages.forges.iter_mut().find(|s| s.host == host) {
            seen.token = token.is_some();
        }
        let call = Call::Secret {
            account: host.account(),
            value: token,
        };
        self.call(self.pages.on, call);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(number: u64) -> Item {
        Item {
            number,
            title: format!("#{number}"),
            state: "open".into(),
            draft: false,
            author: String::new(),
            labels: Vec::new(),
            updated: String::new(),
            url: String::new(),
            branch: String::new(),
            base: String::new(),
            review: String::new(),
            checks: (0, 0, 0),
            comments: 0,
        }
    }

    fn numbers(list: Listed) -> Vec<u64> {
        list.unwrap().unwrap().iter().map(|i| i.number).collect()
    }

    #[test]
    fn every_projects_list_holds_each_projects_together_and_leaves_out_folders_off_a_forge() {
        let mut every = vec![
            ("a", Some(Ok(vec![item(1), item(2)]))),
            ("plain folder", Some(Err("not a GitHub repository".into()))),
            ("b", None),
        ];
        let (list, groups) = gather(&every);
        assert_eq!(numbers(list), [1, 2]);
        assert_eq!(groups, [("a", 2)]);
        every[2].1 = Some(Ok(vec![item(7)]));
        let (list, groups) = gather(&every);
        assert_eq!(numbers(list), [1, 2, 7]);
        assert_eq!(groups, [("a", 2), ("b", 1)]);
    }

    #[test]
    fn every_projects_list_waits_for_an_answer_and_says_why_when_all_failed() {
        let waiting: [(&str, Listed); 1] = [("a", None)];
        assert_eq!(gather(&waiting).0, None);
        let failed = [("a", Some(Err("gh is not signed in".to_owned())))];
        assert_eq!(gather(&failed).0, Some(Err("gh is not signed in".into())));
    }
}
