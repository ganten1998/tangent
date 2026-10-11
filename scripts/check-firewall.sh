#!/usr/bin/env bash
# The shared crates must not be able to reach the host.
#
# `tangent-ui` and `tangent-core` are the GUI and the engine, and the
# `tangent` binary is the host around them: the window, the MIDI device, the
# native file dialogs, the single-instance lock, the recorder and the plugin
# host are its business and nobody else's (docs/RECORDER-PLAN.md §2). That is
# what lets the GUI run in a bare egui context, which is how the test suite
# drives the whole app and how the compositor paints its panels into a video
# frame; and the process is the binary's too, so no library may end it.
#
# Leaving those crates out of the dependency list makes all of it a COMPILE
# ERROR rather than a code-review question. This script asserts that the
# arrangement is still true, because the way it stops being true is somebody
# adding one convenient dependency for one good reason.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
say() { printf '  %-46s %s\n' "$1" "$2"; }

# ── Forbidden dependencies ───────────────────────────────────────────────────
#
# `--target all` is LOAD-BEARING and was missing until 2026-08-15. `cargo tree`
# filters to the HOST platform by default, so a dependency declared under
# `[target.'cfg(windows)'.dependencies]` is invisible to this script on the
# machine it is actually run on. Proof, in this repo: `tangent/Cargo.toml`
# declares `windows-sys` exactly that way, and the old idiom found nothing here
# while `--target all` finds it. A Windows-only camera or audio crate added to
# `tangent-ui` would therefore have passed this check forever, on the one platform
# nobody can test locally.
for crate in tangent-ui tangent-core; do
  # eframe/midir/rfd/fd-lock/winit: the original five — a window, a MIDI device,
  # a native dialog, a single-instance lock.
  # cpal/nokhwa/objc2/windows/tangent-record: the recorder's half. A camera, an
  # audio device and an encoder are exactly the same category of thing, and the
  # reason to name them here is that the list is literal crate names rather than
  # a category, so nothing else would stop them.
  # vst3/clack-host/tangent-host: the plugin host, for the same reason.
  # midly: the SMF parser INSIDE tangent-record, and the one name on this list
  # that is not a native dependency. It is here because the list is literal
  # crate names rather than a category, so forbidding `tangent-record` did
  # nothing to stop somebody adding `midly = "0.5"` to tangent-ui directly and
  # passing this check — and the whole reason `smf_read.rs` and `smf_write.rs`
  # are hand-rolled is that the wall is meant to be mechanical rather than a
  # thing review has to remember. Two hundred lines of SMF walking is the price
  # of the wall, and it is a price already paid twice; the check is what stops
  # the third person deciding it was not worth it.
  for dep in eframe midir rfd fd-lock winit \
             cpal nokhwa objc2 windows tangent-record midly \
             vst3 clack-host tangent-host; do
    # Match the crate NAME field of a cargo-tree line, without trying to
    # describe cargo's box-drawing prefix in a regex.
    if cargo tree -p "$crate" --edges normal --prefix none --target all 2>/dev/null \
         | awk '{print $1}' | grep -qx "$dep"; then
      say "$crate must not depend on $dep" "FAIL"
      fail=1
    else
      say "$crate has no $dep" "ok"
    fi
  done
done

# ── Forbidden calls ──────────────────────────────────────────────────────────
# `process::exit` unwinds nothing and runs no destructor. In the binary it is a
# legitimate way to refuse a second instance; in a library it ends the process
# behind the binary's back, so nothing the binary would save on the way out is
# saved, and under `cargo test` it takes the whole test run with it.
for crate in tangent-ui tangent-core; do
  hits=$(grep -rnE '\bprocess::exit\b|\bstd::process::abort\b' "$crate/src" 2>/dev/null || true)
  if [ -n "$hits" ]; then
    say "$crate must not call process::exit" "FAIL"
    echo "$hits" | sed 's/^/      /'
    fail=1
  else
    say "$crate never exits the process" "ok"
  fi
done

# ── Direction of dependency ──────────────────────────────────────────────────
# tangent -> tangent-ui -> tangent-core, never the reverse.
if cargo tree -p tangent-ui --edges normal --prefix none --target all 2>/dev/null \
     | awk '{print $1}' | grep -qx 'tangent'; then
  say "tangent-ui must not depend on the binary" "FAIL"; fail=1
else
  say "tangent-ui does not depend on the binary" "ok"
fi

# ── The Minimal build still compiles ─────────────────────────────────────────
# `tangent` with NO features is the chord display alone: no recorder, no camera,
# no plugin host. It is a real configuration, the fallback when a platform
# cannot carry the rest, and it had stopped compiling, because nothing in the
# repo ever built it.
#
# This is the only stage here that invokes the compiler. It is a `check`, not a
# build, and it shares the workspace target dir, so it is seconds once warm.
# Set NO_MINIMAL=1 to skip it when iterating on the structural checks alone.
# scripts/check.sh does exactly that: the gate checks the Minimal build in a
# leg of its own, which shows the compiler's errors when it fails, and then
# runs this script with NO_MINIMAL=1 so the configuration is checked once.
if [ "${NO_MINIMAL:-0}" = 1 ]; then
  say "Minimal build check skipped (NO_MINIMAL=1)" "--"
elif cargo check -q -p tangent --no-default-features 2>/dev/null; then
  say "the Minimal build (no features) compiles" "ok"
else
  say "the Minimal build (no features) must compile" "FAIL"
  echo "      re-run: cargo check -p tangent --no-default-features" >&2
  fail=1
fi

# ── The fulfilment image copies every workspace member ───────────────────────
# A cargo workspace will not load with a member missing, so the Dockerfile has
# to COPY all of them — even the GUI crates it never builds, purely so the root
# manifest parses. That rule was written in a comment at the top of the
# Dockerfile and comments do not fail builds: the rename left `COPY ivory` in
# place, and two members added since were never added at all. The failure lands
# on a Fly builder minutes into a deploy, which is the worst place to find it.
missing=""
for m in $(sed -n 's/^members *= *\[\(.*\)\]/\1/p' Cargo.toml | tr -d '" ' | tr ',' ' '); do
  grep -q "^COPY $m \./$m\$" tools/tangent-fulfil/Dockerfile || missing="$missing $m"
done
if [ -n "$missing" ]; then
  say "the fulfilment Dockerfile copies every workspace member" "FAIL"
  echo "      missing COPY lines for:$missing" >&2
  fail=1
else
  say "the fulfilment Dockerfile copies every member" "ok"
fi

if [ "$fail" = 0 ]; then
  echo "  firewall intact"
else
  echo "  FIREWALL BREACHED: see the header of scripts/check-firewall.sh" >&2
  exit 1
fi
