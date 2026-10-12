# Tangent 5.0.0

<!--
DRAFT for `scripts/publish-github.sh --notes-file docs/release/next/RELEASE-NOTES-5.0.0.md`.
Everything above the first "---" line below this comment is for the owner and
must be deleted before publishing (publish-github.sh posts the whole file).

Rewritten from scratch on 2026-10-12 (owner: "it's a DAW, all content must be
written from scratch for this release"). Sources: CHANGELOG.md [5.0.0], which
covers everything since 4.4.1, the last stable release anyone downloaded. The
body leads with what Tangent is now, then what is new since 4.4, then the
facts a 4.x user needs before upgrading. No third-party product is named,
except BtbN in the licences paragraph, which is attribution the GPL source
offer needs.

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

## Tangent is now a DAW

Tangent began as a window that named the chord you were playing. Over the
4.x releases it learned to record, to arrange and to mix, and 5.0 is where
that work comes together: Tangent is a digital audio workstation, and this
is its first stable release as one.

You record audio and MIDI into it, arrange clips on a timeline, edit notes
in a piano roll, program drums on sixteen pads, and mix on a desk with
buses, returns, sends, folders and automation. It hosts VST3 plugins on
macOS, Windows, Linux and FreeBSD, and LV2 plugins on Linux and FreeBSD. It
comes with its own instruments (modelled keys, an FM synth, an analog-style
synth, a sampler, a drum machine) and a shelf of effects, so a new session
makes a sound before you install anything.

What Tangent started as is still here. It names the chord under your hands,
writes it on a staff, places it on a guitar neck and draws it on the circle
of fifths, and a take can film the window with your camera in it. They are
now features of the DAW rather than the whole of it.

Tangent is free with up to 32 tracks. A supporter key removes the limit, and
every key already sold does the same.

If you used 4.x, please read **Before you upgrade** below: some things from
4.x do not carry over.

## New since 4.4

**Mixing and routing**

- **Buses and returns,** as many as you want, on every install: New Bus and
  New Return on a track's right-click. Any track sends to any number of
  them, each send at its own level, after the fader unless you set it
  before.
- **Folders sum their tracks.** A folder runs its own inserts over its
  tracks' mix, and its fader, pan, sends and meter act on the group.
- **Routes have no limit,** and the pickers grey out a destination that
  would feed a signal back into itself.
- **Automation reaches plugins,** in four modes, and the master can be
  automated like any track. Nodes box-select, copy, paste and make steps.
- **Sidechain** into Tangent Comp or any VST3 with a key input, from any
  track or folder.
- **Track freezing** (`Cmd/Ctrl+Shift+F`) renders a track through its
  inserts and plays the file, so its plugins stop costing CPU.
- **Render ahead** runs heavy plugin chains in longer stretches, for the
  headroom of a large buffer without its latency, and bouncing and freezing
  now use every core.

**Instruments, effects and plugins**

- **Layered instruments.** Several instruments on one track all play from
  the same MIDI and sum on its channel.
- **Plugins follow the song.** VST3 and LV2 plugins are told the tempo
  (ramps included), the meter, the position, and whether the song is
  playing, recording or looping. Synced delays, arpeggiators and drum
  machines set to follow the host now do.
- **Tangent Insert** puts outboard gear in an insert slot. Pick the outputs
  it sends on and the inputs it returns on; PING measures the round trip and
  hands it to delay compensation, so the session stays in time with the
  hardware.
- **New built-in instruments,** beside the FM synth and the drum machine:
  Tangent Keys (eight modelled sounds: two grands, two tine pianos, two
  reed pianos and two clavs), Tangent Analog (an analog-style synth with an
  arpeggiator) and Tangent Sampler.
- **A shelf of built-in effects,** all new in this release: EQ, Comp, Tape,
  Imager, Plate, Echo, Chorus, Trem, Wah, Drive, Amp (with seven pedal
  slots), IR (with a wizard that captures impulse responses from your own
  gear), Denoise and Bitcrush. Every one has presets you can save and share.

**Arranging and editing**

- **Quantize for audio takes** as well as MIDI (`Q`): attacks are found and
  moved onto the grid, and each attack stays as you played it.
- **Glue** for audio clips as well as MIDI, and every clip is a loop from
  the moment it is made.
- **The MIDI editor** gained step entry that walks back with Backspace,
  humanize (`Shift+H`), a note menu, and arrow keys that move the playhead.
- **Middle-drag pans** the arrangement and the MIDI editor (experimental).
- A session opens on the view you left it on, and carries its own theme,
  grid and zoom.

**Recording and export**

- **Export what you want.** Pick which tracks are in a mixdown or which
  stems are written, and export the whole song, the loop or a custom range.
  Stems are named after their tracks, and a new stems export moves the
  previous one's files into `older/` instead of deleting anything.
- **Send to Device** plays the master out of a second output as well, such as
  a virtual audio cable, so a streaming or call app can use it as an input.
  On Windows it works while your interface plays through ASIO.
- **Audio Setup chooses the output,** and says why when ASIO cannot play.

**Everywhere else**

- **Thirty-two themes,** each with its own palette of track colours.
- **FreeBSD is a supported platform,** with a native package and two
  offline bundles.
- **Every program in the Windows download is signed.**

The full list, including more than 250 fixes, is in `CHANGELOG.md`.

## Experimental in 5.0

These work and are tested, but have not yet been tried on enough real
setups. Please report anything that misbehaves.

- **Tangent Insert** (sending a track out through your interface to outboard
  gear and back). Press PING with the gear connected before you rely on its
  timing.
- **Tempo and position for hosted plugins.** Plugins that follow the host's
  tempo, such as drum machines, synced delays and arpeggiators, now receive
  it. Tell me about any plugin that does not lock to the song.
- **Middle-button drag** to move around the arrangement and the MIDI editor.

## What a supporter key does now

The free app holds up to 32 tracks: MIDI tracks, audio tracks and folders
count. A supporter key removes that limit. Every key already sold does the
same, since keys have no expiry and cover every version. Buses, returns,
sends and routes are never counted and have no limit for anyone.

The limit only ever stops a new track. A project with more than 32 tracks
opens, plays, edits, exports and saves whole without a key; only adding the
next track asks for one.

## Before you upgrade

**Tangent as a VST3 plugin is gone.** Versions 3.0 to 4.4 could install
Tangent itself as a VST3 plugin inside another DAW. 5.0 does not. The macOS
and Windows installers remove a `Tangent.vst3` an older installer left in
your system's VST3 folder. **On Linux, delete `~/.vst3/Tangent.vst3`
yourself** (or the one under your install prefix's `lib/vst3`). A project in
another DAW that used it will report the plugin as missing. Tangent still
hosts VST3 plugins on every platform, and LV2 on Linux and FreeBSD.

**Settings from 4.x are cleared.** Before 5.0, Tangent kept its settings in
`~/.config/ivory/` (on Windows, `%USERPROFILE%\.config\ivory`). 5.0 uses
`~/.config/tangent/` and **deletes the old folder the first time it
starts**, without reading it. That folder held your settings, the chord
names you taught Tangent, and your supporter key. Your projects and
recordings were never in it and are not touched.

- **Enter your supporter key again:** right-click anywhere, Support
  Tangent..., paste it, Activate. It is in the email it first came in. If
  you cannot find that email, reply to your Gumroad receipt and it will be
  sent again.
- To keep chord names you taught 4.x, copy the `ivory` folder somewhere
  safe **before you start 5.0 for the first time**; the file that holds
  them is `overrides.json`. Bringing it back into 5.0 has not been tested
  on every 4.x version, so keep the copy until you have checked.

**Files saved by 5.0 do not open in older versions.** The first time 5.0
saves over a settings file, a project or a template written by an earlier
build, it keeps the original beside it, once (`settings.pre-v23.json`,
`<project>.pre-v8`, `drumpad.json.pre-v8`). Putting those back gives an
older build its file as it was.

**Projects save when you say so.** New, Open and Quit ask before they discard
unsaved changes, and every launch opens a fresh default session; your last
project is an Open away. A drum kit still saves itself.

## Downloads

| Platform | File |
|---|---|
| macOS 11+ (Apple Silicon and Intel) | `Tangent-5.0.0-macos.pkg` (installer), `Tangent-5.0.0-macos-universal.dmg` / `.zip` |
| Windows 10+ (x64) | `Tangent-5.0.0-windows-setup.exe` (installer), `tangent-5.0.0-windows-x86_64.zip` (portable) |
| Linux x86_64 / aarch64 (glibc 2.30+) | `tangent-5.0.0-linux-x86_64.tar.gz`, `tangent-5.0.0-linux-aarch64.tar.gz` |
| FreeBSD 15 amd64 | `tangent-5.0.0-freebsd-FreeBSD-15-amd64.pkg`, plus `-desktop.tar.gz` and `-complete.tar.gz` offline bundles |

`SHA256SUMS` covers every file: `shasum -a 256 -c SHA256SUMS`.

**macOS:** signed with a Developer ID and notarized by Apple, so it opens
without a Gatekeeper prompt.

**Windows:** every program in the download (the installer, Tangent, its
video encoder and the uninstaller) is signed under the author's name, so
Windows 11's Smart App Control lets it run. SmartScreen judges by
reputation, which a new signature has to earn, so it may still say "Windows
protected your PC" the first time: click **More info**, then **Run anyway**.
Windows remembers the choice.

**Linux:** extract the tarball and run `./install.sh` (no root needed). It
checks for `libasound.so.2` and `liblilv-0.so.0` and prints your
distribution's install command if one is missing. It can also set up
real-time audio scheduling; `--no-rt` skips that.

**FreeBSD:** `pkg install ./tangent-5.0.0-freebsd-FreeBSD-15-amd64.pkg`
with a network, or unpack a bundle and run `sh install.sh` on a machine
without one (`desktop` for a machine that already runs a desktop,
`complete` for one that has only Xorg).

**Licences:** Tangent is MIT. The Windows and Linux downloads carry a
GPL ffmpeg build as `tangent-ffmpeg` (on Linux one we build from pinned
sources, on Windows BtbN's); its source is published as
the releases
[`third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22`](https://github.com/ganten1998/tangent/releases/tag/third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22)
(Windows) and
[`third-party-src-ffmpeg-8.1.3-tangent-linux`](https://github.com/ganten1998/tangent/releases/tag/third-party-src-ffmpeg-8.1.3-tangent-linux)
(Linux). The macOS and FreeBSD builds link LAME for MP3 export, and its LGPL
texts ship beside the app in `lame-licenses/`.

Tangent works offline. It collects no data and has no account and no update
check.
