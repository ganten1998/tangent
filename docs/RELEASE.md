# Tangent Release Checklist

The operational side of shipping Tangent. Technical design is in `DESIGN.md`;
historical CI war stories are in `spec/product-docs.md` §3. This file describes
what the scripts in `scripts/` actually do; when they change, change this.

## The version policy, enforced by `release.sh`

`main` carries `X.Y.Z-dev`. Only a deliberate release writes a real version,
and `scripts/release.sh` makes that structural rather than a thing to
remember. It refuses, with `exit 2`, when:

1. **The version is the placeholder.** A build on main cannot claim a number,
   because `-dev` is not a number.
2. **No tag `v<version>` exists.** A release is a thing you can go back to; an
   artifact built from an untagged commit is a thing you cannot. Tag first,
   build second.
3. **`CHANGELOG.md` has no heading for the version.** `publish-github.sh`
   already refuses to publish without release notes; failing here means
   finding that out before the build rather than after it.
4. **A pre-release is past `rc.3`.** Pre-releases are `rc.N` now. Needing a
   fourth candidate means the freeze was not a freeze; fix, do not cut.
   (`-beta`/`-alpha` predate the policy and only draw a note.)

Why this exists: six version numbers were burnt in a single day, beta.10
through beta.15, none tagged, none released, several bumped inline inside a
build command. Nothing leaked; the burn was simply invisible until somebody
counted. Do not rewrite history to tidy burnt numbers away.

## Hard gates before any public release

1. **Test suite green.** `cargo test --workspace` alone is NOT this gate. It
   misses two things: the full differential sweep is `#[ignore]`d, and
   `--workspace` unifies features so `tangent-core` is always compiled WITH
   `learning`, leaving the stock engine path unexercised. All three must pass:

   ```sh
   cargo test --workspace
   cargo test -p tangent-core                                    # stock engine, no learning
   cargo test -p tangent-core --test differential -- --ignored   # full golden sweep
   ```

   `tangent-core/tests/blast_radius.rs` is `#[ignore]`d too, but it is a
   measurement rather than a gate; run it only when the learning re-ranker
   changed.

   This gate is now ENFORCED: `release.sh` runs `scripts/check.sh` (the
   three commands above, a compile of the Minimal build with
   `cargo check -p tangent --no-default-features`, the clippy lint policy,
   and the check that three scripts spell the FreeBSD version alike) and
   refuses to cut a release on any failure. The checklist entry remains so
   a human knows what the refusal means.

2. **THIRD-PARTY-LICENSES current.** Regenerate after any dependency change:
   `scripts/gen-third-party-licenses.sh`. This gate has been skipped before
   and the file has drifted badly for it; run it, do not eyeball it. Also
   ENFORCED now: `release.sh` regenerates and byte-diffs the file, refusing
   on drift.

3. **The name is Tangent everywhere a buyer looks.** The old trademark gate
   here was about "Ivory" colliding with Synthogy Ivory; the 2.3.0 rename
   retired that collision, and reviving the old name in copy is the only way
   to get it back. New marketing text, release notes and store edits say
   Tangent. The Gumroad short link `/l/ivory` and the legacy `Ivory-*` asset
   aliases are deliberate survivals (published links must keep resolving) and
   are not an invitation to write the word anywhere new.

4. **Store links resolve.** See the last section; the check is
   `scripts/check-store-links.sh` and the rule is publish, check, paste,
   deploy, never edit links in anticipation.

## Code signing

### macOS: signed, notarized, and enforced

`scripts/build-macos.sh` signs with the first installed **Developer ID
Application** certificate (override with `TANGENT_SIGN_ID`), hardened runtime
and entitlements included, then notarizes via `notarytool` (keychain profile
`tangent-notary`, override with `TANGENT_NOTARY_PROFILE`) and staples the
ticket, so a first launch works with no network and no Gatekeeper prompt.

The script **refuses to package** a signed build that carries no notarization
ticket; the check is `stapler validate` on the bundle, not the exit status of
the steps, so it catches every path to an unnotarized artifact. The escape
hatch is `TANGENT_ALLOW_UNNOTARIZED=1` and it is loud. With no certificate
installed the build falls back to ad-hoc for local use and says, correctly,
do not publish it.

Create the notary profile once with:

