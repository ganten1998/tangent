# Changelog

All notable changes to Tangent are documented in this file. Entries before 2.3.0
refer to the app under its former name, Ivory.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions 1.x are the original Python/Qt app; 2.x is the Rust rewrite.

## [Unreleased]

### Added

- **Tangent Keys: a piano and an electric piano, computed rather than
  recorded.** A third built-in instrument beside the DX7 and the drumpad, on
  the sentinel `@tangent-keys`, containing no samples at all. **Piano 1** is a
  jazz grand voiced for solo playing, built as a digital waveguide — a delay
  line whose length is the pitch, a frequency-dependent loss filter for the
  decay, and an all-pass cascade for the stiffness that stretches a piano's
  partials sharp. **EP Mk.1** is a tine electric piano taken dry, and it is
  built a completely different way on purpose: a tine is a stiff bar, not a
  flexible string, so its modes are nowhere near harmonic and a waveguide
  cannot make them. It is modal instead — a bank of decaying sinusoids, each
  sitting where the metal actually puts it. Its velocity response is three
  mechanisms rather than a volume curve: a contact time that shortens with
  force and therefore feeds the high modes, a neoprene tip that stiffens as it
  compresses, and an electromagnetic pickup whose output goes as the inverse
  square of its gap. The last one is why hitting it hard *barks* — the transfer
  is lopsided only when the tine is swinging wide, so the distortion arrives
  with force and is simply absent when the note is quiet, which is something a
  waveshaper on the output could never do without dirtying the soft notes too.
  Measured on E4 across MIDI velocity 8 to 127, the bell runs from 48 dB under
  the fundamental to 18 dB under it, and waveform asymmetry from 0.017 to
  0.382, both monotonically. The whole module is pure `std` and adds no
  dependency, the same as the DX7's, so it compiles standalone on a bench that
  renders a scripted performance to a WAV in under a second — which is how all
  of the above was tuned and how three defects invisible in the source were
  found. `docs/KEYS.md` has the design notes and the measurement method.

- **The in-house instruments lost their amount limit.** Every strip whose
  first bay names the DX7 or Tangent Keys now owns a full instance of it, the
  way every drum lane has always owned its kit. Before this, one shared synth
  hid behind the mask: a second DX7 track was silent by design, and choosing
  the Rhodes on one keyboard track silently re-voiced the piano on another.
  Now eight piano tracks, or a piano and an EP side by side, or two DX7s
  playing out of two different cartridges, are all just tracks. Which
  cartridge, which patch and how the panel knobs sit are saved per strip, the
  export bounce mirrors all of it, and old sessions load with every strip
  seeded from the session-wide choice they were saved with.

- **Tangent Keys grew a panel: two sounds, two knobs.** Clicking the bay
  still lists Piano 1 and EP Mk.1, and beneath them now sit the two controls
  that survive the argument against knobs on a physical model. Voicing scales
  the hammer contact the way felt does, mellow to bright, with the centre
  being exactly the tuning the bench signed; Width moves the per-key stereo
  seating from a point source to a little wider than stock. Both are per
  strip, both live on the physics rather than on the output, and choosing a
  sound no longer closes the window, so you can pick, voice and play in one
  visit.

### Fixed

- **A capture buffer smaller than the PipeWire graph's cycle silently halved
  the recording**, and on the machine that found it, every ALSA video take
  came back at double speed against its own video with up to half its frames
  counted as dropped. The graph hands the `pipewire` PCM one quantum per
  cycle; a `Fixed(256)` request under a 512-frame `clock.min-quantum` gave the
  ring 256 of every 512 frames and *discarded the rest below every counter* —
  no xrun, no error, honest timestamps, a clean-looking stream at exactly half
  rate. The take's timeline is indexed by device frames, so a fifteen-second
  performance was written as a "7.5-second" take, and the desk's tap ring —
  drained at the halved pace — overflowed from the eighth second on, which is
  where the huge `frames_dropped` numbers actually came from. The input now
  sizes its ring to cover twice the graph's floor (the request still decides
  the period, so nobody's graph gets dragged coarser), and `takeprobe` — a new
  headless instrument that runs the app's own capture drain against a real
  device — measured the same stream at full rate with the fix and half rate
  without it. JACK takes were never affected: a JACK client's buffers are the
  graph's own cycle, so there is no ring to mismatch.

- **A take whose measured rate is far from its nominal one now says so out
  loud.** The rate fit measured the halving above perfectly — `epsilon_ppm:
  -499993` sat in every damaged manifest — and nothing read the number. A
  device-fed take with a fitted clock more than 10,000 ppm off nominal now
  raises a problem naming both rates: crystals lie by parts per million,
  delivery faults lie by factors, and there is nothing between.

