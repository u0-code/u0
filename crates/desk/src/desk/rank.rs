//! Finding what is typed among what is listed, as t3code's palette finds
//! it (`packages/shared/src/paletteSearch.ts`): every word typed must start
//! a word somewhere, whole words above prefixes above words typed together
//! above one typo above the middle of a word; groups ordered by their best.
//! The palette and the sidebar's search both rank here. A file's path is
//! found by `repo::paths`.

use std::{ops::Range, time::SystemTime};

/// A whole word.
const WHOLE: f32 = 1.;
/// The whole name, as typed.
const WHOLE_NAME: f32 = 1.5;
/// Words typed together ("vscode" for "VS Code").
const JOINED: f32 = 0.6;
/// One letter wrong, missing, extra or swapped.
const TYPO: f32 = 0.45;
/// Inside a word, in a title only, from four letters.
const MID_WORD: f32 = 0.3;
/// A match in hidden keywords alone ranks its group no higher than this.
const KEYWORD: f32 = 0.65;
/// Words a name may leave out of a query written as a phrase.
const FILLER: [&str; 10] = [
    "a", "an", "and", "for", "in", "of", "on", "the", "to", "with",
];

/// What is typed, folded as names are: lower case, without accents, split
/// into words; `exact` when it is all in double quotes.
#[derive(Debug, Clone, Default)]
pub struct Query {
    whole: Vec<char>,
    words: Vec<Vec<char>>,
    exact: bool,
}

impl Query {
    pub fn new(typed: &str) -> Self {
        let typed = typed.trim();
        let quote = |c: char| matches!(c, '"' | '“' | '”');
        let exact = typed.chars().count() > 2 && typed.starts_with(quote) && typed.ends_with(quote);
        let inner = typed.trim_matches(|c: char| {
            c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '“' | '”' | '‘' | '’')
        });
        let words: Vec<Vec<char>> = inner
            .split_whitespace()
            .map(|w| w.chars().map(fold).collect())
            .collect();
        let whole = words.join(&' ');
        Self {
            whole,
            words,
            exact,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// How a field counts toward a match.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// The name shown (the first field): words typed together count, and
    /// in a title (`mid_word`) so does the middle of a word.
    Name { mid_word: bool },
    /// Something else the row is known by: a branch, a number.
    Other,
    /// Where it is (its project): it helps, but never makes a match strong.
    Context,
    /// A hidden word: only at a word start, never with a typo; with
    /// `whole`, only a whole word.
    Keyword { whole: bool },
}

/// A field of a row, and what a match in it is worth.
#[derive(Debug, Clone, Copy)]
pub struct Field<'a> {
    pub text: &'a str,
    pub weight: f32,
    pub kind: Kind,
}

/// How well a row answers a query.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// What rows sort by: `quality`, lifted by use.
    pub score: f32,
    /// About 0.3 (inside a word) to 1.5 (the whole name).
    pub quality: f32,
    /// What its group ranks by, where lower than `quality`.
    pub group_quality: f32,
    /// Every word matched at a word start, without a typo.
    pub strong: bool,
    /// Every word matched a whole word.
    pub whole_words: bool,
    /// Where the name matched, in characters, to set in bold.
    pub ranges: Vec<Range<usize>>,
}

/// A text folded, and where its words start.
struct Folded {
    text: Vec<char>,
    starts: Vec<bool>,
}

/// One character as names are compared: lower case, its accent off, a curly
/// apostrophe straight.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    let c = match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    };
    match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è'..='ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ì'..='ï' | 'ī' | 'į' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'š' | 'ś' => 's',
        'ž' | 'ź' | 'ż' => 'z',
        'ς' => 'σ',
        '‘' | '’' => '\'',
        c => c,
    }
}

/// A Chinese or Japanese character, each of which starts a word.
fn cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff)
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric()
}

