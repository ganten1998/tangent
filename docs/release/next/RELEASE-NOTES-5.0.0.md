# Tangent 5.0.0

<!--
DRAFT for `scripts/publish-github.sh --notes-file docs/release/next/RELEASE-NOTES-5.0.0.md`.
Everything above the first "---" line below this comment is for the owner and
must be deleted before publishing (publish-github.sh posts the whole file).

Written from CHANGELOG.md [Unreleased], the unpublished [5.0.0] section dated
2026-08-31 and [4.5.0], because the last stable release anyone downloaded is
4.4.1: everything since then is new to them.

Two sentences in here depend on things not yet verified on a real machine:
  1. "copy license.key and overrides.json back" (the ~/.config/ivory section).
     Verify on a machine with a 4.4.1 config before publishing, or cut it
     down to "paste your key again".
  2. The FreeBSD line assumes bsdbox builds the set. If FreeBSD ships later,
     change that line to "FreeBSD packages follow shortly".
Required by docs/RELEASE.md step 9: the SmartScreen note and the
~/.config/ivory note. Both are below. No em dashes (the owner's rule).

Owner decisions of 2026-10-12, folded in here and in CHECKLIST.md:
  - The ~/.config/ivory purge is ACCEPTED and documented, not migrated: no
    import code. "Before you upgrade" below and CHANGELOG.md's 5.0.0
    "Removed" section say what a 4.x user loses and what to do.
  - The GitHub repository is made PUBLIC at publish time (not before). The
    download links and the ffmpeg source links below 404 until it is.
  - Old GitHub releases are deleted only after 5.0.0 is published and
    verified as Latest (CHECKLIST.md section J). The third-party-src-*
    releases are never deleted.
  - The licence gaps are fixed before release: LAME's texts ship in
    lame-licenses/, and the two ffmpeg source releases are published by
    scripts/publish-third-party-src.sh before this one (CHECKLIST.md G0).
-->

---

Tangent 5.0 is the biggest release since the rewrite. The last stable
version was 4.4.1, and since then Tangent has grown from a keyboard monitor
that records into a recording DAW that still shows you what you are playing:
buses and returns, hardware inserts, layered instruments, export pickers,
plugins that follow the song's tempo, FreeBSD, and thirty-two themes.

Please read **Before you upgrade** below if you used 4.x: two things from
4.x do not carry over.

## Highlights

- **Buses and returns, as many as you want, for everyone.** New Bus and New
  Return on a track's right-click. Any track sends to any number of them
  from its Routing window, each send at its own level, post-fader unless you
  flip it to pre. Buses, returns, sends and routes have no limit on any
  install.
- **Folders sum their tracks.** A folder runs its own rack over its
  children's mix, and its fader, pan, sends and meter govern the group. A
  compressor in a folder's bay is a bus compressor.
- **Tangent Insert: your outboard gear in an insert bay.** Pick the
  interface outputs it sends on and the inputs it comes back on. PING
  measures the loop's round trip and hands it to delay compensation, so the
  rest of the session stays in time with the hardware.
- **Layered instruments.** Several instruments on one track all play from
  the same MIDI and sum on its channel: a piano and a pad from one keyboard,
  or a drum kit with a sampler under it.
- **Plugins follow the song.** VST3 and LV2 plugins are told the tempo
  (ramps included), the time signature, the position and whether the song is
  playing, recording or looping. Anything set to follow the host (Superior
  Drummer, synced delays, arpeggiators, LFOs) now does.
- **Export what you want.** Pick which tracks are in a mixdown or which
  stems are written, and export the whole song, the loop or a custom range.
  Stems are named after their tracks (`Bass.wav`, `Bass 2.wav`), a folder is
  never exported twice, and a new stems export moves the previous one's
  leftovers into `older/` instead of deleting anything.
- **Send to Device.** The master can also go to a second output, such as a
  virtual cable (BlackHole or Loopback on a Mac, VB-Audio's CABLE on
  Windows), so OBS or Discord can take it as a microphone.
- **Tangent Bitcrush**, a new house effect, beside Tangent Keys, the
  effects suite, Analog and the DX7.
- **Automation reaches plugins**, with four automation modes and the master
  automatable like any track.
- **Quantize for audio takes as well as MIDI**, glue, track freezing,
  render ahead, and a desk that renders on every core.
- **Thirty-two themes**, each with its own palette of track colours.
- **FreeBSD is a supported platform**, with a native package and two
  offline bundles.
- **Middle-drag pans** the arrangement and the MIDI editor (experimental).

## What a supporter key does now

The free app holds up to 32 tracks: MIDI and audio tracks and folders count.
A supporter key takes that limit away. Every key already sold does the same,
since keys have no expiry and cover every version. Buses, returns, sends and
routes are never counted and have no limit for anyone.

The limit only ever stops a new track. A project with more than 32 tracks
opens, plays, edits, exports and saves whole without a key; only adding the
next track asks for one.

## Before you upgrade

**Tangent as a VST3 plugin is gone.** 3.0 to 4.4 could install Tangent
itself as a VST3 plugin inside another DAW. That plugin is retired: the 5.0
installers do not offer it. The macOS and Windows installers remove a
`Tangent.vst3` an older installer left in your system's VST3 folder; on
Linux, delete `~/.vst3/Tangent.vst3` yourself. A DAW project that used it
will report the plugin as missing. Tangent still hosts VST3 plugins on every
platform, and LV2 on Linux and FreeBSD.

**Settings from 4.x start fresh.** Before 5.0, Tangent kept its settings in
`~/.config/ivory/` (on Windows, `%USERPROFILE%\.config\ivory`). 5.0 uses
`~/.config/tangent/` and **removes the old folder the first time it starts**,
without reading it. That folder held your settings, the chord names you
taught Tangent, and your supporter key. Your projects and recordings are
not in it and are not touched.

- Your supporter key is in the email it came in. Paste it again: right-click
  anywhere, Support Tangent..., Activate. If you cannot find the email, reply
  to your Gumroad receipt and it will be sent again.
- To keep chord names you taught 4.x, copy the `ivory` folder somewhere safe
  **before you start 5.0 for the first time**. After 5.0 has started once,
  quit it and copy `license.key` and `overrides.json` from your copy into
  `~/.config/tangent/`.

**Files saved by 5.0 do not open in older versions.** The first time 5.0
saves over a settings file, a project or a template written by a 5.0 beta
or an earlier build, it keeps the original beside it, once
(`settings.pre-v23.json`, `<project>.pre-v8`, `drumpad.json.pre-v8`).
Putting those back gives an older build its file as it was.

**Nothing saves itself any more.** New, Open and Quit ask before they
discard unsaved changes, and every launch opens a fresh default session;
your last project is a File > Open away. A drum kit still saves itself.

## Downloads

| Platform | File |
|---|---|
| macOS 11+ (Apple Silicon and Intel) | `Tangent-5.0.0-macos.pkg` (installer), `Tangent-5.0.0-macos-universal.dmg` / `.zip` |
| Windows 10+ (x64) | `Tangent-5.0.0-windows-setup.exe` (installer), `tangent-5.0.0-windows-x86_64.zip` (portable) |
| Linux x86_64 / aarch64 (glibc 2.30+) | `tangent-5.0.0-linux-x86_64.tar.gz`, `tangent-5.0.0-linux-aarch64.tar.gz` |
| FreeBSD 15 amd64 | `tangent-5.0.0-freebsd-FreeBSD-15-amd64.pkg`, plus `-desktop.tar.gz` and `-complete.tar.gz` offline bundles |

`SHA256SUMS` covers every file: `shasum -a 256 -c SHA256SUMS`.

**macOS:** signed with a Developer ID and notarized by Apple, so it opens
with no Gatekeeper prompt.

**Windows:** every program in the download (the installer, Tangent, its
video encoder and the uninstaller) is signed under the author's name, so
Windows 11's Smart App Control lets it run. SmartScreen goes on reputation,
which a new signature has to earn, so it may still say "Windows protected
your PC" the first time: click **More info**, then **Run anyway**. Windows
remembers the choice.

**Linux:** extract the tarball and run `./install.sh` (no root needed). It
checks for `libasound.so.2` and `liblilv-0.so.0` and prints your
distribution's install command if one is missing. It can also set up
real-time audio scheduling; `--no-rt` skips that.

**FreeBSD:** `pkg install ./tangent-5.0.0-freebsd-FreeBSD-15-amd64.pkg`
with a network, or unpack a bundle and run `sh install.sh` on a machine
without one (`desktop` for a machine that already runs a desktop,
`complete` for one that has only Xorg).

**Licences:** Tangent is MIT. The Windows and Linux downloads carry an
unmodified GPL ffmpeg build as `tangent-ffmpeg`; its source is published as
the releases
[`third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22`](https://github.com/ganten1998/tangent/releases/tag/third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22)
(Windows) and
[`third-party-src-ffmpeg-7.0.2`](https://github.com/ganten1998/tangent/releases/tag/third-party-src-ffmpeg-7.0.2)
(Linux). The macOS and FreeBSD builds link LAME for MP3 export, and its LGPL
texts ship beside the app in `lame-licenses/`.

Tangent is free (MIT), collects no data, has no account and no update
check. The full list of changes is in `CHANGELOG.md`.
