# Tangent 5.0.0 release-day checklist

Audit of 2026-10-12 against main at 0ed41a68, updated the same day with
the owner's decisions and the fixes they called for. Every line is READY, BLOCKED
(with why) or OWNER (a person has to do it). Do them in order: the order is
the one docs/RELEASE.md enforces (publish, check, paste, deploy), with the
blockers this audit found put in front. Nothing in this file has been run.

## A. Decisions (OWNER)

Owner decisions of 2026-10-12 are marked DECIDED; the repository work they
needed was done the same day (commits listed in section B).

- [x] **D1. GitHub visibility. DECIDED: make `ganten1998/tangent` PUBLIC at
  publish time, not before** (step G0). While it is private every
  `github.com/ganten1998/tangent/releases/latest/download/...` link
  (README.md, tools/tangent-fulfil/src/main.rs, docs/STORE-CONTENT.md) 404s
  for anyone signed out, `publish-github.sh`'s anonymous permalink check
  fails, `publish-third-party-src.sh` refuses to run, and LICENSING.md's
  LAME relink route and the README's source links assume a public source.
- [x] **D2. What "wipe GitHub" means. DECIDED: delete the old releases only
  after 5.0.0 is published and verified as Latest** (section J): old
  releases and their remote tags go; the repository, its name, `v5.0.0`
  and every `third-party-src-*` release stay. Never delete and recreate the
  repository.
- [x] **D3. The version number. DONE: 5.0.0.** `[Unreleased]` is folded into
  `## [5.0.0] - 2026-10-12` (the old never-published 2026-08-31 notes are
  kept at the end of that section, em dashes removed), and an empty
  `## [Unreleased]` heading sits above it as at every earlier cut.
  `copy_scan.rs` now reads the newest version's section when
  `[Unreleased]` is empty, so the gate scans the 5.0.0 copy.
- [x] **D4. 4.x users lose their config folder. DECIDED: accept and
  document, no import.** `purge_pre_5_0()` (tangent-core/src/config.rs,
  called from tangent/src/main.rs) deletes `~/.config/ivory` unread at
  first launch; it held `license.key`, `overrides.json` (taught chord
  names) and `settings.json`. CHANGELOG.md 5.0.0 "Removed" and the release
  notes' "Before you upgrade" say what is lost and what to do (paste the
  key again; back up the folder before the first 5.0 launch to keep taught
  names). F6 still verifies that advice on a real 4.4.1 machine.
- [ ] **D5. FreeBSD timing.** The owner wants Gumroad live only after
  FreeBSD is built and verified. bsdbox is unreachable; there is no FreeBSD
  build path off the box (scripts/build-freebsd.sh refuses any other OS;
  GhostBSD does not boot under UTM on this Mac). Either wait for the box, or
  publish without FreeBSD and add it to the same release later (step E
  fallback) while the Gumroad page stays off. README.md and
  docs/STORE-CONTENT.md now list FreeBSD 15 and its package on the release
  page, which is true only once it is there.
- [ ] **D6. FFmpeg source (licence). DECIDED: fix before release; staged,
  publishing still open.** Both source releases are staged under
  `~/Library/Caches/tangent-dist/third-party-src/`, and
  `scripts/publish-third-party-src.sh` creates them (step G0; `DRY_RUN=1`
  rehearsed). The Windows one
  (`third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22`, 1.1 GB, 148 files,
  staged by `scripts/fetch-ffmpeg-sources.sh`) is complete as far as BtbN's
  scripts define the build. **The Linux one is now complete**: the owner
  chose to replace johnvansickle.com's 7.0.2 (whose library sources and
  build scripts are published nowhere) with an ffmpeg built by our own
  `scripts/build-ffmpeg-linux.sh` from pinned sources (FFmpeg 8.1.3, x264,
  LAME, NASM), and every build stages its own source as
  `third-party-src-ffmpeg-8.1.3-tangent-linux` (about 16 MB). Still open:
  rebuild every Linux artifact with it before shipping (any tarball made
  before carries 7.0.2), and the residual 7.0.2 question in LICENSING.md
  for copies already handed out.
