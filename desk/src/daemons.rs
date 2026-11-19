//! The daemons the desk follows, and how the left column groups them.
//!
//! The first is always the ghost core on this machine, which the desk
//! starts and stops itself. Its group holds your sessions with the solver,
//! and any other character that core runs. Every other daemon is a
//! connection from the settings, which the desk only connects to: whoever
//! runs it starts and stops it. Each is asked now and then who it runs,
//! with `ghost.list`, and its group lists them. Opening one of them follows
//! her alone, on a link of her own: a daemon's group costs one small
//! question every little while, not a console for everyone it runs. Each
//! of them is also asked, every minute, what is waiting on you, for
//! Attention.

use std::collections::HashMap;

use ghost_link::protocol::{Attention, Listed};

use crate::{
    link::{Listing, Order},
    model::{LinkState, Update},
    settings::{Connection, THIS_MACHINE},
};

/// Which daemon: the core on this machine, or a connection by its name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Host {
    Local,
    Remote(String),
}

impl Host {
    /// What its group is called.
    pub fn label(&self) -> &str {
        match self {
            Self::Local => THIS_MACHINE,
            Self::Remote(name) => name,
        }
    }

    /// The key its group is folded under.
    pub fn key(&self) -> String {
        match self {
            Self::Local => "group:machine".to_owned(),
            Self::Remote(name) => format!("group:remote:{name}"),
        }
    }
}

/// What the desk knows of who a daemon runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    pub link: LinkState,
    pub ghosts: Vec<Listed>,
}

impl Cast {
    fn new() -> Self {
        Self {
            link: LinkState::Connecting,
            ghosts: Vec::new(),
        }
    }
}

/// One daemon the desk follows.
pub struct Daemon {
    pub host: Host,
    pub url: String,
    /// Its token, or why it has none though it needs one.
    pub token: Result<Option<String>, String>,
    pub cast: Cast,
    /// What each of its characters, by her id, is waiting on you for, as
    /// she last said.
    pub attention: HashMap<String, Vec<Attention>>,
    /// The characters whose answer to that came in since the desk last
    /// looked, so that it can forget what was seen of theirs and is gone.
    pub attended: Vec<String>,
    /// Whether it has said who it runs since the desk last looked, so
    /// that what was seen of a character it no longer runs is forgotten.
    pub rostered: bool,
    /// Why the last thing done to one of its characters from Attention
    /// could not be done, until the desk has said so.
    pub refused: Option<String>,
    /// Asking who it runs. None when there is no asking it: its token's
    /// variable is not set.
    listing: Option<Listing>,
}

impl Daemon {
    /// The core on this machine, at `url`.
    pub fn local(url: &str, token: Option<String>) -> Self {
        Self::follow(Host::Local, url, Ok(token))
    }

    /// A connection from the settings. Its token is read now, from the
    /// variable it names.
    pub fn remote(connection: &Connection) -> Self {
        Self::follow(
            Host::Remote(connection.name.clone()),
            &connection.url,
            connection.token(),
        )
    }

    fn follow(host: Host, url: &str, token: Result<Option<String>, String>) -> Self {
        let mut cast = Cast::new();
        let listing = match &token {
            Ok(token) => Some(Listing::follow(url.to_owned(), token.clone())),
            Err(why) => {
                cast.link = LinkState::Lost(why.clone());
                None
            }
        };
        Self {
            host,
            url: url.to_owned(),
            token,
            cast,
            attention: HashMap::new(),
            attended: Vec::new(),
            rostered: false,
            refused: None,
            listing,
        }
    }

    /// Ask it again now who it runs, if it is being asked at all.
    pub fn retry(&self) {
        if let Some(listing) = &self.listing {
            listing.retry();
        }
    }

    /// Ask everyone it runs what is waiting on you, now.
    pub fn attend(&self) {
        if let Some(listing) = &self.listing {
            listing.attend();
        }
    }

    /// Do `order` to its character `ghost`, as `who`.
    pub fn order(&self, ghost: &str, who: &str, order: Order) {
        if let Some(listing) = &self.listing {
            listing.order(ghost.to_owned(), who.to_owned(), order);
        }
    }

    /// Take in what its listing has learned. Says whether anything did.
    pub fn pump(&mut self) -> bool {
        let Some(listing) = &self.listing else {
            return false;
        };
        let mut changed = false;
        while let Ok(update) = listing.rx.try_recv() {
            match update {
                Update::Link(state) => self.cast.link = state,
                Update::Roster(ghosts) => {
                    // Whoever it no longer runs waits on nobody.
                    self.attention
                        .retain(|id, _| ghosts.iter().any(|g| g.id == *id));
                    self.cast.ghosts = ghosts;
                    self.rostered = true;
                }
                Update::Attention { ghost, items } => {
                    self.attention.insert(ghost.clone(), items);
                    self.attended.push(ghost);
                }
                Update::Ordered { why } => {
                    if why.is_some() {
                        self.refused = why;
                    }
                }
                _ => continue,
            }
            changed = true;
        }
        changed
    }
}

