//! Scheduled tasks, as t3code has them: a prompt said to an agent in a
//! project on a schedule (once, every so many minutes, or at a time of
//! day), each run a session of its own or the next turn of one. The server
//! keeps them in `tasks.json` in the home, and runs them itself, so that
//! they run while no window is open.

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use agent_client_protocol::v1::schema::SessionId;
use serde::{Deserialize, Serialize};

use crate::{store, when, whole};

/// What `tasks.json` holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tasks {
    pub tasks: Vec<Task>,
}

/// A prompt said on a schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Its number, given by the server when it is first kept; 0 until then.
    pub id: u64,
    pub title: String,
    pub prompt: String,
    /// The registry id of the agent it is said to.
    pub agent: String,
    /// The model chosen for its sessions; none, the agent's default.
    #[serde(default)]
    pub model: Option<String>,
    /// The mode its sessions are put in (one that asks nothing lets it run
    /// unattended); none, the agent's own.
    #[serde(default)]
    pub mode: Option<String>,
    /// The project it works in.
    pub project: PathBuf,
    pub schedule: Schedule,
    #[serde(default)]
    pub paused: bool,
    /// Each run is a new session, or the next turn of the one it began.
    #[serde(default)]
    pub same: bool,
    /// Each new session works in a worktree of its own.
    #[serde(default)]
    pub worktree: bool,
    /// The session its runs are said in, once there is one: its first
    /// run's, where each is said in the same one, or the one it was set
    /// from.
    #[serde(default)]
    pub session: Option<SessionId>,
    /// When it runs next (seconds since the epoch); none, never again.
    #[serde(default)]
    pub next: Option<u64>,
    #[serde(default)]
    pub last: Option<Run>,
    /// How many times it ran.
    #[serde(default)]
    pub runs: u32,
}

/// When a task runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Schedule {
    /// Once, at that time (seconds since the epoch).
    Once(u64),
    /// Every so many minutes.
    Every(u32),
    /// At `minute` minutes into the day, on `days` (0 for Sunday to 6;
    /// none, every day), in this computer's time zone.
    Daily { minute: u16, days: Vec<u8> },
    /// Each time its address is called, as t3code's webhook tasks: a POST
    /// to `/hook/<this token>` where Ghost listens (and through its tunnel,
    /// while one is up). Nothing holds a call made while Ghost is not
    /// running: the sender has to deliver it again.
    Hook(String),
}

/// How a task's last run went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    /// When it began (seconds since the epoch).
    pub at: u64,
    /// The session it was said in, once it had one.
    pub session: Option<SessionId>,
    /// Still going.
    pub going: bool,
    /// Why it failed, if it did.
    pub error: Option<String>,
}

/// The shortest interval a task runs at, as t3code's.
pub const LEAST_MINUTES: u32 = 1;

impl Schedule {
    /// When it runs first after `after` (seconds since the epoch); none,
    /// never.
    pub fn next(&self, after: u64) -> Option<u64> {
        match self {
            Self::Once(at) => (*at > after).then_some(*at),
            Self::Every(minutes) => Some(after + u64::from((*minutes).max(LEAST_MINUTES)) * 60),
            Self::Daily { minute, days } => {
                when::next_daily(store::time(after), *minute, days).map(store::secs)
            }
            Self::Hook(_) => None,
        }
    }

    /// What a call to a webhook task says, put where its prompt says
    /// `{{body}}`: marked as data from outside, not words of yours.
    pub fn filled(prompt: &str, body: &str) -> String {
        prompt.replace(
            "{{body}}",
            &format!("<webhook-data untrusted>\n{body}\n</webhook-data>"),
        )
    }

