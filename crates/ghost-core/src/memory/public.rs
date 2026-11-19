//! What she posts in public, and how it does.
//!
//! Her posts on each platform, with the latest count of likes, replies,
//! reposts and views the platform reported for them, and the number of
//! people following her there, each time it was reported. This is each
//! platform's record and is read a platform at a time.

use chrono::{DateTime, Utc};
use rusqlite::{Transaction, params};
use serde::Serialize;

use super::{Memory, as_i64, time_at};
use crate::{
    Result,
    journal::{Entry, Event},
};

/// A public post of hers and the latest count of how it did.
#[derive(Debug, Clone, Serialize)]
pub struct Standing {
    pub id: String,
    pub at: DateTime<Utc>,
    /// What she posted, if her journal has it.
    pub text: Option<String>,
    pub likes: i64,
    pub replies: i64,
    pub reposts: i64,
    pub views: i64,
}

/// Bumped when `SCHEMA`, or what `fold` writes, changes: see
/// [`super::Feature`].
pub(super) const VERSION: i64 = 1;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS followers (
    seq INTEGER PRIMARY KEY, at TEXT NOT NULL, channel TEXT NOT NULL, count INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS followers_channel ON followers (channel, seq);
CREATE TABLE IF NOT EXISTS posts (
    channel TEXT NOT NULL, id TEXT NOT NULL, seq INTEGER NOT NULL, at TEXT NOT NULL,
    text TEXT, likes INTEGER NOT NULL DEFAULT 0, replies INTEGER NOT NULL DEFAULT 0,
    reposts INTEGER NOT NULL DEFAULT 0, views INTEGER NOT NULL DEFAULT 0,
    measured INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (channel, id)
);
";

pub(super) fn fold(tx: &Transaction<'_>, entry: &Entry, at: &str) -> Result<()> {
    let seq = entry.seq;
    match &entry.event {
        Event::Posted {
            channel,
            id: Some(id),
            text,
        } => {
            tx.execute(
                "INSERT INTO posts (channel, id, seq, at, text) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(channel, id) DO UPDATE SET text = ?5",
                params![channel, id, seq, at, text],
            )?;
        }
        Event::Measured {
            channel,
            followers,
            posts,
        } => {
            if let Some(count) = followers {
                tx.execute(
                    "INSERT INTO followers (seq, at, channel, count) VALUES (?1, ?2, ?3, ?4)",
                    params![seq, at, channel, i64::try_from(*count).unwrap_or(i64::MAX)],
                )?;
            }
            let n = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
            for post in posts {
                tx.execute(
                    "INSERT INTO posts (channel, id, seq, at, likes, replies, reposts, views, measured)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
                     ON CONFLICT(channel, id) DO UPDATE SET
                         likes = ?5, replies = ?6, reposts = ?7, views = ?8, measured = 1",
                    params![
                        channel,
                        post.id,
                        seq,
                        at,
                        n(post.likes),
                        n(post.replies),
                        n(post.reposts),
                        n(post.views)
                    ],
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

impl Memory {
    /// Follower counts on a channel as they were reported, oldest first.
    pub fn followers(&self, channel: &str, limit: usize) -> Result<Vec<(DateTime<Utc>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT at, count FROM followers WHERE channel = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let mut counts = stmt
            .query_map(params![channel, as_i64(limit)], |r| {
                Ok((time_at(r, 0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        counts.reverse();
        Ok(counts)
    }

    /// Her measured posts on a channel, newest first.
    pub fn standings(&self, channel: &str, limit: usize) -> Result<Vec<Standing>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, at, text, likes, replies, reposts, views FROM posts
             WHERE channel = ?1 AND measured = 1 ORDER BY seq DESC LIMIT ?2",
        )?;
        Ok(stmt
            .query_map(params![channel, as_i64(limit)], |r| {
                Ok(Standing {
                    id: r.get(0)?,
                    at: time_at(r, 1)?,
                    text: r.get(2)?,
                    likes: r.get(3)?,
                    replies: r.get(4)?,
                    reposts: r.get(5)?,
                    views: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::fold;
    use crate::journal::{Event, PostStats};

    #[test]
    fn posts_carry_their_latest_counts() {
        let (_, m) = fold(vec![
            Event::Posted {
                channel: "x".into(),
                id: Some("p1".into()),
                text: "the door on Kuixinglou street".into(),
            },
            Event::Measured {
                channel: "x".into(),
                followers: Some(40),
                posts: vec![PostStats {
                    id: "p1".into(),
                    likes: 3,
                    ..Default::default()
                }],
            },
            Event::Measured {
                channel: "x".into(),
                followers: Some(52),
                posts: vec![PostStats {
                    id: "p1".into(),
                    likes: 11,
                    replies: 2,
                    views: 900,
                    ..Default::default()
                }],
            },
        ]);
        let posts = m.standings("x", 10).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(
            (posts[0].likes, posts[0].replies, posts[0].views),
            (11, 2, 900)
        );
        assert_eq!(
            posts[0].text.as_deref(),
            Some("the door on Kuixinglou street")
        );
        let counts: Vec<i64> = m
            .followers("x", 10)
            .unwrap()
            .into_iter()
            .map(|c| c.1)
            .collect();
        assert_eq!(counts, [40, 52]);
        // Another platform's counts are its own.
        assert!(m.standings("qq", 10).unwrap().is_empty());
        assert!(m.followers("qq", 10).unwrap().is_empty());
    }
}
