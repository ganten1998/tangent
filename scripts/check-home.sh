#!/usr/bin/env bash
# No test reaches a person's files: the proof, and the net under the fix.
#
# **The fix is in the build.** Every road to a person's home goes through
# `tangent_core::config::home` (`tangent-ui`'s `ban_scan` refuses a second
# one), and a test build turns that road into a scratch folder of its own:
# tangent-core's `test-home` feature, which only `[dev-dependencies]` lines
# turn on (the binary's, tangent-ui's, and tangent-core's own, naming
# itself, for its integration tests). `#[cfg(test)]` could not do it,
# because it holds only while a crate compiles as its own unit tests: the
# tangent-core and tangent-ui the binary's tests link, and the tangent-core
# its own `tests/` link, are ordinary builds, and they resolved the owner's
# real `~/.config/tangent`, `Documents/Tangent Media` and `Movies/Tangent`
# once the internal builds' staging folder was gone (design W8-safe-tests).
#
# **`run` is the proof, and the net.** It runs a command (check.sh's test
# legs) with HOME pointed at a fresh folder seeded with decoys where Tangent
# keeps a person's files: a settings file, the taught chords, a licence and
# its Konami marker, a legacy plugin state, the pre-5.0 config, an Untitled
# project, a take. Anything the command creates, changes or deletes under
# those folders fails the run, named. Decoys and not an empty home, because
# the owner's home is not empty, and the roads that matter most only write
# over something that is already there (a migration saved at load, a reset
# that relocks the marker, a New over Untitled). The scratch home also takes
# whatever a hosted plugin writes into a home of its own.
#
# The real folders are watched as well, read-only: a listing and every
# modification time against a stamp in the temp folder, before and after,
# and nothing in them is opened. A change there means a road that reached
# them without HOME (an absolute path, the password database), and it fails
# the run, whoever else was running. **Tangent running is not an excuse**:
# any change was a note while Tangent was open, because its own writes look
# the same as a test's, and that switched the net off exactly when the
# owner's app was open, which is most of the time, for exactly the files a
# stray test would write. A test's write cannot be told from the app's
# without opening the owner's files, so a settings save, a project or
# anything else a running Tangent writes fails like a test's would, and
# says so: close it and run again.
#
# **One file is the running app's alone: its log** (`.config/tangent/
# tangent.log`, and the `.1` it rotates to), modified while Tangent runs.
# Only `main` attaches it, on `config::dir`, which a test build sends to
# its scratch home, so no test reaches it; and the gate's own load makes
# the open app write it: the first gate run under this rule failed on an
# audio dropout the owner's Tangent logged mid-run. That is noted, never
# failed; the log made, deleted or written with Tangent closed fails like
# anything else. Measured with the owner's Tangent open through the
# W8-safe-tests rounds: it wrote nothing there in two windows of 4 and 32
# minutes, saved its settings once in a third, and logged a dropout in a
# fourth.
#
# **`features` proves no build that ships carries `test-home`, and every
# test build that links tangent-core does.** Resolver 2 gives a
# dev-dependency's features to test builds alone; `cargo tree` resolves
# features the way a build does, so the shipped configuration (every
# feature of the binary, on every target) is read with dev edges off, and
# each workspace member's test build with them on, alone, as check.sh's
# `-p` legs build it: one that links tangent-core must carry it
# (tangent-core's own included), or its tests resolve the real home; and a
# check that cannot see the feature where it is would pass for the wrong
# reason. And since an artifact is whatever the scripts build, no script
# outside the gate may build a test, bench or example target, which would
# resolve dev-dependency features into the binary built beside it.
#
# **`selftest` holds `run` to those rules** with stub commands, in a home
# of its own (its "real" folders are its own too): check.sh runs it first.
#
# Usage:
#   scripts/check-home.sh run <command> [args...]
#   scripts/check-home.sh features
#   scripts/check-home.sh selftest
#
# CHECK_HOME=<dir> makes `run` watch that folder instead of a fresh one (it
# must hold none of the watched folders yet, and is left as it was found
# when the run is clean): the road for a runner that sets HOME itself to the
# same folder. CARGO=<command> runs cargo through a wrapper. CHECK_HOME_APP
# names the app's process (`tangent`, the default), so `selftest` can stand
# a process of its own in for it.
set -euo pipefail
cd "$(dirname "$0")/.."