impl Folded {
    fn new(source: &str) -> Self {
        let chars: Vec<char> = source.chars().collect();
        let at = |i: Option<usize>| i.and_then(|i| chars.get(i)).copied();
        let starts = (0..chars.len())
            .map(|i| {
                let c = chars[i];
                let prev = at(i.checked_sub(1));
                let before = at(i.checked_sub(2));
                let next = at(Some(i + 1));
                // "Julius's": what follows an apostrophe goes on the word.
                let after_apostrophe =
                    matches!(prev, Some('\'' | '’')) && before.is_some_and(word_char);
                word_char(c)
                    && !after_apostrophe
                    && prev.is_none_or(|p| {
                        !word_char(p)
                            || (p.is_lowercase() && c.is_uppercase())
                            // The last capital of "JSONSchema" starts "Schema".
                            || (p.is_uppercase()
                                && c.is_uppercase()
                                && next.is_some_and(char::is_lowercase))
                            || p.is_numeric() != c.is_numeric()
                            || cjk(c)
                    })
            })
            .collect();
        Self {
            text: chars.into_iter().map(fold).collect(),
            starts,
        }
    }

    fn len(&self) -> usize {
        self.text.len()
    }

    fn starts_with_at(&self, at: usize, what: &[char]) -> bool {
        self.text.get(at..at + what.len()) == Some(what)
    }

    /// Where `what` is, each place.
    fn places<'a>(&'a self, what: &'a [char]) -> impl Iterator<Item = usize> + 'a {
        (0..self.len()).filter(move |i| self.starts_with_at(*i, what))
    }

    fn ends_word(&self, end: usize) -> bool {
        end >= self.len() || self.starts[end] || !word_char(self.text[end])
    }

    /// The end of the word at `at`, up to the next word start.
    fn word_end(&self, at: usize) -> usize {
        let mut end = at + 1;
        while end < self.len() && !self.starts[end] && word_char(self.text[end]) {
            end += 1;
        }
        end
    }

    /// The end of the run of letters and digits at `at`, across word starts.
    fn run_end(&self, at: usize) -> usize {
        let mut end = at;
        while end < self.len() && word_char(self.text[end]) {
            end += 1;
        }
        end
    }
}

/// A word typed, matched in a field: its worth and where.
struct Hit {
    score: f32,
    ranges: Vec<Range<usize>>,
}

/// The best place `token` matches in `field`.
fn match_token(
    field: &Folded,
    token: &[char],
    typos: bool,
    name: bool,
    mid_word: bool,
) -> Option<Hit> {
    let n = token.len();
    // A word typed with punctuation first (".ts") brings its own boundary.
    let own_boundary = !word_char(token[0]);
    let mut best: Option<Hit> = None;
    for at in field.places(token) {
        if field.starts[at] || own_boundary {
            let end = at + n;
            let score = if field.ends_word(end) {
                WHOLE
            } else {
                0.7 + 0.2 * n as f32 / (field.run_end(at) - at) as f32
            };
            if best.as_ref().is_none_or(|b| score > b.score) {
                best = Some(Hit {
                    score,
                    ranges: std::iter::once(at..end).collect(),
                });
            }
            if score >= WHOLE {
                return best;
            }
        } else if mid_word && n >= 4 && best.is_none() {
            best = Some(Hit {
                score: MID_WORD,
                ranges: std::iter::once(at..at + n).collect(),
            });
        }
    }
    let weak = best.as_ref().is_none_or(|b| b.score <= MID_WORD);
    if weak
        && name
        && let Some(joined) = joined(field, token)
    {
        return Some(joined);
    }
    if !weak || !typos || n < 3 || !token.iter().any(|c| c.is_alphabetic()) {
        return best;
    }
    // One typo, the first letter kept (unless the first two are swapped in
    // a longer word), across the whole run of letters ("githbu", "GitHub").
    for at in (0..field.len()).filter(|i| field.starts[*i]) {
        let first = field.text[at];
        let kept = first == token[0]
            || (n > 3 && first == token[1] && field.text.get(at + 1) == Some(&token[0]));
        if !kept {
            continue;
        }
        let word = &field.text[at..field.run_end(at)];
        // A short word may only miss a letter or swap two as it is typed.
        let short = n <= 4;
        let lengths: &[usize] = if short {
            &[n, n + 1]
        } else {
            &[n, n - 1, n + 1]
        };
        for &len in lengths {
            if len <= word.len() && one_edit(token, &word[..len], short && len == n) {
                return Some(Hit {
                    score: TYPO,
                    ranges: std::iter::once(at..at + len).collect(),
                });
            }
        }
    }
    best
}