### Added

- **JACK on Linux**, beside ALSA, in the Audio Setup panel's SYSTEM row — the
  row that has been a readout on every Linux build until now. Choosing it makes
  Tangent an ordinary client of the graph, named `Tangent`, so its ports appear
  in qjackctl, Carla or Patchance and can be patched into a DAW. It works
  against real jackd and against PipeWire's JACK layer equally, which is what
  nearly every desktop Linux is running.

  **It is offered for the routing, not for the latency.** The expectation was
  the opposite — a JACK client skips pipewire-alsa and the four-period ring
  `BUFFER_PERIODS` had to add for it — and the measurement did not support it:
  capture to playback through the monitor ring came out at 23.4 ms against
  ALSA's 5.6 ms on the dev box. `docs/LINUX-JACK-FINDINGS.md` has the runs and
  shows why neither figure means what it looks like, the short version being
  that pipewire-alsa reports a PCM delay of zero (so the ALSA number omits the
  output leg entirely) and cpal opens two independent JACK clients rather than
  one duplex client (so the JACK number carries a whole extra graph cycle).
  Settling it needs `rtclick` on a wired loopback. Until then, JACK is the
  right answer for being patchable into a session and the wrong one for a
  smaller number.

  What it does fix outright is **which interface you record.** Through ALSA on
  a PipeWire machine there is one openable device, called `pipewire`, and it
  follows whatever the desktop's default source happens to be — the fault that
  once recorded a laptop microphone with an interface plugged in. Through JACK
  the ports carry the interface's real name.

  **It costs the artifact nothing.** libjack is `dlopen`ed on demand rather than
  linked, so `readelf -d` on the Linux binary gains no entry and a desktop with
  no pro-audio stack installed runs exactly what it ran before. No new crate
  enters the dependency graph either — `jack` was already there for the FreeBSD
  build. Choosing JACK on a machine where no server answers falls back to ALSA
  rather than leaving the app silent.

### Fixed

- **The Audio Status line no longer names a ring that does not exist.** The
  period and the ring were derived from `BUFFER_PERIODS`, a per-PLATFORM
  constant, where they are really a property of the HOST in use — so a Linux
  build had one number for two driver stacks. Under JACK that would have
  reported a 128-frame period for a 512-frame callback, next to a 2048-frame
  ring belonging to nobody. `BufferRule` now carries the distinction from the
  open, where the real host is known.
- **Under JACK the input picker no longer offers devices it cannot open.** The
  PipeWire node list is a workaround for cpal's ALSA host, and it REPLACES
  cpal's own list rather than joining it — so through JACK, where there is no
  `pipewire` PCM to aim, every row in the picker would have failed to open and
  the one that works would have been hidden behind them. A saved `pw:` choice
  from an earlier ALSA session falls back to the host's default instead of
  resolving to nothing.

## [5.0.0] - 2026-08-31

The version the arrangement became real in — groups that are buses, automation
that reaches a plugin, a timeline that keeps its promises, and a fourth
platform.

### Added

- **Folders are buses.** A group used to be a drawing: it indented its rows and
  changed nothing about the sound. A folder now sums its children in the
  engine, runs its own rack over that sum, and its fader, pan, send and meter
  govern the group — so a compressor in a folder's bay is a bus compressor and
  the meter finally moves. A folder drags its children with it, and they stay
  its children when it lands.
- **Automation across four modes**, with plugin parameters as first-class
  lanes. The master automates like any track, across all six of its bays.
- **Delay compensation that understands groups.** Every source waits for what
  it does not already pay: its own rack, plus every rack it is folded through.
  Drum kits are compensated too — they never were, so with any lookahead plugin
  loaded the drums played early against everything else.
- **LV2 hosting on Linux and FreeBSD**, plugin editor windows included. VST3
  everywhere.
- **FreeBSD is a supported platform**, with a native OSS backend for cpal
  beside the ALSA shim, native MIDI, and a package rather than a script.
- **Sixteen themes with per-theme palettes**, each carrying its own master
  colour, its own automation dial and its own strip colours — solved against
  contrast floors rather than picked by eye.

### Changed

- **Nothing saves itself.** Every edit used to reach the disk within a frame,
  which is an autosave under another name: there was never an unsaved change,
  so there was nothing to prompt about and no way to walk away from a session
  without keeping it. New, Open and Quit now ask before they discard anything.
  One deliberate exception: the drum kit sidecar still saves itself, because a
  kit is hours of sample assignment and the one thing a session must never
  lose to a missed save.
