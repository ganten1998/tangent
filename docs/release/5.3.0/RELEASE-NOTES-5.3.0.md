# Tangent 5.3.0

5.3.0 finishes the tape machine that came in 5.2.0, keeps every desk on one
clock when delay compensation is holding it back, plays every note of a
built-in instrument on its own frame, and lets you hear the plugins across
the master while you track. Coming from 5.0 or 5.1, this is the one page to
read: what 5.2.0 and 5.1.0 brought is further down.

## In time on every desk

A desk whose plugins report a delay holds every track back by the longest
of them: 25 ms for a Tape in its deep wow range, 2 ms in the studio range,
about 11 ms for an IR. 5.3.0 keeps what you hear and what you record on
that one clock.

- **The click is heard on the beat of the music.** It was never held, so it
  played ahead of the music by the whole hold. It is now held with the
  music in the speakers and in the take, its count-in, reverb send and bus
  sends with it, and on every desk measured its attack falls exactly where
  it does on an empty one. With tape start and stop on, the song's click
  still waits for the tape to reach speed.
- **A recorded part lands on the beat it was played to.** Audio and MIDI
  were placed with nothing taken off for the hold, so a part played in time
  with the backing landed late by it. Both are now placed against what you
  heard, to the sample, and a take recorded from the top starts at 0:00.
- **A record or a Play that waits on a count-in starts on the count's last
  click.** The song started a screen refresh or two after the go, and a
  record begun on it could lose the start of its first note, or a MIDI note
  played on the go. Both now start on the go itself.
- **An armed instrument track answers your keys at once, and its own part
  stays on the beat.** It waited with the rest of the desk, so a key played
  into it sounded 25 ms late with a deep-range Tape on another track. It now
  reads its own part a wait behind the song, so your keys answer after only
  its own effects (and any plugins across the master) while the song plays,
  stops or records, and the part it plays back lands with the mix.
- **Short drum hits and the ends of notes are heard.** On a held desk a
  drum hit shorter than the hold was never heard at all, and every longer
  hit and every note lost its last stretch, 25 ms with a deep-range Tape.
  They play out now, exactly as on an empty desk, the hold later.

The interface's own round trip, its input and output buffers and
converters, is still not taken off a recording.

## Hear the master while you track

While you track with monitoring through Tangent, the speakers play every
plugin across the master, exactly as the recording gets it. In 5.2 the
speakers dropped them all as soon as an audio input was armed, so a Tape or
a trim on the master went silent in your headphones while the take kept it.
Arming and disarming now crossfade instead of clicking, a balance curve on
the master moves the speakers too, and the click and a recorded part's
placement wait with the master's plugins, so both stay on the beat.

## Every note on its own frame

- **The DX7 and Tangent Keys play every note, release and pedal move on its
  own frame.** They struck each note at the start of the stretch their
  track rendered at once, up to 10.6 ms early, so playback and the exported
  file could disagree and a strum inside one stretch played as a block
  chord. Playback now renders what the export renders, to the bit.
- **The Analog synth, the sampler, the drum kit and hosted instruments take
  every release and pedal move on its own frame**, and a key struck again
  lets its last note go there.
- **Hosted plugins play every stretch of the look-ahead.** At buffers of 64
  to 511 frames a stretch could run 31 frames past the 512 a hosted plugin
  takes, and a VST3 refused it whole: an effect dropped out for 516 frames,
  and an instrument lost every note inside.
- **Tangent Keys renders a part the same at any buffer size and wherever
  Play is pressed**, and the DX7's LFO no longer depends on how the audio
  is cut.

## Tangent Tape, finished

- **The deep wow range reaches 3 % at full Wow, at any Rate**, and follows
  the Wow dial in proportion: 1.5 % at 0.5 and 0.9 % at the default 0.3. In
  5.2.0 it peaked at 2.6 to 2.9 % depending on Rate, and swung further than
  its share lower down the dial.
