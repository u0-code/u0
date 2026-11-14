//! Handing over what she has.
//!
//! One act wherever she is: something of hers goes to the people in front
//! of her as itself, not as words about it. What she has is what she kept,
//! an image she has seen in this conversation, or a file she made for the
//! people here. Each comes to bytes in its own way: what she kept or saw is
//! read from `media/` in her directory, never through her machine; what she
//! made is read off her machine when it is sent.

use ghost_core::{media, memory::PlaceState};

use super::Mind;
use crate::Result;

/// Something of hers, found and ready to be handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Gift {
    /// An image under `media/`: its name there, and what it is in her words
    /// (her note on it, or what she saw in it).
    Image { file: String, what: String },
    /// A file she made: its path on her machine.
    Made { path: String },
}

impl Mind {
    /// What she would hand over, asked for as `file`, or as image `image`
    /// of message `message`; or, as a sentence for her, why she cannot.
    pub(super) fn giveable(
        &self,
        place: &PlaceState,
        file: Option<&str>,
        message: Option<i64>,
        image: Option<i64>,
    ) -> Result<std::result::Result<Gift, String>> {
        if let Some(message) = message {
            return Ok(self
                .seen_here(place, message, image)?
                .map(|(file, what)| Gift::Image { file, what })
                .map_err(|why| format!("{why} Nothing was sent.")));
        }
        let wanted = file.map(str::trim).unwrap_or_default();
        if wanted.is_empty() {
            return Ok(Err(
                "Say what to hand over: `file` for something you kept or made, or `message` for an image here. Nothing was sent."
                    .into(),
            ));
        }
        let memory = self.ghost.memory();
        // Something she kept, by the number recall shows it with, or by the
        // name it is kept under.
        let held = match number(wanted) {
            Some(seq) => match memory.belonging(seq)? {
                Some(held) => Some(held),
                None => {
                    return Ok(Err(format!(
                        "You hold nothing numbered {seq}. Nothing was sent."
                    )));
                }
            },
            None => memory.holding(wanted)?,
        };
        if let Some(held) = held {
            if media::path(self.ghost.dir(), &held.file).is_none() {
                return Ok(Err(format!(
                    "You kept {} but it is no longer on disk, so it cannot be sent. Nothing was sent.",
                    held.file
                )));
            }
            return Ok(Ok(Gift::Image {
                file: held.file,
                what: held.note,
            }));
        }
        // A file she made for the people here, by the name she was given or
        // the end of it.
        let made = self.deliverables(place)?.into_iter().find(|path| {
            path == wanted
                || std::path::Path::new(path)
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy() == wanted)
                || path.ends_with(wanted)
        });
        Ok(match made {
            Some(path) => Ok(Gift::Made { path }),
            None => Err(format!(
                "You have nothing called {wanted} to hand over here. What you can send is something you kept, \
                 by its number or file name as recall showed it; an image in a message here, by its m number; \
                 or a file you made for the people here, by its name. Nothing was sent."
            )),
        })
    }
}

/// The number in `#12`, `[#12]` or `12`, if that is all there is.
fn number(wanted: &str) -> Option<i64> {
    let bare = wanted
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_start_matches('#');
    (!bare.is_empty() && bare.chars().all(|c| c.is_ascii_digit()))
        .then(|| bare.parse().ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::number;

    #[test]
    fn a_thing_is_named_by_its_number_however_recall_wrote_it() {
        assert_eq!(number("#12"), Some(12));
        assert_eq!(number("[#12]"), Some(12));
        assert_eq!(number("12"), Some(12));
        assert_eq!(number("12.png"), None);
        assert_eq!(number("#"), None);
    }
}