- [x] **D7. LAME licence texts (licence). DONE.** `assets/lame-licenses/`
  (LAME's COPYING and LICENSE, LGPL-3.0, GPL-3.0, our NOTICE.txt) ships in
  the macOS app (zip, dmg, pkg), the FreeBSD package and both bundles, and
  native Linux builds; check-artifacts.sh refuses those artifacts without
  it. Windows and the cross-built Linux tarballs do not link LAME and carry
  none. THIRD-PARTY-LICENSES has a LAME section.
- [ ] **D8. The next dev version** after the release (5.0.1-dev or
  5.1.0-dev), for docs/RELEASE.md step 10.

## B. Repository edits before the cut

- [x] **B1. CHANGELOG. DONE** (D3), with two new "Removed" entries: Tangent
  as a VST3 plugin (added in 3.0.0) and the `~/.config/ivory` purge with
  what a 4.x user should do. release.sh refusal 3 now matches only the
  2026-10-12 heading.
- [x] **B2. First-run Welcome card. DONE:** "Welcome to Tangent 5.0!"
  (tangent-ui/src/dialogs.rs, WELCOME_LINES; no test pinned the old line).
- [x] **B3. README.md. DONE:** the Windows note says every program is
  signed (no "unsigned 5.0.0"), Linux aarch64 and FreeBSD 15 are listed,
  and the licensing section links both ffmpeg source releases and names
  `lame-licenses/`.
- [x] **B4. docs/STORE-CONTENT.md. DONE:** signing sentence, glibc 2.30,
  FreeBSD and aarch64 listed, and the FFmpeg source lines name the two
  releases. Do not paste the post-purchase block before step G0 has run
  and `scripts/check-store-links.sh` (which now sweeps the
  `third-party-src-*` links too) is green.
- [x] **B5. docs/RELEASE.md step 8. DONE:** pushes `main` and the release
  tag only (`git push origin main v<v>`, `git push github main v<v>`),
  never `--tags`.
- [x] **B6. DONE:** the log line says it removed `~/.config/ivory`.
- [x] **C3 (moved here). The dmg. DONE:** release.sh now requires
  `Tangent-<v>-macos-universal.dmg` with the pkg and zip (it is a README
  link, and publish-github.sh refuses a release without it);
  build-macos.sh alone still treats it as best-effort for tester builds.
- [x] **B11. Every readable written from scratch for a DAW. DONE** (branch
  `copy-5.0`; owner, 2026-10-12: "it's a DAW, all content must be written
  from scratch for this release"). GUMROAD.md, STORE-CONTENT.md (description
  and post-purchase page), the key email, README.md, docs/ARTIFACT-README.md,
  the release notes, WEBSITE-CARD.md, ANNOUNCE.md, the Welcome card, About,
  the desktop entry, the FreeBSD package text, the macOS installer's welcome
  page, the Windows FileDescription and `--help`. STORE-KIT.md is
  regenerated from them. The GitHub description is step G5.
- [ ] **B7. Rename `docs/release/next/` to `docs/release/5.0.0/`** in the
  release commit (copy_scan follows it, copy_scan.rs:506-537). Delete the
  owner header from `RELEASE-NOTES-5.0.0.md` before publishing.
- [ ] **B8. Version.** `Cargo.toml` line 78: `version = "5.0.0"`, then
  `cargo check` so Cargo.lock follows (release.sh:77-79 cross-checks). The
  bump does not stale THIRD-PARTY-LICENSES (it lists no workspace crate).
- [ ] **B9. Commit** B7 and B8 together (B1 to B6 and these drafts are
  already committed; release.sh notes a dirty tree).
- [ ] **B10. Tag.** `git tag -a v5.0.0 -m 'Tangent 5.0.0'` (release.sh:110
  refuses without it).

## C. Mac build (READY once B is done)

Preconditions found on this Mac:

- READY: "Developer ID Application: Matthew Gantenbein (6GFLLR9599)" and
  "Developer ID Installer" certificates are in the keychain
  (build-macos.sh:382-398; check-artifacts.sh:134).
- READY (verify): notary profile `tangent-notary` (build-macos.sh:424-445,
  refuses to package unstapled at 468-485). Last notarized install was
  2026-10-10. Verify with `xcrun notarytool history --keychain-profile
  tangent-notary` (contacts Apple).
- READY (verify): Azure Artifact Signing variables are exported from
  `~/.zshenv` (sign-windows.sh:55-56); `jsign` and `az` are installed. The
  token comes from `az account get-access-token`: run `az account show`
  first; an expired login fails the Windows leg.
- READY: ASIO SDK at `vendor/asiosdk_2.3.3_2019-06-14` (build-cross.sh:239
  refuses without it); ffmpeg pins cached in `vendor/ffmpeg/`; makensis,
  7zz, zig, cargo-zigbuild, cargo-xwin installed.
- READY: `dist -> ~/Library/Caches/tangent-dist` symlink is in place.

Steps:

- [ ] **C1. OWNER: quit Tangent.** check.sh fails if anything but the log
  is written in Tangent's folders during the run (docs/design/unlimited-tracks.md, W8 item 5).
- [ ] **C2.** `scripts/release.sh` runs, in order: the -dev refusal, tag,
  CHANGELOG heading, rc cap, THIRD-PARTY-LICENSES regenerate and byte-diff
  (release.sh:149-162), `scripts/check.sh` (release.sh:173-178), then
  `build-macos.sh` (sign, notarize, staple), `build-installer.sh macos`,
  `build-cross.sh` (Linux x86_64 and aarch64, Windows zip and signed
  setup.exe), the artifact checks and `dist/SHA256SUMS`. It must end
  `==> OK.`, not `FINISHED WITH PROBLEMS`.
- [ ] **C3.** release.sh now refuses to finish clean without
  `dist/Tangent-5.0.0-macos-universal.dmg` (see B, C3).

Expected in `dist/`: `Tangent-5.0.0-macos.pkg`,
`Tangent-5.0.0-macos-universal.zip`, `Tangent-5.0.0-macos-universal.dmg`,
`Tangent-5.0.0-windows-setup.exe`, `tangent-5.0.0-windows-x86_64.zip`
(plus the `.pdb`, never uploaded), `tangent-5.0.0-linux-x86_64.tar.gz`,
`tangent-5.0.0-linux-aarch64.tar.gz`, `SHA256SUMS`.

## D. Push the source (READY after C)

- [ ] **D-1.** `git push origin main v5.0.0` (Codeberg is the source of
  truth; the FreeBSD box fetches the tag from it).

## E. FreeBSD (BLOCKED: bsdbox unreachable at .173/.174)

- [ ] **E1. OWNER: power on bsdbox** and confirm `ssh bsdbox` (the
  HostKeyAlias entry covers the address change).
- [ ] **E2.** On the box: `git fetch origin --tags && git checkout v5.0.0`.
- [ ] **E3.** As the normal user: `SKIP_TESTS=1 scripts/release.sh`.
  SKIP_TESTS is required and correct here: four tests are ARM-calibrated
  and fail on every x86 box by construction (two golden hashes, two
  wall-clock budgets), so check.sh is red there whatever the code does; the
  gate already passed on the Mac for this tagged commit. This builds the
  `.pkg`. The box had no doas (memory, 2026-09-15); without doas or sudo
  build-freebsd.sh skips the two bundles (build-freebsd.sh:231-241) and release.sh records that as a
  problem (release.sh:269-274).
- [ ] **E4.** As root, bundles from the binary already built (no cargo as
  root): `su root -c 'sh scripts/build-freebsd.sh --no-build'`.
- [ ] **E5.** As the user: `scripts/release.sh --sums-only` to re-run the
  artifact checks on all three files (check-artifacts.sh:51-110).
- [ ] **E6.** Copy `dist/tangent-5.0.0-freebsd-FreeBSD-15-amd64.pkg`,
  `...-desktop.tar.gz` and `...-complete.tar.gz` into the Mac's `dist/`,
  then on the Mac `scripts/release.sh --sums-only` so SHA256SUMS covers all
  ten artifacts (docs/RELEASE.md:268-272).
- Fallback if the box stays down (D5): publish without FreeBSD (step G),
  keep Gumroad off, and when the box is back do E2 to E6 and run
  `scripts/publish-github.sh` again with no `--notes-file`: on an existing
  release it uploads with `--clobber`, including the new SHA256SUMS
  (publish-github.sh:244-265).

## F. Smoke tests before anything is public

- [ ] **F1. macOS. OWNER for the launch** (never launched by an agent).
  `spctl -a -vvv -t install dist/Tangent-5.0.0-macos.pkg` and
  `xcrun stapler validate dist/Tangent-5.0.0-macos.pkg`, install the pkg,
  open it, play, record a take, export, quit.
- [ ] **F2. Linux x86_64: READY (void is reachable).**
  `scripts/ship-linux.sh void` installs the tarball and proves it starts,
  finds ALSA and opens a window.
- [ ] **F3. Linux aarch64: BLOCKED (no box known).**
  `scripts/ship-linux.sh <host> aarch64` needs an aarch64 Linux host; an
  aarch64 Linux VM in UTM on this Mac works. Without one, the tarball is
  verified only by the build's DT_NEEDED and glibc-floor checks.
- [ ] **F4. Windows: OWNER on real Windows** (Windows 11 with Smart App
  Control if possible). Run the setup.exe: SmartScreen names "Matthew
  Gantenbein", the app and `tangent-ffmpeg.exe` start under Smart App
  Control, ASIO appears in Audio Setup, play, record, export (MP3 goes
  through tangent-ffmpeg), uninstall. check-artifacts.sh proves a
  signature is PRESENT, not that the chain validates
  (docs/RELEASE.md:296-297); only this run proves that.
- [ ] **F5. FreeBSD: OWNER or the FreeBSD tester.** `pkg install ./...pkg`
  on a FreeBSD 15 machine, launch, sound, MIDI from /dev/umidi; one bundle
  on a machine without a network if available.
- [ ] **F6. Upgrade from 4.4.1: OWNER.** On a machine (or account) with a
  4.4.1 `~/.config/ivory` holding a key: install 5.0.0, confirm the folder
  is removed, the key pastes and activates, and copying `license.key` and
  `overrides.json` back into `~/.config/tangent/` restores the key and the
  taught names (the release notes promise this; cut the sentence if not).
- [ ] **F7. Superior Drummer follows the host: OWNER.** Load SD3 on a track,
  set it to follow host tempo, play a tempo ramp and a loop, and export.
- [ ] **F8. Tangent Insert on real hardware: OWNER.** Patch a send/return
  loop, PING, compare bypass, run a realtime export with it in circuit, and
  unplug the device mid-play.
- [ ] **F9. Live-device pool: OWNER** (docs/PERF-UNLIMITED.md section 7,
  never measured): 128 frames, a 1,024-lane idle desk, then 64 lanes with
  EQ, comp and plate on each; watch Audio Setup's dropout line for a few
  minutes each.

## G. Publish to GitHub

- [ ] **G0. OWNER: make the repository public, then publish the source
  releases** (D1, D6). `gh repo edit ganten1998/tangent --visibility public
  --accept-visibility-change-consequences`, then
  `DRY_RUN=1 scripts/publish-third-party-src.sh` and the same without
  `DRY_RUN`. It creates `third-party-src-ffmpeg-8.1.3-tangent-linux` and
  `third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22` as pre-releases with
  `--latest=false` (they must never become Latest), uploads about 1.1 GB,
  and fails if Latest moved. Do this before G4: README.md, the release
  notes and the binaries' `ffmpeg-licenses/SOURCE.txt` link these
  releases.
- [ ] **G1. OWNER:** `gh auth status` is signed in with release rights.
- [ ] **G2.** `git push github main v5.0.0`. Never `--tags` (docs/RELEASE.md
  step 8): it would put every old tag back after the wipe.
- [ ] **G3.** Strip the owner header from the notes, then
  `DRY_RUN=1 scripts/publish-github.sh --notes-file docs/release/5.0.0/RELEASE-NOTES-5.0.0.md`.
  It must list every artifact in three spellings (version-scoped, the
  version-less alias, the legacy `Ivory-*`/`ivory-*` and `-arm64` names) and
  not refuse any permanent link.
- [ ] **G4.** The same without `DRY_RUN`. It creates the release (stable,
  so it becomes Latest over 4.4.1), uploads, and re-checks every
  `/releases/latest/download/` alias anonymously. It must end
  `==> OK. Every permanent download link resolves to 5.0.0.`
- [ ] **G5. OWNER: the repository's description.** It still reads "Tangent:
  a MIDI keyboard monitor that names what you play in real time. 88 keys,
  chord detection, a guitar neck, theory diagrams, and a VST3 plugin. Free,
  offline, no telemetry." The VST3 plugin is gone and Tangent is a DAW. Set
  it (and, if wanted, topics) once the repository is public:

  ```sh
  gh repo edit ganten1998/tangent \
    --description "Tangent: a DAW for recording, arranging and mixing audio and MIDI, with VST3 and LV2 hosting, built-in instruments and effects, and live chord naming. macOS, Windows, Linux and FreeBSD. Free with up to 32 tracks. Offline, no telemetry." \
    --add-topic daw --add-topic audio --add-topic midi --add-topic music-production \
    --add-topic vst3 --add-topic lv2 --add-topic rust --add-topic freebsd
  ```

  The description is 235 characters (GitHub allows 350) and has no em dash.
  Codeberg's repository description, if it carries the old line, takes the
  same text.

## H. Verify as a stranger

- [ ] **H1.** `scripts/check-store-links.sh` (anonymous fetches) must end
  `==> all N links resolve`.
- [ ] **H2. OWNER:** in a private window, signed out of GitHub, download the
  pkg, setup.exe and the Linux tarball from the README's links.

## I. Store, service, site, mail (OWNER, in this order)

- [ ] **I1. Gumroad:** re-enable the product; paste the Description from
  `GUMROAD.md`; paste the post-purchase block from `docs/STORE-CONTENT.md`
  only after B4. Preview with Preview, never by buying (a purchase mints a
  real key).
- [ ] **I2. Redeploy the fulfil service** (W8 owner checklist item 1: the
  deployed email may still say a key unlocks nothing). From the repo root:
  `fly deploy -c tools/tangent-fulfil/fly.toml --dockerfile tools/tangent-fulfil/Dockerfile`.
  The boot log must say `signing self-test OK`. Preview the email first with
  `cargo test -- --ignored --nocapture print_email` in tools/tangent-fulfil.
- [ ] **I3. Website card:** `WEBSITE-CARD.md` into
  `~/Dropbox/Projects/Web/ganten-site/projects.html`, then `bash upload.sh`.
- [ ] **I4. Announcement:** `ANNOUNCE.md`, dry run, then the owner's `--send`.
- [ ] **I5.** `main` back to the next `-dev` version (D8), commit, push to
  origin.

## J. The GitHub wipe (DECIDED: only after G and H are green)

What depends on GitHub releases (all resolve by exact asset name against the
LATEST release, so they survive deleting OLD releases as long as 5.0.0 is
Latest and carries the aliases, which publish-github.sh uploads):

- README.md:5,31-33,39 (download button, table and releases page),
  README.md:177 (issues).
- The supporter-key email: tools/tangent-fulfil/src/main.rs:286,294,296, in
  every email already sent, plus the older `Ivory-*` names and the
  `github.com/ganten1998/ivory/...` slug in emails from before the rename.
- The Gumroad post-purchase page and description (STORE-CONTENT.md:160-238,
  GUMROAD.md).
- The website card (old slug `ganten1998/ivory`, projects.html:134,168).
- LICENSING.md ("Where the source is"): the `third-party-src-*` source
  releases live in this same repository and must never be deleted while
  binaries are offered.
- Nothing in the app: no update check (README Privacy section); the app
  links only to ganten.neocities.org and the Gumroad page
  (tangent-ui/src/dialogs.rs:12605,14128).

Safe meaning (recommended): delete old releases and their remote tags, keep
the repository. After 5.0.0 is Latest and H1 is green, for each old release:
`gh release delete <tag> --repo ganten1998/tangent --cleanup-tag --yes`
(including the 5.0.0-beta.1 draft and the beta.13 FreeBSD pre-release), then
remove any remaining old remote tags with `git push github --delete <tag>`.
Keep `v5.0.0` and any `third-party-src-*`. Local and Codeberg tags are not
touched. Consequences: old version-specific URLs (release pages and
`/releases/download/<tag>/...`) 404, which nothing in the repo hands out;
every permalink keeps working. Deleting the old releases also stops offering
the old Windows builds that carried ffmpeg n8.1.2, so only the Linux 8.1.3
(ours) and Windows n8.1.3 source releases remain owed, plus 7.0.2 for Linux
copies already handed out (D6; LICENSING.md notes the residual
question for copies already handed out).

Unsafe meaning: delete and recreate the repository. It drops GitHub's
redirect from the old `ganten1998/ivory` name (pre-rename emails and the
website card use it), the issue tracker, and every release at once; there is
a window where every buyer link 404s until 5.0.0 is published again; and
the redirect is not guaranteed to come back for a new repository of the
same name. Do not do this.

Do not run the wipe before G4: deleting 4.4.1 while 5.0.0 is not yet Latest
leaves `latest` on whatever older stable release remains, or on none, and
every link either hands out an old build or 404s.