/// Whether `a` and `b` are one edit apart: a letter changed, added, taken
/// away, or two side by side swapped; with `swap_only`, only a swap.
fn one_edit(a: &[char], b: &[char], swap_only: bool) -> bool {
    if a.len().abs_diff(b.len()) > usize::from(!swap_only) {
        return false;
    }
    let same = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[same..], &b[same..]);
    let tail = |s: &[char]| s.get(1..).unwrap_or(&[]).to_vec();
    let swapped = a.len() > 1 && b.len() > 1 && a[0] == b[1] && a[1] == b[0] && a[2..] == b[2..];
    if swap_only {
        return swapped;
    }
    swapped || a == b || tail(a) == tail(b) || tail(a) == b || a == tail(b)
}

/// Whole words typed together and the start of the last ("gomod" for "Go
/// modules"), but not short words before a short start ("inth" in "in
/// thread").
fn joined(field: &Folded, token: &[char]) -> Option<Hit> {
    let first_start = field.starts.iter().position(|s| *s);
    for start in (0..field.len()).filter(|i| field.starts[*i] && field.text[*i] == token[0]) {
        let mut runs: Vec<Range<usize>> = Vec::new();
        let (mut at, mut rest) = (start, token);
        while !rest.is_empty() && at < field.len() {
            let end = field.word_end(at);
            if rest.len() <= end - at && field.starts_with_at(at, rest) {
                runs.push(at..at + rest.len());
                rest = &[];
            } else if rest.starts_with(&field.text[at..end]) {
                runs.push(at..end);
                rest = &rest[end - at..];
                at = end;
                while at < field.len() && !field.starts[at] {
                    at += 1;
                }
            } else {
                break;
            }
        }
        let Some(last) = runs.last().cloned() else {
            continue;
        };
        if !rest.is_empty() || runs.len() < 2 {
            continue;
        }
        if last.end == field.word_end(last.start)
            || last.len() >= 3
            || Some(start) == first_start
            || runs[..runs.len() - 1].iter().all(|r| r.len() >= 3)
        {
            return Some(Hit {
                score: JOINED,
                ranges: runs,
            });
        }
    }
    None
}

/// Where `words` stand side by side in `field`, as whole words.
fn phrase_in(field: &Folded, words: &[Vec<char>]) -> Option<Range<usize>> {
    let own_boundary = !word_char(words[0][0]);
    'places: for start in field.places(&words[0]) {
        if !field.starts[start] && !own_boundary {
            continue;
        }
        let mut end = start + words[0].len();
        for word in &words[1..] {
            let mut next = end;
            while next < field.len() && field.text[next].is_whitespace() {
                next += 1;
            }
            if next == end || !field.starts_with_at(next, word) {
                continue 'places;
            }
            end = next + word.len();
        }
        if field.ends_word(end) {
            return Some(start..end);
        }
    }
    None
}

