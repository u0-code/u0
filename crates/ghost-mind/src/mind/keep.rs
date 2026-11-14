//! Keeping what she has seen, and letting it go.
//!
//! Keeping is one act wherever she is: something she saw, which she wants
//! to have again, kept with a note in her own words. In a conversation that
//! is an image in a message she has looked at; at work it is an image a
//! tool showed her (see `work.rs`). Either way it is a `Kept` entry, and the
//! note is found again by `recall` from anywhere.

use ghost_core::{journal::Event, media, memory::PlaceState};

use super::Mind;
use crate::{Result, actions::Action};

impl Mind {
    /// Why keeping, letting go or handing over, as she asked for it, cannot
    /// be done, as a sentence for her; `None` when it can, or when it is
    /// some other act.
    pub(super) fn refusal(&self, place: &PlaceState, action: &Action) -> Result<Option<String>> {
        Ok(match action {
            Action::Keep { note, .. } if note.trim().is_empty() => Some(
                "The note is empty: say what it is and when you would want it again. Nothing was kept."
                    .into(),
            ),
            Action::Keep { message, image, .. } => self
                .seen_here(place, *message, *image)?
                .err()
                .map(|why| format!("{why} Nothing was kept.")),
            Action::LetGo { thing } => self.cannot_let_go(*thing)?,
            Action::SendFile {
                file,
                message,
                image,
                ..
            } => self
                .giveable(place, file.as_deref(), *message, *image)?
                .err(),
            _ => None,
        })
    }

    /// The file under `media/` that image `image` (counting from 1) of
    /// message `message` is, and what she saw in it; or, as a sentence for
    /// her, why she does not have it. Only a message in the conversation she
    /// is looking at, and only an image she has looked at and has on disk.
    pub(super) fn seen_here(
        &self,
        place: &PlaceState,
        message: i64,
        image: Option<i64>,
    ) -> Result<std::result::Result<(String, String), String>> {
        let image = image.unwrap_or(1);
        let Some(index) = u32::try_from(image - 1).ok() else {
            return Ok(Err(format!(
                "There is no image {image}: images are counted from 1."
            )));
        };
        let in_here = self
            .ghost
            .memory()
            .line(message)?
            .is_some_and(|(key, _)| key == place.key);
        if !in_here {
            return Ok(Err(format!(
                "There is no message m{message} in this conversation."
            )));
        }
        // What she made of it the last time she looked.
        let seen = self
            .ghost
            .journal()
            .of_kinds_after(&["viewed"], message)?
            .into_iter()
            .rev()
            .find_map(|e| match e.event {
                Event::Viewed {
                    message: m,
                    image: i,
                    what,
                    file,
                } if m == message && i == index => Some((file, what)),
                _ => None,
            });
        Ok(match seen {
            Some((Some(file), what)) if media::path(self.ghost.dir(), &file).is_some() => {
                Ok((file, what))
            }
            Some(_) => Err(format!(
                "Image {image} of m{message} was not saved when you looked at it, so you do not have it."
            )),
            None => Err(format!(
                "You have not seen image {image} of m{message}, so you do not have it."
            )),
        })
    }

    /// Keep image `image` of message `message` with her note. Returns why
    /// not, if it cannot be kept; then nothing is written.
    pub(super) fn keep(
        &mut self,
        place: &PlaceState,
        message: i64,
        image: Option<i64>,
        note: &str,
    ) -> Result<Option<String>> {
        let note = note.trim();
        if note.is_empty() {
            return Ok(Some("the note was empty".into()));
        }
        let file = match self.seen_here(place, message, image)? {
            Ok((file, _)) => file,
            Err(why) => return Ok(Some(why)),
        };
        self.record(Event::Kept {
            file,
            note: note.to_owned(),
            from: Some(place.key.clone()),
        })?;
        Ok(None)
    }

    /// Why she cannot let go of `thing`, if she cannot: she does not hold
    /// anything by that number.
    fn cannot_let_go(&self, thing: i64) -> Result<Option<String>> {
        Ok(match self.ghost.memory().belonging(thing)? {
            Some(_) => None,
            None => Some(format!(
                "You hold nothing numbered {thing}. Nothing was let go."
            )),
        })
    }

    /// Let go of `thing`, if she holds it. Returns whether she did.
    pub(super) fn let_go(&mut self, thing: i64) -> Result<bool> {
        if self.cannot_let_go(thing)?.is_some() {
            return Ok(false);
        }
        self.record(Event::LetGo { kept: thing })?;
        Ok(true)
    }
}