CARGO="${CARGO:-cargo}"

# The folders under a home where Tangent keeps a person's files, the ones
# that exist. Globbed, so a new spelling (a staging twin, a second media
# root) is watched as soon as anything makes it.
watched() {
  local h="$1" d
  local -a found=()
  shopt -s nullglob
  found=("$h"/.config/tangent* "$h"/.config/ivory "$h"/Documents/Tangent* \
         "$h"/Movies/Tangent* "$h"/Videos/Tangent* "$h"/Music/Tangent*)
  shopt -u nullglob
  for d in ${found[@]+"${found[@]}"}; do
    if [ -e "$d" ] || [ -L "$d" ]; then
      printf '%s\n' "$d"
    fi
  done
}

# Every entry under the watched folders of a home: its path, and for a file
# its checksum and size. Reads what it lists, so only for a scratch home.
inventory() {
  local d
  while IFS= read -r d; do
    find "$d" ! -type f -print
    find "$d" -type f -exec cksum {} +
  done < <(watched "$1") | LC_ALL=C sort
}

# A real home's watched folders: names only, nothing opened.
listing() {
  local d
  while IFS= read -r d; do
    find "$d" -print
  done < <(watched "$1") | LC_ALL=C sort
}

# What was modified after the stamp, under a home's watched folders.
touched() {
  local d
  while IFS= read -r d; do
    find "$d" -newer "$2" -print
  done < <(watched "$1") | LC_ALL=C sort
}

# The decoys: one of each thing a person's home holds that a road to it
# would read, write over or delete, dated long ago so anything the command
# touches is newer than the stamp.
seed() {
  local h="$1" d
  mkdir -p "$h/.config/tangent" "$h/.config/ivory" \
    "$h/Documents/Tangent Media/Projects/Untitled" \
    "$h/Movies/Tangent/2026-01-01_12-00-00_take" "$h/Music/Tangent"
  printf '{"decoy": "scripts/check-home.sh"}\n' > "$h/.config/tangent/settings.json"
  printf '{"version": 1, "overrides": [{"intervals": [0, 4, 7], "name": "Decoy"}]}\n' \
    > "$h/.config/tangent/overrides.json"
  printf 'decoy\n' > "$h/.config/tangent/license.key"
  printf 'konami\n' > "$h/.config/tangent/cheat.unlock"
  printf 'decoy\n' > "$h/.config/tangent/insert-state-3-0.bin"
  printf 'decoy\n' > "$h/.config/ivory/license.key"
  printf '{"decoy": "scripts/check-home.sh"}\n' \
    > "$h/Documents/Tangent Media/Projects/Untitled/project.tan"
  printf '{}\n' > "$h/Movies/Tangent/2026-01-01_12-00-00_take/take.json"
  printf 'decoy\n' > "$h/Music/Tangent/decoy.txt"
  while IFS= read -r d; do
    find "$d" -exec touch -t 200001010000 {} +
  done < <(watched "$h")
}

# Whether Tangent itself is running (the app's executable is `tangent` on
# every platform; a test binary is `tangent-<hash>`). It only decides what a
# failure says.
tangent_running() {
  pgrep -x "${CHECK_HOME_APP:-tangent}" >/dev/null 2>&1
}