- **New starts from the shipped template, always.** Seeding the default wrote
  the LIVE desk and ran on every New — so opening somebody's session and
  pressing New quietly re-wrote the default with that session's desk.
- **The app wears its own wordmark**, the Tonnetz lattice the splash and the
  theory band already draw, drawn a second time for small sizes because a
  hairline does not survive being resampled into a taskbar.

### Fixed

- **A curve on a fader no longer makes that fader dead.** A lane carrying
  nothing but its base node had the engine reading the curve and the desk
  reading the fader, and the two could disagree for ever: one session played
  28 dB down while its master read 0.0 dB and pushing it did nothing.
- **A folder is folded exactly once.** Four places put a channel's audio on the
  bus and each ran the whole strip again over the group — so a stateful plugin
  advanced at twice real time and put out noise, and a clip on a folder at
  -6 dB played at -12.
- **The window icon fits in one X11 request.** The new artwork asked for 24 MB
  in a single `_NET_WM_ICON`, which poisons the connection rather than being
  refused as a property — so the app failed to start on Linux and FreeBSD with
  an error that named neither X11 nor icons.
- **Quitting with unsaved changes can be discarded.** The prompt's own Discard
  answer could not dismiss it: the flag it tested was the one the host consumes
  to close the window, so the question came straight back for ever.
- **A group is no longer offered as a send destination**, and a route into a
  track that becomes one is dropped rather than left carrying audio that
  reaches nothing.

## [4.5.0] - 2026-08-18

Tangent makes a sound out of the box, and you can hear what you are looking at.

### Added

- **A built-in instrument.** Out of the box the app drew a piano and made no
  sound: every note it could name, engrave and place on a fretboard was silent
  until a VST3 was found, installed and loaded. There is now a tine electric
  piano behind it, and it plays whenever no plugin is loaded. It is synthesised
  rather than sampled, which costs no bytes at all, ships on every platform,
  cannot be missing, and raises no question about what may be redistributed. It
  steps aside the moment a real instrument is loaded.
- **Space sounds what is highlighted.** Notes toggled onto the neck, placed on
  the circle of fifths, or arriving from a keyboard are the same notes by the
  time they are drawn, and "play what I can see" is the only rule worth
  remembering. Space still stops a take while one is rolling, which is what
  every DAW has already trained into the key; the rest of the time "stop" means
  nothing.
- **Clicking a key or a fret sounds it** when keytoggle is off, and lets it go.
  It used to do nothing at all, which is the wrong answer for a picture of a
  piano.
- **Letter names on the keys and on the neck's dots**, off by default, one menu
  row each. They are a beginner's stickers: the fastest way to learn where the
  notes are and clutter the moment you have.

## [4.4.1] - 2026-08-18

A Linux fix release; the macOS and Windows builds are unchanged. Found by
running 4.4.0 on the machine it was built on, under i3.

### Added

- **The camera works on Linux.** A V4L2 backend (via `linuxvideo`), behind the
  same seam as the macOS one: enumeration by stable bus identity, the shared
  format-choice policy, YUYV converted directly and MJPEG decoded (pure-Rust
  `zune-jpeg`) — which matters because most UVC webcams offer their full frame
  rate only compressed. Preview and recorded takes both.

### Changed

- **The Linux and Windows artifacts now carry their own video encoder.** An
  unmodified, checksum-pinned static ffmpeg ships as `tangent-ffmpeg` beside
  the app binary, and Tangent finds it there before consulting `PATH` — so
  filming a take needs nothing installed. Its GPL licence and provenance ship
  beside it in `ffmpeg-licenses/`. (macOS needs none of this: AVFoundation.)
- The Linux installer now checks for a Vulkan driver — the one remaining
  system dependency for video, present on nearly every desktop — and prints
  the distribution's exact one-line fix when it is missing, instead of leaving
  it to be discovered mid-take.

### Fixed

- **The right-click menu no longer vanishes on the way to its lower items**
  under i3 and other Linux window managers. The desktop close rule was "close
  when neither menu window has focus"; i3 hands focus to the submenu window
  and never gives it back, so the first hover of a plain row read as
  "clicked away". The rule now also requires the pointer to be outside both
  windows.
- **Dialogs cannot open invisibly behind the main window on tiling WMs.**
  Dialogs are modal — the app drops all input while one is open — and i3 was
  tiling them underneath the floating main window, which read as the app
  freezing the moment a camera was selected. Menus and dialogs now carry the
  X11 window-type hints (`POPUP_MENU`, `DIALOG`) that tell the WM to float
  them.
