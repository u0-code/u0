# What a contributor runs. `just check` is what CI runs, in the same order;
# if it passes here it passes there.

# List the recipes.
default:
    @just --list

# Everything CI checks.
check: fmt-check clippy test docs deny typos

# Formatting, without changing anything.
fmt-check:
    cargo fmt --all -- --check

# Lints, with warnings as errors.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Every test. None of them needs a network, a key or a container.
test:
    cargo test --workspace

# The documentation builds, and every link in it resolves.
docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Licences, advisories and sources of every dependency (needs cargo-deny).
deny:
    cargo deny check

# Spelling (needs typos-cli).
typos:
    typos

# Builds with the oldest compiler the code promises to build with.
msrv:
    cargo +1.89 check --workspace --all-targets

# Format, and apply the lint fixes that can be applied mechanically.
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

# Check a commit subject or a pull request title.
subject title:
    scripts/check-subject.sh "{{title}}"

# Have git check each commit's first line as it is made.
hooks:
    ln -sf ../../scripts/commit-msg .git/hooks/commit-msg
    @echo "commit subjects are now checked by scripts/check-subject.sh"

# Run her from a directory, with the example configuration unless you have your own.
run dir config="ghostd.toml":
    cargo run --release -p ghostd -- run {{dir}} --config {{config}}