run() {
  [ "$#" -gt 0 ] || { echo "check-home: run what?" >&2; exit 2; }
  local real="$HOME" scratch own=0 status=0 bad=0 was_running=0 work
  if [ -n "${CHECK_HOME:-}" ]; then
    scratch="$CHECK_HOME"
    [ -d "$scratch" ] || { echo "check-home: CHECK_HOME=$scratch is not a folder" >&2; exit 2; }
    if [ -n "$(watched "$scratch")" ]; then
      echo "check-home: $scratch already holds a watched folder; give it a clean home" >&2
      watched "$scratch" | sed 's/^/    /' >&2
      exit 2
    fi
  else
    scratch="$(mktemp -d "${TMPDIR:-/tmp}/tangent-check-home.XXXXXX")"
    own=1
  fi
  if [ "$(cd "$scratch" && pwd -P)" = "$(cd "$real" && pwd -P)" ]; then
    echo "check-home: the home to watch is the real one ($real)" >&2
    exit 2
  fi
  work="$(mktemp -d "${TMPDIR:-/tmp}/tangent-check-home-work.XXXXXX")"
  seed "$scratch"
  inventory "$scratch" > "$work/scratch.before"
  listing "$real" > "$work/real.before"
  if tangent_running; then
    was_running=1
    echo "check-home: Tangent is running: whatever it writes in its folders during this run but its" >&2
    echo "  log fails the run, since a test's write cannot be told from its own" >&2
  fi
  touch "$work/stamp"

  # The command, in the scratch home. The toolchain stays where it is, and
  # no developer switch may aim the run at a folder of its choosing.
  (
    export CARGO_HOME="${CARGO_HOME:-$real/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$real/.rustup}"
    export HOME="$scratch"
    unset TANGENT_CONFIG_DIR TANGENT_MEDIA_ROOT IVORY_SETTINGS_PATH \
      XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME
    exec "$@"
  ) || status=$?

  inventory "$scratch" > "$work/scratch.after"
  touched "$scratch" "$work/stamp" > "$work/scratch.touched"
  if ! cmp -s "$work/scratch.before" "$work/scratch.after" || [ -s "$work/scratch.touched" ]; then
    echo "check-home: the run reached what a person keeps under its home ($scratch):" >&2
    diff "$work/scratch.before" "$work/scratch.after" | grep '^[<>]' \
      | sed -e 's/^</    gone or changed:/' -e 's/^>/    made or changed:/' >&2 || true
    sed 's/^/    touched: /' "$work/scratch.touched" >&2
    bad=1
  fi

  listing "$real" > "$work/real.after"
  touched "$real" "$work/stamp" > "$work/real.touched"
  tangent_running && was_running=1
  # The running app's own log, set apart from what it touched (see the
  # header): written to, with the app running, and nothing else.
  : > "$work/real.log"
  if [ "$was_running" = 1 ]; then
    grep -Fx -e "$real/.config/tangent/tangent.log" -e "$real/.config/tangent/tangent.log.1" \
      "$work/real.touched" > "$work/real.log" || true
    grep -vFx -e "$real/.config/tangent/tangent.log" -e "$real/.config/tangent/tangent.log.1" \
      "$work/real.touched" > "$work/real.rest" || true
    mv "$work/real.rest" "$work/real.touched"
  fi
  if ! cmp -s "$work/real.before" "$work/real.after" || [ -s "$work/real.touched" ]; then
    echo "check-home: the run changed the real Tangent folders under $real:" >&2
    diff "$work/real.before" "$work/real.after" | grep '^[<>]' \
      | sed -e 's/^</    gone:/' -e 's/^>/    made:/' >&2 || true
    sed 's/^/    touched: /' "$work/real.touched" >&2
    if [ "$was_running" = 1 ]; then
      echo "  Tangent was running, and what it writes there (a settings save, a project) cannot be" >&2
      echo "  told from a test's without opening it: close it and run again." >&2
    else
      echo "  Tangent was not running: a road reached them without HOME." >&2
    fi
    bad=1
  elif [ -s "$work/real.log" ]; then
    echo "check-home: note: Tangent, running, wrote its own log during the run (no test reaches it):" >&2
    sed 's/^/    /' "$work/real.log" >&2
  fi
  rm -rf "$work"

  if [ "$bad" = 1 ]; then
    echo "check-home: the watched home is kept for a look: $scratch" >&2
    [ "$status" = 0 ] && status=1
  elif [ "$own" = 1 ]; then
    rm -rf "$scratch"
  else
    # A home handed in is left as it was found: the decoys go.
    rm -rf "$scratch/.config/tangent" "$scratch/.config/ivory" "$scratch/Documents/Tangent Media" \
      "$scratch/Movies/Tangent" "$scratch/Music/Tangent"
    rmdir "$scratch/.config" "$scratch/Documents" "$scratch/Movies" "$scratch/Music" 2>/dev/null || true
  fi
  return "$status"
}

# tangent-core's features in one resolution: `cargo tree` with the given
# arguments, the `{f}` of the first tangent-core line, after a `+` so that
# a resolution with no feature at all still says it found the crate.
# Nothing at all when the tree holds no tangent-core; fails only when cargo
# does.
core_features() {
  local out
  out="$("$CARGO" tree "$@" --target all --prefix none --format '{p}|{f}')" || return 1
  printf '%s\n' "$out" | awk -F'|' '$1 ~ /^tangent-core / && !n++ { print "+" $2 }'
}

