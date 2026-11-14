//! The images that came with what she heard, kept on disk.
//!
//! A platform's link to an image stops working after a while, so what she
//! has looked at is kept in her directory, under `media/`. Each file is
//! named by the BLAKE3 hash of its bytes: the same image sent twice is kept
//! once, and a name in her journal always means the same bytes.
//!
//! The journal records the name (`Viewed`); the bytes are not in the
//! journal, and are not derived from it either. They are what she was
//! shown, kept beside the record of it.

use std::{fs, path::Path};

use crate::Result;

/// Where the files are, inside a ghost directory.
pub const MEDIA_DIR: &str = "media";

/// The name an image is kept under: its hash, and an extension for its type.
pub fn name(bytes: &[u8], mime: &str) -> String {
    format!("{}.{}", blake3::hash(bytes).to_hex(), extension(mime))
}

/// Keep an image under `dir/media/`, unless it already is. Returns the
/// name it is kept under.
pub fn store(dir: &Path, bytes: &[u8], mime: &str) -> Result<String> {
    let name = name(bytes, mime);
    let media = dir.join(MEDIA_DIR);
    let path = media.join(&name);
    if path.exists() {
        return Ok(name);
    }
    fs::create_dir_all(&media)?;
    // Written aside and moved into place, so that a file under its final
    // name is always whole.
    let partial = media.join(format!(".{name}.partial"));
    fs::write(&partial, bytes)?;
    fs::rename(&partial, &path)?;
    Ok(name)
}

/// The type of an image from its first bytes, for the kinds every provider
/// takes: PNG, JPEG, GIF and WebP. What a server or a file name says an
/// image is cannot be trusted; the bytes can.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Width and height of a PNG, JPEG, GIF or WebP, read from its header
/// without decoding it. `None` when the header is not one of those, or is
/// cut short.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let byte = |at: usize| bytes.get(at).copied().map(u32::from);
    let be16 = |at: usize| Some((byte(at)? << 8) | byte(at + 1)?);
    let le16 = |at: usize| Some(byte(at)? | (byte(at + 1)? << 8));
    let le24 = |at: usize| Some(le16(at)? | (byte(at + 2)? << 16));
    let be32 = |at: usize| Some((be16(at)? << 16) | be16(at + 2)?);
    match sniff(bytes)? {
        // The IHDR chunk comes first, and holds both.
        "image/png" => Some((be32(16)?, be32(20)?)),
        "image/gif" => Some((le16(6)?, le16(8)?)),
        "image/webp" => match bytes.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let (b0, b1, b2, b3) = (byte(21)?, byte(22)?, byte(23)?, byte(24)?);
                Some((
                    1 + (((b1 & 0x3f) << 8) | b0),
                    1 + (((b3 & 0x0f) << 10) | (b2 << 2) | ((b1 & 0xc0) >> 6)),
                ))
            }
            b"VP8X" => Some((1 + le24(24)?, 1 + le24(27)?)),
            _ => None,
        },
        // A JPEG says its size in its start-of-frame segment, somewhere
        // after whatever metadata comes first.
        _ => {
            let mut at = 2;
            loop {
                if byte(at)? != 0xff {
                    return None;
                }
                let marker = byte(at + 1)?;
                match marker {
                    0xff => at += 1,
                    0x01 | 0xd0..=0xd8 => at += 2,
                    0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                        return Some((be16(at + 7)?, be16(at + 5)?));
                    }
                    _ => at += 2 + usize::try_from(be16(at + 2)?).ok()?,
                }
            }
        }
    }
}

/// The extension for an image type, `bin` for one not known.
fn extension(mime: &str) -> &'static str {
    let mime = mime.split(';').next().unwrap_or_default().trim();
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" | "image/pjpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/heic" => "heic",
        "image/avif" => "avif",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_image_is_named_by_its_hash_and_kept_once() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"not really a png";
        let kept = store(dir.path(), bytes, "image/png").unwrap();
        assert_eq!(kept, format!("{}.png", blake3::hash(bytes).to_hex()));
        let path = dir.path().join(MEDIA_DIR).join(&kept);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        // The same image again is the same file, and nothing is left aside.
        assert_eq!(store(dir.path(), bytes, "image/png").unwrap(), kept);
        assert_eq!(fs::read_dir(dir.path().join(MEDIA_DIR)).unwrap().count(), 1);
    }

    #[test]
    fn an_image_is_known_by_its_first_bytes() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff(b"\xff\xd8\xff\xe0"), Some("image/jpeg"));
        assert_eq!(sniff(b"GIF89a..."), Some("image/gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"<html>"), None);
    }

    #[test]
    fn the_size_of_an_image_is_read_from_its_header() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        assert_eq!(dimensions(&png), Some((640, 480)));
        assert_eq!(dimensions(b"GIF89a\x20\x03\x58\x02"), Some((800, 600)));
        // A JPEG with an APP0 segment before its frame header.
        let jpeg = b"\xff\xd8\xff\xe0\0\x04ab\xff\xc0\0\x11\x08\x01\xe0\x02\x80";
        assert_eq!(dimensions(jpeg), Some((640, 480)));
        let mut webp = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0".to_vec();
        webp.extend([0x7f, 0x02, 0x00, 0xdf, 0x01, 0x00]);
        assert_eq!(dimensions(&webp), Some((640, 480)));
        // Cut short, or not an image: nothing is guessed.
        assert_eq!(dimensions(&png[..18]), None);
        assert_eq!(dimensions(b"<html>"), None);
    }

    #[test]
    fn the_extension_follows_the_type() {
        assert_eq!(extension("image/jpeg"), "jpg");
        assert_eq!(extension("IMAGE/GIF; charset=binary"), "gif");
        assert_eq!(extension("application/octet-stream"), "bin");
    }
}
