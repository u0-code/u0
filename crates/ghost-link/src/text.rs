//! Small text helpers adapters keep needing.

/// Split text that is too long for one message into parts of at most `limit`
/// characters, cutting on a line break or a space where there is one.
pub fn chunks(text: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while rest.chars().count() > limit {
        let cut = rest
            .char_indices()
            .nth(limit)
            .map_or(rest.len(), |(i, _)| i);
        let head = &rest[..cut];
        let at = head
            .rfind('\n')
            .or_else(|| head.rfind(' '))
            .filter(|i| *i > 0)
            .unwrap_or(cut);
        out.push(rest[..at].trim_end().to_owned());
        rest = rest[at..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_owned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_splits_on_boundaries() {
        assert_eq!(chunks("short", 10), vec!["short"]);
        assert_eq!(
            chunks("one two three four", 9),
            vec!["one two", "three", "four"]
        );
        assert_eq!(chunks("你好世界你好世界", 4), vec!["你好世界", "你好世界"]);
        assert_eq!(
            chunks("line one\nline two", 12),
            vec!["line one", "line two"]
        );
        assert!(chunks("   ", 10).is_empty());
    }
}
