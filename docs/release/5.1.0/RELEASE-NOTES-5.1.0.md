# Tangent 5.1.0

## Tangent Keys, rebuilt

Every sound in Tangent Keys was re-voiced against measurements of a
reference instrument, one measured quantity at a time: how each
partial decays, how the level follows velocity, the ring around a note,
the release. Two sounds were added, and the keyboard now holds ten:

- **Intimate Grand** and **Concert Grand** (Piano 1 and Piano 2 until now):
  new instruments, built from banks of string modes. Each note has three
  strings coupled at the bridge, which is where a piano's two-stage decay
  comes from. There is a hammer graded by key and velocity, the action's
  knock and the dampers' thump, and sympathetic strings that ring around
  every note and after it. Both half-pedal, hold a sostenuto (CC 66) and
  shift for the una corda (CC 67).
- **EP Mk.1** and **EP Suitcase**, the tine pianos, dry and through their
  amp.
- **Clav** and **Clav Wah**.
- **Reed 200** and **Reed Amp**. Reed Amp was Reed 140: it is now the reed
  piano through its amplifier, with the cabinet's slap, its throb and a
  room.
- **Electric Grand** (new): short, stiff strings read by pickups, with no
  soundboard, and strings of its own that ring in sympathy under the pedal.
- **Nylon Guitar** (new): six nylon strings under a fingertip. Each note
  goes to the string a hand would choose, and the open strings and the
  body's air ring with what is played. Every key plays: a key below the
  low E or past the fingerboard gets a string of its own, as on a harp
  guitar.

**Presets for every sound.** The Keys window has the same preset strip as
the effect panels: type a name and Save keeps every knob of the chosen
sound, its menu lists that sound's presets to load, and Delete removes one
once you confirm. The window opens larger, so the ten sounds, every knob
and the strip fit without scrolling.

The ten sounds now sit together: played hard, they are within about 5 dB
of one another. A Keys track also sounds the same at any buffer size.
Tangent Keys is no longer marked experimental.

## Also fixed

- A named chord spells its notes from its root on the staff, the Tonnetz
  and the circle of fifths. D7(b9,#11) drew its third as G flat and its
  sharp eleventh as A flat whenever flats were preferred.
- The staff's alternate reading names every sounding note: a triad that
  explains only part of the chord no longer trails the real alternate.

## Before you upgrade

**A mix balanced on the 5.0 Keys sounds may want its faders looked at.**
Against 5.0.0, at the same velocities, the Intimate Grand plays 2 to 4 dB
louder through the middle of its range and 7 dB louder at fortissimo. The
Concert Grand plays within 2 dB of where it was through the middle and
7 dB louder at fortissimo. EP Suitcase is 2 to 4 dB quieter, and the two
clavs are 3 to 9 dB louder.

**A track saved on Reed 140 opens on Reed Amp** with the knobs it had; its
Tremolo now sets the depth of the amplifier's throb. A track saved on
Piano 1 or Piano 2 opens on the Intimate or the Concert Grand.

**The two grands and the Electric Grand play A0 to C8.** A key outside a
piano's 88 is silent on them, as it is on the instrument.

**A Keys track holds 22.5 MB of memory** (5.7 MB in 5.0), all of it taken
when the track is made, never while it plays.

## Downloads

| Platform | File |
|---|---|
| macOS 11+ (Apple Silicon and Intel) | `Tangent-5.1.0-macos.pkg` (installer), `Tangent-5.1.0-macos-universal.dmg` / `.zip` |
| Windows 10+ (x64) | `Tangent-5.1.0-windows-setup.exe` (installer), `tangent-5.1.0-windows-x86_64.zip` (portable) |
| Linux x86_64 / aarch64 (glibc 2.30+) | `tangent-5.1.0-linux-x86_64.tar.gz`, `tangent-5.1.0-linux-aarch64.tar.gz` |
| FreeBSD 15 amd64 | `tangent-5.1.0-freebsd-FreeBSD-15-amd64.pkg`, plus `-desktop.tar.gz` and `-complete.tar.gz` offline bundles |

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

**FreeBSD:** `pkg install ./tangent-5.1.0-freebsd-FreeBSD-15-amd64.pkg`
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
