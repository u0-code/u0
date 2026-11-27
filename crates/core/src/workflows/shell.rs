//! Where a value stands in a workflow's command, read as bash reads it
//! (brush-parser, the parser of the brush shell): its grammar is bash's,
//! stricter than the `/bin/sh` that runs the command on Linux (dash).
//!
//! A value is taken only where the shell hands it to a program as a word:
//! as an argument of a simple command whose name is written plainly and is
//! not a builtin that reads its words as names, numbers or code. Anywhere
//! else (arithmetic, a subscript, `${x:off}`, `[[ ]]`, an assignment, a
//! redirection, a here-document, a pattern, a function's body) the command
//! is refused, as is one that does not parse: what is not known to be a
//! word is never guessed to be one.

use brush_parser::ast::{
    Command, CommandPrefixOrSuffixItem, CompoundCommand, CompoundList, SimpleCommand,
};
use brush_parser::word::WordPiece;
use brush_parser::{Parser, ParserOptions};

/// How a value stands where it is put: bare, or within quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Within {
    Bare,
    Single,
    Double,
}

/// What stands for the `n`th value while the command is parsed: text that
/// is a plain word in every context, and no part of a name.
pub fn mark(n: usize) -> String {
    format!("%ghost:{n}%")
}

/// The builtins a value may be an argument of: they print it or change
/// to it, and never read it as a name, a number or code. Every other
/// builtin of bash or dash (`eval`, `let`, `declare`, `read`, `test`,
/// `trap`, `unset`, ...) is refused.
const PLAIN: [&str; 3] = ["echo", "printf", "cd"];

/// The builtins of bash (`compgen -b`) and dash.
const BUILTINS: &[&str] = &[
    ".",
    ":",
    "[",
    "alias",
    "bg",
    "bind",
    "break",
    "builtin",
    "caller",
    "cd",
    "chdir",
    "command",
    "compgen",
    "complete",
    "compopt",
    "continue",
    "declare",
    "dirs",
    "disown",
    "echo",
    "enable",
    "eval",
    "exec",
    "exit",
    "export",
    "false",
    "fc",
    "fg",
    "getopts",
    "hash",
    "help",
    "history",
    "jobs",
    "kill",
    "let",
    "local",
    "logout",
    "mapfile",
    "popd",
    "printf",
    "pushd",
    "pwd",
    "read",
    "readarray",
    "readonly",
    "return",
    "set",
    "shift",
    "shopt",
    "source",
    "suspend",
    "test",
    "times",
    "trap",
    "true",
    "type",
    "typeset",
    "ulimit",
    "umask",
    "unalias",
    "unset",
    "wait",
];

/// How each of the `count` marks in `line` stands, or `None` when one is
/// anywhere but an argument of a plain command, or `line` does not parse.
pub fn place(line: &str, count: usize) -> Option<Vec<Within>> {
    // Where a here-document ends, dash and bash do not always agree with
    // the parser (a backslash before its newline): whatever stands after
    // one may be read otherwise than the parser read it.
    if line.contains("<<") {
        return None;
    }
    let mut walk = Walk {
        found: vec![None; count],
        twice: false,
    };
    walk.program(line)?;
    if walk.twice {
        return None;
    }
    walk.found.into_iter().collect()
}

struct Walk {
    found: Vec<Option<Within>>,
    twice: bool,
}

impl Walk {
    fn program(&mut self, text: &str) -> Option<()> {
        let program = Parser::new(text.as_bytes(), &ParserOptions::default())
            .parse_program()
            .ok()?;
        for list in &program.complete_commands {
            self.list(list)?;
        }
        Some(())
    }

    fn list(&mut self, list: &CompoundList) -> Option<()> {
        for item in &list.0 {
            for (_, pipeline) in &item.0 {
                for command in &pipeline.seq {
                    self.command(command)?;
                }
            }
        }
        Some(())
    }