features() {
  local shipped got member fail=0 tested=0
  shipped="$(core_features -p tangent --all-features -e normal,build)" \
    || { echo "check-home: cargo tree could not resolve the binary as it ships" >&2; exit 1; }
  printf '  %-34s %s\n' "the binary as it ships:" "tangent-core [${shipped#+}]"
  case ",${shipped#+}," in *,test-home,*)
    echo "check-home: a build of the binary that ships carries test-home: it would keep every" >&2
    echo "  setting and project in a temp folder" >&2
    fail=1 ;;
  esac
  # **Every test build that links tangent-core carries it**, each member
  # read alone, the way check.sh's `cargo test -p` legs build it: a
  # workspace run unifies any one member's dev-dependency line into all of
  # them, which is how tangent-core's own integration tests went without it
  # unseen. A member that gains tangent-core later is read the day it does.
  for member in $(sed -n 's/^members *= *\[\(.*\)\]/\1/p' Cargo.toml | tr -d '" ' | tr ',' ' '); do
    got="$(core_features -p "$member" -e normal,build,dev)" \
      || { echo "check-home: cargo tree could not resolve $member's test build" >&2; exit 1; }
    [ -n "$got" ] || continue
    tested=$((tested + 1))
    printf '  %-34s %s\n' "$member's test build:" "tangent-core [${got#+}]"
    case ",${got#+}," in *,test-home,*) ;; *)
      echo "check-home: $member's test build does not carry test-home: its tests resolve the real home" >&2
      fail=1 ;;
    esac
  done
  # The binary, tangent-ui and tangent-core itself, or this read nothing.
  if [ "$tested" -lt 3 ]; then
    echo "check-home: only $tested test builds link tangent-core: the members were not read" >&2
    fail=1
  fi
  # **And the artifacts are what the scripts build.** A cargo build that
  # also compiles a test, bench or example target is building the
  # dev-dependencies, so it resolves `test-home` into everything it builds,
  # the binary it would ship included. No script but the gate's own may ask
  # for one (`cargo test` is the gate's, and ships nothing).
  local script found hits=""
  for script in scripts/*.sh; do
    case "$script" in scripts/check*.sh | scripts/test-census.sh) continue ;; esac
    found="$(grep -nE -- '--(all-targets|tests|benches|examples|test|bench|example)([[:space:]=]|$)' "$script" \
      | grep -vE '^[0-9]+:[[:space:]]*#' | sed "s|^|$script:|" || true)"
    if [ -n "$found" ]; then
      hits="$hits$found"$'\n'
    fi
  done
  if [ -n "$hits" ]; then
    echo "check-home: a script builds a test, bench or example target, so what it builds carries" >&2
    echo "  test-home:" >&2
    printf '%s' "$hits" | sed 's/^/    /' >&2
    fail=1
  fi
  return "$fail"
}

# **`run`, held to its rules with stub commands.** Everything happens under
# one temp folder: HOME is pointed at a "real" home made there (decoys in
# its Tangent folders, dated long ago), so the real folders `run` watches
# are its own, and nothing of anybody's is read or written. The app is
# stood in for by a `sleep` (CHECK_HOME_APP), so the branch where Tangent is
# running is exercised whether or not Tangent is open. Revert checks: the
# old note (any change while the app runs) passes four runs this wants
# failed, the first two among them; the log's exception widened to whatever
# the app touched passes the first and the log-and-setting one; no
# exception at all fails the run that writes only the log. Where no `pgrep`
# can see a process the app is never seen running, and the cases that need
# it say so.
selftest() {
  local t self stand=0 fails=0 sees=1
  t="$(mktemp -d "${TMPDIR:-/tmp}/tangent-check-home-selftest.XXXXXX")"
  self="$(pwd -P)/scripts/check-home.sh"
  sleep 600 &
  stand=$!
  # Not a job this shell reports on when the trap ends it.
  disown "$stand"
  trap "kill $stand 2>/dev/null; rm -rf '$t'" EXIT
  if ! command -v pgrep >/dev/null 2>&1; then
    sees=0
  elif ! CHECK_HOME_APP=sleep tangent_running; then
    echo "check-home selftest: cannot see its stand-in for the app" >&2
    exit 1
  fi
  local r="$t/real"
  mkdir -p "$r/.config/tangent" "$r/Documents/Tangent Media/Projects/Untitled"
  printf '{}\n' > "$r/.config/tangent/settings.json"
  printf 'konami\n' > "$r/.config/tangent/cheat.unlock"
  printf '[decoy] tangent starting\n' > "$r/.config/tangent/tangent.log"
  printf '{}\n' > "$r/Documents/Tangent Media/Projects/Untitled/project.tan"
  find "$r/.config/tangent" "$r/Documents/Tangent Media" -exec touch -t 200001010000 {} +

  # expect_run <pass|fail> <running|absent> <what> <command...>
  expect_run() {
    local want="$1" app=tangent-check-home-absent what="$3" got=0
    if [ "$2" = running ]; then
      app=sleep
      if [ "$sees" = 0 ]; then
        printf '  %-62s %s\n' "$what" "skipped (no pgrep: the app is never seen)"
        return
      fi
    fi
    shift 3
    # TMPDIR in the selftest's folder: a failed run keeps its home for a
    # look, and these fail on purpose.
    HOME="$r" TMPDIR="$t" CHECK_HOME_APP="$app" "$self" run "$@" > "$t/out" 2>&1 || got=$?
    if { [ "$want" = pass ] && [ "$got" = 0 ]; } || { [ "$want" = fail ] && [ "$got" != 0 ]; }; then
      printf '  %-62s %s\n' "$what" "ok"
    else
      printf '  %-62s %s\n' "$what" "FAIL (exit $got, wanted $want)"
      sed 's/^/      /' "$t/out"
      fails=1
    fi
  }
  # The real folders, reached by a road that ignores HOME (an absolute path).
  expect_run fail running "a real setting written while the app runs fails" \
    sh -c 'printf x >> "$1/.config/tangent/settings.json"' _ "$r"
  expect_run fail running "a real Konami marker deleted while the app runs fails" \
    rm "$r/.config/tangent/cheat.unlock"
  expect_run fail absent "a real project made while the app is closed fails" \
    touch "$r/Documents/Tangent Media/Projects/Untitled/second.tan"
  # The running app's own log, and only while it runs.
  expect_run pass running "the app's own log, written while it runs, is noted" \
    sh -c 'printf x >> "$1/.config/tangent/tangent.log"' _ "$r"
  expect_run fail absent "the app's log written while it is closed fails" \
    sh -c 'printf x >> "$1/.config/tangent/tangent.log"' _ "$r"
  expect_run fail running "the log and a setting written while the app runs fails" \
    sh -c 'printf x >> "$1/.config/tangent/tangent.log"; printf x >> "$1/.config/tangent/settings.json"' _ "$r"
  expect_run fail running "the app's log deleted while it runs fails" \
    rm "$r/.config/tangent/tangent.log"
  # The scratch home's decoys, reached through HOME.
  expect_run fail running "a decoy setting written through HOME fails" \
    sh -c 'printf x >> "$HOME/.config/tangent/settings.json"'
  expect_run fail absent "a decoy licence deleted through HOME fails" \
    sh -c 'rm "$HOME/.config/tangent/license.key"'
  expect_run fail absent "a staging twin made through HOME fails" \
    sh -c 'mkdir -p "$HOME/.config/tangent-staging"'
  # What keeps to itself passes, the app running or not; a failure is its own.
  expect_run pass running "a write elsewhere in HOME passes, the app running" \
    sh -c 'mkdir -p "$HOME/elsewhere" && printf x > "$HOME/elsewhere/f"'
  expect_run pass absent "nothing written passes" true
  TANGENT_CONFIG_DIR=/nowhere TANGENT_MEDIA_ROOT=/nowhere IVORY_SETTINGS_PATH=/nowhere \
    expect_run pass absent "the developer's switches do not reach the command" \
    sh -c '[ -z "${TANGENT_CONFIG_DIR:-}${TANGENT_MEDIA_ROOT:-}${IVORY_SETTINGS_PATH:-}" ]'
  expect_run fail absent "a failing command's status comes through" false
  return "$fails"
}

case "${1:-}" in
  run) shift; run "$@" ;;
  features) features ;;
  selftest) selftest ;;
  *) echo "usage: scripts/check-home.sh run <command> [args...] | features | selftest" >&2; exit 2 ;;
esac
