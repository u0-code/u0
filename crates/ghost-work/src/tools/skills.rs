//! `find_skills`, `install_skill`, `uninstall_skill` and `use_skill`: the
//! tools she gets and reads skills with. See [`crate::skills`] for what
//! they act on and why it is safe to act on it without asking anyone.

use std::sync::Arc;

use async_trait::async_trait;
use ghost_brain::ToolSpec;
use serde_json::{Value, json};

use crate::{
    prompts::{self, Loop, Store, TEXTS},
    skills::Skills,
    tool::{Outcome, Tool, text},
};

/// Search the index of skills.
pub struct FindSkills(Arc<Skills>);

impl FindSkills {
    pub fn new(skills: Arc<Skills>) -> Self {
        Self(skills)
    }
}

#[async_trait]
impl Tool for FindSkills {
    fn name(&self) -> &'static str {
        "find_skills"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "find_skills",
            json!({"query": {"type": "string"}}),
            &["query"],
        )
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let query = match text(words, args, "query") {
            Ok(q) => q,
            Err(refusal) => return refusal,
        };
        match self.0.find(words, query).await {
            Ok(found) => Outcome::ok(found),
            Err(why) => Outcome::fail(why),
        }
    }
}

/// Install a skill the index lists.
pub struct InstallSkill(Arc<Skills>);

impl InstallSkill {
    pub fn new(skills: Arc<Skills>) -> Self {
        Self(skills)
    }
}

#[async_trait]
impl Tool for InstallSkill {
    fn name(&self) -> &'static str {
        "install_skill"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "install_skill",
            json!({"id": {"type": "string"}}),
            &["id"],
        )
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let id = match text(words, args, "id") {
            Ok(id) => id,
            Err(refusal) => return refusal,
        };
        match self.0.install(words, id).await {
            Ok(said) => Outcome::ok(said),
            Err(why) => Outcome::fail(words.fill(
                TEXTS,
                "not_installed",
                &[("name", id.to_owned()), ("why", why)],
            )),
        }
    }
}

/// Take out a skill she installed.
pub struct UninstallSkill(Arc<Skills>);

impl UninstallSkill {
    pub fn new(skills: Arc<Skills>) -> Self {
        Self(skills)
    }
}

#[async_trait]
impl Tool for UninstallSkill {
    fn name(&self) -> &'static str {
        "uninstall_skill"
    }

    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        prompts::spec(
            words,
            lp,
            "uninstall_skill",
            json!({"name": {"type": "string"}, "why": {"type": "string"}}),
            &["name", "why"],
        )
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let (name, why) = match (text(words, args, "name"), text(words, args, "why")) {
            (Ok(n), Ok(w)) => (n, w),
            (Err(refusal), _) | (_, Err(refusal)) => return refusal,
        };
        match self.0.uninstall(words, name, why) {
            Ok(said) => Outcome::ok(said),
            Err(why) => Outcome::fail(why),
        }
    }
}

/// Read a skill she has, or one of its files.
pub struct UseSkill(Arc<Skills>);

impl UseSkill {
    pub fn new(skills: Arc<Skills>) -> Self {
        Self(skills)
    }
}

#[async_trait]
impl Tool for UseSkill {
    fn name(&self) -> &'static str {
        "use_skill"
    }

    /// Its description ends with every skill she has, a line each, so
    /// that she knows what there is to use without reading any of it.
    fn spec(&self, words: &Store, lp: Loop) -> ToolSpec {
        let mut spec = prompts::spec(
            words,
            lp,
            "use_skill",
            json!({"name": {"type": "string"}, "file": {"type": "string"}}),
            &["name"],
        );
        let lines: Vec<String> = self
            .0
            .listing()
            .into_iter()
            .map(|s| {
                words.fill(
                    TEXTS,
                    "skill_line",
                    &[("skill", s.name), ("description", s.description)],
                )
            })
            .collect();
        spec.description =
            ghost_core::narrate::fill(&spec.description, &[("skills", lines.join("\n"))]);
        spec
    }

    async fn call(&self, args: &Value, words: &Store) -> Outcome {
        let name = match text(words, args, "name") {
            Ok(n) => n,
            Err(refusal) => return refusal,
        };
        match self.0.read(words, name, args["file"].as_str()) {
            Ok((said, origin)) => Outcome::ok(said).from(origin),
            Err(why) => Outcome::fail(why),
        }
    }
}
