//! The desk's words in your language, by rust-i18n, as gpui-kit's are: one
//! locale for both, set as the settings say (`language`; none, the
//! system's). Each string is its English, which is also its key in
//! `locales/app.yml`; one with no translation there shows in English. What
//! an agent says, and what the server says (its errors, a git's), stays
//! as it was written, as t3code leaves it.

use gpui::SharedString;

/// The languages offered, by locale, each in its own words.
pub const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("zh-CN", "简体中文")];

/// `english`, in the language now set.
pub fn tr(english: &str) -> SharedString {
    SharedString::from(rust_i18n::t!(english).into_owned())
}

/// Speak `language` (a locale of `LANGUAGES`); none, the system's where
/// it is one of them, else English.
pub fn set(language: Option<&str>) {
    let system = sys_locale::get_locale().unwrap_or_default();
    let chosen = language.unwrap_or_else(|| of_system(&system));
    rust_i18n::set_locale(chosen);
}

/// The locale of `LANGUAGES` the system's (`zh-Hans-CN`, `en-GB`) is.
fn of_system(system: &str) -> &'static str {
    if system.starts_with("zh") && !system.contains("Hant") && !system.ends_with("TW") {
        "zh-CN"
    } else {
        "en"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_s_chinese_is_simplified_chinese_and_the_rest_english() {
        assert_eq!(of_system("zh-Hans-CN"), "zh-CN");
        assert_eq!(of_system("zh-CN"), "zh-CN");
        assert_eq!(of_system("zh-Hant-TW"), "en");
        assert_eq!(of_system("en-GB"), "en");
    }

    #[test]
    fn a_string_is_said_in_the_language_set_and_in_english_where_none_is() {
        rust_i18n::set_locale("zh-CN");
        assert_eq!(tr("Settings…"), "设置…");
        assert_eq!(
            tr("Nothing like this was written"),
            "Nothing like this was written"
        );
        rust_i18n::set_locale("en");
        assert_eq!(tr("Settings…"), "Settings…");
    }
}