- **Moving a Tape control no longer clicks.** Throwing Bias across its
  travel on a bass note put a burst of treble up to 11.5 dB over the tape's
  own at 15 ips, and up to 18 dB at 30 ips; Bump at 30 ips gave up to 11 dB.
  This is the tick a double-click reset, a typed value or stepped automation
  could make. Every filter now glides to its next setting a sample at a
  time, and nothing changes while the controls are still.
- **The swaying play head keeps the top octave at 44.1 and 48 kHz.** A
  16 kHz tone pulsed by 4.6 dB at 44.1 kHz, and white noise came out 1.8 dB
  dull at 16 kHz there and 2.3 dB dull at 20 kHz at 48. The head now reads
  through a sixteen-point interpolator: at the defaults the top octave stays
  within about a tenth of a decibel of a still head at every rate, and an
  aged tape loses the same treble at every rate. A still tape plays exactly
  what it did.
- **Tape costs half the processor time it did in 5.2.0.** Sixteen Tapes
  cost 181 to 184 microseconds a 128-frame block at 48 kHz, about 11 an
  insert, where 5.2.0's cost 366 to 403, and one Tape at its defaults costs
  9.7 microseconds against about 22. What it plays is unchanged, bit for
  bit.
- **Tape start and stop costs less.** The block where the tape reaches
  speed rendered every track's look-ahead at once, eight times an ordinary
  block's work; tracks now come back a few at a time, and the dearest block
  of a start costs about 1.3 times the varispeed dial's, where it cost 2.5
  to 5.7 times.
- **The panel's wow and flutter figure reads the meter when Rate is
  slowed.** With Rate at its bottom it could read a quarter of what a
  DIN 45507 / IEC 386 meter measures; it now reads within 11 % of the meter
  across speeds, heads, Rate, Wow and Flutter at 44.1 kHz. Out of the box it
  still reads 0.04 %.
- **Typing 0 into Bias lands on the aligned point**, and no Bias reading
  shows a signed zero.
- **A Tape preset saved in 5.0 or 5.1 sounds the same on every track.** It
  names six of the thirteen controls, and loading it left the other seven
  where the track had them. Every control it does not name now goes to its
  default, as a 5.1 session does.
- **Tape start and stop stays with its project.** A project or template
  saved before 5.2, or one with no desk saved in it at all, kept the switch
  as the last session left it, so Play could spin up a desk that never
  asked for it. These now open with start and stop off; a project saved
  with it on opens on.

## Also fixed

- **One bad sample no longer stops the effects after it.** A plugin or an
  audio interface that delivered a single invalid sample (NaN or infinity)
  left every filter, compressor, Tape and reverb after it silent or broken
  from then on, and from the interface it could take the whole mix through
  the effects bus's reverb. Such a sample is now silence where it comes in.
  Ordinary audio passes through bit for bit.
- **A track with the drum kit, armed and monitored, runs its effects once
  per block.** Its effects ran twice, and a bypassed Tape, Denoise or IR
  mixed the two signals. While the input is monitored, the kit now goes past
  the track's effects, in time with the rest of the mix.
- **The click's effects stay whole in the take.** A reverb on the click was
  cut off in the take 54 ms after each beat while the speakers played it
  whole.
- **The Windows zip's permanent download link works again.** The
  version-less `tangent-windows-x86_64.zip` was left off 5.2.0, so that link
  answered 404. Every stable release uploads it again.

- **The welcome card comes up once on every new version**, even if its
  "Don't show this again" box was ticked, so you see what is new.

## New in 5.2

### Tangent Tape, a tape machine

Tangent Tape runs at four speeds, 3.75, 7.5, 15 and 30 inches per second,
and each is its own machine: the speed sets the playback EQ, where the head
bump sits (near 35 Hz at 15 ips and 70 Hz at 30), how much loud treble the
tape can take, and the colour of its hiss. Changing it while playing
re-voices the machine smoothly, and the pitch never moves.

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
  warble, 0.9 % at the default Wow and 3 % at full. While it is selected
  Tape reports 25 ms of delay, compensated across the desk.

**A panel of its own.** Tape's controls stand in four groups the way a tape
machine lays them out, Transport, Record, Tape and Playback, and three of
the captions show what the dials come to: the weighted wow and flutter, the
hiss floor and the head bump's frequency.