```sh
xcrun notarytool store-credentials "tangent-notary" \
  --apple-id <id> --team-id <team> --password <app-specific-password>
```

The `.pkg` is signed separately by `build-installer.sh` with a **Developer ID
Installer** certificate; `release.sh` fails an unsigned pkg at check time.

### Windows: every PE is signed, and release.sh refuses any unsigned

`scripts/sign-windows.sh` Authenticode-signs a PE via **jsign** against
Azure Artifact Signing (signtool is Windows-only and this binary is
cross-built from macOS). Four PEs ship, and all four take that one route:
`build-cross.sh` signs the inner `tangent.exe` and the bundled
`tangent-ffmpeg.exe` as it stages them; makensis signs the uninstaller while
it builds the installer (`!uninstfinalize` in `installer/windows/tangent.nsi`,
with `= 0` so a failed signing fails the build - without it makensis ignores
the exit code and ships the uninstaller unsigned, which was tested); and
`build-cross.sh` signs `setup.exe` after makensis writes it.

**The timestamp is RFC 3161, and that is not optional.**
`timestamp.acs.microsoft.com` speaks only RFC 3161; jsign's default is the
legacy Authenticode protocol, and it fails AFTER Azure has signed, as
"Malformed content" / "Unable to complete the timestamping" (the first real
signing, 2026-09-24). `sign-windows.sh` passes `--tsmode RFC3161`; any
other route to the same account (signtool with `/tr <url> /td sha256`, a CI
action) must ask for RFC 3161 too. The certificate lives three days and the
timestamp is what keeps a signature valid after it expires, so an
untimestamped signature stops validating within days: on a Smart App
Control machine, a Tangent that stops starting.

Two Windows gates, and they judge different files. SmartScreen judges the
OUTER installer when it is first opened. **Smart App Control** (Windows 11)
checks EVERY program each time it starts and has no per-app override, not
even for an administrator: an unknown, unsigned program is simply blocked.
A tester hit exactly that on 2026-09-24, days after installing, when Smart
App Control left its evaluation mode. It needs a valid signature, not
reputation, so a signed build runs there from its first release - but only
if the encoder and the uninstaller are signed as well as the app.

With none of `TRUSTED_SIGNING_ENDPOINT` / `TRUSTED_SIGNING_ACCOUNT` /
`TRUSTED_SIGNING_PROFILE` set, the PEs ship unsigned and the build says so
for each one; with only some of them set the build **fails** rather than
quietly shipping an unsigned binary you believe is signed. A token can be
supplied directly as `TRUSTED_SIGNING_TOKEN`; set it around the signing
only, since an exported variable reaches every child of the build. Either
way `sign-windows.sh` hands the token to jsign through jsign's environment
(`--storepass env:…`), never on its command line, where any process on the
machine could read it for as long as a signing runs. `release.sh` then checks the
signature of `setup.exe`, of each PE inside it (extracted with 7-Zip:
`tangent.exe`, `tangent-ffmpeg.exe`, `Uninstall.exe`) and of each PE in the
portable zip (`scripts/pe-signed.py`), and a Windows release runs RED until
the Azure account exists, on purpose.

