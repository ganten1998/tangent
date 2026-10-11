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
# tests a leg names, which must PASS at least that many of them, in a home
# `scripts/check-home.sh` watches like every test leg here.
#
# **A filter that matches nothing is green.** `cargo test some::path` with no
# test under that path reports "0 passed" and exits 0, so a leg that names
# its tests by filter would turn green the day one of them was renamed or
# deleted, which is exactly the day it should not. So each named leg says
# how many tests it is for, and the count that ran is checked against it.
named_leg() {
  local want="$1" out ran
  shift
  if ! out="$(scripts/check-home.sh run cargo test "$@" 2>&1)"; then
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

echo "==> cargo test --workspace  (in a scratch home it watches: no test reaches a person's files)"
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
#
# **In a home it watches, and every test leg below too** (design
# W8-safe-tests). The fix is in the build: every road to a person's home is
# `tangent_core::config::home`, which a test build sends to a scratch folder
# (tangent-core's `test-home`, turned on only by [dev-dependencies] lines:
# the binary's, tangent-ui's, and tangent-core's own for its `tests/`;
# `cfg(test)` holds only while a crate compiles as its own unit tests and
# never reached the copies everything else links, which resolved the
# owner's real `~/.config/tangent` once the internal builds' staging folder
# was gone). `scripts/check-home.sh run` is the proof and the net: HOME is a
# fresh folder seeded with decoys where Tangent keeps a person's files, the
# real folders are watched read-only, and a run that creates, changes or
# deletes anything in either fails here, naming it, Tangent open or not
# (close it first: but for its log, which no test reaches, what it writes
# cannot be told from a test's write). The selftest holds `run` to that
# with stub commands before anything relies on it.
scripts/check-home.sh selftest
scripts/check-home.sh run cargo test --workspace --exclude linuxvideo --no-fail-fast

echo "==> no build that ships carries test-home  (resolver 2 keeps it in test builds)"
# A dev-dependency's features reach test builds alone, so `cargo build` and
# every artifact resolve the real home. Read the way a build resolves
# features: the binary with every feature on every target, dev edges off (it
# may not carry it), and each member's test build alone, dev edges on, as the
# `-p` legs below build it (every one that links tangent-core must carry it,
# tangent-core's own included, or its tests resolve the real home, and the
# check could not see the feature at all).
scripts/check-home.sh features

echo "==> cargo test --release  (the render and export goldens, in the profile that ships)"
# **The shipped builds are release, and every leg above is debug.** The render
# goldens pin what the engine sounds like to the bit, and for a while they
# pinned a fact about the PROFILE as well: the optimiser fused the pan law's
# sine and cosine into one `sincos` call that rounds its last bit its own way,
# so the `auto-pan` desk hashed one way here and another in the build people
# run (`instrument::pan_gains` has the measurement and the fix). Rendering
# them in release too is what keeps "the same bits from both" true, and the
# export goldens (the WAV bytes an export writes, through the same engine)
# ride along in the same binary. The hashes are pinned on aarch64 macOS only,
# which is the machine this gate runs on; elsewhere the desk invariants
# beside them still run.
if [ "$(uname -sm)" = "Darwin arm64" ]; then GOLDENS=3; else GOLDENS=1; fi
named_leg "$GOLDENS" --release -p tangent --bin tangent -- \
  render_golden:: engine_bridge::tests::every_corpus_export_is_the_file_it_was

echo "==> the ban scans  (no live code names the fixed desk, no surface undersells a key)"
# **Every source scan the unlimited desk left, by name.** Each wave of that
# work (docs/design/unlimited-tracks.md) took something away (the desk's
# width, strip space, the seat arithmetic, the in-place shelf renames, a
# second road to the licence) and kept a scan that refuses it back, beside
# the code it guards. They already ran in the suite above; this leg names
# them, so deleting or renaming one turns the gate red here instead of
# quietly retiring the ban (`named_leg` counts them). `ban_scan` is the
# tree-wide one over every crate's shipped code, `copy_scan` the phrase scan
# over every surface that says what a key does, and the dash rule over
# everything else that ships: every crate's strings, and what a download or
# an installer carries for a person to read. W8-safe-tests added the second
# road to a home (`ban_scan`), what the scans count as a test (a cfg is read
# by evaluating it, a module declaration however it is laid out), the media
# chooser's lines named exactly, and the two tests that hold each test build
# to its scratch home: `-p tangent-ui` and `-p tangent` alone, because a
# workspace run unifies either crate's dev-dependency line into the other's.
named_leg 30 -p tangent-ui --lib -- --exact \
  ban_scan::no_live_code_names_the_fixed_desk \
  ban_scan::no_live_code_finds_a_home_but_the_one_road \
  ban_scan::a_home_found_off_the_one_road_fails_the_scan_when_planted \
  ban_scan::a_cfg_that_only_spells_test_or_says_not_test_is_read_as_shipping \
  ban_scan::a_test_only_module_is_found_however_its_declaration_is_laid_out \
  era_corpus::a_second_media_root_beside_the_chooser_fails_the_scan_when_planted \
  settings::tests::the_media_choosers_build_on_the_one_road_to_a_home \
  ban_scan::every_banned_name_and_clamp_fails_the_scan_when_planted \
  ban_scan::every_exemption_names_what_is_still_there \
  ban_scan::test_only_modules_are_found_from_their_declarations \
  copy_scan::no_surface_says_a_key_is_only_a_thank_you \
  copy_scan::every_refused_phrase_fails_the_scan_when_planted \
  copy_scan::no_shipped_string_in_any_crate_has_an_em_dash \
  copy_scan::nothing_the_downloads_and_installers_carry_has_an_em_dash \
  copy_scan::a_shipped_file_is_read_as_it_reaches_the_person_it_is_for \
  era_corpus::the_lifts_read_no_live_constant \
  era_corpus::nothing_but_the_choosers_spells_the_real_config_or_media_path \
  settings::tests::the_historical_migrations_do_not_read_todays_constants \
  recorder::strip_ref_tests::no_view_code_does_strip_index_arithmetic \
  recorder::strip_ref_tests::no_live_code_works_out_a_seat \
  recorder::strip_ref_tests::no_live_code_reads_strip_space \
  app::paywall_tests::the_gate_reads_only_is_supporter \
  app::paywall_tests::the_fixture_key_is_read_only_by_tests \
  app::paywall_tests::no_test_builds_the_app_through_the_production_door \
  app::lane_refs_tests::every_lane_shaped_field_of_the_app_has_a_class \
  app::lane_refs_tests::the_classification_scan_fails_a_planted_lane \
  app::width_tests::persistence_and_drawing_never_read_the_licence \
  app::save_tests::a_save_writes_the_desk_the_session_shows \
  copy_scan::the_lexer_reads_shipped_literals_only \
  copy_scan::the_new_copy_passes_the_scan
named_leg 14 -p tangent --bin tangent -- --exact \
  home_tests::this_test_build_keeps_every_home_path_in_a_scratch_folder \
  instrument::lanesets::no_engine_walk_clamps_a_lane_onto_another \
  instrument::lanesets::the_clamp_scan_finds_every_spelling_it_bans \
  instrument::lanesets::no_renderer_walk_shifts_a_lane_into_a_word \
  instrument::lanesets::the_shift_scan_finds_a_lane_mask_in_a_walk \
  instrument::stack_budget::the_key_snapshot_is_not_a_lane_field \
  engine_bridge::tests::the_desktop_pushes_the_desk_only_through_the_bridge \
  desktop::tests::the_host_reads_none_of_the_uis_desk_width_constants \
  desktop::tests::the_host_works_out_no_seat_and_builds_no_lane_mask \
  desktop::tests::a_reorder_sends_the_engine_nothing_but_the_plan \
  desktop::tests::a_reorder_moves_no_state_file \
  konami_e2e::the_engine_and_the_shelf_never_read_the_licence \
  shelf::tests::no_new_build_path_renames_or_deletes_a_legacy_sidecar \
  slot_gaps::no_host_road_names_a_bay_by_the_frozen_maps_seat

echo "==> cargo test -p tangent-core  (stock engine — feature unification hides it above)"
# The crate alone is also the one build where its integration tests get
# `test-home` from nothing but its own dev-dependency line (the workspace
# run above unifies the binary's in): `tests/test_home.rs` pins it here.
scripts/check-home.sh run cargo test -p tangent-core

echo "==> cargo test -p tangent-core --test differential -- --ignored  (full 13,133-row sweep)"
scripts/check-home.sh run cargo test -p tangent-core --test differential -- --ignored

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
