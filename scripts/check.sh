#!/usr/bin/env bash
# The release gate: every command docs/RELEASE.md gate 1 names, plus the
# Minimal build, the crate firewall and the lint policy under the clippy
# driver, stopping at the first failure so release.sh can refuse on a single
# exit code.
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

# `named_leg <at least> <cargo test args...>`: a test run filtered to the
# tests a leg names, which must PASS at least that many of them.
#
# **A filter that matches nothing is green.** `cargo test some::path` with no
# test under that path reports "0 passed" and exits 0, so a leg that names
# its tests by filter would turn green the day one of them was renamed or
# deleted, which is exactly the day it should not. So each named leg says
# how many tests it is for, and the count that ran is checked against it.
named_leg() {
  local want="$1" out ran
  shift
  if ! out="$(cargo test "$@" 2>&1)"; then
    printf '%s\n' "$out"
    return 1
  fi
  printf '%s\n' "$out" | grep -E '^test result:' || true
  ran="$(printf '%s\n' "$out" | awk '/^test result:/ {s += $4} END {print s + 0}')"
  if [ "$ran" -lt "$want" ]; then
    echo "  this leg names $want test(s) and only $ran ran: one was renamed or removed" >&2
    return 1
  fi
}

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

echo "==> cargo test --release render_golden  (the render goldens, in the profile that ships)"
# **The shipped builds are release, and every leg above is debug.** The render
# goldens pin what the engine sounds like to the bit, and for a while they
# pinned a fact about the PROFILE as well: the optimiser fused the pan law's
# sine and cosine into one `sincos` call that rounds its last bit its own way,
# so the `auto-pan` desk hashed one way here and another in the build people
# run (`instrument::pan_gains` has the measurement and the fix). Rendering
# them in release too is what keeps "the same bits from both" true. The
# golden itself is pinned on aarch64 macOS only, which is the machine this
# gate runs on; elsewhere the desk invariants beside it still run.
if [ "$(uname -sm)" = "Darwin arm64" ]; then GOLDENS=2; else GOLDENS=1; fi
named_leg "$GOLDENS" --release -p tangent --bin tangent render_golden::

echo "==> cargo test -p tangent-core  (stock engine — feature unification hides it above)"
cargo test -p tangent-core

echo "==> cargo test -p tangent-core --test differential -- --ignored  (full 13,133-row sweep)"
cargo test -p tangent-core --test differential -- --ignored

echo "==> cargo check -p tangent --no-default-features  (the Minimal build)"
# **The Minimal build is part of the gate.** `tangent` with no features is the
# chord display alone, the fallback for a platform that cannot carry the rest.
# Its check lived only in check-firewall.sh, which nothing ran, and it failed
# at every commit from 2026-09-16 until it moved here: six separate breakages,
# two of them a gate and doc comment carried off their item by an insertion,
# four of them recorder-only code added without the gate its neighbours carry.
# A cfg that is wrong in Minimal is invisible in every build that has
# `recorder`, which is every other build of `tangent` in this file.
#
# A compile check and nothing more. Whatever only the recorder calls is dead
# code without it, so this build warns by the hundred; the warnings are dropped
# on the way past, and the whole output is shown only when it fails.
if ! cargo check -q -p tangent --no-default-features 2>/dev/null; then
  cargo check -p tangent --no-default-features || true
  echo "the Minimal build (no features) does not compile" >&2
  exit 1
fi

echo "==> cargo check --manifest-path plugin/Cargo.toml  (the VST3 plugin)"
# **The plugin is its own workspace, so nothing above builds it.** It links
# tangent-ui and tangent-core exactly as the desktop does, and every change to
# their public API (a constructor, a dialog variant, a signature) reaches it;
# without this leg a change that breaks only the plugin was found at release
# time, by the plugin build. A compile check: its tests are the shared
# crates' tests, which ran above.
cargo check --manifest-path plugin/Cargo.toml

echo "==> scripts/test-census.sh  (no crate loses tests without a ledger)"
# A test deleted with the code it covered leaves the suite green; the census
# makes every drop say what replaced it (see the script's header).
scripts/test-census.sh

echo "==> scripts/check-firewall.sh  (the shared crates cannot reach the host)"
# **The rest of check-firewall.sh is part of the gate too.** What
# `tangent-ui` and `tangent-core` may depend on and call, which way the crates
# depend on each other, and the fulfilment image's COPY lines were checked
# only when somebody ran it by hand, which is how its Minimal stage could fail
# for weeks unseen (the leg above). NO_MINIMAL=1 skips that stage here: the
# leg above has just checked the same configuration, and shows the errors.
NO_MINIMAL=1 scripts/check-firewall.sh

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
