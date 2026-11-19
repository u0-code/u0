//! The shapes words are put into for a brain: filling a template, and
//! lists that hold no words of their own.
//!
//! The words themselves live with whoever calls a brain, in its prompt
//! files; nothing here says anything.

/// Replace `{{key}}` placeholders in one pass.
///
/// Values are never rescanned, so text that came from other people cannot
/// smuggle a placeholder in. Unknown keys are left as written.
pub fn fill(template: &str, pairs: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len() + 256);
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        let key = after[..close].trim();
        match pairs.iter().find(|(k, _)| *k == key) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(&rest[open..open + 2 + close + 2]),
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out
}

/// Rows of `(when, what)` in order, with a run of rows that happened at
/// the same remembered time put under it once.
pub fn grouped(rows: impl IntoIterator<Item = (String, String)>) -> String {
    let mut runs: Vec<(String, Vec<String>)> = Vec::new();
    for (at, text) in rows {
        match runs.last_mut() {
            Some((last, texts)) if *last == at => texts.push(text),
            _ => runs.push((at, vec![text])),
        }
    }
    runs.iter()
        .map(|(at, texts)| match texts.as_slice() {
            [one] => format!("- {}: {one}", capital(at)),
            many => format!(
                "- {}:\n{}",
                capital(at),
                many.iter()
                    .map(|t| format!("  - {t}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text with its first letter capitalised.
pub fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// Pieces of her past, as a list. Empty when there are none.
pub fn past(pieces: &[&crate::identity::Past]) -> String {
    pieces
        .iter()
        .map(|p| format!("- {}", p.text.trim().replace('\n', " ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_is_single_pass() {
        let out = fill(
            "a {{x}} b {{ y }} c {{z}} {{",
            &[("x", "{{y}}".into()), ("y", "Y".into())],
        );
        assert_eq!(out, "a {{y}} b Y c {{z}} {{");
    }

    #[test]
    fn rows_remembered_at_the_same_time_are_put_under_it_once() {
        let out = grouped([
            ("this morning".to_owned(), "a".to_owned()),
            ("this morning".to_owned(), "b".to_owned()),
            ("about two hours ago".to_owned(), "c".to_owned()),
        ]);
        assert_eq!(
            out,
            "- This morning:\n  - a\n  - b\n- About two hours ago: c"
        );
    }
}