/// How well `fields` answer `query`: each word typed must match one of
/// them; the quality is the mean of each word's best, lifted where the
/// name is the whole query or holds it as a phrase. None when a word
/// matches nowhere.
pub fn find(query: &Query, fields: &[Field], typos: bool) -> Option<Found> {
    if query.is_empty() {
        return None;
    }
    let folded: Vec<Folded> = fields.iter().map(|f| Folded::new(f.text)).collect();
    let name = folded.first().filter(|f| f.len() > 0);
    if query.exact {
        let mut best = 0f32;
        let mut range = None;
        for (n, (f, field)) in folded.iter().zip(fields).enumerate() {
            if let Some(r) = phrase_in(f, &query.words) {
                best = best.max(field.weight);
                if n == 0 {
                    range = Some(r);
                }
            }
        }
        if best == 0. {
            return None;
        }
        let bonus = match &range {
            Some(r) if r.start == 0 && name.is_some_and(|f| r.end == f.len()) => WHOLE_NAME - WHOLE,
            Some(_) if query.words.len() > 1 => 0.25,
            _ => 0.,
        };
        return Some(found(
            best + bonus,
            true,
            best >= WHOLE,
            range.into_iter().collect(),
        ));
    }
    let filler = |w: &[char]| FILLER.iter().any(|f| f.chars().eq(w.iter().copied()));
    let filler_optional = query.words.iter().any(|w| !filler(w));
    let (mut total, mut counted, mut strong, mut whole) = (0f32, 0, true, true);
    let mut ranges = Vec::new();
    for token in &query.words {
        let (mut best, mut best_name, mut at_start) = (0f32, None, false);
        for (n, (f, field)) in folded.iter().zip(fields).enumerate() {
            if f.len() == 0 || (n > 0 && at_start && best >= field.weight) {
                continue;
            }
            let (mid_word, keyword, least) = match field.kind {
                Kind::Name { mid_word } => (mid_word, false, 0.),
                Kind::Keyword { whole } => (false, true, if whole { WHOLE } else { 0.7 }),
                Kind::Other | Kind::Context => (false, false, 0.),
            };
            let Some(hit) = match_token(f, token, typos && !keyword, n == 0, mid_word) else {
                continue;
            };
            if hit.score < least {
                continue;
            }
            if hit.score >= 0.7 && field.kind != Kind::Context {
                at_start = true;
            }
            best = best.max(hit.score * field.weight);
            if n == 0 {
                best_name = Some(hit);
            }
        }
        if best == 0. {
            if filler_optional && filler(token) {
                continue;
            }
            return None;
        }
        strong &= at_start;
        ranges.extend(best_name.map(|h| h.ranges).unwrap_or_default());
        total += best;
        counted += 1;
        whole &= if filler(token) {
            filler_optional
        } else {
            token.len() >= 3 && best >= WHOLE
        };
    }
    let mut quality = total / counted as f32;
    if let Some(name) = name {
        if name.text == query.whole {
            quality += WHOLE_NAME - WHOLE;
        } else if query.words.len() > 1
            && name
                .text
                .windows(query.whole.len())
                .any(|w| w == query.whole)
        {
            quality += 0.25;
        }
    }
    ranges.sort_by_key(|r| r.start);
    Some(found(quality, strong, whole, ranges))
}

fn found(quality: f32, strong: bool, whole_words: bool, ranges: Vec<Range<usize>>) -> Found {
    Found {
        score: quality,
        quality,
        group_quality: quality,
        strong,
        whole_words,
        ranges,
    }
}

/// A name and the words it is also known by (an action's, a setting's):
/// a hidden word counts less, and for a short query only whole; a match in
/// them alone never lifts the group over a name that matches.
pub fn terms(query: &Query, name: &str, keywords: &[&str], typos: bool) -> Option<Found> {
    // Filler words ("the ci") do not make a short query long.
    let meaningful: Vec<&Vec<char>> = query
        .words
        .iter()
        .filter(|w| !FILLER.iter().any(|f| f.chars().eq(w.iter().copied())))
        .collect();
    let short = if meaningful.is_empty() {
        query.whole.len() < 4
    } else {
        meaningful.iter().map(|w| w.len() + 1).sum::<usize>() - 1 < 4
    };
    let mut fields = vec![Field {
        text: name,
        weight: 1.,
        kind: Kind::Name { mid_word: false },
    }];
    fields.extend(keywords.iter().map(|k| Field {
        text: k,
        weight: if short { 0.8 } else { 0.6 },
        kind: Kind::Keyword { whole: short },
    }));
    let mut f = find(query, &fields, typos)?;
    if f.ranges.is_empty() {
        f.group_quality = f.quality.min(KEYWORD);
    }
    Some(f)
}

/// A session, by its title, its pull request, its branch (without the
/// prefix Ghost gives a worktree's) and its project's name; lifted by how
/// lately you used it.
pub fn session(
    query: &Query,
    title: &str,
    pr: Option<u64>,
    branch: Option<&str>,
    project: &str,
    used: SystemTime,
    now: SystemTime,
    typos: bool,
) -> Option<Found> {
    let pr = pr.map(|n| format!("#{n}")).unwrap_or_default();
    let branch = branch.map_or("", |b| b.strip_prefix("ghost/").unwrap_or(b));
    let fields = [
        Field {
            text: title,
            weight: 1.,
            kind: Kind::Name { mid_word: true },
        },
        Field {
            text: &pr,
            weight: 1.,
            kind: Kind::Other,
        },
        Field {
            text: branch,
            weight: 0.6,
            kind: Kind::Other,
        },
        Field {
            text: project,
            weight: 0.25,
            kind: Kind::Context,
        },
    ];
    let mut f = find(query, &fields, typos)?;
    f.score = f.quality * boost(used, now);
    // A title that is the whole query never lifts its group over an
    // action of that name: every new session is "New session" a while.
    f.group_quality = f.quality.min(WHOLE_NAME - 0.01);
    Some(f)
}

