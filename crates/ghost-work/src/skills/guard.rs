//! The structural safeguards around a skill, in one place.
//!
//! Nobody approves a skill she installs, so what keeps one in its place is
//! its shape, checked here before anything is written: a `SKILL.md` whose
//! frontmatter is what the open format says, named as its folder is; a
//! bounded number of files of a bounded size; no links and no path that
//! leads out of its folder; text everywhere except under `assets/`, where
//! a file may be anything small. A skill is only ever read: nothing here
//! makes a file of it executable, and nothing anywhere runs one. A hash of
//! what was written tells, when her body starts again, whether the folder
//! on disk is still what she took.

use std::path::{Component, Path};

/// The most files one skill may have.
pub const MAX_FILES: usize = 100;
/// The most bytes all of a skill's files may come to.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// The most bytes one file under `assets/` may be.
pub const MAX_ASSET: u64 = 1024 * 1024;
/// The longest name the format allows.
const NAME: usize = 64;
/// The longest description the format allows.
pub const DESCRIPTION: usize = 1024;
/// The longest `compatibility` the format allows.
const COMPATIBILITY: usize = 500;

/// What a `SKILL.md` says of itself, and what follows that.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Front {
    pub name: String,
    pub description: String,
    pub compatibility: Option<String>,
    /// Everything after the frontmatter: what she reads when she uses it.
    pub body: String,
}

/// A name as the format allows one: 1 to 64 lowercase letters, digits and
/// hyphens, with no hyphen at either end and never two together.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

/// Read a `SKILL.md` that lives in a folder called `folder`, and check it
/// as the format says: frontmatter first, a `name` that is a valid name and
/// the folder's, a `description` that says something and not too much.
/// Anything else in the frontmatter is passed over.
pub fn front(text: &str, folder: &str) -> Result<Front, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines
        .by_ref()
        .find(|l| !l.trim().is_empty())
        .map(str::trim_end)
        != Some("---")
    {
        return Err("its SKILL.md does not begin with frontmatter between --- lines.".to_owned());
    }
    let mut head = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if matches!(line.trim_end(), "---" | "...") {
            closed = true;
            break;
        }
        head.push(line);
    }
    if !closed {
        return Err("its SKILL.md never closes its frontmatter with a --- line.".to_owned());
    }
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_owned();
    let fields = fields(&head);
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim().to_owned())
    };
    let name = get("name").unwrap_or_default();
    if name.is_empty() {
        return Err("its SKILL.md gives no name.".to_owned());
    }
    if !name_ok(&name) {
        return Err(format!(
            "its name, {name:?}, is not a skill's name: 1 to 64 lowercase letters, digits and single hyphens, not at either end."
        ));
    }
    if name != folder {
        return Err(format!(
            "its SKILL.md calls it {name} but its folder is {folder}, and the two must be the same."
        ));
    }
    let description = get("description").unwrap_or_default();
    if description.is_empty() {
        return Err("its SKILL.md says nothing of what it is for.".to_owned());
    }
    if description.chars().count() > DESCRIPTION {
        return Err(format!(
            "its description is longer than the {DESCRIPTION} characters a skill may have."
        ));
    }
    let compatibility = get("compatibility").filter(|c| !c.is_empty());
    if compatibility
        .as_ref()
        .is_some_and(|c| c.chars().count() > COMPATIBILITY)
    {
        return Err(format!(
            "what it says it is compatible with is longer than the {COMPATIBILITY} characters a skill may have."
        ));
    }
    Ok(Front {
        name,
        description,
        compatibility,
        body,
    })
}

/// The top-level keys of a frontmatter and their values as text, read
/// leniently: a plain, quoted or block value, and nested ones passed over
/// as one string. Only the few scalar keys the format has are ever asked
/// for, so this is not YAML, only enough of it to read those whatever way
/// they were written.
fn fields(lines: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        at += 1;
        if line.starts_with([' ', '\t', '#', '-']) || line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        // What is indented under the key belongs to it.
        let start = at;
        while at < lines.len()
            && (lines[at].trim().is_empty() || lines[at].starts_with([' ', '\t']))
        {
            at += 1;
        }
        let under: Vec<&str> = lines[start..at].iter().map(|l| l.trim()).collect();
        out.push((key.trim().to_owned(), scalar(value.trim(), &under)));
    }
    out
}

