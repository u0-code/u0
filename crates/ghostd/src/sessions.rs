//! The sessions a console lists: her conversations on the console
//! channel, kept by her and not by any console, so that every console of
//! hers, on this machine or another, now or after its own settings are
//! lost, lists the same ones by the same names.
//!
//! Any console may read the list. Beginning a session with a name,
//! renaming one and setting one aside are an operator's, as speaking to
//! her is ([`operator`](crate::operator)), and go to her mind, which
//! writes them in her journal: it is the one writer.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use ghost_core::{memory, social};
use ghost_mind::{Operator, Question, SessionChange};
use ghost_protocol::{
    CONSOLE, Frame, Session, SessionDelete, SessionNew, SessionRename, code, method,
};

use crate::{cast::Character, operator};

#[cfg(test)]
mod tests;

/// The longest name a session is given; more is cut.
const TITLE_MOST: usize = 80;
/// The longest id a session may be asked for by.
const ID_MOST: usize = 128;

/// One of the `sessions.*` requests, `id`, from a console that `owner`
/// says is the owner's, with `at_console` the name its hello gave.
pub(crate) async fn answer(
    id: u64,
    frame: &Frame,
    owner: bool,
    at_console: Option<&str>,
    her: &Character,
) -> Frame {
    let method = frame.method.as_deref().unwrap_or_default();
    if method == method::SESSIONS {
        return match all(her).await {
            Ok(all) => Frame::ok(
                id,
                &all.into_iter()
                    .filter(|s| !s.set_aside)
                    .map(wire)
                    .collect::<Vec<_>>(),
            ),
            Err(why) => Frame::err(id, code::INTERNAL, why),
        };
    }
    let name = match operator::operator(owner, at_console) {
        Ok(name) => name,
        Err((code, why)) => return Frame::err(id, code, why),
    };
    let by = Operator {
        key: social::key(CONSOLE, &name.to_lowercase()),
        name: name.to_owned(),
    };
    let done = match method {
        method::SESSION_NEW => {
            // Asked for with no params at all, it is asked for with none.
            let asked = if frame.params.as_ref().is_none_or(serde_json::Value::is_null) {
                Ok(SessionNew::default())
            } else {
                frame.params::<SessionNew>()
            };
            match asked {
                Ok(asked) => begin(her, asked, by).await,
                Err(e) => Err((e.code, e.message)),
            }
        }
        method::SESSION_RENAME => match frame.params::<SessionRename>() {
            Ok(asked) => rename(her, asked, by).await.map(|()| serde_json::json!({})),
            Err(e) => Err((e.code, e.message)),
        },
        method::SESSION_DELETE => match frame.params::<SessionDelete>() {
            Ok(asked) => set_aside(her, asked, by)
                .await
                .map(|()| serde_json::json!({})),
            Err(e) => Err((e.code, e.message)),
        },
        other => Err((code::METHOD_NOT_FOUND, format!("unknown method {other:?}"))),
    };
    match done {
        Ok(value) => Frame::ok(id, &value),
        Err((code, why)) => Frame::err(id, code, why),
    }
}

type Refusal = (i64, String);

/// Every conversation of hers on the console channel, those set aside
/// among them.
async fn all(her: &Character) -> Result<Vec<memory::Session>, String> {
    let value = her
        .mind()
        .ask(Question::Sessions)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

async fn begin(
    her: &Character,
    asked: SessionNew,
    by: Operator,
) -> Result<serde_json::Value, Refusal> {
    let title = match asked.title.as_deref().map(tidy) {
        Some(Some(title)) => Some(title),
        Some(None) => return Err(nothing_in_title()),
        None => None,
    };
    let taken: Vec<String> = all(her)
        .await
        .map_err(|why| (code::INTERNAL, why))?
        .into_iter()
        .map(|s| s.id)
        .collect();
    let id = fresh(asked.id.as_deref(), &taken, now_millis());
    if let Some(title) = &title {
        name(her, &id, title.clone(), by).await?;
    }
    let session = Session {
        id,
        title: title.unwrap_or_default(),
        started: Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
        last: None,
    };
    Ok(serde_json::to_value(session).unwrap_or_default())
}

async fn rename(her: &Character, asked: SessionRename, by: Operator) -> Result<(), Refusal> {
    let id = fit(&asked.id).ok_or_else(unfit)?;
    let title = tidy(&asked.title).ok_or_else(nothing_in_title)?;
    name(her, id, title, by).await
}

async fn set_aside(her: &Character, asked: SessionDelete, by: Operator) -> Result<(), Refusal> {
    let id = fit(&asked.id).ok_or_else(unfit)?;
    let change = SessionChange::SetAside {
        place: social::key(CONSOLE, id),
    };
    match her.mind().session(change, by).await {
        Ok(true) => Ok(()),
        Ok(false) => Err((
            code::NO_SUCH_SESSION,
            format!("there is no session {id} in the list"),
        )),
        Err(e) => Err((code::INTERNAL, e.to_string())),
    }
}

async fn name(her: &Character, id: &str, title: String, by: Operator) -> Result<(), Refusal> {
    let change = SessionChange::Name {
        place: social::key(CONSOLE, id),
        title,
    };
    match her.mind().session(change, by).await {
        Ok(_) => Ok(()),
        Err(e) => Err((code::INTERNAL, e.to_string())),
    }
}

/// A session as the protocol gives it.
fn wire(s: memory::Session) -> Session {
    let stamp = |at: chrono::DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Micros, true);
    Session {
        id: s.id,
        title: s.title,
        started: stamp(s.started),
        last: s.last.map(stamp),
    }
}

/// A name as it is kept: on one line, its spaces as one, cut at the most
/// a name is; `None` when nothing is left.
fn tidy(title: &str) -> Option<String> {
    let one = title.split_whitespace().collect::<Vec<_>>().join(" ");
    (!one.is_empty()).then(|| one.chars().take(TITLE_MOST).collect())
}

/// `id` if it can be a session's: something, not too long, with nothing
/// around it and nothing in it that is not to be read.
fn fit(id: &str) -> Option<&str> {
    let ok = !id.is_empty()
        && id == id.trim()
        && id.chars().count() <= ID_MOST
        && !id.chars().any(char::is_control);
    ok.then_some(id)
}

/// The id a new session is given: the one proposed, if it is fit and no
/// session has it; otherwise one made from the time, `s` and the
/// milliseconds as the desk has always made them, and the next one on
/// while that is taken.
fn fresh(proposed: Option<&str>, taken: &[String], millis: u128) -> String {
    if let Some(id) = proposed.and_then(fit)
        && !taken.iter().any(|t| t == id)
    {
        return id.to_owned();
    }
    (millis..)
        .map(|n| format!("s{n}"))
        .find(|id| !taken.contains(id))
        .unwrap_or_default()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

fn unfit() -> Refusal {
    (
        code::INVALID_PARAMS,
        "`id` is no session's: give one on one line, with nothing around it".to_owned(),
    )
}

fn nothing_in_title() -> Refusal {
    (
        code::INVALID_PARAMS,
        "there is nothing in `title` to call it".to_owned(),
    )
}
