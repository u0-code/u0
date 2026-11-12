//! OAuth 1.0a request signing (RFC 5849), as X's API expects it for requests
//! made on behalf of one account.

use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, KeyInit, Mac};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha1::Sha1;

/// Everything except the unreserved characters of RFC 3986.
const ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// The four secrets that identify an app acting as one account.
#[derive(Clone)]
pub struct Credentials {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub token: String,
    pub token_secret: String,
}

fn encode(s: &str) -> String {
    utf8_percent_encode(s, ENCODE).to_string()
}

/// The `Authorization` header for one request.
///
/// `url` is the address without its query string. `params` are the query
/// parameters, plus form parameters if the body is form-encoded; a JSON body
/// is not part of the signature.
pub fn authorization(
    credentials: &Credentials,
    method: &str,
    url: &str,
    params: &[(&str, &str)],
    nonce: &str,
    timestamp: u64,
) -> String {
    let timestamp = timestamp.to_string();
    let mut oauth = vec![
        ("oauth_consumer_key", credentials.consumer_key.as_str()),
        ("oauth_nonce", nonce),
        ("oauth_signature_method", "HMAC-SHA1"),
        ("oauth_timestamp", timestamp.as_str()),
        ("oauth_token", credentials.token.as_str()),
        ("oauth_version", "1.0"),
    ];

    // The signature covers every parameter, encoded and then sorted.
    let mut all: Vec<(String, String)> = oauth
        .iter()
        .chain(params)
        .map(|(k, v)| (encode(k), encode(v)))
        .collect();
    all.sort();
    let joined = all
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let base = format!(
        "{}&{}&{}",
        method.to_uppercase(),
        encode(url),
        encode(&joined)
    );
    let key = format!(
        "{}&{}",
        encode(&credentials.consumer_secret),
        encode(&credentials.token_secret)
    );
    let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes()).expect("hmac takes any key length");
    mac.update(base.as_bytes());
    let signature = STANDARD.encode(mac.finalize().into_bytes());

    oauth.push(("oauth_signature", signature.as_str()));
    oauth.sort_unstable();
    let fields = oauth
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("OAuth {fields}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example from X's "Creating a signature" documentation.
    #[test]
    fn matches_the_documented_example() {
        let credentials = Credentials {
            consumer_key: "xvz1evFS4wEEPTGEFPHBog".into(),
            consumer_secret: "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw".into(),
            token: "370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb".into(),
            token_secret: "LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE".into(),
        };
        let header = authorization(
            &credentials,
            "post",
            "https://api.twitter.com/1.1/statuses/update.json",
            &[
                ("include_entities", "true"),
                (
                    "status",
                    "Hello Ladies + Gentlemen, a signed OAuth request!",
                ),
            ],
            "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg",
            1_318_622_958,
        );
        assert!(
            header.contains("oauth_signature=\"hCtSmYh%2BiHYCEqBWrE7C7hYmtUk%3D\""),
            "{header}"
        );
        assert!(
            header.starts_with("OAuth oauth_consumer_key=\"xvz1evFS4wEEPTGEFPHBog\", oauth_nonce=")
        );
    }

    #[test]
    fn encoding_leaves_only_unreserved_characters() {
        assert_eq!(encode("Ladies + Gentlemen"), "Ladies%20%2B%20Gentlemen");
        assert_eq!(encode("a-b.c_d~e"), "a-b.c_d~e");
        assert_eq!(encode("茶"), "%E8%8C%B6");
    }
}
