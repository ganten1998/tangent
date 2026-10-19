# Tangent 5.2.0

## Tangent Tape, a tape machine

Tangent Tape now runs at four speeds, 3.75, 7.5, 15 and 30 inches per
second, and each is its own machine: the speed sets the playback EQ, where
the head bump sits (near 35 Hz at 15 ips and 70 Hz at 30), how much loud
treble the tape can take, and the colour of its hiss. Changing it while
playing re-voices the machine smoothly, and the pitch never moves.

- **Wow, Flutter and Rate**: the slow sway of the reels and the pinch
  roller, the fast shake of the capstan and the guides, and how fast every
  turning part turns. Out of the box they sit at a healthy studio
  transport's published figure, 0.04 % weighted at 15 ips.
- **Bias**, either side of the aligned point: under-biased tape is brighter
  and saturates sooner in the low end, over-biased tape is darker and
  cleaner. The machine lines itself up again after every change, so the
  level stays put.
- **Age and Damage**: worn tape and heads (the treble goes first, the two
  tracks drift apart, the hiss rises), and dropouts, creases and worn edges
  counted per metre of tape. Both open at 0.
- **A deep wow range**: a second play head 25 ms back with a worn cassette's
  warble, about 1 % at the default Wow and up to 2.6 % at full. While it is
  selected Tape reports 25 ms of delay, compensated across the desk.

**A panel of its own.** Tape's controls stand in four groups the way a tape
machine lays them out, Transport, Record, Tape and Playback, and three of
the captions show what the dials come to: the weighted wow and flutter, the
hiss floor and the head bump's frequency.

A fresh Tape is a 15 ips machine lined up the way its tape's datasheet
says: Drive opens at 0.5, about 10 dB of headroom over 0 VU, so normal
levels stay clean and loud peaks round off. Its hiss is white behind each
speed's playback EQ and no longer swells with the music, and it is the same
at every sample rate.

## Tape start and stop

Right-click the PITCH CONT dial: under its range a new switch makes Play
spin the whole desk up from a standstill, and Stop or Pause wind it down to
a halt, the pitch following the speed as a tape's does. Out of the box the
start takes half a second and the stop 0.7 s; either can be set from 0.1 to
10 s. As the tape slows the treble dulls and the low end sinks, and the
click joins on the first beat the tape reaches at full speed. Recording,
count-ins, exports and freezes never use it, and while an armed input is
monitored it stands aside, so Play and Stop stay instant on a tracking desk.
It is saved with the project and off until you switch it on.

## A new session plays a grand piano

The default project's second track, the one that opens armed, now holds
Tangent Keys on its Intimate Grand instead of Tangent DX7. The drums stay on
the first track, and the DX7 is still in the instrument picker.

## Also fixed

- A bypassed Tangent Denoise or Tangent IR stays in time: a bypassed bay
  plays its input exactly as late as it reports, so bypassing changes what
  you hear and never when.
- An input monitored under the PITCH CONT dial plays at its own pitch
  whatever its channel count. A 64-channel interface's input played a 1 kHz
  tone at 874.5 Hz.

## Before you upgrade

**Tape tracks move 2 ms earlier.** Tape now tells delay compensation about
its 2 ms play head, so a Tape track saved in 5.0 or 5.1 plays in time with
everything else. A track nudged by hand to make up for the old offset will
now be 2 ms out.

**A Tape saved in 5.0 or 5.1 hears the new machine.** Its six knobs are
kept and the new controls read their defaults, so it plays at 15 ips: about
2 dB more headroom at the same Drive, the head bump an octave lower and a
little flutter. Wobble is now Wow alone. Switching to 7.5 ips brings back
the old curve.

**Tape costs about a quarter more processor time than in 5.1**, around 29
microseconds an insert per 128-frame block at 48 kHz against 23, a little
over one percent of one core. Making it cheaper again is planned.

**A default template you have built on keeps its instrument.** One that is
still as Tangent wrote it (no clips, markers, automation or track names, no
other plugins, the tempo and meter it came with, and its instrument on the
sound it came with) is replaced at the next launch with the new desk.
Levels or knob settings changed on their own do not keep a template: name
one of its tracks to keep it. Settings > Reset > The Default Template
brings in the new desk, and replaces everything in that template.

**The developer switches are spelled with Tangent's name.** The environment
variables that turn on diagnostics start with `TANGENT_` now, and the old
prefix is no longer read.

**On Linux, Tangent's MIDI input is named `tangent` (port `tangent-in`).** A
patchbay connection saved to the old names has to be made again once.

**A 4.x settings folder is left where it is.** 5.0.0 and 5.1.0 deleted it at
launch; it is still not read.

## Downloads

| Platform | File |
|---|---|
| macOS 11+ (Apple Silicon and Intel) | `Tangent-5.2.0-macos.pkg` (installer), `Tangent-5.2.0-macos-universal.dmg` / `.zip` |
| Windows 10+ (x64) | `Tangent-5.2.0-windows-setup.exe` (installer), `tangent-5.2.0-windows-x86_64.zip` (portable) |
| Linux x86_64 / aarch64 (glibc 2.30+) | `tangent-5.2.0-linux-x86_64.tar.gz`, `tangent-5.2.0-linux-aarch64.tar.gz` |
| FreeBSD 15 amd64 | `tangent-5.2.0-freebsd-FreeBSD-15-amd64.pkg`, plus `-desktop.tar.gz` and `-complete.tar.gz` offline bundles |

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

**FreeBSD:** `pkg install ./tangent-5.2.0-freebsd-FreeBSD-15-amd64.pkg`
with a network, or unpack a bundle and run `sh install.sh` on a machine
without one (`desktop` for a machine that already runs a desktop,
`complete` for one that has only Xorg).

**Licences:** Tangent is MIT. The Windows and Linux downloads carry a
GPL ffmpeg build as `tangent-ffmpeg` (on Linux one we build from pinned
sources, on Windows BtbN's); its source is published as the releases
[`third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22`](https://github.com/ganten1998/tangent/releases/tag/third-party-src-ffmpeg-n8.1.3-btbn-2026-09-22)
(Windows) and
[`third-party-src-ffmpeg-8.1.3-tangent-linux`](https://github.com/ganten1998/tangent/releases/tag/third-party-src-ffmpeg-8.1.3-tangent-linux)
(Linux). The macOS and FreeBSD builds link LAME for MP3 export, and its LGPL
texts ship beside the app in `lame-licenses/`.

Tangent works offline. It collects no data and has no account and no update
check.