A fresh Tape is a 15 ips machine lined up the way its tape's datasheet
says: Drive opens at 0.5, about 10 dB of headroom over 0 VU, so normal
levels stay clean and loud peaks round off. Its hiss is white behind each
speed's playback EQ and does not swell with the music, and it is the same
at every sample rate.

### Tape start and stop

Right-click the PITCH CONT dial: under its range a switch makes Play spin
the whole desk up from a standstill, and Stop or Pause wind it down to a
halt, the pitch following the speed as a tape's does. Out of the box the
start takes half a second and the stop 0.7 s; either can be set from 0.1 to
10 s. As the tape slows the treble dulls and the low end sinks, and the
click joins on the first beat the tape reaches at full speed. A take films
the tape as the room hears it; arrangement recording, count-ins, exports
and freezes never use it, and while an armed input is monitored it stands
aside, so Play and Stop stay instant on a tracking desk. It is saved with
the project and off until you switch it on.

### A new session plays a grand piano

The default project's second track, the one that opens armed, holds Tangent
Keys on its Intimate Grand instead of Tangent DX7. The drums stay on the
first track, and the DX7 is still in the instrument picker.

### Also fixed in 5.2

- A bypassed Tangent Denoise or Tangent IR stays in time: a bypassed bay
  plays its input exactly as late as it reports, so bypassing changes what
  you hear and never when.
- An input monitored under the PITCH CONT dial plays at its own pitch
  whatever its channel count. A 64-channel interface's input played a 1 kHz
  tone at 874.5 Hz.

## New in 5.1

### Tangent Keys, rebuilt

Every sound in Tangent Keys was re-voiced against measurements of a
reference instrument, one measured quantity at a time: how each partial
decays, how the level follows velocity, the ring around a note, the
release. The keyboard holds ten sounds:

- **Intimate Grand** and **Concert Grand**: built from banks of string
  modes, three strings to a note coupled at the bridge, a hammer graded by
  key and velocity, the action's knock and the dampers' thump, and
  sympathetic strings that ring around every note. Both half-pedal, hold a
  sostenuto (CC 66) and shift for the una corda (CC 67).
- **EP Mk.1** and **EP Suitcase**, the tine pianos, dry and through their
  amp.
- **Clav** and **Clav Wah**.
- **Reed 200** and **Reed Amp**, the reed piano dry and through its
  amplifier, with the cabinet's slap, its throb and a room.
- **Electric Grand**: short, stiff strings read by pickups, with strings of
  its own that ring in sympathy under the pedal.
- **Nylon Guitar**: six nylon strings under a fingertip. Each note goes to
  the string a hand would choose, and the open strings and the body's air
  ring with what is played.

**Presets for every sound**, with the same preset strip as the effect
panels. Played hard, the ten sounds are within about 5 dB of one another,
and a Keys track sounds the same at any buffer size.

### Also fixed in 5.1

- A named chord spells its notes from its root on the staff, the Tonnetz
  and the circle of fifths.
- The staff's alternate reading names every sounding note.

## Before you upgrade

### From 5.2

- **A fresh Tape's studio head is brighter**, because it keeps its top
  octave: white noise through it comes out 0.4 dB brighter at 10 kHz,
  0.9 dB at 12.5 kHz and 1.9 dB at 16 kHz at 44.1 kHz, up to 2.5 dB at
  20 kHz at 48 kHz, and a quarter of a decibel at most at 96 kHz.
- **Parts you recorded in 5.2 on a desk that delay compensation held back
  stay where they landed**: up to 25 ms late against the music with a
  deep-range Tape on the desk. Nudge them if they sound late.
- **Parts on the DX7, Tangent Keys and the other instruments play their
  notes where they are written.** A note that sounded up to 10.6 ms early
  now sounds on its own frame, so a part you tightened by ear against the
  old timing may now sit a little late, and an export of such a part is not
  the same file it was.
