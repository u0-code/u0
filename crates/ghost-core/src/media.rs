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
    fn the_extension_follows_the_type() {
        assert_eq!(extension("image/jpeg"), "jpg");
        assert_eq!(extension("IMAGE/GIF; charset=binary"), "gif");
        assert_eq!(extension("application/octet-stream"), "bin");
    }
}