    /// As said in a list: "Every 30 min", "Weekdays at 09:00", "Once, Tue 3 Nov 15:00".
    pub fn said(&self, twelve: bool) -> String {
        match self {
            Self::Once(at) => format!(
                "Once, {}",
                when::said_at(store::time(*at), SystemTime::now(), twelve)
            ),
            Self::Every(m) if m % 60 == 0 && *m >= 60 => match m / 60 {
                1 => "Every hour".into(),
                h => format!("Every {h} hours"),
            },
            Self::Every(m) => format!("Every {m} min"),
            Self::Daily { minute, days } => {
                let at = when::clock(when::at_minute(SystemTime::now(), 0, *minute), twelve);
                match days.as_slice() {
                    [] => format!("Daily at {at}"),
                    [1, 2, 3, 4, 5] => format!("Weekdays at {at}"),
                    days => {
                        let names: Vec<&str> = days
                            .iter()
                            .filter_map(|d| DAYS.get(usize::from(*d)).copied())
                            .collect();
                        format!("{} at {at}", names.join(", "))
                    }
                }
            }
            Self::Hook(_) => "On webhook".into(),
        }
    }
}

/// The days of the week, Sunday first, as a schedule names them.
pub const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

pub fn path(home: &Path) -> PathBuf {
    home.join("tasks.json")
}

/// The tasks kept in `home`; none when there is no file. One that does not
/// read is refused with why, and nothing is saved over it.
pub fn load(home: &Path) -> Result<Tasks, String> {
    whole::read_json(&path(home)).map_err(|e| {
        format!("{e}. Move it away to start afresh; until then no task runs or is kept.")
    })
}

pub fn bytes(tasks: &Tasks) -> Vec<u8> {
    serde_json::to_vec_pretty(tasks).expect("the tasks serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_runs_once_only_ahead_and_every_so_often_from_its_last_run() {
        assert_eq!(Schedule::Once(100).next(50), Some(100));
        assert_eq!(Schedule::Once(100).next(100), None);
        assert_eq!(Schedule::Every(30).next(1_000), Some(1_000 + 1_800));
        // Less than a minute is a minute.
        assert_eq!(Schedule::Every(0).next(0), Some(60));
    }

    #[test]
    fn a_daily_task_runs_at_its_time_on_the_days_chosen() {
        let now = store::secs(SystemTime::now());
        let any = Schedule::Daily {
            minute: 9 * 60 + 30,
            days: Vec::new(),
        };
        let next = any.next(now).unwrap();
        assert!(next > now && next <= now + 86_400);
        assert_eq!(when::clock(store::time(next), false), "09:30");
        // Only on Sundays: within a week, and on a Sunday.
        let sundays = Schedule::Daily {
            minute: 9 * 60,
            days: vec![0],
        };
        let next = sundays.next(now).unwrap();
        assert!(next > now && next <= now + 7 * 86_400);
        assert_eq!(when::weekday(store::time(next)), 0);
    }

    #[test]
    fn a_schedule_is_said_as_a_list_shows_it() {
        assert_eq!(Schedule::Every(30).said(false), "Every 30 min");
        assert_eq!(Schedule::Every(120).said(false), "Every 2 hours");
        let weekdays = Schedule::Daily {
            minute: 9 * 60,
            days: vec![1, 2, 3, 4, 5],
        };
        assert_eq!(weekdays.said(false), "Weekdays at 09:00");
        let some = Schedule::Daily {
            minute: 17 * 60 + 5,
            days: vec![0, 6],
        };
        assert_eq!(some.said(false), "Sun, Sat at 17:05");
    }

    #[test]
    fn tasks_read_back_as_written_and_a_file_that_does_not_read_is_refused() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(load(home.path()).unwrap(), Tasks::default());
        let tasks = Tasks {
            tasks: vec![Task {
                id: 1,
                title: "Triage".into(),
                prompt: "Look at new issues".into(),
                agent: "claude-acp".into(),
                model: None,
                mode: None,
                project: "/p".into(),
                schedule: Schedule::Every(60),
                paused: false,
                same: true,
                worktree: false,
                session: None,
                next: Some(10),
                last: None,
                runs: 0,
            }],
        };
        whole::write(&path(home.path()), &bytes(&tasks)).unwrap();
        assert_eq!(load(home.path()).unwrap(), tasks);
        std::fs::write(path(home.path()), "{\"tasks\": 3}").unwrap();
        assert!(load(home.path()).unwrap_err().contains("Move it away"));
    }
}