/// One value: `value` as written after the key, and the lines under it.
fn scalar(value: &str, under: &[&str]) -> String {
    let folded = |lines: &[&str]| {
        lines
            .iter()
            .filter(|l| !l.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ")
    };
    match value.chars().next() {
        Some('|') => under.join("\n").trim().to_owned(),
        Some('>') => folded(under),
        Some('"') => {
            let whole = std::iter::once(value)
                .chain(under.iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            let inner = whole[1..].rsplit_once('"').map_or(&whole[1..], |(i, _)| i);
            inner
                .replace("\\\"", "\"")
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\\", "\\")
        }
        Some('\'') => {
            let whole = std::iter::once(value)
                .chain(under.iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            let inner = whole[1..].rsplit_once('\'').map_or(&whole[1..], |(i, _)| i);
            inner.replace("''", "'")
        }
        _ => {
            let plain = value.split(" #").next().unwrap_or(value).trim();
            let mut all = vec![plain];
            all.extend(under.iter().copied());
            folded(&all)
        }
    }
}

/// Whether `path`, relative to a skill's folder, stays inside it: plain
/// parts only, nothing absolute, nothing that climbs out.
pub fn inside(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Whether a file at `path` in a skill may be anything, and not only text.
pub fn asset(path: &str) -> bool {
    path.starts_with("assets/")
}

/// Whether `bytes` are text: UTF-8, with no NUL in them.
pub fn text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// A hash of every file under `dir`, by its path from there and what is
/// in it. A link, or anything that is neither a file nor a folder, makes
/// it fail: what she installed never has one.
pub fn hash(dir: &Path) -> std::io::Result<String> {
    let mut files = Vec::new();
    walk(dir, dir, &mut files)?;
    files.sort();
    let mut hasher = blake3::Hasher::new();
    for relative in files {
        let bytes = std::fs::read(dir.join(&relative))?;
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            walk(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push(relative);
        } else {
            return Err(std::io::Error::other(format!(
                "{} is neither a file nor a folder",
                path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_lowercase_letters_digits_and_single_hyphens_inside() {
        for good in ["pdf", "web-app-testing", "a1", &"a".repeat(64)] {
            assert!(name_ok(good), "{good}");
        }
        for bad in [
            "",
            "PDF",
            "-pdf",
            "pdf-",
            "web--app",
            "a_b",
            "a b",
            &"a".repeat(65),
        ] {
            assert!(!name_ok(bad), "{bad}");
        }
    }

    #[test]
    fn frontmatter_is_read_however_its_values_are_written() {
        let plain = front(
            "---\nname: pdf\ndescription: Work with PDFs. Use when a PDF is involved. # aside\nlicense: Apache-2.0\nmetadata:\n  author: someone\n  version: \"1.0\"\nallowed-tools: Bash(python:*)\n---\n\n# PDF\n\nDo this.\n",
            "pdf",
        )
        .unwrap();
        assert_eq!(plain.name, "pdf");
        assert_eq!(
            plain.description,
            "Work with PDFs. Use when a PDF is involved."
        );
        assert_eq!(plain.body, "# PDF\n\nDo this.");

        let folded = front(
            "---\nname: pdf\ndescription: >\n  Work with PDFs,\n  and forms.\n---\nbody",
            "pdf",
        )
        .unwrap();
        assert_eq!(folded.description, "Work with PDFs, and forms.");
        let quoted = front(
            "---\nname: 'pdf'\ndescription: \"Say \\\"hi\\\": it's: fine\"\n---\nbody",
            "pdf",
        )
        .unwrap();
        assert_eq!(quoted.description, "Say \"hi\": it's: fine");
        let multi = front(
            "---\nname: pdf\ndescription: Work with PDFs\n  across lines.\n---\n",
            "pdf",
        )
        .unwrap();
        assert_eq!(multi.description, "Work with PDFs across lines.");
    }

    #[test]
    fn frontmatter_that_breaks_the_format_is_refused_with_why() {
        let refused = |text: &str, folder: &str| front(text, folder).unwrap_err();
        assert!(refused("# no frontmatter", "pdf").contains("does not begin"));
        assert!(refused("---\nname: pdf\ndescription: x\n", "pdf").contains("never closes"));
        assert!(refused("---\ndescription: x\n---\n", "pdf").contains("no name"));
        assert!(refused("---\nname: Pdf\ndescription: x\n---\n", "Pdf").contains("not a skill"));
        assert!(
            refused("---\nname: pdf\ndescription: x\n---\n", "docs").contains("must be the same")
        );
        assert!(refused("---\nname: pdf\n---\n", "pdf").contains("nothing of what"));
        let long = format!("---\nname: pdf\ndescription: {}\n---\n", "x".repeat(1025));
        assert!(refused(&long, "pdf").contains("1024"));
    }

    #[test]
    fn a_path_that_climbs_out_or_is_absolute_is_not_inside() {
        for good in ["SKILL.md", "references/forms.md", "assets/logo.png"] {
            assert!(inside(good), "{good}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "references/../../x",
            "a\\b",
            "./x",
        ] {
            assert!(!inside(bad), "{bad}");
        }
    }
}