/// One group down the left: a daemon and the characters listed under it.
pub struct Group<'a> {
    pub host: &'a Host,
    pub link: &'a LinkState,
    pub ghosts: Vec<&'a Listed>,
}

/// Whether `g`, whom the daemon `d` runs, is the solver: a character of
/// the core on this machine with the seal the solver welcomed the desk
/// with, `solver`; until then, a core that runs one character runs only
/// the solver.
pub fn is_solver(d: &Daemon, g: &Listed, solver: Option<&str>) -> bool {
    d.host == Host::Local
        && (Some(g.card.seal.as_str()) == solver || (solver.is_none() && d.cast.ghosts.len() == 1))
}

/// The groups down the left, this machine's first. The solver is not
/// listed among this machine's characters: your sessions with it stand for
/// it.
pub fn groups<'a>(daemons: &'a [Daemon], solver: Option<&str>) -> Vec<Group<'a>> {
    let mut groups: Vec<Group<'a>> = daemons
        .iter()
        .map(|d| {
            let ghosts = d
                .cast
                .ghosts
                .iter()
                .filter(|g| !is_solver(d, g, solver))
                .collect();
            Group {
                host: &d.host,
                link: &d.cast.link,
                ghosts,
            }
        })
        .collect();
    groups.sort_by_key(|g| *g.host != Host::Local);
    groups
}

#[cfg(test)]
mod tests {
    use ghost_link::protocol::GhostCard;

    use super::*;

    fn listed(id: &str) -> Listed {
        Listed {
            id: id.to_owned(),
            card: GhostCard {
                name: id.to_uppercase(),
                pronouns: "she/her".to_owned(),
                city: "C".to_owned(),
                country: "CN".to_owned(),
                seal: format!("seal-{id}"),
                bio: None,
            },
            awake: Some(true),
            doing: None,
            avatar: false,
        }
    }

    fn daemon(host: Host, ids: &[&str], link: LinkState) -> Daemon {
        Daemon {
            host,
            url: "ws://127.0.0.1:1".to_owned(),
            token: Ok(None),
            cast: Cast {
                link,
                ghosts: ids.iter().map(|id| listed(id)).collect(),
            },
            attention: HashMap::new(),
            attended: Vec::new(),
            rostered: false,
            refused: None,
            listing: None,
        }
    }

    fn ids<'a>(group: &Group<'a>) -> Vec<&'a str> {
        group.ghosts.iter().map(|g| g.id.as_str()).collect()
    }

    #[test]
    fn this_machine_comes_first_and_each_connection_holds_only_its_own_characters() {
        let daemons = [
            daemon(
                Host::Remote("studio".into()),
                &["weiwei", "lin"],
                LinkState::Connected,
            ),
            daemon(Host::Local, &["solver"], LinkState::Connected),
            daemon(
                Host::Remote("lab".into()),
                &[],
                LinkState::Lost("refused".into()),
            ),
        ];
        let groups = groups(&daemons, Some("seal-solver"));
        let labels: Vec<&str> = groups.iter().map(|g| g.host.label()).collect();
        assert_eq!(labels, ["This machine", "studio", "lab"]);
        assert!(ids(&groups[0]).is_empty(), "the solver is its sessions");
        assert_eq!(ids(&groups[1]), ["weiwei", "lin"]);
        assert!(ids(&groups[2]).is_empty());
        assert_eq!(*groups[2].link, LinkState::Lost("refused".into()));
    }

    #[test]
    fn this_machine_lists_every_character_of_its_core_but_the_solver() {
        let several = [daemon(
            Host::Local,
            &["mei", "solver", "lin"],
            LinkState::Connected,
        )];
        assert_eq!(
            ids(&groups(&several, Some("seal-solver"))[0]),
            ["mei", "lin"]
        );
        // Before the solver has said who it is, a core running one
        // character runs only the solver.
        let alone = [daemon(Host::Local, &["solver"], LinkState::Connected)];
        assert!(ids(&groups(&alone, None)[0]).is_empty());
        // Every character on a connection is listed, whatever her seal.
        let remote = [daemon(
            Host::Remote("studio".into()),
            &["solver"],
            LinkState::Connected,
        )];
        assert_eq!(ids(&groups(&remote, Some("seal-solver"))[0]), ["solver"]);
    }
}
