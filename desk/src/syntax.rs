//! Code coloured the way the thread's design colours it: keywords blue,
//! strings green, numbers amber, function names violet, comments grey,
//! everything else the text's colour. It is a light pass by the shape of
//! the words, not a parser: enough to read a block at a glance, for the
//! languages she writes and reads most, and plain text for the rest.

use std::ops::Range;

use gpui::{FontStyle, FontWeight, SharedString, StyledText, TextRun};

use crate::{fonts::Faces, theme};

/// What a stretch of code is, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    Str,
    Number,
    Function,
    Comment,
}

/// How a language marks its comments and which words are its own.
struct Lang {
    line_comment: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    keywords: &'static [&'static str],
}

const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "f32", "f64", "i32", "i64", "u8", "u32", "u64", "usize",
    "bool", "str", "String", "Vec", "Option", "Result", "Some", "None", "Ok", "Err",
];
const PYTHON: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True", "try", "while",
    "with", "yield", "self",
];
const SCRIPT: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];
const GO: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "false",
    "for",
    "func",
    "go",
    "if",
    "import",
    "interface",
    "map",
    "nil",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "true",
    "type",
    "var",
];
const SHELL: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case", "esac",
    "function", "return", "export", "local", "set", "echo", "cd", "exit",
];
const DATA: &[&str] = &["true", "false", "null"];

fn lang(name: &str) -> Option<Lang> {
    let (line_comment, block_comment, keywords): (&[&str], _, &[&str]) =
        match name.trim().to_ascii_lowercase().as_str() {
            "rust" | "rs" => (&["//"], Some(("/*", "*/")), RUST),
            "python" | "py" => (&["#"], None, PYTHON),
            "js" | "javascript" | "ts" | "typescript" | "jsx" | "tsx" | "mjs" => {
                (&["//"], Some(("/*", "*/")), SCRIPT)
            }
            "go" => (&["//"], Some(("/*", "*/")), GO),
            "sh" | "bash" | "zsh" | "shell" | "console" => (&["#"], None, SHELL),
            "toml" | "yaml" | "yml" | "ini" => (&["#"], None, DATA),
            "json" | "jsonc" => (&["//"], None, DATA),
            "c" | "cpp" | "c++" | "h" | "java" | "kotlin" | "swift" | "cs" => {
                (&["//"], Some(("/*", "*/")), SCRIPT)
            }
            _ => return None,
        };
    Some(Lang {
        line_comment,
        block_comment,
        keywords,
    })
}

/// The language a file is in, by its name: what a fenced block would say.
pub fn by_path(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.').map(|(_, e)| e) {
        Some(ext) => ext,
        None if name == "Justfile" || name == "Makefile" => "sh",
        None => "",
    }
}

/// `code` in `lang` cut into stretches, each with what it is, covering all
/// of it in order. A language it does not know is one plain stretch.
pub fn spans(code: &str, lang_name: &str) -> Vec<(Range<usize>, Kind)> {
    let Some(lang) = lang(lang_name) else {
        return vec![(0..code.len(), Kind::Plain)];
    };
    let mut out: Vec<(Range<usize>, Kind)> = Vec::new();
    let push = |r: Range<usize>, k: Kind, out: &mut Vec<(Range<usize>, Kind)>| {
        if r.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((last, kind)) if *kind == k && last.end == r.start => last.end = r.end,
            _ => out.push((r, k)),
        }
    };
    let b = code.as_bytes();
    let mut i = 0;
    while i < code.len() {
        let rest = &code[i..];
        if let Some(mark) = lang.line_comment.iter().find(|m| rest.starts_with(**m)) {
            let _ = mark;
            let end = rest.find('\n').map_or(code.len(), |n| i + n);
            push(i..end, Kind::Comment, &mut out);
            i = end;
            continue;
        }
        if let Some((open, close)) = lang.block_comment
            && rest.starts_with(open)
        {
            let end = rest[open.len()..]
                .find(close)
                .map_or(code.len(), |n| i + open.len() + n + close.len());
            push(i..end, Kind::Comment, &mut out);
            i = end;
            continue;
        }
        let c = b[i];
        if c == b'"' || c == b'\'' || c == b'`' {
            let mut j = i + 1;
            while j < code.len() && b[j] != c && b[j] != b'\n' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(code.len());
            push(i..end, Kind::Str, &mut out);
            i = end;
            continue;
        }
        if c.is_ascii_digit() {
            let end = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_'))
                .map_or(code.len(), |n| i + n);
            push(i..end, Kind::Number, &mut out);
            i = end;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let end = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .map_or(code.len(), |n| i + n);
            let word = &code[i..end];
            let kind = if lang.keywords.contains(&word) {
                Kind::Keyword
            } else if code[end..].starts_with('(') || code[end..].starts_with("!(") {
                Kind::Function
            } else {
                Kind::Plain
            };
            push(i..end, kind, &mut out);
            i = end;
            continue;
        }
        let end = i + rest.chars().next().map_or(1, char::len_utf8);
        push(i..end, Kind::Plain, &mut out);
        i = end;
    }
    out
}

/// `code` set in mono with its colours, ready to draw.
pub fn styled(faces: &Faces, code: &str, lang_name: &str) -> StyledText {
    let runs = spans(code, lang_name)
        .into_iter()
        .map(|(r, kind)| TextRun {
            len: r.len(),
            font: gpui::Font {
                family: faces.mono.clone(),
                features: gpui::FontFeatures::default(),
                fallbacks: Some(crate::fonts::fallbacks()),
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
            },
            color: match kind {
                Kind::Plain => theme::text(),
                Kind::Keyword => theme::code_program(),
                Kind::Str => theme::code_argument(),
                Kind::Number => theme::code_number(),
                Kind::Function => theme::code_function(),
                Kind::Comment => theme::faint(),
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect();
    StyledText::new(SharedString::from(code.to_owned())).with_runs(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds<'a>(code: &'a str, lang: &str) -> Vec<(&'a str, Kind)> {
        spans(code, lang)
            .into_iter()
            .filter(|(_, k)| *k != Kind::Plain)
            .map(|(r, k)| (&code[r], k))
            .collect()
    }

    #[test]
    fn rust_is_coloured_by_the_shape_of_its_words() {
        let code = "pub const X: f32 = 15.; // size\nfn well() { div().px(\"a\") }";
        assert_eq!(
            kinds(code, "rust"),
            [
                ("pub", Kind::Keyword),
                ("const", Kind::Keyword),
                ("f32", Kind::Keyword),
                ("15.", Kind::Number),
                ("// size", Kind::Comment),
                ("fn", Kind::Keyword),
                ("well", Kind::Function),
                ("div", Kind::Function),
                ("px", Kind::Function),
                ("\"a\"", Kind::Str),
            ]
        );
    }

    #[test]
    fn a_language_it_does_not_know_is_plain_and_whole() {
        assert_eq!(
            spans("whatever 1 \"x\"", "brainfuck"),
            [(0..14, Kind::Plain)]
        );
    }

    #[test]
    fn spans_cover_the_whole_of_the_code_in_order() {
        let code = "def f(x):\n    return 'é' # done";
        let s = spans(code, "py");
        assert_eq!(s.first().unwrap().0.start, 0);
        assert_eq!(s.last().unwrap().0.end, code.len());
        assert!(s.windows(2).all(|w| w[0].0.end == w[1].0.start));
    }

    #[test]
    fn a_file_name_says_its_language() {
        assert_eq!(by_path("desk/src/thread/style.rs"), "rs");
        assert_eq!(by_path("/tmp/hello.py"), "py");
        assert_eq!(by_path("Justfile"), "sh");
    }
}
