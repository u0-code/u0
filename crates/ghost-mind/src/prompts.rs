//! The templates a brain reads. Data, not code.
//!
//! Each one ships inside the binary and can be overridden by a file of the
//! same name in a prompts directory. They belong to the body: they are how a
//! brain reads her certificate and her journal, and changing them does not
//! change who she is.

use std::{fs, io, path::Path};

#[derive(Debug, Clone)]
pub struct Prompts {
    /// `self.md`: who she is and what moment this is. The system prompt.
    pub this: String,
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
}

impl Prompts {
    /// The templates built into this binary.
    pub fn shipped() -> Self {
        Self {
            this: include_str!("../prompts/self.md").to_owned(),
            attend: include_str!("../prompts/attend.md").to_owned(),
            plan: include_str!("../prompts/plan.md").to_owned(),
            lived: include_str!("../prompts/lived.md").to_owned(),
            reflect: include_str!("../prompts/reflect.md").to_owned(),
            browse: include_str!("../prompts/browse.md").to_owned(),
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
            ("attend.md", &mut prompts.attend),
            ("plan.md", &mut prompts.plan),
            ("lived.md", &mut prompts.lived),
            ("reflect.md", &mut prompts.reflect),
            ("browse.md", &mut prompts.browse),
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
