# What a contributor runs. `just check` is the gate before every commit.

# List the recipes.
default:
    @just --list

# Everything a commit must pass.
check: fmt-check clippy test docs deny typos

# Formatting, without changing anything.
fmt-check:
    cargo fmt --all -- --check

# Lints, with warnings as errors.
clippy:
    cargo clippy --all-targets -- -D warnings

# Every test. None of them needs a network, an account or an agent.
test:
    cargo test

# The documentation builds, and every link in it resolves.
docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# The user guide in website/, both languages, as CI builds it (needs Node;
# not part of `check`). A broken link or anchor fails it.
site:
    cd website && npm ci && npm run build

# Licences, advisories and sources of every dependency (needs cargo-deny).
deny:
    cargo deny check

# Spelling (needs typos-cli).
typos:
    typos

# Format, and apply the lint fixes that can be applied mechanically.
fix:
    cargo fmt --all
    cargo clippy --all-targets --fix --allow-dirty --allow-staged

# Check a commit subject.
subject title:
    scripts/check-subject.sh "{{title}}"

# Have git check each commit's first line as it is made.
hooks:
    ln -sf ../../scripts/commit-msg .git/hooks/commit-msg
    @echo "commit subjects are now checked by scripts/check-subject.sh"

# Ghost.app, signed, in target/release.
bundle:
    ./bundle.sh
