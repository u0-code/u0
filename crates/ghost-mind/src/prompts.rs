//! The templates a brain reads. Data, not code.
//!
//! Each one ships inside the binary and can be overridden by a file of the
//! same name in a prompts directory. They belong to the body: they are how a
//! brain reads her certificate and her journal, and changing them does not
//! change who she is.

use std::{fs, io, path::Path};

#[derive(Debug, Clone)]
pub struct Prompts {
    /// `self.md`: who she is. The system prompt, and the part of every
    /// request that does not change from one to the next.
    pub this: String,
    /// `now.md`: what moment this is and what has been going on. It changes
    /// by the hour, so it goes late in a request, after what stays put.
    pub now: String,
    /// `digest.md`: keeping the earlier part of a long conversation.
    pub digest: String,
    /// `reply_json.md`: how to answer when what she does is written as JSON.
    pub reply_json: String,
    /// `reply_tools.md`: how to answer when what she does is tool calls.
    pub reply_tools: String,
    /// `attend.md`: looking at a conversation.
    pub attend: String,
    /// `plan.md`: sketching a day.
    pub plan: String,
    /// `lived.md`: how a stretch of the day actually went.
    pub lived: String,
    /// `reflect.md`: looking back before sleep.
    pub reflect: String,
    /// `browse.md`: scrolling a feed.
    pub browse: String,
    /// `gate.md`: reading a post over before it goes out.
    pub gate: String,
}

impl Prompts {
    /// The templates built into this binary.
    pub fn shipped() -> Self {
        Self {
            this: include_str!("../prompts/self.md").to_owned(),
            now: include_str!("../prompts/now.md").to_owned(),
            digest: include_str!("../prompts/digest.md").to_owned(),
            reply_json: include_str!("../prompts/reply_json.md").to_owned(),
            reply_tools: include_str!("../prompts/reply_tools.md").to_owned(),
            attend: include_str!("../prompts/attend.md").to_owned(),
            plan: include_str!("../prompts/plan.md").to_owned(),
            lived: include_str!("../prompts/lived.md").to_owned(),
            reflect: include_str!("../prompts/reflect.md").to_owned(),
            browse: include_str!("../prompts/browse.md").to_owned(),
            gate: include_str!("../prompts/gate.md").to_owned(),
        }
    }

    /// The shipped templates, with any file present in `dir` taking precedence.
    pub fn load(dir: Option<&Path>) -> io::Result<Self> {
        let mut prompts = Self::shipped();
        let Some(dir) = dir else {
            return Ok(prompts);
        };
        for (name, slot) in [
            ("self.md", &mut prompts.this),
            ("now.md", &mut prompts.now),
            ("digest.md", &mut prompts.digest),
            ("reply_json.md", &mut prompts.reply_json),
            ("reply_tools.md", &mut prompts.reply_tools),
            ("attend.md", &mut prompts.attend),
            ("plan.md", &mut prompts.plan),
            ("lived.md", &mut prompts.lived),
            ("reflect.md", &mut prompts.reflect),
            ("browse.md", &mut prompts.browse),
            ("gate.md", &mut prompts.gate),
        ] {
            match fs::read_to_string(dir.join(name)) {
                Ok(text) => *slot = text,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(prompts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_overrides_only_its_own_template() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("plan.md"), "my plan").unwrap();
        let p = Prompts::load(Some(dir.path())).unwrap();
        assert_eq!(p.plan, "my plan");
        assert_eq!(p.attend, Prompts::shipped().attend);
    }
}
