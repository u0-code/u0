//! Looking at the images people send her.
//!
//! A person in a chat glances at an image as it arrives. She does the same:
//! when a message with images is heard and one of her brains can see, each
//! image is fetched, kept under `media/`, and described in a few sentences.
//! The description is written into her journal as `Viewed`, and from then on
//! the message reads `[image: …]` wherever she sees it.
//!
//! None of this holds her attention up. If she looks at the conversation
//! before the description is in, she sees `[image]`, as anyone whose
//! picture has not loaded yet would.

use std::{path::Path, sync::Arc, time::Duration as StdDuration};

use chrono::{DateTime, Utc};
use ghost_brain::{Brain, Completion, Effort, Message, Picture};
use ghost_core::{journal::Event, media, narrate};
use tracing::{debug, warn};

use super::{Mind, Outcome};
use crate::Result;

/// The largest image she will fetch. Anything bigger is not a chat image.
const MAX_BYTES: usize = 8 * 1024 * 1024;
/// How long one image may take to arrive.
const FETCH_TIMEOUT: StdDuration = StdDuration::from_secs(20);
/// The most images of one message she looks at.
const AT_MOST: usize = 4;
/// Room for two or three sentences.
const VIEW_TOKENS: u32 = 600;
/// How much of the conversation she has in mind while looking.
const CONTEXT_LINES: usize = 8;

/// What she made of one image.
pub(super) struct Sight {
    pub what: String,
    /// The name it is kept under in `media/`, if it could be kept.
    pub file: Option<String>,
}

impl Mind {
    /// A message with images was heard and written down as entry `message`:
    /// look at them, if she has a brain that can.
    pub(super) fn view(
        &mut self,
        message: i64,
        place_key: &str,
        images: &[String],
        now: DateTime<Utc>,
    ) -> Result<()> {
        // The cheaper brain first: a glance is routine, as a digest is. A
        // setup where only her conversation brain can see still has eyes.
        let Some(brain) = [&self.tick, &self.think]
            .into_iter()
            .find(|b| b.sees())
            .cloned()
        else {
            return Ok(());
        };
        let wanted: Vec<(u32, String)> = images
            .iter()
            .enumerate()
            .filter(|(_, url)| fetchable(url))
            .take(AT_MOST)
            .filter_map(|(i, url)| Some((u32::try_from(i).ok()?, url.clone())))
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        if !self.budget.allow(now) {
            debug!("{place_key}: no calls left this hour to look at an image");
            return Ok(());
        }
        for _ in &wanted {
            self.budget.spend(now);
        }
        let request = self.view_request(place_key, message, now)?;
        let dir = self.ghost.dir().to_path_buf();
        self.jobs.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(FETCH_TIMEOUT)
                .build()
                .unwrap_or_default();
            let mut seen = Vec::new();
            for (image, url) in wanted {
                let result = look(&client, &brain, &request, &dir, &url).await;
                seen.push((image, result));
            }
            Outcome::Viewed { message, seen }
        });
        Ok(())
    }

    /// What she is asked when she looks: where she is and what was said
    /// just before. The image goes in with it.
    fn view_request(
        &self,
        place_key: &str,
        message: i64,
        now: DateTime<Utc>,
    ) -> Result<Completion> {
        let memory = self.ghost.memory();
        let place = match memory.place(place_key)? {
            Some(place) => {
                let people = memory.people_in(place_key, 6)?;
                self.describe_place(&place, &people)
            }
            None => place_key.to_owned(),
        };
        let lines = memory.thread_before(place_key, message, CONTEXT_LINES)?;
        let system = narrate::fill(
            &self.prompts.view,
            &[
                ("where", place),
                (
                    "transcript",
                    narrate::transcript(
                        &lines,
                        i64::MAX,
                        self.prompts.text("directive_mark"),
                        self.life.clock(),
                        now,
                    ),
                ),
            ],
        );
        // The words that go with the image; what to do with it is in `view.md`.
        let look = self.prompts.text("look");
        Ok(Completion::ask(system, look, VIEW_TOKENS).thinking(Effort::Off))
    }

    pub(super) fn on_viewed(
        &mut self,
        message: i64,
        seen: Vec<(u32, std::result::Result<Sight, String>)>,
    ) -> Result<()> {
        for (image, result) in seen {
            match result {
                Ok(Sight { what, file }) => {
                    debug!("#{message}: looked at image {image}");
                    self.record(Event::Viewed {
                        message,
                        image,
                        what,
                        file,
                    })?;
                }
                // The placeholder stays: she knows an image was sent, as
                // she did before she could see.
                Err(e) => warn!("#{message}: could not look at image {image}: {e}"),
            }
        }
        Ok(())
    }
}

/// Only web links are fetched: a link from a platform, never a path.
fn fetchable(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Fetch one image, keep it, and describe it.
async fn look(
    client: &reqwest::Client,
    brain: &Arc<dyn Brain>,
    request: &Completion,
    dir: &Path,
    url: &str,
) -> std::result::Result<Sight, String> {
    let picture = fetch(client, url).await?;
    // Kept even if the description fails: the link will not last, and the
    // bytes are what a later look would need. A failure to keep it does not
    // stop her looking.
    let file = match media::store(dir, &picture.bytes, &picture.mime) {
        Ok(name) => Some(name),
        Err(e) => {
            warn!("could not keep an image: {e}");
            None
        }
    };
    let mut request = request.clone();
    // The words that go with the image are the ones the request was made with.
    let words = request.messages.pop().map(|m| m.text).unwrap_or_default();
    request.messages = vec![Message::user(words).with_images(vec![picture])];
    let reply = brain.complete(&request).await.map_err(|e| e.to_string())?;
    let what = reply.text.trim().to_owned();
    if what.is_empty() {
        return Err("the description came back empty".into());
    }
    Ok(Sight { what, file })
}

/// The bytes behind a link, if they are an image of a size worth looking
/// at. The type is read from the bytes themselves where it can be, since
/// platforms often serve images as `application/octet-stream`.
async fn fetch(client: &reqwest::Client, url: &str) -> std::result::Result<Picture, String> {
    // A link's error names the link; the link is not worth a log line.
    let failed = |e: reqwest::Error| e.without_url().to_string();
    let mut response = client.get(url).send().await.map_err(failed)?;
    if !response.status().is_success() {
        return Err(format!("the link answered {}", response.status()));
    }
    let declared = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
        });
    if let Some(kind) = &declared
        && !kind.starts_with("image/")
        && !kind.ends_with("octet-stream")
    {
        return Err(format!("the link holds {kind}, not an image"));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err("the image is too large to look at".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err("the image is too large to look at".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let mime = media::sniff(&bytes)
        .map(str::to_owned)
        .or_else(|| declared.filter(|kind| taken(kind)))
        .ok_or("not an image she can look at")?;
    Ok(Picture { mime, bytes })
}

/// The kinds of image the providers she runs on take.
fn taken(kind: &str) -> bool {
    matches!(
        kind,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_links_are_fetched() {
        assert!(fetchable("https://example.com/a.png"));
        assert!(!fetchable("file:///etc/passwd"));
        assert!(!fetchable(""));
    }
}
