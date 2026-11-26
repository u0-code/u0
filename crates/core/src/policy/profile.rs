//! The policy on a Mac: what a configuration profile (from MDM, or
//! installed by hand) sets in Ghost's preference domain, read as macOS
//! apps read managed preferences, with `CFPreferencesCopyAppValue`. A key
//! counts only where `CFPreferencesAppValueIsForced` says a profile
//! forces it: one written with `defaults write` is no policy.
//! `policy/ghost.mobileconfig` is the example, every key in it.

use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use core_foundation_sys::{
    number::CFNumberIsFloatType,
    preferences::{CFPreferencesAppValueIsForced, CFPreferencesCopyAppValue},
};
use serde_json::Value;

use super::Policy;

/// Ghost's preference domain: Ghost.app's `CFBundleIdentifier`, as
/// bundle.sh writes it.
const DOMAIN: &str = "local.ghost.desk";

/// A policy's keys, `Policy`'s fields: each is asked for by name, since
/// the managed values of a domain are not listed.
const KEYS: [&str; 4] = ["organization", "support_url", "settings", "limits"];

/// The policy the profiles installed force; none where they force nothing.
pub fn read() -> Result<Option<Policy>, String> {
    let domain = CFString::new(DOMAIN);
    policy(|key| {
        let key = CFString::new(key);
        // SAFETY: both strings live through the calls; a value copied is
        // owned here (the create rule) and released as it is dropped.
        let value = unsafe {
            let value =
                CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef());
            if value.is_null() {
                return Ok(None);
            }
            let value = CFType::wrap_under_create_rule(value);
            let forced = CFPreferencesAppValueIsForced(
                key.as_concrete_TypeRef(),
                domain.as_concrete_TypeRef(),
            );
            (forced != 0).then_some(value)
        };
        value.as_ref().map(json).transpose()
    })
    .map_err(|e| format!("The policy in the profile for {DOMAIN} does not read: {e}"))
}

/// The policy of the values `get` gives for its keys; none where it gives
/// none.
fn policy(get: impl Fn(&str) -> Result<Option<Value>, String>) -> Result<Option<Policy>, String> {
    let mut found = serde_json::Map::new();
    for key in KEYS {
        if let Some(value) = get(key)? {
            found.insert(key.into(), value);
        }
    }
    if found.is_empty() {
        return Ok(None);
    }
    serde_json::from_value(Value::Object(found))
        .map(Some)
        .map_err(|e| e.to_string())
}

/// A property list's value as the JSON of policy.json: a boolean, a
/// number, a string, an array, a dict. A date or data, which no policy
/// holds, is refused.
fn json(value: &CFType) -> Result<Value, String> {
    // SAFETY: each pointer is one the array or dict holds, retained by
    // the wrapper for as long as it is read.
    let wrap = |p| unsafe { CFType::wrap_under_get_rule(p) };
    if let Some(b) = value.downcast::<CFBoolean>() {
        Ok(Value::Bool(b.into()))
    } else if let Some(n) = value.downcast::<CFNumber>() {
        // SAFETY: `n` is a number, alive for the call.
        let number = if unsafe { CFNumberIsFloatType(n.as_concrete_TypeRef()) } == 0 {
            n.to_i64().map(Value::from)
        } else {
            n.to_f64()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
        };
        number.ok_or_else(|| "a number that is not finite".into())
    } else if let Some(s) = value.downcast::<CFString>() {
        Ok(Value::String(s.to_string()))
    } else if let Some(a) = value.downcast::<CFArray>() {
        a.get_all_values()
            .into_iter()
            .map(|v| json(&wrap(v)))
            .collect()
    } else if let Some(d) = value.downcast::<CFDictionary>() {
        let (keys, values) = d.get_keys_and_values();
        keys.into_iter()
            .zip(values)
            .map(|(k, v)| {
                let k = wrap(k)
                    .downcast::<CFString>()
                    .ok_or("a dict's key that is not a string")?;
                Ok((k.to_string(), json(&wrap(v))?))
            })
            .collect::<Result<serde_json::Map<_, _>, String>>()
            .map(Value::Object)
    } else {
        Err("a date or data, which no policy holds".into())
    }
}

#[cfg(test)]
mod tests {
    use core_foundation::{data::CFData, propertylist};

    use super::*;
    use crate::policy::Limit;

    /// A property list, read as `CFPreferences` reads a profile's values.
    fn plist(xml: &str) -> Result<Value, String> {
        let data = CFData::from_buffer(xml.as_bytes());
        let (list, _) =
            propertylist::create_with_data(data, propertylist::kCFPropertyListImmutable)
                .map_err(|e| e.to_string())?;
        // SAFETY: a property list made here, owned by the wrapper.
        json(&unsafe { CFType::wrap_under_create_rule(list) })
    }

    /// The policy a profile's one payload forces, as `read` finds it.
    fn payload(profile: &Value) -> Result<Option<Policy>, String> {
        let payload = &profile["PayloadContent"][0];
        policy(|key| Ok(payload.get(key).cloned()))
    }

    #[test]
    fn the_example_profile_sets_every_key_in_ghosts_domain_and_reads() {
        let profile = plist(include_str!("../../../../policy/ghost.mobileconfig")).unwrap();
        let content = profile["PayloadContent"][0].as_object().unwrap();
        assert_eq!(content["PayloadType"], DOMAIN);
        let keys: Vec<_> = (content.keys())
            .filter(|k| !k.starts_with("Payload"))
            .collect();
        assert_eq!(
            keys.len(),
            KEYS.len(),
            "the example names every key: {keys:?}"
        );
        let p = payload(&profile).unwrap().unwrap();
        assert_eq!(p.organization, "Example Corp");
        assert_eq!(p.settings["telemetry"].value, 0);
        assert!(p.settings["telemetry"].locked && !p.settings["branch_prefix"].locked);
        assert_eq!(p.settings["check_updates"].value, false);
        assert!(p.allows(Limit::Agent("codex-acp")).is_ok());
        assert!(p.allows(Limit::Agent("gemini")).is_err());
        assert!(p.allows(Limit::Tunnel).is_err() && p.allows(Limit::Remote).is_err());
    }

    #[test]
    fn plist_values_map_to_the_json_policy_json_holds() {
        let v = plist(
            r"<plist><dict><key>a</key><array><true/><integer>-3</integer>
            <real>1.5</real><string>x</string><dict/></array></dict></plist>",
        )
        .unwrap();
        assert_eq!(v, serde_json::json!({"a": [true, -3, 1.5, "x", {}]}));
        let date = plist("<plist><date>2026-01-01T00:00:00Z</date></plist>");
        assert!(date.unwrap_err().contains("date"));
    }

    #[test]
    fn a_profile_forcing_nothing_is_no_policy_and_a_wrong_kind_says_why() {
        assert!(payload(&serde_json::json!({})).unwrap().is_none());
        let wrong = plist(
            r"<plist><dict><key>PayloadContent</key><array><dict>
            <key>organization</key><string>Corp</string>
            <key>limits</key><dict><key>tunnel</key><string>no</string></dict>
            </dict></array></dict></plist>",
        )
        .unwrap();
        assert!(payload(&wrong).unwrap_err().contains("invalid type"));
    }
}
