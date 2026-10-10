#!/usr/bin/env bash
# No test reaches a person's files: the proof, and the net under the fix.
#
# **The fix is in the build.** Every road to a person's home goes through
# `tangent_core::config::home` (`tangent-ui`'s `ban_scan` refuses a second
# one), and a test build turns that road into a scratch folder of its own:
# tangent-core's `test-home` feature, which only the binary's and
# tangent-ui's `[dev-dependencies]` turn on. `#[cfg(test)]` could not do it,
# because it is per crate: the tangent-core and tangent-ui the binary's tests
# link are ordinary builds, and they resolved the owner's real
# `~/.config/tangent`, `Documents/Tangent Media` and `Movies/Tangent` once
# the internal builds' staging folder was gone (design W8-safe-tests).
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
# them without HOME (an absolute path, the password database). It fails the
# run unless Tangent itself was running, whose own writes (its log, a
# debounced settings save) look the same; then it is reported, not judged.
#
# **`features` proves no build that ships carries `test-home`.** Resolver 2
# gives a dev-dependency's features to test builds alone; `cargo tree`
# resolves features the way a build does, so the shipped configurations
# (every feature of the binary, on every target, and the plugin, which is
# its own workspace) are read with dev edges off, and the test build with
# them on, which must carry it: a check that cannot see the feature where it
# is would pass for the wrong reason. And since an artifact is whatever the
# scripts build, no script outside the gate may build a test, bench or
# example target, which would resolve dev-dependency features into the
# binary built beside it.
#
# Usage:
#   scripts/check-home.sh run <command> [args...]
#   scripts/check-home.sh features
#
# CHECK_HOME=<dir> makes `run` watch that folder instead of a fresh one (it
# must hold none of the watched folders yet, and is left as it was found
# when the run is clean): the road for a runner that sets HOME itself to the
# same folder. CARGO=<command> runs cargo through a wrapper.
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
# every platform; a test binary is `tangent-<hash>`).
tangent_running() {
  pgrep -x tangent >/dev/null 2>&1
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
  tangent_running && was_running=1
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
  if ! cmp -s "$work/real.before" "$work/real.after" || [ -s "$work/real.touched" ]; then
    tangent_running && was_running=1
    if [ "$was_running" = 1 ]; then
      echo "check-home: note: the real Tangent folders changed during the run, while Tangent itself" >&2
      echo "  was running, so these may be its own writes:" >&2
    else
      echo "check-home: the run changed the real Tangent folders under $real, and Tangent was not" >&2
      echo "  running: a road reached them without HOME:" >&2
      bad=1
    fi
    diff "$work/real.before" "$work/real.after" | grep '^[<>]' \
      | sed -e 's/^</    gone:/' -e 's/^>/    made:/' >&2 || true
    sed 's/^/    touched: /' "$work/real.touched" >&2
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
# arguments, the `{f}` of the first tangent-core line.
core_features() {
  local out
  out="$("$CARGO" tree "$@" --target all --prefix none --format '{p}|{f}')" || return 1
  printf '%s\n' "$out" | awk -F'|' '$1 ~ /^tangent-core / && !n++ { print $2 } END { exit !n }'
}

features() {
  local shipped plugin tested fail=0
  shipped="$(core_features -p tangent --all-features -e normal,build)" \
    || { echo "check-home: cargo tree could not resolve the binary as it ships" >&2; exit 1; }
  plugin="$(core_features --manifest-path plugin/Cargo.toml --all-features -e normal,build)" \
    || { echo "check-home: cargo tree could not resolve the plugin as it ships" >&2; exit 1; }
  tested="$(core_features -p tangent -e normal,build,dev)" \
    || { echo "check-home: cargo tree could not resolve the binary's test build" >&2; exit 1; }
  printf '  %-34s %s\n' "the binary as it ships:" "tangent-core [${shipped}]" \
    "the plugin as it ships:" "tangent-core [${plugin}]" \
    "the binary's test build:" "tangent-core [${tested}]"
  case ",$shipped," in *,test-home,*)
    echo "check-home: a build of the binary that ships carries test-home: it would keep every" >&2
    echo "  setting and project in a temp folder" >&2
    fail=1 ;;
  esac
  case ",$plugin," in *,test-home,*)
    echo "check-home: the plugin carries test-home" >&2
    fail=1 ;;
  esac
  case ",$tested," in *,test-home,*) ;; *)
    echo "check-home: the binary's test build does not carry test-home: its tests resolve the real" >&2
    echo "  home (and this check cannot see the feature where it must be)" >&2
    fail=1 ;;
  esac
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

case "${1:-}" in
  run) shift; run "$@" ;;
  features) features ;;
  *) echo "usage: scripts/check-home.sh run <command> [args...] | features" >&2; exit 2 ;;
esac
