//! Your terminals, on the machine they run on: each a shell in a
//! pseudo-terminal (`pty`), by the desk's id for it, with the last of what
//! it printed kept, so that a desk that opens it again (one come back, or
//! another) sees its screen as it was. The screen itself is the desk's.

use std::{
    collections::{HashMap, VecDeque},
    io::Read as _,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{proto::Event, pty::Pty};

/// How much of what a terminal printed is kept for a desk that opens it
/// again.
const KEPT: usize = 256 * 1024;

struct Term {
    pty: Pty,
    title: String,
    /// The last of what it printed.
    printed: VecDeque<u8>,
    /// How it ended, once it has.
    ended: Option<String>,
}

#[derive(Clone, Default)]
pub struct Terminals(Arc<Mutex<HashMap<u64, Term>>>);

impl Terminals {
    /// Open the terminal `id` in `dir` (on `host`, over SSH), `rows` by
    /// `cols`, `first` typed into it; one open already is said again as it
    /// is: its title, what it printed, and its end. What it prints is said
    /// as it comes, to `send`.
    pub fn open(
        &self,
        id: u64,
        (dir, host): (PathBuf, Option<String>),
        first: Option<String>,
        size: (u16, u16),
        send: Arc<dyn Fn(Event) + Send + Sync>,
    ) {
        if let Some(t) = self.0.lock().expect("terminals").get(&id) {
            send(Event::TermOpened {
                id,
                result: Ok(t.title.clone()),
            });
            send(Event::TermOutput {
                id,
                bytes: t.printed.iter().copied().collect(),
            });
            if let Some(how) = &t.ended {
                send(Event::TermEnded {
                    id,
                    how: how.clone(),
                });
            }
            return;
        }
        let all = self.0.clone();
        // Opening starts a process, and reading waits: a thread of its own.
        std::thread::spawn(move || {
            let (pty, title, mut from) =
                match Pty::open(&dir, host.as_deref(), first.as_deref(), size) {
                    Ok(opened) => opened,
                    Err(why) => {
                        send(Event::TermOpened {
                            id,
                            result: Err(why),
                        });
                        return;
                    }
                };
            let term = Term {
                pty,
                title: title.clone(),
                printed: VecDeque::new(),
                ended: None,
            };
            all.lock().expect("terminals").insert(id, term);
            send(Event::TermOpened {
                id,
                result: Ok(title),
            });
            let mut buf = [0u8; 16 * 1024];
            loop {
                match from.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Some(t) = all.lock().expect("terminals").get_mut(&id) {
                            t.printed.extend(&buf[..n]);
                            let over = t.printed.len().saturating_sub(KEPT);
                            t.printed.drain(..over);
                        }
                        send(Event::TermOutput {
                            id,
                            bytes: buf[..n].to_vec(),
                        });
                    }
                }
            }
            let how = (all.lock().expect("terminals").get_mut(&id)).map(|t| {
                let how = t.pty.ended();
                t.ended = Some(how.clone());
                how
            });
            if let Some(how) = how {
                send(Event::TermEnded { id, how });
            }
        });
    }

    /// Send `bytes` to the terminal `id`, typed or pasted.
    pub fn input(&self, id: u64, bytes: &[u8]) {
        if let Some(t) = self.0.lock().expect("terminals").get(&id) {
            t.pty.send(bytes);
        }
    }

    /// Make the terminal `id` `rows` by `cols`.
    pub fn resize(&self, id: u64, (rows, cols): (u16, u16)) {
        if let Some(t) = self.0.lock().expect("terminals").get(&id) {
            t.pty.resize(rows, cols);
        }
    }

    /// Close the terminal `id`, stopping what runs in it; with a command
    /// still running in it, only when `force`d. Whether it was left, busy.
    pub fn close(&self, id: u64, force: bool) -> bool {
        let mut all = self.0.lock().expect("terminals");
        let busy = all
            .get(&id)
            .is_some_and(|t| t.ended.is_none() && t.pty.busy());
        if busy && !force {
            return true;
        }
        all.remove(&id);
        false
    }
}
