//! Stamps the build with what it was built from: the commit and whether the
//! tree had changes beyond it (vergen-gitcl, by git's own command line), and
//! which builds it is among (`GITHUB_REF`, which GitHub Actions sets: a tag
//! is a release, main is the nightly; anything else was built by hand).
//!
//! The commit is read again when HEAD moves, not on each build, so a
//! change made since the last commit shows as `-dirty` only once something
//! in this crate is rebuilt: what it costs to keep a rebuild short.

use vergen_gitcl::{Emitter, Gitcl};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let git = Gitcl::builder().sha(false).dirty(false).build();
    Emitter::default().add_instructions(&git)?.emit()?;
    println!("cargo:rerun-if-env-changed=GITHUB_REF");
    let channel = match std::env::var("GITHUB_REF").unwrap_or_default() {
        r if r.starts_with("refs/tags/v") => "stable",
        r if r == "refs/heads/main" => "nightly",
        _ => "dev",
    };
    println!("cargo:rustc-env=GHOST_CHANNEL={channel}");
    Ok(())
}