Account setup, from zero, is `docs/WINDOWS-SIGNING.md` - the one-time Azure
walkthrough (an individual's identity check takes minutes; an
organization's takes business days).

### Linux: no signing, but a dependency contract

No signature story is required. What is enforced instead is `DT_NEEDED`:
`scripts/check-linux-deps.sh` refuses any Linux binary that asks the loader
for a library the shared allowlist does not declare. The allowlist is one
file used by both packaging paths, deliberately, because two copies would
disagree the first time somebody edited one. The 5.0 betas shipped a binary
that silently required `liblilv-0.so.0` and did not start on stock Ubuntu
GNOME; this check is why that class of failure cannot recur unnoticed.

## Artifacts and naming

Built into `dist/`; the version is read from the root `Cargo.toml`
(`[workspace.package] version`).

| Artifact | Name | Built by |
|---|---|---|
| macOS installer | `Tangent-<v>-macos.pkg` | `build-installer.sh macos` |
| macOS app zip (primary) | `Tangent-<v>-macos-universal.zip` | `build-macos.sh` |
| macOS DMG (best-effort) | `Tangent-<v>-macos-universal.dmg` | `build-macos.sh` |
| macOS app zip/DMG, host arch only | `Tangent-<v>-macos-<arch>.{zip,dmg}` (arm64 on Apple silicon) | `ARCH=host build-macos.sh` (quick checks, not a release artifact) |
| Windows installer | `Tangent-<v>-windows-setup.exe` | `build-cross.sh` (needs `makensis`) |
| Windows portable | `tangent-<v>-windows-x86_64.zip` | `build-cross.sh` |
| Linux x86_64 | `tangent-<v>-linux-x86_64.tar.gz` | `build-cross.sh` |
| Linux aarch64 | `tangent-<v>-linux-aarch64.tar.gz` | `build-cross.sh` |
| Checksums | `SHA256SUMS` | `release.sh` |

Notes on the set:

- **macOS is universal by default** (`ARCH=host` for a quick local build).
- **The Windows portable zip is a deliverable AND the installer's input.**
  `build-cross.sh` builds it and keeps it; `build-installer.sh` unzips
  exactly it into the NSIS payload, so one build's installed files and
  portable files are the same bytes. It must ship: its version-less alias
  `tangent-windows-x86_64.zip` is the store content's Windows archive
  (`docs/STORE-CONTENT.md`), the legacy `ivory-windows-x86_64.zip` is in
  supporter emails sent before the installers took over at 3.0.0, and
  `publish-github.sh` can only upload an alias for a file that exists in
  `dist/`. If `makensis` is missing, only the zip is built and the run warns
  loudly that no setup.exe was produced. The
  `tangent-<v>-windows-x86_64.pdb` beside it turns a crash offset into a
  function name; it is not a deliverable and `publish-github.sh` does not
  upload it.
- **The VST3 plugin is retired** (2026-08-24, the owner's ruling). No
  installer may carry a `Tangent.vst3`; `release.sh` fails any that does.
- **FreeBSD ships.** `scripts/release.sh` on a FreeBSD host runs
  `scripts/build-freebsd.sh`, checks the `.pkg` and both offline bundles, and
  `publish-github.sh` uploads them. Two things to know: pkg spells a
  prerelease its own way (`5.0.0-rc.1` is `5.0.0.r1`, one shared `sed` in all
  three scripts), so a prerelease's FreeBSD files do not match the
  `*-<version>-*` glob and are found by their own name; and the bundles need
  `pkg fetch`, which needs root — run `release.sh` as root or with `doas`, or
  the run finishes with the missing bundles on the record as problems. The
  `.pkg` installs the licence texts under `share/licenses/tangent/`.

Every archive contains `README.txt` (generated from `docs/ARTIFACT-README.md`),
`LICENSE`, `THIRD-PARTY-LICENSES`, `OFL.txt` and the `font-licenses/` folder.
The Linux tarballs additionally carry `install.sh`, `tangent.desktop`, the
icon, and the bundled `tangent-ffmpeg` encoder with its `ffmpeg-licenses/`;
the Windows payload carries `tangent-ffmpeg.exe` the same way. macOS needs no
bundled encoder (AVFoundation).

## Cross-building: everything compiles on the Mac now

`scripts/build-cross.sh` builds the Windows installer AND both Linux tarballs
from the macOS host. The Linux legs use `cargo zigbuild` against a Debian
bullseye ALSA sysroot the script fetches and caches itself, targeting
glibc 2.30; the old "Linux cannot be cross-compiled from macOS" era is over,
and `build-linux-native.sh` remains for building natively on a Linux box when
that is wanted (a toolchain regression, a glibc question, the suite on Linux).

Details that matter and are easy to lose:

- **The Linux and Windows legs build without `mp3`.** LAME's autotools build
  compiles and runs test programs, which a macOS host cannot do for a foreign
  target. Nothing is lost: MP3 delivery goes through the bundled ffmpeg
  sidecar on both platforms.
- **ASIO rides into the Windows build** when the Steinberg SDK is present
  under `vendor/asiosdk_*` (its licence forbids committing it, so a fresh
  checkout does not have it). Without it `build-cross.sh` still produces a
  WASAPI-only build and says so in one line, and `release.sh` then fails
  the release on the missing `ASIO-NOTICE.txt`. Unpack the SDK into
  `vendor/` before any Windows build meant for a tester or a release. (The
  notice is written whenever `CPAL_ASIO_DIR` is set, and that is cpal's own
  variable: one exported in the shell writes it over a WASAPI-only build
  too, and that build then passes.)
- **`scripts/build-cross.sh check-linux`** type-checks the Linux target from
  macOS with a stub `alsa.pc`, so Linux-only code is checked before pushing
  rather than discovered broken on the Linux box.
- Requirements: `brew install zig`, `cargo install cargo-zigbuild cargo-xwin`,
  the three rustup targets (the script header lists them), `brew install
  makensis` for the installer.
- **No `libjack.so.0` may ever appear in `DT_NEEDED`.** JACK support is
  `dlopen`ed on demand via the vendored `jack-sys` patch;
  `check-linux-deps.sh` is the guard.

## What `release.sh` checks after building

Run on each OS, copy the artifacts into one `dist/`, then
`scripts/release.sh --sums-only` there so `SHA256SUMS` covers everything.
(`SKIP_WINDOWS=1` skips the cross leg; `KEEP_STALE=1` keeps other versions'
artifacts, which are otherwise purged so no stale file rides a checksum or
upload glob.)

Per artifact, it verifies and refuses to bless a release on failure:

- **pkg**: expands it and requires the app component, NO retired VST3
  component, a standalone install choice, `relocatable="false"` (so a stray
  copy elsewhere on the disk cannot swallow the update), an Installer
  signature, and a stapled notarization ticket (`stapler validate`).
- **setup.exe** (needs `7zz`; without it the run FAILS rather than
  skipping): requires `tangent.exe`, NO VST3 payload and `Uninstall.exe`;
  an Authenticode signature on setup.exe itself and on `tangent.exe`,
  `tangent-ffmpeg.exe` and `Uninstall.exe` as extracted from it.
  `scripts/pe-signed.py` checks PRESENCE (a non-empty certificate table),
  not chain validity, which only a first launch on Windows proves.
- **every zip/tar.gz**: `README.txt`, `LICENSE`, `OFL.txt`.
- **macOS zip**: the `Tangent.app` executable present, no `__MACOSX` junk.
- **Windows zip**: `tangent.exe` at the top level; `ASIO-NOTICE.txt`, the
  fingerprint of an ASIO build, so a WASAPI-only Windows build fails the
  release (unless an exported `CPAL_ASIO_DIR` wrote the notice anyway: see
  the ASIO bullet above); and signatures on `tangent.exe` and
  `tangent-ffmpeg.exe`.
- **Linux tarballs**: the `tangent` binary (the pre-2.1.0 empty-tarball bug),
  `install.sh` (the README's first instruction; 4.92.0 shipped without it),
  and `tangent-ffmpeg` (video on a clean machine; also shipped missing once).
- **FreeBSD pkg**: both manifests, `bin/tangent`, `bin/tangent-jack`, the
  desktop entry and icon, and `LICENSE`, `OFL.txt`, `THIRD-PARTY-LICENSES`
  under `share/licenses/tangent/` (the package shipped twice with only
  `LICENSE`); the compact manifest must declare `alsa-lib`, `alsa-plugins`
  and `lilv`, must not depend on a GhostBSD base package, and must carry this
  version.
- **FreeBSD bundles**: the app's own pkg inside, `install.sh`,
  `THIRD-PARTY-LICENSES`, and the `alsa-lib`, `alsa-plugins` and `lilv`
  pkgs; the `complete` bundle additionally `mesa-dri` and `ffmpeg`.

These checks exist because every one of them has actually failed silently at
least once. Do not publish a run that ends `FINISHED WITH PROBLEMS`.

## Release procedure

1. **Version.** On a release commit, replace `-dev` with the real version in
   the root `Cargo.toml` (`[workspace.package]`), run `cargo check` so
   `Cargo.lock` follows, commit. `release.sh` cross-checks the lockfile.
2. **Changelog.** Add the `## [X.Y.Z] - date` heading and what changed;
   `release.sh` refuses without it. Regenerate licenses if dependencies
   changed: `scripts/gen-third-party-licenses.sh`.
3. **Test**: all three commands from gate 1.
4. **Tag locally**: `git tag -a v<v> -m 'Tangent <v>'`. The tag must exist
   before the build; pushing it happens in step 8.
5. **Build**: `scripts/release.sh` on the Mac builds macOS (app, zip, dmg
   and the signed, notarized pkg), Windows (zip and signed setup.exe) and
   both Linux tarballs in one pass, and writes the checksums. On the
   FreeBSD box, `scripts/release.sh` (as
   root or with `doas`, for the bundles) builds and checks the FreeBSD set;
   copy its three files into the Mac's `dist/` before `--sums-only`.
6. **Smoke-test.** `open dist/Tangent.app`. For Linux,
   `scripts/ship-linux.sh <ssh-host> [x86_64|aarch64]` installs the tarball
   on a real Linux box and proves it starts, finds ALSA and opens a window,
   which is the half a cross-build cannot prove. Windows: run the setup.exe
   in a VM or under CrossOver if available.
7. **Checksums** are written by `release.sh` itself, version-scoped, so a
   stale artifact from another version can never ride along.
8. **Push**: the release commit and the tag, to Codeberg (source of truth)
   and the GitHub mirror (`git push github main --tags`).
9. **Publish**: `scripts/publish-github.sh --notes-file <notes>` (the
   `github` remote is the release channel; `DRY_RUN=1` to rehearse). It
   uploads every artifact in THREE spellings: version-scoped, the
   version-less alias (`Tangent-macos.pkg`,
   `Tangent-windows-setup.exe`, `tangent-linux-x86_64.tar.gz`, ...), and the
   legacy `Ivory-*` names that every supporter-key email sent before the
   rename still links. The version-less aliases are what
   `releases/latest/download/<name>` resolves by exact name, and that
   permalink is the download link on the Gumroad post-purchase page, in the
   supporter-key email and in `README.md`; **a release published without the
   aliases 404s for everyone who has already paid, silently.** The script
   re-checks every permalink after upload and exits non-zero if one is dead,
   so a red run is never cosmetic. A version with a pre-release suffix is
   published as a GitHub pre-release, deliberately: `latest/download`
   resolves against the latest stable, so a correctly marked beta leaves the
   permanent links alone. Release notes must include the SmartScreen note.
   For 5.0.0 specifically they must also say that the pre-5.0
   `~/.config/ivory/` directory is removed at first launch rather than
   migrated (a clean break; supporter keys are re-sent by email).
10. **Verify as a stranger**: download each artifact on a clean machine or VM
    (quarantine attribute present) and confirm the documented first-launch
    paths work. Then put `main` back on the next `X.Y.Z-dev`.

## Builds for testers: `scripts/publish-dropbox.sh`

Between releases every build is `X.Y.Z-dev`, and `release.sh` refuses to
touch one, so NONE of its artifact checks (signatures, the ASIO notice, the
payload lists) run on what testers receive. `publish-dropbox.sh
[windows|macos|linux …]` copies the current version's artifacts from
`dist/` to `<Dropbox>/Tangent Development/builds/<HEAD>[-dirty]/`, installer
first, and writes `SHA256SUMS.txt` there. A platform with nothing in `dist/`
is skipped with a line that names it, so the default run (all three)
publishes whatever was built. What it does not know:

- **Which commit built the files.** The folder is named after the commit
  checked out, and `-dirty` after the tracked files, WHEN YOU PUBLISH;
  every `-dev` build has the same file names. Rebuild immediately before
  publishing.
- **What is left over.** Everything in `dist/` with this version's name
  goes: an `ARCH=host` `macos-arm64` pair survives a later universal build,
  a failed Linux leg leaves the previous run's tarballs, and nothing
  removes an old setup.exe before `makensis` runs again.

## Store links: check them, then act in order

`scripts/check-store-links.sh` fetches every `releases/latest/download/` URL
in `README.md`, `docs/STORE-CONTENT.md` and `tools/tangent-fulfil/src/main.rs`.
They resolve by EXACT asset name against whatever release is currently
latest, so a new name 404s until it has shipped.

The order that keeps this honest: **publish, check, paste, deploy.** It has
gone wrong twice, both times by getting ahead of the publish: the key email
was once redeployed before its assets and 404'd for ten minutes, and installer
names were once written into the store content while an older release was
still latest. Both times the text was right and the release was not.

## CI

Deferred. The scripts in `scripts/` are the source of truth and run on the
owner's machines. The Python era burned ~24 of 30 commits on workflow YAML
(`spec/product-docs.md` §3); when CI returns, the workflow must only *call
these scripts*, never inline build logic in YAML.
