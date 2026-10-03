#!/usr/bin/env bash
# The release gate: every command docs/RELEASE.md gate 1 names, plus the lint
# policy under the clippy driver, stopping at the first failure so release.sh
# can refuse on a single exit code.
#
# Three test suites, not one, because `cargo test --workspace` alone misses two
# things (docs/RELEASE.md gate 1):
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
# **`--exclude linuxvideo`, because the root `exclude` does not reach it.**
#
# Cargo's workspace `exclude` stops auto-discovery; it does NOT stop a package
# that a member names as a path dependency from becoming a member itself. The
# other vendored trees escape only because they arrive through
# `[patch.crates-io]`. `cargo metadata` lists `tangent-record/vendor/linuxvideo`
# as a member on every platform despite sitting in that exclude list, so
# `--workspace` builds it unconditionally - and it is a Unix-only crate, so on
# WINDOWS `cargo test --workspace` does not compile at all (13 errors in
# `uoctl`: `std::os::fd`, `libc::ioctl`, a missing `platform` module). The
# target gating in tangent-record/Cargo.toml is correct and irrelevant; a
# member is built because it is a member.
#
# Naming the exclusion here is what the root Cargo.toml's comment already says
# it wants: no `--workspace` opinions about someone else's code.
#
# **`--no-fail-fast`, because otherwise a red gate under-reports.** Without it
# cargo stops at the first failing test binary and the four crates after
# `tangent` never run - which on a cross-platform check hid five failures in
# `tangent_ui` behind one in `tangent`. A gate that is going to fail should say
# how much is failing.
cargo test --workspace --exclude linuxvideo --no-fail-fast

echo "==> cargo test -p tangent-core  (stock engine — feature unification hides it above)"
cargo test -p tangent-core

echo "==> cargo test -p tangent-core --test differential -- --ignored  (full 13,133-row sweep)"
cargo test -p tangent-core --test differential -- --ignored

echo "==> cargo clippy  (the Cargo.toml lint policy only — no broad groups)"
# **Our five packages by name, not --workspace.**
#
# The vendored crates are path dependencies, and cargo does not cap lints on a
# path dependency the way it does on one from the registry — so `--workspace`
# handed `-D warnings` to rfd, linuxvideo and cpal as well, and the gate failed
# on about fifty warnings in code that MUST NOT be touched: those copies stay
# byte-comparable to upstream, which is the whole argument in their vendor
# READMEs. Naming our packages leaves them compiled but unjudged.
#
# A new crate of ours has to be added here. That is the cost of the approach
# and it is the cheaper mistake: a package missing from this line is a package
# nobody lints, which shows up as a warning count that never moves.
cargo clippy -p tangent-core -p tangent-ui -p tangent -p tangent-record -p tangent-host \
  --all-targets -- -A clippy::all -D warnings

echo "==> the FreeBSD version spelling is ONE expression in three scripts"
# pkg(8) spells a prerelease its own way (5.0.0-rc.1 is 5.0.0.r1), and
# build-freebsd.sh names the artifact, release.sh checks and checksums it,
# and publish-github.sh uploads it — each with the same sed. A second
# spelling anywhere is a FreeBSD artifact that builds, passes nothing, and
# never ships; so the three are held byte-identical here rather than by
# memory.
SPELLINGS="$(grep -ho "sed -E 's/-beta[^']*'" \
  scripts/build-freebsd.sh scripts/release.sh scripts/publish-github.sh | sort -u)"
if [ "$(printf '%s\n' "$SPELLINGS" | grep -c .)" != 1 ]; then
  echo "the FreeBSD version sed differs between scripts:" >&2
  printf '%s\n' "$SPELLINGS" | sed 's/^/    /' >&2
  exit 1
fi
for s in scripts/build-freebsd.sh scripts/release.sh scripts/publish-github.sh; do
  grep -q "sed -E 's/-beta" "$s" || { echo "$s lost its FreeBSD version sed" >&2; exit 1; }
done

echo "==> check.sh: all green"