    /// A function is refused whole: a value in its body would read the
    /// function's arguments, and a call of it may run its words.
    fn command(&mut self, command: &Command) -> Option<()> {
        match command {
            Command::Simple(simple) => self.simple(simple),
            Command::Function(_) => None,
            Command::Compound(compound, _) => match compound {
                CompoundCommand::BraceGroup(c) => self.list(&c.list),
                CompoundCommand::Subshell(c) => self.list(&c.list),
                CompoundCommand::ForClause(c) => self.list(&c.body.list),
                CompoundCommand::ArithmeticForClause(c) => self.list(&c.body.list),
                CompoundCommand::WhileClause(c) | CompoundCommand::UntilClause(c) => {
                    self.list(&c.0)?;
                    self.list(&c.1.list)
                }
                CompoundCommand::IfClause(c) => {
                    self.list(&c.condition)?;
                    self.list(&c.then)?;
                    for other in c.elses.iter().flatten() {
                        if let Some(condition) = &other.condition {
                            self.list(condition)?;
                        }
                        self.list(&other.body)?;
                    }
                    Some(())
                }
                CompoundCommand::CaseClause(c) => {
                    for case in &c.cases {
                        if let Some(list) = &case.cmd {
                            self.list(list)?;
                        }
                    }
                    Some(())
                }
                CompoundCommand::Arithmetic(_)
                | CompoundCommand::Coprocess(_)
                | CompoundCommand::ExtendedTest(_) => Some(()),
            },
        }
    }

    /// The marks in a simple command's arguments, if its name is plain:
    /// written with nothing the shell expands, and no builtin but those
    /// that only print or change folder (`printf` with no option, for
    /// `printf -v` reads a name). Marks anywhere else in it go unfound.
    fn simple(&mut self, simple: &SimpleCommand) -> Option<()> {
        let Some(name) = &simple.word_or_name else {
            return Some(());
        };
        let name = name.value.as_str();
        // An alias renames a command for the shell, which the parser
        // never sees: `alias e=eval; e {{v}}`. A command that makes one
        // is refused whole, as one that makes a function is.
        if name == "alias" {
            return None;
        }
        let written = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./+-".contains(c));
        let builtin = BUILTINS.contains(&name);
        if !written || (builtin && !PLAIN.contains(&name)) {
            return Some(());
        }
        let words: Vec<&str> = (simple.suffix.iter())
            .flat_map(|s| &s.0)
            .filter_map(|item| match item {
                // `a=b` given to a command that declares nothing is a word.
                CommandPrefixOrSuffixItem::Word(w)
                | CommandPrefixOrSuffixItem::AssignmentWord(_, w) => Some(w.value.as_str()),
                _ => None,
            })
            .collect();
        let first = words.first().copied().unwrap_or_default();
        if name == "printf" && (first.starts_with('-') || first.contains("%ghost:")) {
            return Some(());
        }
        for word in words {
            self.word(word)?;
        }
        Some(())
    }

    fn word(&mut self, word: &str) -> Option<()> {
        let pieces = brush_parser::word::parse(word, &ParserOptions::default()).ok()?;
        for piece in pieces {
            match piece.piece {
                WordPiece::Text(text) => self.marks(&text, Within::Bare),
                WordPiece::SingleQuotedText(text) => self.marks(&text, Within::Single),
                WordPiece::DoubleQuotedSequence(inner)
                | WordPiece::GettextDoubleQuotedSequence(inner) => {
                    for piece in inner {
                        match piece.piece {
                            WordPiece::Text(text) => self.marks(&text, Within::Double),
                            WordPiece::CommandSubstitution(program) => self.program(&program)?,
                            _ => {}
                        }
                    }
                }
                WordPiece::CommandSubstitution(program) => self.program(&program)?,
                _ => {}
            }
        }
        Some(())
    }

    fn marks(&mut self, text: &str, within: Within) {
        for (n, found) in self.found.iter_mut().enumerate() {
            if text.contains(&mark(n + 1)) {
                self.twice |= found.replace(within).is_some();
            }
        }
    }
}
