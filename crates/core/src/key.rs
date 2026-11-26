//! The key a desk comes in with: Ed25519, written as OpenSSH writes it.
//! A desk makes one as it pairs with a server, one for each server, and
//! keeps its private half in its Keychain (`secrets`); the server keeps
//! only the public half (`access`). Each time a desk links, it signs the
//! nonce the server made for that link, so no secret crosses the wire and
//! no signature is good twice.

use base64::Engine as _;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};
use sha2::Digest as _;

use crate::proto::PROTOCOL;

/// OpenSSH's name for the kind of key.
const KIND: &str = "ssh-ed25519";

/// A key pair, and its private half as it is kept: PKCS#8, base64.
pub struct Key {
    pair: Ed25519KeyPair,
    kept: String,
}

impl Key {
    /// A new key, from the system's randomness.
    pub fn generate() -> Self {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .expect("the system's randomness");
        Self::read(&b64().encode(pkcs8.as_ref())).expect("a key just made reads")
    }

    /// The key kept as `kept`.
    pub fn read(kept: &str) -> Result<Self, String> {
        let pair = b64()
            .decode(kept)
            .ok()
            .and_then(|der| Ed25519KeyPair::from_pkcs8(&der).ok())
            .ok_or("The key kept is not one of Ghost's: pair again")?;
        Ok(Self {
            pair,
            kept: kept.to_owned(),
        })
    }

    /// Its private half, to be kept.
    pub fn kept(&self) -> &str {
        &self.kept
    }

    /// Its public half, as a line of `authorized_keys` has it:
    /// `ssh-ed25519 AAAA…`.
    pub fn public(&self) -> String {
        let raw = self.pair.public_key().as_ref();
        let mut blob = Vec::with_capacity(51);
        for part in [KIND.as_bytes(), raw] {
            blob.extend_from_slice(&u32::try_from(part.len()).expect("short").to_be_bytes());
            blob.extend_from_slice(part);
        }
        format!("{KIND} {}", b64().encode(blob))
    }

    /// Its signature of the server's `nonce`, base64.
    pub fn sign(&self, nonce: &str) -> String {
        b64().encode(self.pair.sign(&signed(nonce)))
    }
}

/// What a desk signs to come in: the protocol, and the nonce the server
/// made for this link.
fn signed(nonce: &str) -> Vec<u8> {
    format!("ghost link {PROTOCOL}\n{nonce}").into_bytes()
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// The wire form of the public key `public` (`ssh-ed25519 AAAA…`), and its
/// 32 bytes; none where it is not such a key.
fn blob(public: &str) -> Option<(Vec<u8>, [u8; 32])> {
    let blob = b64().decode(public.strip_prefix(KIND)?.trim()).ok()?;
    let raw = blob.strip_prefix(b"\0\0\0\x0bssh-ed25519\0\0\0\x20")?;
    let raw = raw.try_into().ok()?;
    Some((blob, raw))
}

/// The fingerprint of the public key `public`, as `ssh-keygen -l` shows
/// it: `SHA256:…`.
pub fn fingerprint(public: &str) -> String {
    blob(public).map_or_else(
        || "not a key".into(),
        |(blob, _)| {
            let digest = sha2::Sha256::digest(blob);
            let text = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
            format!("SHA256:{text}")
        },
    )
}

/// Whether `signature` is the signature of `nonce` by the key `public`.
pub fn verify(public: &str, nonce: &str, signature: &str) -> bool {
    let (Some((_, raw)), Ok(signature)) = (blob(public), b64().decode(signature)) else {
        return false;
    };
    UnparsedPublicKey::new(&ED25519, raw)
        .verify(&signed(nonce), &signature)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_is_the_one_ssh_keygen_shows() {
        let public =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINn+jwyw06cMQmsq31kfpmagt7/lDZu0NoAepj2GThov";
        assert_eq!(
            fingerprint(public),
            "SHA256:bSEW9/hhCZp0K22Ug93oBDHFh6Sw1BSLOnjZLEvrYyQ"
        );
        assert_eq!(fingerprint("ssh-rsa AAAA"), "not a key");
    }

    #[test]
    fn a_signature_verifies_for_its_key_and_nonce_only() {
        let key = Key::read(Key::generate().kept()).unwrap();
        let other = Key::generate();
        let signature = key.sign("n1");
        assert!(verify(&key.public(), "n1", &signature));
        assert!(!verify(&key.public(), "n2", &signature));
        assert!(!verify(&other.public(), "n1", &signature));
        assert!(Key::read("not a key").is_err());
    }
}