- **Tracking through Tangent now carries the master's plugins.** What you
  hear of your own playing comes as late as the plugins across the master
  hold the mix: 2 ms with a Tape in its studio range, 25 ms in its deep wow
  range, 10.7 ms with an IR. Take the ones that add delay off the master
  while you track if you want it tighter.

### From 5.0 or 5.1

- **Tape tracks move 2 ms earlier.** Tape tells delay compensation about
  its 2 ms play head, so a Tape track saved in 5.0 or 5.1 plays in time with
  everything else. A track nudged by hand to make up for the old offset will
  now be 2 ms out.
- **A Tape saved in 5.0 or 5.1 hears the new machine.** Its six knobs are
  kept and the new controls read their defaults, so it plays at 15 ips:
  about 2 dB more headroom at the same Drive, the head bump an octave lower
  and a little flutter. Wobble is now Wow alone. Switching to 7.5 ips brings
  back the old curve.
- **A default template you have built on keeps its instrument.** One that
  is still as Tangent wrote it (no clips, markers, automation or track
  names, no other plugins, the tempo and meter it came with, and its
  instrument on the sound it came with) is replaced at the next launch with
  the new desk. Levels or knob settings changed on their own do not keep a
  template: name one of its tracks to keep it.
- **The developer switches are spelled with Tangent's name.** The
  environment variables that turn on diagnostics start with `TANGENT_`, and
  the old prefix is no longer read.
- **On Linux, Tangent's MIDI input is named `tangent` (port `tangent-in`).**
  A patchbay connection saved to the old names has to be made again once.
- **A 4.x settings folder is left where it is.** 5.0.0 and 5.1.0 deleted it
  at launch; it is still not read.

### From 5.0

- **A mix balanced on the 5.0 Keys sounds may want its faders looked at.**
  Against 5.0.0, at the same velocities, the Intimate Grand plays 2 to 4 dB
  louder through the middle of its range and 7 dB louder at fortissimo. The
  Concert Grand plays within 2 dB through the middle and 7 dB louder at
  fortissimo. EP Suitcase is 2 to 4 dB quieter, and the two clavs are 3 to
  9 dB louder.
- **A track saved on Reed 140 opens on Reed Amp** with the knobs it had; its
  Tremolo sets the depth of the amplifier's throb. A track saved on Piano 1
  or Piano 2 opens on the Intimate or the Concert Grand.
- **The two grands and the Electric Grand play A0 to C8.** A key outside a
  piano's 88 is silent on them, as it is on the instrument.
- **A Keys track holds 22.5 MB of memory** (5.7 MB in 5.0), all of it taken
  when the track is made, never while it plays.

## Known

- With monitoring off, as with an interface's direct monitoring, the
  speakers still leave out the plugins across the master while an audio
  input is armed. Hearing them there needs a second copy of every master
  plugin, which is planned next.
- While you track with monitoring on, the take carries your playing as late
  as the master's plugins hold the mix, just as you heard it. A part
  recorded onto a track is placed on the beat either way.
- A few kinds of armed track (one that sends into a bus with a delay of its
  own, a folder, a frozen track) still wait with the mix while the song
  plays them back.
- The interface's own round trip is not taken off a recording.

## Downloads

| Platform | File |
|---|---|
| macOS 11+ (Apple Silicon and Intel) | `Tangent-5.3.0-macos.pkg` (installer), `Tangent-5.3.0-macos-universal.dmg` / `.zip` |
| Windows 10+ (x64) | `Tangent-5.3.0-windows-setup.exe` (installer), `tangent-5.3.0-windows-x86_64.zip` (portable) |
| Linux x86_64 / aarch64 (glibc 2.30+) | `tangent-5.3.0-linux-x86_64.tar.gz`, `tangent-5.3.0-linux-aarch64.tar.gz` |
| FreeBSD 15 amd64 | `tangent-5.3.0-freebsd-FreeBSD-15-amd64.pkg`, plus `-desktop.tar.gz` and `-complete.tar.gz` offline bundles |

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

**FreeBSD:** `pkg install ./tangent-5.3.0-freebsd-FreeBSD-15-amd64.pkg`
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
