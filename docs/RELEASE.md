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

   This gate is now ENFORCED: `release.sh` runs `scripts/check.sh` (exactly
   the three commands above plus the clippy lint policy) and refuses to cut
   a release on any failure. The checklist entry remains so a human knows
   what the refusal means.

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

### Windows: both PEs are signed, and release.sh refuses either unsigned

`scripts/sign-windows.sh` Authenticode-signs a PE via **jsign** against
Azure Artifact Signing (signtool is Windows-only and this binary is
cross-built from macOS). `build-cross.sh` calls it twice: on the inner
`tangent.exe` before staging, and on `setup.exe` after makensis writes it,
because SmartScreen judges the OUTER installer. With none of
`TRUSTED_SIGNING_ENDPOINT` / `TRUSTED_SIGNING_ACCOUNT` /
`TRUSTED_SIGNING_PROFILE` set, the PEs ship unsigned and the build says so
once; with only some of them set the build **fails** rather than quietly
shipping an unsigned binary you believe is signed. A token can be supplied
directly as `TRUSTED_SIGNING_TOKEN`. `release.sh` then verifies both
signatures at artifact-check time (`scripts/pe-signed.py`) and a Windows
release runs RED until the Azure account exists, on purpose.

Account setup, from zero, is `docs/WINDOWS-SIGNING.md` - the one-time Azure
walkthrough (identity validation takes days; everything else takes minutes).

Known residual: NSIS generates `Uninstall.exe` on the user's machine at
install time, so it cannot be signed from here. Documented, out of scope.

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
| Windows installer | `Tangent-<v>-windows-setup.exe` | `build-cross.sh` (needs `makensis`) |
| Linux x86_64 | `tangent-<v>-linux-x86_64.tar.gz` | `build-cross.sh` |
| Linux aarch64 | `tangent-<v>-linux-aarch64.tar.gz` | `build-cross.sh` |
| Checksums | `SHA256SUMS` | `release.sh` |

Notes on the set:

- **macOS is universal by default** (`ARCH=host` for a quick local build).
- **The Windows portable zip is a transient installer input, not a
  deliverable.** `build-cross.sh` builds it, `build-installer.sh` unzips
  exactly it into the NSIS payload, and then it is deleted. The owner ships
  Windows as the setup.exe alone. If `makensis` is missing the zip is kept
  rather than shipping nothing, with a loud warning that the deliverable was
  not produced.
- **The VST3 plugin is retired** (2026-08-24, the owner's ruling). No
  installer may carry a `Tangent.vst3`; `release.sh` fails any that does.
- **The pkg is not yet in `release.sh`'s build path.** Run
  `scripts/build-installer.sh macos` yourself after `build-macos.sh`, before
  checksums. The supporter email hands out the pkg permalink, so a release
  published without a fresh pkg breaks that link.
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
  under `vendor/asiosdk_*` (its licence forbids committing it); without it
  the build is WASAPI-only and says so.
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
  copy elsewhere on the disk cannot swallow the update), and an Installer
  signature.
- **setup.exe** (needs `7zz`): requires `tangent.exe`, NO VST3 payload, and
  `Uninstall.exe`.
- **every zip/tar.gz**: `README.txt`, `LICENSE`, `OFL.txt`.
- **macOS zip**: the `Tangent.app` executable present, no `__MACOSX` junk.
- **Windows zip** (only exists when makensis was missing): `tangent.exe`.
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
5. **Build**: `scripts/release.sh` on the Mac builds macOS, Windows and both
   Linux tarballs in one pass, then run `scripts/build-installer.sh macos`
   for the pkg (see the gap above) and `scripts/release.sh --sums-only` to
   fold it into the checksums. On the FreeBSD box, `scripts/release.sh` (as
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