/// How much use lifts a session: 1 for one long left, about 2 for one used
/// today; half as much every fourteen days.
fn boost(used: SystemTime, now: SystemTime) -> f32 {
    let days = now.duration_since(used).unwrap_or_default().as_secs_f32() / 86_400.;
    1. + 0.5f32.powf(days / 14.)
}

/// Whole words and word starts share a class, so that "Compact" does not
/// lift settings over "Compaction".
fn class(quality: f32) -> u8 {
    if quality >= WHOLE_NAME {
        3
    } else if quality >= 0.7 {
        2
    } else {
        u8::from(quality >= 0.4)
    }
}

/// The rows of each group that answer the query, each group in order of
/// how well (word matches, then the class of match, whole words, then
/// score), and the groups by their best class, ties in the order given.
/// Typos are forgiven only where nothing matched at a word start.
pub fn rank<G, T>(
    groups: Vec<(G, Vec<T>)>,
    score: impl Fn(&T, bool) -> Option<Found>,
) -> Vec<(G, Vec<(T, Found)>)> {
    let pass = |typos: bool| -> Vec<Vec<Option<Found>>> {
        groups
            .iter()
            .map(|(_, items)| items.iter().map(|t| score(t, typos)).collect())
            .collect()
    };
    let mut found = pass(false);
    if !found.iter().flatten().flatten().any(|f| f.strong) {
        found = pass(true);
    }
    let mut ranked: Vec<(G, Vec<(T, Found)>)> = groups
        .into_iter()
        .zip(found)
        .filter_map(|((group, items), found)| {
            let mut rows: Vec<(T, Found)> = items
                .into_iter()
                .zip(found)
                .filter_map(|(t, f)| Some((t, f?)))
                .collect();
            rows.sort_by(|(_, a), (_, b)| {
                let key = |f: &Found| (class(f.quality).min(2), class(f.quality), f.whole_words);
                key(b).cmp(&key(a)).then(b.score.total_cmp(&a.score))
            });
            (!rows.is_empty()).then_some((group, rows))
        })
        .collect();
    // A stable sort: groups of one class keep the order given.
    ranked.sort_by_key(|(_, rows)| {
        std::cmp::Reverse(rows.iter().map(|(_, f)| class(f.group_quality)).max())
    });
    ranked
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn titles(typed: &str, list: &[&str]) -> Vec<String> {
        let now = SystemTime::now();
        let q = Query::new(typed);
        rank(vec![((), list.to_vec())], |t, typos| {
            session(
                &q,
                t,
                None,
                None,
                "",
                now - Duration::from_hours(30 * 24),
                now,
                typos,
            )
        })
        .into_iter()
        .flat_map(|((), rows)| rows.into_iter().map(|(t, _)| t.to_owned()))
        .collect()
    }

    #[test]
    fn whole_words_rank_above_prefixes_above_the_middle_of_a_word() {
        assert_eq!(
            titles(
                "auth",
                &[
                    "Fix oauth redirect",
                    "Authentication cleanup",
                    "Share auth across worktrees"
                ]
            ),
            [
                "Share auth across worktrees",
                "Authentication cleanup",
                "Fix oauth redirect"
            ]
        );
    }

    #[test]
    fn camel_case_paths_and_digits_start_words_and_short_words_never_match_inside_one() {
        let at = |title: &str, typed: &str| {
            find(
                &Query::new(typed),
                &[Field {
                    text: title,
                    weight: 1.,
                    kind: Kind::Name { mid_word: true },
                }],
                false,
            )
            .and_then(|f| f.ranges.first().cloned())
        };
        assert_eq!(at("useThreadSearch hook", "search"), Some(9..15));
        assert_eq!(at("fix/sidebar-perf", "perf"), Some(12..16));
        assert_eq!(at("Upgrade to pnpm12", "12"), Some(15..17));
        assert_eq!(at("Fix JSONSchema parsing", "sch"), Some(8..11));
        assert_eq!(at("Rename index.ts", ".ts"), Some(12..15));
        assert_eq!(at("Decision log", "ci"), None);
        assert_eq!(at("Research notes", "arch"), Some(4..8));
    }

    #[test]
    fn words_typed_together_are_found_but_scattered_letters_are_not() {
        let list = ["VS Code extension settings", "Fix UI bug in sidebar"];
        assert_eq!(titles("vscode", &list), ["VS Code extension settings"]);
        assert_eq!(titles("fixuibug", &list), ["Fix UI bug in sidebar"]);
        assert!(titles("lint", &list).is_empty());
        assert!(titles("inth", &["Show skill names in thread overview"]).is_empty());
        assert_eq!(
            titles("gomod", &["Migrate to Go modules"]),
            ["Migrate to Go modules"]
        );
    }

    #[test]
    fn one_typo_is_forgiven_only_when_nothing_matches_exactly() {
        assert_eq!(
            titles("githbu", &["GitHub API usage", "Other"]),
            ["GitHub API usage"]
        );
        assert_eq!(
            titles("railwya", &["Hosting alternatives to Railway"]).len(),
            1
        );
        assert_eq!(
            titles("srv", &["Server restart", "Other"]),
            ["Server restart"]
        );
        assert!(titles("ing", &["Nightly build fix"]).is_empty());
        assert!(titles("cron", &["Config cleanup"]).is_empty());
        assert_eq!(
            titles(
                "ilde",
                &["Typesafe App Builder", "De-risk idle tunnel cleanup"]
            ),
            ["De-risk idle tunnel cleanup", "Typesafe App Builder"]
        );
    }

    #[test]
    fn case_accents_quotes_and_filler_words_do_not_stand_in_the_way() {
        assert_eq!(titles("CAFE", &["Café menu"]), ["Café menu"]);
        assert_eq!(titles("don't", &["Don’t merge yet"]), ["Don’t merge yet"]);
        assert_eq!(
            titles("fix the sidebar", &["Fix Sidebar Lag"]),
            ["Fix Sidebar Lag"]
        );
        assert!(titles("the", &["Fix sidebar lag"]).is_empty());
        let list = [
            "Fix sidebar lag",
            "Sidebar scroll lag",
            "Lag in the sidebar",
        ];
        assert_eq!(titles("sidebar lag", &list).len(), 3);
        assert_eq!(titles("\"sidebar lag\"", &list), ["Fix sidebar lag"]);
        assert!(titles("\"sidebra lag\"", &list).is_empty());
    }

    #[test]
    fn of_two_equal_matches_the_one_used_lately_comes_first() {
        let now = SystemTime::now();
        let q = Query::new("goldbug");
        let days = |d: u64| now - Duration::from_hours(d * 24);
        let list = [
            ("Goldbug epilogue", days(20)),
            ("Goldbug processes", days(1)),
        ];
        let ranked = rank(vec![((), list.to_vec())], |(t, used), typos| {
            session(&q, t, None, None, "", *used, now, typos)
        });
        assert_eq!(ranked[0].1[0].0.0, "Goldbug processes");
    }

    #[test]
    fn groups_go_by_their_best_match_and_keep_their_order_on_ties() {
        let q = Query::new("settings");
        let ranked = rank(
            vec![
                ("actions", vec!["Open settings", "New session"]),
                (
                    "sessions",
                    vec!["Settings sync rewrite", "Session settings"],
                ),
                ("projects", vec!["Settings app"]),
            ],
            |t, typos| terms(&q, t, &[], typos),
        );
        let names: Vec<(&str, Vec<&str>)> = ranked
            .into_iter()
            .map(|(g, rows)| (g, rows.into_iter().map(|(t, _)| t).collect()))
            .collect();
        assert_eq!(
            names,
            [
                ("actions", vec!["Open settings"]),
                (
                    "sessions",
                    vec!["Settings sync rewrite", "Session settings"]
                ),
                ("projects", vec!["Settings app"]),
            ]
        );
    }

    #[test]
    fn a_hidden_word_needs_a_word_start_and_never_forgives_a_typo() {
        let q = |t: &str| Query::new(t);
        assert!(terms(&q("arch"), "Search project contents", &[], false).is_none());
        assert!(terms(&q("rust"), "GitHub sharing", &["trusted"], false).is_none());
        assert!(terms(&q("duckdb"), "Search engine", &["duckduckgo"], true).is_none());
        let by_keyword = terms(&q("preferences"), "Open settings", &["preferences"], false);
        assert!(by_keyword.is_some_and(|f| f.group_quality <= KEYWORD));
    }
}