- The empty device-picker no longer sends Linux users to "System Settings >
  Privacy & Security"; it names the actual fix (the `video` group).
- **A slow machine no longer records a fast video.** The video's frames are
  timestamped on the take's clock, but a machine that composites slower than
  real time used to fall behind schedule and stay there — every frame carried
  late content at an early timestamp, and a whole performance came out
  time-compressed. The pump now holds the timeline: when compositing cannot
  keep up, the missed ticks repeat the previous frame (a visible, counted
  stutter) and the fresh-frame work is capped by wall time per window frame so
  a saturated machine keeps a usable UI. The take summary reports repeated
  frames alongside dropped ones. All platforms.
- **`take.json` now records the video.** The manifest is written at Stop,
  before the encoder has finished the file, so its `video` section was always
  null and `take.mp4` never appeared in `files` — on every platform, since the
  recorder shipped. The session now keeps the manifest it wrote and folds a
  full video report in when the encoder finishes: container and codecs, size
  and rate, frames the rate implies, and frames the camera actually delivered.
- **Filming a take can no longer take the app down on Linux.** The video
  compositor needs a Vulkan adapter; on machines without one it used to fall
  through to wgpu's OpenGL backend, whose first make-current against the
  window's own GL context is `EGL_BAD_ACCESS` — an abort, mid-take. A GL
  adapter is now refused up front with a message that names the fix
  (a hardware Vulkan driver, or mesa's lavapipe on any GPU), and a software
  Vulkan adapter is accepted when the hardware ask comes up empty.

## [4.4.0] - 2026-08-18

The largest release since the Rust rewrite. Tangent writes music down, and a
take is now simply the window it was played in.

### Added

- **A sheet music panel.** Six clefs, all fifteen key signatures, accidentals
  spelled for the key you are in, `8va` above and below the ledger lines, and
  letter names inside the noteheads by default. Several clefs can be stacked at
  once and every staff shows every note, which is a teaching view: a violist
  and a cellist read the same chord in their own clefs from one keyboard. Key
  signatures are on the staff's own right-click menu.
- **The chord readout names its runners-up.** The staff carries the chord name
  and the next two best matches, so "C6, or Am7" instead of a single answer to
  a question that has two.
- **The theory band is four panels you arrange.** `1` to `4` toggle circle of
  fifths, Tonnetz, harmonic triangles and sheet music. Pressing a number for a
  panel that is showing moves it to the end, so the same keys choose what is up
  and where. All four off collapses the band; any number from empty fills it.
- **Video on Windows and Linux.** They encode through `ffmpeg`, which has to be
  on the PATH; a take that cannot find it names the install command and still
  writes its audio and its MIDI. macOS is unchanged and still encodes natively
  through AVFoundation, with nothing to install.
- **A take-settings panel behind a settings cog**, holding everything that used
  to be nine controls in the middle of the recorder band.
- **Five instrument slots**, up from three, and a pair of VU meters.
- **The chord strip is a setting**, off by default, so piano and chord name and
  nothing else is reachable again.

### Changed

- **A take records the window.** It used to be an arrangement of its own, with
  the app's bands in one pane, the camera composited into another and a layout
  picker deciding which floated over which. Now the video is the window: the
  same bands in the same order, with the camera where the window already puts
  it, at the top left of the recorder band. The layout picker is gone.
- **The window is 16:9** while the usual bands are up, so a take needs neither
  letterbox nor crop. The theory band absorbs the difference and gives up about
  five per cent of its height.
- **The video compositor no longer depends on the renderer the window is
  using.** It borrows the window's graphics device when there is one and opens
  its own when there is not, which is what made it look macOS-only: it is wgpu
  and egui, the same two things that draw the window, and it was never
  platform-specific.
- **Nothing is behind a supporter key.** The heart is drawn for everybody, in
  the colour they choose, and hovering it shows the thanks. There are no locked
  features in this build, and none are planned.
- The recorder band is a transport again: record, stop and the cog as three
  icons of one size, with reachable metronome and input faders.
- VST3 scanning is recursive, takes folders you add yourself, and rescans
  without a restart.
- The launch splash spells TANGENT on the app's own Tonnetz.

### Fixed

- The Welcome card waited for the splash instead of opening on top of it, and
  fits its own text again.
- Chord alternates no longer flicker between equally scored matches.
- Em dashes are gone from every string the app can put on screen.

## [3.0.0] - 2026-08-14

### Added

- **The fretboard works as an input.** With Keytoggle on, click the neck to
  place notes and read the chord off the piano above, instead of only the other
  way round. Shapes stay where you put them: entered by hand, a voicing is no
  longer rearranged into the position Tangent would have chosen. Clicking a
  second fret on a string moves that note rather than adding one the guitar
  could never sound.
- **Keyboard shortcuts.** Hold **H** and the list slides down; let go and it
  slides away. `K` keytoggle, `R` clear the notes you placed, `G` guitar view,
  `C` chord detection, `D` dark mode, `B` window border, `F` cycle typeface,
  `A` about, `S` supporter key.
- **JetBrains Mono** as a third bundled typeface, alongside Courier Prime and
  Terminess. Cycle with `F` or from the menu.
- **A theory band.** A tall section above everything else with three ways of
  looking at what you are playing, any combination of which can be showing at
  once. Choose them under **Theory** in the right-click menu, or press `T` to
  walk through them.
  - It shows what you **put there**, not what you are playing, so it stays put
    while your hands are busy — the point of it is to look at a key or a chord
    while working on something else. With Keytoggle on, click the piano, the
    neck, or the diagrams themselves to place notes; clicking a chord vertex
    places the whole triad. **Follow MIDI** in the same menu turns live
    tracking on if you want it.
  - One meaning, one mark, across all three: a note you are sounding is a
    filled disc in the same colour the piano uses for a held key, the root is
    that disc with a ring round it, and a key that contains everything you are
    playing gets a soft wash. Each has a one-line legend.
  - **Circle of fifths** — every key shaded by how much of what you are playing
    fits it, so keys that are close light up together. A dot on the rim for
    each note you are sounding: a triad is a tight cluster, a tritone is a
    straight line across. Relative minors sit on the same spoke as their major,
    because they have the same key signature.
  - **Tonnetz** — the lattice where left-to-right is fifths and the diagonals
    are thirds, so every major triad is a triangle pointing up, every minor
    triad one pointing down, and two chords sharing two notes share an edge.
  - **Harmonic triangles** — I, IV and V pointing up with i, iv and v inverted
    through the same centre, each lit when you play it. Between them those
    three chords use every note of the key and no others, which is the reason
    so many songs need no more than them.
- **A VST3 plugin.** Tangent now runs inside a DAW as well as on its own, with
  the same display: chord readout, guitar view and theory band, reading the
  notes on the track it is on. It produces no audio, so it belongs on a MIDI or
  instrument track rather than in an effect slot. Its settings live in the DAW
  project, not in your global settings file, so two instances and the
  standalone can be set up differently. macOS is a signed universal binary.
- **Installers**, one per platform, with the app and the plugin as separate
  choices — install either, or both. Each puts the plugin where the VST3
  specification says it goes, so every host finds it without being configured:
  `/Library/Audio/Plug-Ins/VST3` on macOS, `Common Files\VST3` on Windows, and
  `~/.vst3` on Linux. The Linux tarball carries an `install.sh` that needs no
  root and understands `--app`, `--vst3`, `--prefix`, `--uninstall` and
  `--dry-run`.

- **A shortcut for naming a chord.** Hold the chord, press **N**, and start
  typing: the box opens with the detected name selected, so the first
  keystroke replaces it. Naming a voicing is what this app is for, and it used
  to take a right-click, a scan down a long menu, a click, and then another
  click into the field. **E** corrects the current reading, **M** opens the
  chords you have taught, **L** turns chord learning on and off, **P** swaps
  sharps for flats. All of them, and the rest, are on the **H** card.
- **Barres are something you draw.** Hold and drag along a fret and every
  string you cross is fretted there. Placing two notes that happen to share a
  fret no longer draws a bar across them — by hand, a barre is a thing you
  meant.
- **Capo styles.** Click the capo to cycle black, brushed silver and wood.
  Black by default, because the default fingerboard is rosewood.
- **A first launch shows everything.** A brand-new install opens with the
  guitar view and all three theory diagrams on. An existing settings file is
  never rearranged.

### Changed

- **Real flats and sharps.** ♭ and ♯ throughout the theory band, set the way
  music is set — raised and smaller, at the top right — rather than spelled
  with the letter "b". The relative-minor ring is legible again: it used to
  lower-case whole names, so "Bb" came out as "bb".
- The theory diagrams' outlines, roman numerals and key signatures are darker
  on light and lighter on dark; they were pitched as background texture and
  were hard to read in both modes.
- Detached windows rise and fall with the main window, so focusing the piano
  brings its own readouts with it.

### Fixed

- **`tangent.exe` has never had an icon.** Every Windows release was
  cross-compiled from macOS, and the resource step was behind two gates that
  both test the machine doing the building rather than the machine the program
  is for — so it never ran once, and the shipped binary had no icon in
  Explorer, none on the taskbar, and blank Properties. It now has both an icon
  and a version block.
- **JetBrains Mono could not be selected.** It was bundled and listed in the
  menu, but cycling with `F` alternated between the first two faces forever.
- The right-click menu inside a plugin: every row was dead, and the menu had
  no visible edge against the app behind it. It also fits the editor now
  rather than running off the bottom.
- Turning a band on inside a DAW makes the editor taller instead of shrinking
  everything to fit the height it already had.
- The capo no longer stands up into the piano above the neck, and the Tonnetz
  keeps its bottom row below 100% window size.

## [2.3.0] - 2026-08-13

### Added

- **A guitar view.** A fretboard under the piano showing the same notes where a
  guitarist would actually play them. Turn it on with **Show Fretboard** in the
  right-click menu; it is off until you ask for it, so the window does not
  change size on you after an update. **Tuning** (Standard, Drop D, DADGAD,
  Open G, Open D, Half Step Down, and a 4-string bass) and **Capo** appear in
  the same menu while it is showing.
- One MIDI note can be up to six places on a guitar. Middle C is five of them,
  and the high E string cannot reach it at all, so the view picks the shape a
  player would use, weighing hand span, open strings, barres and where the hand
  already is. It then holds that shape steady while you add notes to a chord,
  instead of jumping around.
- Notes that will not fit are never quietly missing. An open string is a ring
  behind the nut, a note outside the guitar's range is a hollow dot with an
  arrow showing which octave it moved, a note the guitar can make but not
  alongside the others is a faint ring where it wanted to go, and a string to
  damp is an `×`. Anything genuinely left off is counted underneath: "6 of 10
  notes".

- **Three fingerboard woods** (Rosewood, Maple and Ebony) under **Wood** in the
  same menu. Each is a whole palette rather than a stain: on maple the strings,
  frets and inlays go dark, because light strings on blonde wood cannot be
  seen.
- **The fretboard pops out** into its own window with **Detach Fretboard**,
  exactly like the chord display: close it to put it back, right-click anywhere
  for the menu, and it remembers where you left it.

### Fixed

- The fretboard no longer resizes itself when it has something to tell you. The
  status line used to take a fifth of the panel, so the neck shrank and grew as
  notes went in and out of range.
- Note dots are smaller, so two on neighbouring strings no longer run together.
- The piano no longer shows faint gaps between some white keys. Each key was
  drawn a pixel narrower than its slot, so about a dozen slivers of background
  showed through, spaced irregularly across the keyboard. Visible in light mode
  only, which is why it lasted this long.

### Changed

- **Renamed from Ivory to Tangent.** Synthogy's Ivory is a well-known virtual
  piano instrument, and a plugin build would have put this app in the same
  browser. The clavichord's tangent is the blade at the back of each key that
  strikes the string and stays in contact with it for as long as the note
  sounds, which is what the display does with a held note.
- The binary is now `tangent`; artifacts are `Tangent-<version>-macos-*`,
  `tangent-<version>-windows-*` and `tangent-<version>-linux-*`.
- Nothing you have is affected. `~/.config/tangent/` keeps your settings and
  taught chords, existing supporter keys keep working, and downloads published
  under the old `Ivory-*` names keep resolving.

### Fixed (from the 2.2.0 tester report)

- Child windows open centred on the main window instead of the top-left of the
  screen.
- The detached chord window is no longer locked to the piano's width, opens at a
  readable 460x150, and remembers its size and position.
- The main window remembers its position.
- Long labels such as `Eb Minor Pentatonic` wrap to two lines instead of
  shrinking to a fraction of the size of a short name.
- The detached window has a border, so it no longer bleeds into a dark desktop.
- "Apply in all keys" in Teach Chord Name starts checked and remembers the last
  choice.

## [2.2.0] - 2026-08-12

### Added

- **Supporter keys.** Ivory stays free and complete; a supporter key is a thank
  you, not a gate. Nothing is removed from the free app — chord detection,
  Chord Learning, every font and setting all stay exactly as they are. Keys are
  verified entirely offline: no network call, no account, no expiry, no machine
  fingerprint, and no phoning home, ever. A missing, corrupt or mistyped key
  simply leaves you with the free app rather than an error.
- **Supporter heart** — a small pixel heart in the corner of the chord view for
  supporters. Click it to cycle its colour; hide it from the menu if you would
  rather not have it there.
- **A welcome note** on first launch explaining what the app is and how it is
  paid for, with a "don't show this again" checkbox.
- **Terminess Nerd Font Mono** as a bundled font option, in regular and bold.
  Courier Prime remains the default.

### Changed

- Chord-engine internals rewritten: pitch-class set algebra now runs on 12-bit
  masks instead of hash sets, so chord detection — which runs on every MIDI
  event — is allocation-free on its hot path and about 5–6× faster. Every name
  the engine produces is byte-identical to the previous release across the full
  13,133-voicing test corpus, in both the stock and Chord Learning builds.
- Diminished, half-diminished and augmented chords now use standard lead-sheet
  symbols: `C°`, `C°7`, `Cø7`, `C+` in place of the spelled-out forms.
- `IVORY_LOG=debug` turns on diagnostic logging. Quiet by default; the app
  writes nothing unless asked.

### Fixed

- Scales and modes keep their reading when the run is closed off with its own
  octave. `C-D-E-F-G-A-B-C` now reads `C Ionian` instead of `CΔ13`, and the same
  goes for the natural minor, both pentatonics and both blues scales. A spread
  tertian stack is still named as a chord, so real `CΔ13` and `C6/9` voicings
  are unaffected.
- `C-D-E` reads `C(add9)`. It was being pulled into `D7(#11)` by an identity
  bonus that fired without the tritone that defines a #11 chord; the bonus now
  requires that tritone to actually sound. Slash upper structures such as
  `C7(#11)/Gb` are unchanged.
- A major add9 with its 3rd in the bass now reads as sus2 over that bass, rooted
  on the add9's own root: `C-E-G-D` over E is `C2/E`, and `C-Ab-Bb-Eb` is
  `Ab2/C`. Minor add9 voicings never trip this and still read `Xm(add9)/bass`.
- A bass-rooted maj7 shell that also carries the 6th/13th reads as a major 13th:
  `B-D#-G#-A#` is `BΔ13`, not `G#m(add9)/B`.
- A scale must now account for every note you are sounding. A six-note
  `C-D-E-F-G-A` is no longer named as the five-note `C Major Pentatonic` it
  merely contains — it reads `Dm11`. Genuine pentatonics and exact six-note
  scales are unchanged; all twelve pitch classes together read
  `Chromatic Scale`.
- The Dock showed a generic hexagon instead of Ivory's icon on macOS, and the
  icon itself is now a properly rounded, padded squircle rather than a
  full-bleed square.

## [2.1.0] - 2026-08-04

### Added

- **Chord Learning.** *Correct Chord Name…* lists the readings Ivory actually
  weighed for the voicing you are holding, with their scores, and trains a
  general preference toward the one you pick — so similar voicings shift too,
  rather than just the chord in front of you. Measured reach: one correction
  changes 1,182 of 13,133 corpus voicings (9%), often in unrelated keys.
- *Enable / Disable Chord Learning* in the context menu, and a **Forget
  Learning** button in *Manage Taught Chords…* that restores stock naming
  exactly — verified to restore all 13,133 corpus readings.
- *Manage Taught Chords…* now shows whether learning is on, how many
  corrections have been made, and which leanings have been picked up.
- Every training attempt reports its outcome in plain language: learned,
  already wins, outranked by a later naming rule, too far behind to nudge, or
  not one of the readings Ivory weighed.

### Fixed

- The re-ranker could blank the chord display. Candidate admission was applied
  after the learned adjustment, so a few ordinary corrections could push every
  candidate for a voicing below the threshold. Admission now uses the
  unadjusted score and only ranking uses the adjusted one: learning reorders
  readings, it can never eliminate one.
- Training matched the winning candidate by its final printed label, but slash
  notation, rootless-dominant renaming and dim/aug re-rooting all rewrite that
  label after scoring — so corrections silently did nothing on exactly the
  ambiguous voicings worth correcting.
- `overrides.json` is written with write-then-rename, so an interrupted write
  can no longer take your taught chord names with it.
- Packaging: the Linux tarball could be produced without a binary. A failed
  build fell through to `tar` and emitted an archive of fonts and licenses that
  looked releasable; every step is now checked and a failure leaves no archive
  behind.
- Packaging: the macOS zip and dmg were built from the app alone, so the
  bundled instructions never left the build machine.

## [2.0.0] - 2026-07-29

Ground-up rewrite in Rust. Same look, same behaviors, same settings file — no
Python runtime, no Qt, a single self-contained executable per platform.

### Added

- **Teach Chord Name…** — pin your own name to an exact voicing, optionally in
  all keys. Taught names are consulted before detection and stored in
  `~/.config/tangent/overrides.json`, separate from your settings.
- **Manage Taught Chords…** — review and delete taught names.
- **Reset Settings to Default** in the context menu.
- `custom_font_path` — an optional settings key pointing at any TTF/OTF on your
  machine, loaded ahead of the bundled fonts in every text style.
- Courier Prime (Regular and Bold) is embedded in the binary on all three
  platforms, so Ivory looks identical everywhere. `OFL.txt` and the font files
  ship in every release artifact.
- Real application icons: an `.icns` bundle on macOS and an embedded `.ico` on
  Windows.
- Single-instance handling via a lock file. A crashed instance no longer blocks
  relaunch.
- Any panic now surfaces as an "Ivory Error" dialog with the message and a
  backtrace instead of vanishing.

### Changed

- **Chord engine corrections.** The rewrite reproduces the Python engine's
  shipped behavior exactly, except where that behavior was a documented bug or
  an indefensible musical reading. Twenty-odd corrections, each audited against
  a 13,133-voicing corpus. Among them: a root-position `A-C-E-G` reads `Am7`
  rather than `C6`; `C-F-G-Bb` reads `C7sus4`; `C-G-A-D` reads `C6/9`;
  rootless dominants that contain the defining tritone resolve to the implied
  root, so `E-Bb-D` reads `C9`; and a reading may no longer hide a note you are
  actually sounding.
- Rendering is event-driven rather than a busy loop — repaints are scheduled
  from MIDI and timer events at the same cadences the Python app used.
- `detached_chord_height` is honored on load (the Python app overwrote it with
  50 on every start).
- Release builds are ad-hoc signed on macOS. Gatekeeper still blocks the first
  launch, and macOS 15 removed the right-click → Open bypass, so the route is
  System Settings → Privacy & Security → Open Anyway.

### Removed

- The Python runtime dependency. PySide6, mido and python-rtmidi are gone; the
  Linux build no longer needs a `pip install` to start.
- Courier New is never bundled or redistributed. Courier Prime replaces it.

> **One-way caveat when downgrading.** Ivory 2.x reads Python 1.1.0's
> `~/.config/tangent/settings.json` in place, so upgrading is seamless. Going back
> to Python 1.1.0 is not: it rewrites that file with its own fixed set of 13
> keys, discarding `custom_font_path` and any other key it does not recognize.
> Taught chord names live in a separate file, `~/.config/tangent/overrides.json`,
> which the Python app never touches.

## [1.1.0] - 2025-12-31

Final Python release.

### Changed

- Migrated from PyQt5 to PySide6.
- Courier Prime is bundled with the Linux package and installed to the system
  font path, replacing the reliance on a system Courier New.

### Added

- A richer `.deb` recipe: icons generated at 16–256 px, bundled fonts with a
  font-cache refresh, and a startup check for PySide6.

## [1.0.0] - 2025-12-13

First tagged release, ported from the original GTK3 prototype to Qt.

### Added

- Full 88-key MIDI keyboard visualization (A0–C8).
- Chord detection engine covering triads, sevenths, extended chords (9/11/13),
  altered dominants (b9, #9, #11, b13 and combinations), add chords, suspended
  chords (sus2, sus4, 9sus, 13sus), 6/9, inversions, slash chords and rootless
  voicings.
- Scale detection for clustered notes.
- Detachable chord display window, with borderless support.
- Dark mode with theme-aware context menus.
- Sustain-pedal visualization and customizable note colors.
- MIDI device selection, and a flats/sharps preference.
- Window size presets (50%, 75%, 100%, 125%, 150%, 175%, 200%) and borderless
  mode with drag support.
- Settings persistence at `~/.config/tangent/settings.json`.
- Cross-platform builds for Linux, Windows and macOS.

### Changed

- Chord labels moved to parenthetical notation: `Cadd9` → `C(add9)`,
  `Cmadd9` → `Cm(add9)`, `C9sus` → `C9(sus)`, `C7b9` → `C7(b9)`,
  `C7b9#11` → `C7(b9,#11)`.

### Fixed

- Minor 6th versus major 6th conflicts in closed voicings.
- 9sus versus add9 ambiguity, resolved using chord span.
- Minor add9 slash notation, e.g. `Cm(add9)/G`.
- Inversion bonuses for triads and 7th chords.
- Rootless voicing detection, and scale-versus-chord detection for clustered
  notes.
