#!/usr/bin/env bash
# The release gate: every command docs/RELEASE.md gate 3 names, plus the lint
# policy under the clippy driver, stopping at the first failure so release.sh
# can refuse on a single exit code.
#
# Three test suites, not one, because `cargo test --workspace` alone misses two
# things (docs/RELEASE.md gate 3):
#   - the full 13,133-row differential sweep is #[ignore]d, so --workspace only
#     ever runs the 1,500-row fast subset;
#   - --workspace unifies features, so tangent-core is always compiled WITH
#     `learning` and the stock engine path is never exercised at all.
#
# Clippy is the fourth leg, and it is scoped by the policy in Cargo.toml (the
# [workspace.lints.*] tables and the comment block above them) rather than by
# clippy's defaults. The clippy:: entries in that table only run under the
# clippy driver — a plain cargo build or test never evaluates them — so without
# this step the policy's clippy half has no teeth anywhere. `-A clippy::all`
# because the policy deliberately enables no broad groups (the scoring ladder in
# detector.rs trips a pile of style lints whose "fixes" would churn frozen,
# differential-locked code); the narrow policy set itself must be CLEAN, hence
# `-D warnings`. A specific lint level always overrides a group level in rustc,
# so the table's own entries survive the group-wide allow.
#
# NO `cargo fmt` step, and that is settled rather than forgotten: rustfmt.toml's
# header measured the damage (4,170 lines rewritten across the frozen engine)
# and exists to pin the config IF formatting is ever adopted. Do not add one.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Homebrew rust shadows rustup's cargo on PATH; prefer the rustup toolchain.
TOOLCHAIN_BIN="$(rustup which cargo 2>/dev/null | xargs dirname || true)"
[ -n "$TOOLCHAIN_BIN" ] && export PATH="$TOOLCHAIN_BIN:$HOME/.cargo/bin:$PATH"

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> cargo test -p tangent-core  (stock engine — feature unification hides it above)"
cargo test -p tangent-core

echo "==> cargo test -p tangent-core --test differential -- --ignored  (full 13,133-row sweep)"
cargo test -p tangent-core --test differential -- --ignored

echo "==> cargo clippy  (the Cargo.toml lint policy only — no broad groups)"
cargo clippy --workspace --all-targets -- -A clippy::all -D warnings

echo "==> check.sh: all green"
