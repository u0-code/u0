//! The desk's words in your language, by rust-i18n, as gpui-kit's are: one
//! locale for both, set as the settings say (`language`; none, the
//! system's). Each string is its English, which is also its key in
//! `locales/app.yml`; one with no translation there shows in English. What
//! an agent says, and what the server says (its errors, a git's), stays
//! as it was written, as t3code leaves it.

use gpui::SharedString;

/// The languages offered, by locale, each in its own words: t3code's.
pub const LANGUAGES: [(&str, &str); 10] = [
    ("en", "English"),
    ("zh-CN", "简体中文"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("fr", "Français"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("pt-BR", "Português (Brasil)"),
    ("ru", "Русский"),
    ("sv", "Svenska"),
];

/// `english`, in the language now set.
pub fn tr(english: &str) -> SharedString {
    SharedString::from(rust_i18n::t!(english).into_owned())
}

/// `english`, in the language now set, each `%{name}` in it filled in with
/// its value in `args`.
pub fn tr_with(english: &str, args: &[(&str, &dyn std::fmt::Display)]) -> SharedString {
    let (names, values): (Vec<&str>, Vec<String>) = args
        .iter()
        .map(|(name, value)| (*name, value.to_string()))
        .unzip();
    let text = rust_i18n::t!(english);
    SharedString::from(rust_i18n::replace_patterns(&text, &names, &values))
}

/// Speak `language` (a locale of `LANGUAGES`); none, the system's where
/// it is one of them, else English.
pub fn set(language: Option<&str>) {
    let system = sys_locale::get_locale().unwrap_or_default();
    let chosen = language.unwrap_or_else(|| of_system(&system));
    rust_i18n::set_locale(chosen);
}

/// The locale of `LANGUAGES` the system's (`zh-Hans-CN`, `de-AT`) is, by
/// its language. Chinese in Traditional characters (`Hant`, or Taiwan,
/// Hong Kong and Macau without a script) is not offered: English, as any
/// language not offered is.
fn of_system(system: &str) -> &'static str {
    let mut tags = system.split(['-', '_']);
    let language = tags.next().unwrap_or_default();
    let rest: Vec<&str> = tags.collect();
    match language {
        "zh" if rest.contains(&"Hans") => "zh-CN",
        "zh" if rest.iter().any(|t| ["Hant", "TW", "HK", "MO"].contains(t)) => "en",
        "zh" => "zh-CN",
        "pt" => "pt-BR",
        _ => (LANGUAGES.iter())
            .map(|(locale, _)| *locale)
            .find(|locale| *locale == language)
            .unwrap_or("en"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_s_language_is_the_one_offered_and_traditional_chinese_english() {
        for (system, offered) in [
            ("zh-Hans-CN", "zh-CN"),
            ("zh-CN", "zh-CN"),
            ("zh-Hans-HK", "zh-CN"),
            ("zh-Hant-TW", "en"),
            ("zh-HK", "en"),
            ("zh-MO", "en"),
            ("de-AT", "de"),
            ("pt-PT", "pt-BR"),
            ("ja_JP", "ja"),
            ("en-GB", "en"),
            ("nl-NL", "en"),
        ] {
            assert_eq!(of_system(system), offered, "{system}");
        }
    }

    #[test]
    fn a_string_is_said_in_a_language_and_in_english_where_it_has_none() {
        let zh = |key| rust_i18n::t!(key, locale = "zh-CN").into_owned();
        assert_eq!(zh("Settings…"), "设置…");
        assert_eq!(
            zh("Nothing like this was written"),
            "Nothing like this was written"
        );
    }

    /// The `%{name}`s of `text`, by name.
    fn holes(text: &str) -> Vec<&str> {
        let mut found: Vec<&str> = (text.split("%{").skip(1))
            .filter_map(|s| s.split_once('}').map(|(name, _)| name))
            .collect();
        found.sort_unstable();
        found
    }

    #[test]
    fn every_string_is_in_every_language_with_its_values_kept() {
        let mut said: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
        for line in include_str!("../locales/app.yml").lines() {
            if let Some(key) = line.strip_prefix('"').and_then(|l| l.strip_suffix("\":")) {
                said.push((key, Vec::new()));
            } else if let (Some((_, of)), Some((locale, text))) =
                (said.last_mut(), line.trim().split_once(": "))
            {
                of.push((locale, text));
            }
        }
        assert!(said.len() > 1000, "{} strings", said.len());
        for (key, of) in &said {
            for (locale, _) in &LANGUAGES[1..] {
                let text = of.iter().find(|(l, _)| l == locale).map(|(_, t)| *t);
                let text = text.unwrap_or_else(|| panic!("{key:?} has no {locale}"));
                assert_eq!(holes(text), holes(key), "{key:?} in {locale}");
            }
        }
    }
}
