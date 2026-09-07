# Changelog

All notable changes to Tangent are documented in this file. Entries before 2.3.0
refer to the app under its former name, Ivory.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions 1.x are the original Python/Qt app; 2.x is the Rust rewrite.

## [Unreleased]

### Added

- **The MIDI editor has a pedal lane.** The chip marked PED, at the pencil's
  shoulder, swaps the lane along the foot of the editor between velocity and
  the sustain pedal. The pedal is drawn as a step — a shelf where it is held
  and a riser where it moves, never a slope, because a controller holds its
  value and a line sloping between two edges would be a picture of an
  interpolation nothing performs. The shelf is filled down to the floor, so
  where the damper is down reads from across the room, and it takes its colour
  from the same gradient a velocity does: a half-pedal is a different colour
  from a full one.

  Drag in the lane to paint. The height of the hand is the value, which is what
  lets one gesture do both jobs — along the top it paints the damper down,
  along the floor it paints it up, because zero is a pedal nobody is pressing.
  There is no separate erase to go and find. A press that does not travel
  writes a single edge, "from here, this much". Strokes are bounded: what stood
  after the span goes on standing, so painting four beats of damper in the
  middle of a pedalled passage does not silently lift the pedal for the rest of
  the clip.

  One lane and not a new row: the editor is already a tool strip, a ruler, the
  roll and a lane, and every row added to that stack is height the roll loses.
  Sostenuto and soft ride above the damper as thin rails — a part that uses
  them should not look like a part that does not — and they survive an edit
  untouched.

- **A looping clip shows where every pass ends, audio and MIDI alike.** The
  clip stops being a rectangle: a triangular divot is taken out of its top and
  bottom edge at each turn of the loop, so a one-bar part dragged over four
  bars reads as four bars of one part instead of as one green box, and a
  four-bar drum take pulled over sixteen shows its four passes through the
  waveform. Nothing is drawn ON the clip — the material is taken AWAY — so it
  says the same thing on a near-black theme and on a bone one with no colour to
  choose. The bite is a tenth of the lane deep, which is the band the waveform
  and the note miniature are both kept clear of, and it is dropped when a pass
  is too narrow to place: a hundred notches a point apart is a moiré claiming
  to be structure, not a picture of a loop. An empty MIDI item that has not
  been played into yet gets it too, which is exactly when it is most useful.

- **Glue flattens a clip that repeats, on its own.** Gluing used to need two
  clips; one is enough now if it loops, and it welds that clip's own repeats
  into it. A one-bar part playing C D E F, stretched until it shows C D E F /
  C D, becomes a clip holding those six notes as one pass with no seam — what
  was on screen is now what is in the document. Nothing sounds different by a
  sample, and the next pull of its right edge repeats the whole six-note phrase
  and puts a divot after the second D.

- **The MIDI editor's loop bar scrolls the roll when you drag past its edge.**
  The bar sits at the end of the first pass, so on a clip longer than the
  window the handle and the place you want to put it were rarely on screen
  together and the only way to move a loop far was to zoom out, drag, and zoom
  back in. Sideways only: a loop length is a time and has no pitch, so the keys
  stay put under a hand that is setting one.

- **The MIDI editor has a loop bar.** The clip's right end carries a bar down
  the roll with a grip tab on the ruler above it: drag it left to shorten the
  loop and right to lengthen it. Shortening turns one pass into several without
  touching the clip's place on the timeline, which is how a four-bar part
  becomes four repeats of one bar; lengthening past the clip's own end grows
  the clip with it, because the schedule drops everything past the clip's end
  and a loop whose tail is silent is a control that lies. It follows the SNAP
  chip like everything else in the editor, so a loop lands on the grid the
  notes in it landed on, and it will not be pulled shorter than one grid line.
  A note that ends on the turn keeps its own edge — notes win on the roll, and
  the tab on the ruler is the half of the handle that nothing can cover.

- **Press and hold a DX7 patch name to hear it.** The instrument's picker
  sounds the patch under your finger at middle C, on the strip the window
  belongs to, for as long as the button is down. It works in both panes, on
  the loaded cartridge's rows and on the staged bank's slots, and on the
  built-in row at the top; the click it is layered on still means what it
  meant, so a press that ends in a click leaves the strip on that patch and
  writes it down, and a press that wanders off or turns into a drag puts the
  strip back on the row the picker is lighting. Auditioning was always what
  this window did, but only in the sense that it LOADED the patch: hearing one
  meant having a keyboard under your other hand, which is no use for browsing
  a thirty-two voice bank. The note goes to the picker's own strip rather than
  wherever the desk's MIDI is pointing, so it works on a strip that is neither
  armed nor selected, which is the state a picker is opened in as often as
  not, and it stays out of the take: a bank browsed over a rolling recording
  puts nothing in the part.

- **The MIDI editor has the band's scrubber.** The strip along the top of the
  piano roll now carries the whole clip in miniature, between the clip's name
  and the pencil: a recessed box the width of the clip and its pad, the notes
  drawn small inside it against their own pitch range so the shape of the
  part shows, the playhead in the roll's red, and the part of the clip the
  roll is showing as a lit window with a handle at each end. Drag the window
  to pan; drag either handle to zoom, the other end standing still; press on
  the clip outside the window to put the playhead there, snapped as a press
  on the ruler is, and keep the button down to scrub it along. The window
  stops at the clip's own edges, the same wall the edge-scroll stops at, and
  never zooms narrower than the wheel's quarter second; a window the wheel
  has pushed past the clip stretches the box to hold it rather than falling
  off. The whole thing takes no height from the roll — it lives in room the
  strip was not using — and steps aside on an editor too narrow to hold both
  it and the name. It is drawn whether or not the clip already fits the roll:
  at the fit the window fills the box, and its two handles are the way in.

- **The DX7's bank menu is a bank builder.** The instrument's window is two
  numbered panes now. On the left, the loaded cartridge: thirty-two rows, read
  off a `.syx` by "Load bank...", and never written to. On the right, the
  STAGED bank: thirty-two numbered slots, empty until something is put in one.
  Drag a row across and it lands in the slot it was dropped on; right-click
  rows to mark several and drag one of them, or press Stage, and they cross
  together; Remove empties a slot; click a staged row to hear it. Load another
  cartridge and the staged bank stays, so a bank can be mixed from any number
  of files ("stage their perfect bank", in the owner's words), and "Save
  bank..." writes it out as an ordinary cartridge that Dexed and a real DX7
  will open. Both panels open on `Documents/Tangent Media/SYSEX/`, and the
  save panel suggests `default.syx`, because that file is now the bank the
  instrument opens on: a strip that names no cartridge of its own plays
  `default.syx` from the shelf if there is one and the bank in the binary if
  not, so the bank you built comes back every session. "Factory" reads
  "Default" for the same reason, and re-reads the file. "Patch..." stays at
  the top: it edits what is playing, and its Save now puts the patch into the
  staged bank (back into its own slot, when it came from one) rather than
  appending to a file beside the settings that nothing ever showed. An unsaved
  edit is never lost on the way: choosing another patch puts a changed one back
  into its slot, or into the first empty slot with a line in the picker saying
  so, and Patch... opens on what was just picked. The staged bank itself is
  written down under `~/.config/tangent/` on every change and is back at the
  next launch, so a quit throws nothing away. Delete or Backspace empties the
  lit staged slots; Clear empties them all, after asking. Saving over a bank
  that is already on the shelf asks first, in the picker on macOS and in the
  panel elsewhere. The SYSEX folder is made at launch, and the patches the old
  editor saved to `my-patches.syx` beside the settings are copied onto it once,
  where Load bank can reach them; the original is left where it was.

- **The log says which macOS it is, and whether an input is open.** "macos
  aarch64" covers eleven releases, and one report cost a day to the difference
  between Sonoma and Tahoe; the header carries the product version now. And
  there was no `audio in:` line at all, on a build whose Recorder band cannot be
  hidden — so the input stream is always open and the log never once said so.

- **`TANGENT_ACTIVITY=1` asks macOS to treat the process as latency-critical.**
  Opt-in and off by default, so behaviour on every macOS version is unchanged
  unless somebody is deliberately testing it.

- **Stems export asks which stems: dry, or processed.** The export dialog's
  SOURCE row is joined by a STEMS row with two chips. *Processed* writes each
  lane through its own insert rack — the mix's own sound, one file per lane,
  and the default. *Dry* writes each lane before its own inserts, for a remix
  somewhere else. The rule is the same at every strip: a dry stem is that
  strip's own bays taken out and nothing else's, so a group's dry stem is the
  group before its bus compressor with its children exactly as they reach the
  mix. The two write to different folders — `stems/` and `stems-dry/` — so a
  second export cannot overwrite the first with different-sounding files under
  identical names. Choosing dry does not move one sample of the master.

- **Drag to the edge and the view follows.** Holding a drag against the edge
  of a view now scrolls it that way, on all three surfaces that needed it: the
  MIDI editor's roll in all four directions, the arrangement's lanes sideways
  along the song and downward through the track stack, and the mixer's desk
  across its channels. Things can be moved beyond what is currently on screen
  without resizing or zooming out first — a note to a pitch off the top, a
  clip to the ninth track, a channel past the eighth column. One rate law
  serves all three, measured in points per SECOND rather than per frame, so
  the same hold covers the same ground whether the window is painting at 240
  frames a second or ticking over at twenty; it ramps from a slow floor at the
  zone's inner lip to a ceiling at the rail and holds that ceiling however far
  past the edge the hand goes. The gesture asks for its own frames, so it
  keeps moving while the hand is perfectly still. View gestures — the alt-drag
  pan, the navigator — are exempt: a view that auto-scrolled while being panned
  would accelerate away from the hand steering it.

- **Nothing waits in silence any more.** Every operation that used to freeze
  the window with no explanation now says what it is doing. A survey traced
  seventy-six of them. The ones that can be counted show a real meter —
  loading a project counts its plugins, a pasted track counts its bays, an
  export counts its racks — and the ones that cannot name what they are
  waiting on instead, by the plugin's own name where there is one. The overlay
  obeys the rule the frozen frame forces on it: it is painted on the frame
  BEFORE the blocking call, because a call that blocks produces no frames to
  paint in. It waits a moment before appearing, so an operation that turns out
  to be quick never flashes anything, and it holds a minimum time so a fast one
  does not blink. It also gives up on an operation that never returns, rather
  than owning the screen forever.
- **Messages moved to the top-left corner and stopped interrupting.** A themed
  notification that lasts three to five seconds and can be clicked away.
  Messages that are *conditions* — a missing MIDI port, a device that
  disappeared — stand until the condition clears rather than expiring, and a
  dismissal sticks without silencing the next genuine occurrence.
- **Silverface**, the nineteenth theme: the faceplate of the Dual Showman
  Reverb, with the red jewel pilot lamp as the master.

### Changed

- **Only an ARMED track hears the keyboard.** Selecting a MIDI track used to
  open it to live notes as well, on the reasoning that trying an instrument out
  should not begin by arming a track. What that cost in the room was arm
  reading as a lie: an armed track sitting silent while an unarmed one sounded
  and metered, because selection is quiet and sticky — every click on a header
  sets it, and nothing on the strip says it also opens a MIDI gate. The lane
  answering the keyboard was not the lane wearing the R. Arm is now the whole
  of it, and the price is paid knowingly: auditioning an instrument means
  arming its track first.

  The MIDI editor is untouched and is still the road that needs no arm — while
  it is open, its clip's lane is the only lane that hears, armed or not.

- **Every clip is a loop from the moment it is made, audio as well as MIDI.**
  A drawn MIDI item and an imported `.mid` were already pinned at their own
  length and looping from birth; a recorded take, a split's tail and every
  audio clip in the app were not, so the same right-edge drag laid down another
  pass on one and stopped at the end of the file on the next. It now lays down
  another pass on all of them. A loop as long as the clip it sits on is one
  pass and sounds like nothing at all — no repeat, no seam, no change to any
  session — so what this actually buys is that pulling any clip's right edge
  repeats it, which is the reason anybody records four bars of a groove. The
  loop can be turned off per clip from the clip's own right-click menu.

  An imported audio file is the one clip that cannot be given its loop where it
  is made, because its length is the decoder's fact and not the app's: it gets
  both numbers together the first time the file is decoded. The consequence for
  a project saved before this, worth stating plainly: an audio clip still
  carrying the whole-file length gains a loop the first time it is opened here,
  and nothing about it sounds different, but its right edge now pulls out into
  repeats. A clip whose loop has been turned off keeps it off — that door only
  opens once.

- **Right-click a clip and untick Loop to stop it repeating.** The clip menu
  has a Loop row with a checkmark beside it when the clip repeats: press it and
  the loop goes, press it again and it comes back over the clip's whole length.
  Turning it off leaves the clip exactly where and as long as it was — a clip
  pulled out to four passes and then unlooped stays four bars on the timeline
  and plays its material once, because the length is the timeline's business
  and this row has no say in it. Like every other row in that menu it acts on
  the whole selection, so a marquee over eight takes and a press on one of them
  unloops all eight. Choosing how long a pass is remains the MIDI editor's loop
  bar; this row is only the on and the off.

- **Glue makes one clip that loops over the whole of what was glued.** It used
  to be the escape hatch from the loop — the way to turn several repeating
  items into one long pass — and it is now the same rule as everything else:
  the weld is a clip, and every clip is a loop over its own length. Nothing
  about the result sounds different, because the sources' repeats are written
  out into real notes as they always were and a loop as long as its clip is a
  single pass. What changes is the right edge: pull it and the whole welded
  phrase, silences and all, comes round again.

- **Draw a MIDI item with Cmd-drag on a lane, not shift-drag.** Shift is no
  longer bound to it at all. On this timeline shift already means "add to the
  selection" on a clip's body and "stretch" from a clip's edge, both of them a
  few points from the empty ground this gesture starts on, and a modifier that
  means three things depending on what is under it is a modifier nobody can
  learn. Cmd is the platform's command finger — Ctrl off macOS — and it is
  already what the editor's rubber band uses. The item is still born pinned at
  the length it was drawn and looping from that moment, notes or no notes, so
  the first pull of its right edge lays down a second pass.

- **Alt-click a record button to arm that track and nothing else; shift-click to
  build a set.** The one-lane gesture was on shift, which is the multi-select
  key everywhere else on this desk and therefore meant the opposite of what the
  rest of the app teaches. Alt takes it over, and it ignores the strip selection
  the way no other control does — spreading it would arm the very tracks the
  gesture exists to stand down. Shift now ADDS rather than toggles, so sweeping
  it across four lanes leaves four armed instead of turning back off whichever
  were already on. A plain click still toggles one lane, which is the only
  gesture here that can disarm without disarming everything. Both views, since
  the arrangement's header and the mixer's strip have always shared one road.

### Changed

- **The DX7's built-in patch is INIT VOICE, and the tine piano is no longer
  called E.PIANO 1.** The "(built in)" row of the picker used to play a tine
  electric piano under that name. The numbers were always the app's own, typed
  in so that it would have a sound it could give away; the name was a Yamaha
  factory voice's, and an original patch wearing it claimed to be that patch.
  Now the row is the instrument's own INIT VOICE, the patch every DX7 starts
  from: a sine at the fundamental, every other operator ready at level 0. A
  short cartridge is padded with it and an empty staged slot is written as it,
  which is what a real cartridge holds where nothing was stored. A fresh
  install sounds exactly as it did, because what it plays is the first patch of
  the bank in the binary, not the built-in row. The piano survives as the
  engine's test fixture, named TANGENT EP, with its first strike pinned to the
  hash it has always had.

- **Nine dialogs that asked nothing stopped stopping you.** The take report,
  the MIDI errors, the startup notices, the no-free-channel refusal and the
  "Tangent" notice window are notifications now. Everything that asks a real
  question — save before closing, flatten the tempo, the three destructive
  confirmations — is untouched, as are the file and folder pickers and the
  three native boxes that fire when there is no window left to draw into.
- **The render is the biggest thing on screen while it runs**, sized from the
  monitor rather than a fixed 520 by 210, and nothing that merely *announces*
  can cover it any more. A dialog that *asks* still comes forward, because a
  question hidden behind a progress window is an app that has stopped for no
  visible reason.
- **Mono's pressed keys are legible.** They were near-white on near-white keys,
  a highlight you could not see. Carbon retired to make room for Silverface;
  its palette indices stay frozen where saved desks expect them.
- **Tangent Imager's Ensemble works on a single microphone, which is the whole
  reason it exists.** It used to build its width out of the SIDE of the
  record, and a mono track has no side, so on the commonest thing in the app
  the knob did nothing whatsoever and the vectorscope drew the flat line to
  agree. It builds width out of the MID now, which every record has, and what
  it makes is ADDED to whatever image the record arrived with instead of
  fading that image out, so a stereo mix keeps the picture it was mixed with.
  The dial is calibrated in the correlation number printed under the
  goniometer: half the travel is half the way to zero, and the top is the
  widest a pair can be and still fold to mono untouched. And it holds your
  level while it does it, to within a twentieth of a decibel across the whole
  travel, so it is a knob you can A/B. Folding the output to mono still hands
  back the middle of your mix and nothing else, which is the promise the unit
  is built on and the one thing none of this was allowed to cost.
- **Tangent Tape's Drive reaches the tape now.** Measured at the level people
  actually record at, rather than the full scale nobody records at, the old
  knob ran from a thousandth of a percent of distortion to about one and a
  half percent, with three quarters of its travel doing nothing anybody could
  hear. Its top is about fourteen decibels hotter, so full travel is a machine
  being slammed: twenty-two percent, with the treble rounding that is the
  point of the unit arriving alongside it. Two things came with that. The
  record amplifier runs at four times the session rate, because a saturator
  driven this hard folds the harmonics it makes back down into the music as
  tuneless hash if it does not, and on a 7 kHz note at full drive that hash
  was a third of the output; it is under two percent of it now. And the
  playback side is lined up per record level, the way a real machine is lined
  up with a tone, so the knob still is not a volume control where the music
  sits: a quarter of a decibel at the shipped setting, under two at the very
  top, and what the top does cost you is the top of the band rather than the
  level, which is what a tape takes.
- **Tape's Bump, Roll and Trim print decibels and hertz**, which is what their
  automation lanes always said they were. The panel used to read all three as
  a percentage, so the lane and the knob quoted two different numbers for the
  same control.
- **A Tape or Imager setting saved by an earlier development build will sound
  different.** Both of those knobs kept their names and their stored numbers
  while what the numbers MEAN changed underneath them, and nothing converts
  them on the way in. A Tape Drive of 1.0 used to be a mild one and a half
  percent of distortion and is twenty-two percent today; the old sound is at
  0.7 now, so multiplying a stored Drive by 0.7 puts it back. An Imager
  Ensemble has no conversion at all, because that control changed from fading
  the record's own image out to adding to it, and no position of the new knob
  reproduces the old one on a stereo source. This is written down rather than
  migrated because neither unit has ever appeared in a released build: the
  whole suite is still in this section, so the only sessions that can be
  holding the old meaning are ones made while it was being built. The day it
  ships, this stops being a note and becomes something the loader has to do.

### Fixed

- **The sustain pedal is recorded into the clip and played back.** A part
  performed with the damper down came back dry: the take kept only the instants
  the KEYS came up, the clip had nowhere to say the pedal was still down, and
  playback dutifully cut every note at its key-up. All three pedals — sustain,
  sostenuto and soft — are now written into the clip as they were played, at
  their own values rather than as a switch, so half-pedalling survives; a pedal
  already down when the red button is pressed is restated at the take's start,
  so it is not released later by an event with no cause; and the clip's pedal
  reaches the instrument at playback, whether that is the built-in keyboard,
  the DX7, the analog, or a VST3 in a bay.

  The notes themselves are untouched. Stretching them to the pedal lift is the
  obvious cheap fix and it is the wrong one twice over: it throws away
  half-pedalling, and it destroys the difference between a chord rolled under
  the pedal and a chord held, which is a difference a pianist can hear.

  The pedal travels with the clip everywhere the notes do — split (the tail
  inherits the level it was cut under), glue, the loop (every pass opens with
  the pedal it inherits), and the `.mid` export. A `.mid` dragged in carries
  its pedal too, which it never did.

- **The loop bar stops at the clip instead of stretching it.** Dragging the
  loop's length past the end of the clip used to widen the clip to meet it. The
  clip's length belongs to the timeline, so the bar now stops there. To get a
  longer pass with silence in it, draw a second item and glue the two together.

- **The loop bar says it can be grabbed.** It answers a hover with the resize
  cursor, like the arrangement's own grip. The MIDI editor set no cursors
  anywhere, which is a description of the panel rather than a reason.

- **A strike's cost is asserted where the number means something.** The budget
  behind it is a release budget — eight notes at a tenth of a millisecond is a
  third of a 2.7 ms block — but `cargo test` builds unoptimised, so the figure
  being checked was a debug one. Measured: 10.3 µs in release against 104–110 in
  debug, a factor of ten, with nothing in the instrument changed. It passed for
  months only because debug happened to sit under a bar it was never measuring
  against, and went red when the machine got slower. The figure is still printed
  in both builds; the gate is release-only.

- **A dragged folder now lands on the row it was dropped on.** A single track
  dropped on row 7 ends up at row 7; a folder did not, and was always wrong by
  exactly the number of children it carried, because the landing spot counted
  only the rows that had not been lifted out. Drag a folder from row 4 past row
  7 and it arrived three rows above where it was let go. The block's head is
  what lands on the drop row now, its children follow it down, and the rows it
  passed close up behind it — which is what a single track has always done and
  what the arithmetic was written to match.

- **Phosphor's and Nocturne's keyboards were upside down.** Every other theme
  paints pale naturals and dark sharps in light mode, and inverts in dark mode.
  Those two did the opposite in each — light mode gave them a black keyboard and
  dark mode a pale one — because a theme's naturals are pinned to its light
  ground, and being dark-designed is exactly what those two are. Measured across
  the set, the sixteen others put their light-mode naturals between 0.61 and
  0.97 luminance; those two sat at 0.011 and 0.004. The polarity is decided by
  the piano now rather than inherited from the ground: the naturals are the pale
  key in light mode and the dark key in dark mode, on every theme. The two
  colours themselves are untouched — only which key wears which.

- **A held DX7 patch rings for the whole press.** The note stopped as soon as
  the pointer moved six points, which a hand holding a button still does, so it
  cut out part-way through a press that had not ended. The press is the gate
  now, as on a piano key. Dragging a patch into the staged bank sounds it on the
  way, which is the patch being chosen.

- **A recorded MIDI take now repeats when you pull its right edge, like a drawn
  one.** A MIDI item drawn on a lane has been a loop from birth — drag its edge
  and it lays down another pass — but a recorded take was born with no loop, so
  the same drag on the same kind of item revealed empty time instead. The two
  ways of getting a part onto a lane behaved differently at the one gesture
  everybody uses on a part they like. The take's own length is its loop now.
  Audio takes are untouched: there the file's remaining length is the limit, and
  repeating it would be a different feature.

- **A folder dragged past its own children turned into one of them.** A folder
  carries its rows, so dropping it below them names a target that is already
  inside the block being carried. The move itself is then a no-op — the block
  comes out and goes back where it was — but the membership decision that runs
  first is not: it asks which folder the target row belongs to, gets the dragged
  folder back, and writes that onto the folder itself. It stopped being a folder,
  became a child of itself, the group flattened around it, and the row that had
  been track 5 came back reading as track 6. That drop is refused whole now,
  since nothing about it could ever have been meaningful.

- **Phosphor's and Nocturne's pale piano keys stopped glaring.** The lift that
  makes those two themes' keyboards visible walked the pale key away from the
  naturals until the two were far enough apart — and toward pure white, which on
  exactly those themes it always reached, because their dark naturals sit at
  about 0.095 luminance and nothing can get far enough from that. So the walk
  spent every step, arrived at the screen's limit, and still fell short of the
  line it was aiming at. The pale key now stops at the bone this desk already
  paints with everywhere else: dimmer than any other theme's pale key, still
  more than five times the contrast of its own naturals, and no longer the
  brightest thing on screen. Themes that name their own pale key keep it.

- **The arrangement's little menus ignored the theme.** The clip menu, the FX
  menu, the input menu and the bay drag chip all painted a hardcoded cream with
  hardcoded near-black ink — the shipped theme's own colours, written out as
  literals — so on the other fifteen themes they were a pale card on a dark desk,
  and on the dark ones they glared. They derive their surface now, the way every
  other painted panel here does: the paper comes from the theme's face, moves
  itself when that face sits where neither ink would read, and the ink is a
  property of the paper rather than a colour borrowed from somewhere else.

- **Both arm modifiers can undo themselves.** Shift could add a lane to the
  armed set and not take one back out, and alt could make a lane the only armed
  one and not stand it down — so either gesture had to be abandoned mid-thought
  to be reversed. Shift now flips whatever it crosses and alt on the lane that is
  already alone disarms everything. A modified arm also touches one lane rather
  than the whole strip selection, which is what both modifiers are for; a plain
  press still carries the selection.

- **The master's FX bays had no right-click menu in the arrangement.** Every
  other rack on that view offers Delete FX and Replace FX; the master's six bays
  offered nothing, because the right-click matched the channel headers' bays
  alone and never the bus row's. A plugin could go into the master there and
  only come out by going to the mixer for it. Both of the master's racks answer
  now — its first three slots and its last three are seated far apart, so the
  menu resolves the seat the same way the left-click always has and cannot open
  pointed at a neighbour's plugin.

- **Analog could take the whole app down from the audio thread.** Letting go of
  a key in mono or unison falls back onto a key still held and, with
  multi-trigger on, re-attacks the whole voice stack. That road was the one
  place a voice was made to sound without being woken first, and waking is where
  a voice takes its filter lane — so any voice past the stack's old width, after
  the Voices dial had been turned mid-phrase, sounded with no lane at all. The
  render then asked for group 31 of three and panicked inside the CoreAudio
  callback, which ends the process and whatever was unsaved with it. Those
  voices are woken now, like every other voice that is about to sound. And
  underneath that, a voice or a fading corpse that reaches the render without a
  lane is left out of the block rather than indexed: nothing upstream should
  produce one, but the cost of being wrong about that was the app.

- **The Project dialog's Done button fell off the bottom edge.** It has done
  this twice. The stack was laid out straight down with Done as its last item
  and the window sized by hand to fit, so the first time a row was added the
  button went past the bottom and the fix was to grow the window — which is not
  a rule, and the next row put it back off the edge, leaving a dialog whose only
  way out was Esc. The button is reserved at the foot now and the body scrolls
  above it, the same shape the Export dialog already uses, so a row added later
  can push the body into a scroll and can no longer push the way out off the
  screen. The reservation is measured against the real button under the real
  theme by a test, rather than being another number somebody has to remember.

- **The metronome's sound row said the same thing twice, and a chosen click
  could go missing.** With nothing chosen the two controls read "built-in" and
  "default" — one naming the click you were on, one naming the click you would
  go back to, which are the same click, so neither said what pressing it would
  do. They are **Default** and **Custom** now, in that order, and Custom wears
  the chosen file's own name once there is one. And the file is now KEPT rather
  than pointed at: choosing a click copies it into Tangent's own folder and the
  setting names the copy, so renaming a folder or emptying a Downloads
  directory no longer drops the metronome silently back to the shipped click.
  One slot, replaced each time — this is a preference, not a library.

- **The DX7 picker opens lit on the patch the strip is playing.** It had been
  lighting the row for exactly one frame: the host's answer to the open, the
  same call that fills the list after a load, cleared the selection on the
  argument that belongs only to a freshly loaded bank. The row that is lit is
  now the row the strip's setting names, in both cases.

- **An audio system that opens and then plays nothing is set aside instead of
  being tried for ever.** A driver that refuses to open is easy — the open
  fails, Tangent falls back, and the band says why. The expensive case is a
  driver that says yes and does nothing: the stream is "running", the watchdog
  declares it dead two seconds later, the rebuild opens the same driver, it says
  yes again, and the app oscillates between silence and rebuilding for as long
  as anybody leaves it there. Nothing in that loop is an error, so nothing in it
  ever fell back. Windows has a stock example — Realtek ships a generic ASIO
  wrapper, so a machine with no interface still presents "an ASIO device", and a
  tester's log has Tangent announcing it was back on ASIO and playing silence.
  An engine torn down having served zero callbacks never worked at all; two of
  those in a row is a driver rather than a pulled cable, and the system is set
  aside for the session in favour of one that plays. Plugging the interface in
  gives it another turn, so the set-aside can never be permanent. No driver is
  named or matched by name: the test is whether audio came out.

- **The ASIO fallback was a one-shot, so the second time it was needed the app
  went silent for the session.** With `audio_system` set to ASIO and no
  interface plugged in, Tangent opens the platform default instead and says so.
  It marks what it is standing in for — the mark keeps the reprobe watching for
  the interface and carries the "back on ASIO" line when it returns — and it was
  also, wrongly, being asked whether a fallback was allowed at all. It cannot
  answer that: the mark is cleared in one place, and only once the chosen system
  is back, which the stand-in itself rules out. So it stayed set. Drop the
  engine for any reason — an output that went quiet, a band closed and reopened
  — and the rebuild restored ASIO, failed on it, found the mark still set,
  refused to fall back a second time, and spent its retries on hardware that was
  not there. Permanent silence, on a machine where choosing WASAPI by hand
  worked perfectly. The decision to fall back no longer consults the mark; the
  toast is still shown once per stand-in rather than once per rebuild.

- **A part recorded on an armed lane played back without the plugins it was
  recorded through.** There is one insert rack per channel and it is clocked
  once a block, so when the control room is monitoring an input it claims that
  channel's rack and the timeline's own pass stands aside. The claim was made
  for *any* armed channel, monitoring or not — and monitoring ships off, so the
  ordinary state of an armed channel was a rack claimed by a room that was not
  listening, whose entire output was then removed from the speakers again by
  the monitor gate. The lane stood aside for nothing and its audio went to the
  bus dry: record a part, leave the lane armed, press play, and the compressor
  or amp you recorded through is not on it. Nothing platform-specific — every
  desk did it. The rack now goes to whichever path can use it. If the room is
  listening it keeps it and takes the lane's own audio through it as well, so
  one run serves both. If the room is not listening and the lane holds audio,
  the timeline takes it. If the room is not listening and the lane holds
  nothing, the room keeps it exactly as before, so tracking through an amp sim
  while the interface does the monitoring still reaches the take wet.

- **The rate follower was building two AudioUnits a second, forever.** cpal's
  macOS `default_output_config` instantiates an AUHAL just to read a device's
  buffer range, and the poll that watches for a sample-rate change called it
  twice a second for the life of the process — 21 ms median, 32 ms at the 95th
  percentile, measured, on a thread that exists to notice one number changing.
  It reads that number directly now. `TANGENT_RATE_POLL=auhal` puts the old road
  back for an A/B, and a test pins the two to the same answer.

- **macOS could not report a dropout, so it always reported none.** Both
  counters behind the panel's dropout figure are structurally blind on a Mac:
  one matches an error string only ALSA and JACK produce, the other is compiled
  for Linux. The merge of them answered "nothing" on every Mac ever shipped, so
  the readout said zero to people who were listening to it crackle. CoreAudio's
  own overload notification is a third source now. Off macOS the arithmetic is
  byte-identical.

- **A note entered by double-click rang for ever on Analog.** Every preview in
  the MIDI editor is gated by a hand — a press sounds it, the release quiets it
  — except this one. A double-click writes its note on the second *release*, so
  the note-on happens with the gesture already over and the release handler
  that would have closed the gate already returned past. The note-off was never
  sent at all: the note stood open until the next click anywhere in the roll.
  That is inaudible on every instrument that decays, which is how it survived —
  a piano, a Rhodes, a kit and most DX7 patches all fall silent on their own.
  Analog does not: its envelope ships sustaining at 0.8 and holds for exactly as
  long as the gate is open, so what was a silent leak everywhere else was an
  endless note there. The written note is given a length now — its own, so what
  you hear is what you drew — and underneath that, anything the editor is
  sounding with no deadline and no hand on it is let go of the next frame,
  which covers every early return in that file rather than only the one that
  was found.

- **A take's video was short by exactly the number of frames it repeated.** The
  pump has two clocks and until now it used both. `Compositor::frame` is
  pipelined one deep — it submits tick *n* and hands back tick *n − 1* — so a
  composited frame arrived carrying a timestamp one tick behind the pump's own
  counter, while a repeated frame, which composites nothing, carried the
  counter itself. Interleave the two and time runs backwards over every repeat:
  the encoder refuses a frame whose timestamp is not past the last one, counts
  it in a field that reached no manifest and no toast, and answers *fine*. One
  repeat cost exactly one real frame, invisibly, with the mp4 the only place it
  showed: a 10-second window take asked for 306 frames, repeated 43, and its
  file held 263. The file's clock is the encoder's own count now, which cannot
  run backwards by construction, and a frame the encoder will not take is
  counted as a frame the encoder dropped — so this class of fault can never be
  silent again.

- **A window take of a quiet moment ended on a red warning about a video with
  nothing wrong with it.** A screen capture delivers only when the screen
  changes, so a still window sends nothing and the take repeats the frame it
  already has — the picture is exactly right and no machine was late. Those
  repeats were counted beside the ones a machine that cannot composite in time
  makes, and every take that held still asked the owner to "lower the video
  size in Export", which could not have helped. They are counted apart now
  (`frames_still` in `take.json`), and only a real shortfall speaks.

- **The cursor wore a ring in every window take.** `capturesMouseClicks` is a
  different thing from capturing the pointer: it paints macOS's own click
  decoration, a hard ring that swells around the cursor on every press, into
  the frame — several times a bar over somebody playing, and burnt in with no
  way to take it out afterwards. The pointer is still in the picture; the
  decoration is not.

- **A window take was encoded at a bit rate meant for a camera.** macOS was the
  only backend with no compression settings at all, so AVFoundation picked its
  own average rate: a measured 1600×1040 take came out at 1.4 Mbit/s. Video of
  a room survives that; a video of the app's own window is thin lines, small
  labels and a waveform, which is the first detail a starved encoder throws
  away. Takes are now given a rate scaled to their size and frame rate, High
  profile rather than the default, and a keyframe every two seconds so scrubbing
  through one is not a decode of the whole passage. Linux and Windows never had
  this: `ffmpeg` runs x264 against a quality target and spends what the picture
  needs.

- **Every double-click gesture worked once and then went dead under a hand that
  stayed put.** egui counts a run of clicks in one place 1, 2, 3, 3, 3…: from
  the third onward it reports a *triple*, and the double-click flag every
  gesture on this desk read answers false until the hand pauses for six tenths
  of a second or moves more than six points away. Tangent pairs the clicks
  itself now and spends each pair, so the next two make another.

- **A note would not go in near the playhead, on either side of it.** Two faults
  meeting. A note's edges are spread wide so a hand can catch them and pull —
  seven points inside the note and four outside — and that band answered for
  double-clicks too, which have no meaning on a resize handle: eleven points
  around every note border where a double-click did nothing at all. And the
  band lands on the playhead, because a click with the pen up locates, snapped,
  and the double-click that follows writes its note from the same snapped grid
  line: the new note's border and the playhead are the same pixel. A click
  reads past the resize zones now — on the note it is the note, past it is the
  empty roll it looks like. The arrangement's clips had the same fault at their
  resize ends and fade corners, where a double-click would not open a MIDI item.

- **The IR wizard's level meter was frozen, and it opened pinned at full
  scale.** Two faults on the same readout, and each one hid the other. The
  host published the wizard's live picture only while the IR road was "doing
  something", and it decided that partly by reading the very meter the picture
  carries — a return at −200 dBFS made the host conclude nothing was happening
  and stop pushing. The stage-2 tone runs with an empty capture buffer, so
  during the whole of the level page that return level was the *only* thing
  that could keep the picture flowing: a loop bringing nothing back froze the
  wizard on whatever it last held, for the rest of the session. The case that
  went dark is exactly the case the page exists for. A dead loop is the one
  fault still fixable with the cable in your hand at the interface, the page
  has a sentence ready for it — "Nothing is coming back — check the loop's
  cables and the unit's bypass" — and that sentence is chosen by a number the
  host had decided not to send. Underneath it, the resting picture itself was
  wrong: `IrLive` derived its default, so four dBFS fields whose own
  documentation says −200 for silence rested at 0.0, and on a dBFS scale 0.0 is
  not "nothing", it is FULL SCALE. Every wizard is built from that default and
  then handed it, so a freshly opened level page drew its bar hard right and
  warned about clipping over a loop nobody had measured yet. The picture now
  follows the PANEL rather than the signal — an open wizard is pushed to every
  frame whatever the numbers say — silence rests at silence, and a road with no
  engine behind it publishes silence rather than going on showing levels off a
  device that is no longer open. The level page also carries the road's own
  phase line now, the way the sweep and result pages always have, so a tone
  asked for with no audio engine running says that instead of metering as a
  dead loop and sending somebody to check cables that are not the problem.

- **A full-speed export rendered every plugin at its last-saved knobs, not the
  ones the desk is playing.** An export builds a second engine and furnishes it
  by hand, and it filled every rack from the sidecar files on disk — which are
  written when a project is saved, when a plugin's own window is closed, when a
  session is replaced and at quit, and by nothing else. No knob writes them,
  and the suite's own units have no window whose closing writes them either. So
  a compressor dialled in on a track and exported before the next save was
  rendered by a mirror still wearing whatever that bay wore at the last save —
  its shipped defaults, if the bay had never been saved at all. On the master
  that reads as "the export sounds wrong". On stems it reads as something
  worse, and is how it was reported: the settings a unit sits at before
  somebody dials it in are precisely the ones that do nothing — a flat EQ, zero
  drive, a compressor still at 1:1 — so a **Processed** stems export came out
  indistinguishable from the dry one, with every part of the dry/processed
  switch working correctly on audio that had been through a rack with nothing
  in it. An export now takes each bay's knobs off the desk, and falls back to
  the file only for a bay the running engine cannot answer for.

- **The busy overlay came up underneath the windows it was explaining.** It
  painted in the same egui band as the dialogs and one band below the export
  render window, so a "Preparing export" or "Writing files" cover could sit
  behind the very window whose work it was reporting. There is one rule now,
  stated once and used by both covers — this one and the launch splash: a cover
  paints above every band any surface of this app can ask for, because a cover
  is the last thing on screen before the app stops answering. And on the
  desktop, where a dialog is a window of its own that no paint order can reach
  over, an announced block now waits one frame longer: for a frame that no
  longer carries the window that asked for the work. A window is destroyed by
  not being drawn, and the pass that would not draw it was the pass that
  blocked — so pressing Export used to freeze the Export dialog on top of the
  progress cover for the whole length of the render.

- **A plugin on an audio track did nothing at all.** Not "processed silence" —
  it did not run. An audio clip passed through its lane's fader, pan, mute,
  solo and send, and then went straight to the master having entered no insert
  rack anywhere in the engine; the lane's rack was reached only by MIDI notes
  and a live input, and a lane holding nothing but audio never got that far.
  A compressor dropped into an audio track's bay metered zero and changed
  nothing, in the speakers and in the exported file alike. Measured on a
  fixture with a constant clip thirty decibels over a 20:1 compressor's
  threshold, on that compressor's own lane: the master peaked at 0.8999999
  with the plugin loaded and 0.8999999 without it, bit for bit. It now peaks
  at 0.0285 — thirty decibels of gain reduction that had been going nowhere.

  **This changes the mix of existing projects.** Any project with a plugin in
  an audio track's bay has been rendering as if that bay were empty, and will
  now render through it: the master, the stems and every bounce. An export
  made before this release is not reproducible after it. Nothing needs
  converting — the desk always showed the plugin, and the audio has finally
  caught up with what it said — but a mix balanced against a bay that was
  doing nothing will want another listen.

  The clips' separate delay compensation went with it. They used to hold
  themselves back by an arithmetic figure that reconstructed, per lane, what
  the lane would pay downstream; they now wait on the lane's own delay line by
  the lane's own formula, which is the one that already knew about groups and
  about an amp's seven pedal seats.

- **The window title was costing a sixth of every frame.** It asked whether the
  project was dirty once per frame, and asking meant serialising the entire
  document to JSON — 2.6 ms on a modest session. It is a structural fingerprint
  now, about thirty times cheaper, with the exact comparison kept everywhere a
  wrong answer could lose work.
- **The piano roll drew two circles per note in its velocity lane**, seventeen
  of them stacked in a column a fifth of a point wide. With the marks culled to
  what can actually be seen, a drum programme went from sixty-five percent of a
  frame to nineteen.
- **Opening a project made twenty-six thousand filesystem calls, twice**, walking
  every one of 4,389 seats when two of them held anything.
- **Consecutive strikes popped on every MIDI instrument**, worst under the
  pedal, because a re-struck key reused its voice and then reset it — clearing
  a ringing waveguide mid-cycle, reseeding modal banks, overwriting a DX7 slot.
  A hammer adds energy to a string rather than silencing it, and restrikes now
  superpose. Underneath it was a slot leak: a percussive DX7 patch held by the
  pedal decayed to silence and never gave its voice back.
- **Every Keys knob does what its label says.** Two were wired to nothing and
  six lied about what they did. The dispersion map ran backwards, which is what
  put a bell tone in the Clavinet and made the concert grand more metallic than
  the jazz one.
- **The trackpad stopped being a slot machine.** Zoom gestures stepped once per
  scroll event, and a trackpad flick is thirty events plus an inertial tail.

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

- **Tangent Keys grew a panel: two sounds, eight knobs each.** Clicking the
  bay still lists Piano 1 and EP Mk.1, and beneath them now sits the sound's
  own rack. The piano has Voicing, Width, Decay, Brilliance, Unison, Stretch,
  Release and Soundboard; the electric piano has Voicing, Width, Decay, Bell,
  Bark, Chime, Release and Tonebar. The lists differ because the instruments
  differ: one has a course of strings to detune and a soundboard to open, the
  other has a bell to weigh and a pickup to lean on, and showing one fixed
  rack with half of it greyed out would be a lie about both.

  Every knob acts on the physics rather than on the output, and every one is
  centred on the tuning the bench signed off, so an untouched strip and a
  freshly initialised sound make exactly the sound this release was voiced
  for. Double-click a knob to put it back; the Initialise button puts the
  whole sound back and leaves the other sound alone. Settings are per strip
  AND per sound, so a piano track and a Rhodes track keep their own, and
  switching a strip between the two finds each as it was left.

- **A kit is a file you can send someone.** Saving a kit from the drumpad
  writes a `.tgkit` that carries **the samples themselves** alongside the
  layout - every pad's gain, pan, tune, choke, note-off behaviour and trim -
  so a kit built on one machine loads on another with no shared folders and
  no missing samples. Loading one copies the audio into the open project the
  same way dragging a file in does, so the project stays self-contained.
  Kits saved before this still open: they name paths, and those paths are
  still tried.

- **Tangent Keys grew from two sounds to eight.** The keyboard now carries
  two grands, two tine pianos, two clavs and two reed pianos, every one a
  physical model with its own eight-knob rack, chosen per strip:

  *Piano 1* is the jazz grand as it was, with two things it always needed:
  a unison whose three strings no longer die together - the fast first
  breath and the slow singing tail a real course has - and the knock of
  the action itself, a fixed wooden thud under the note that scales with
  how hard the key is put down. *Piano 2* is the concert instrument:
  longer wire, a third more ring, sweeter treble for its pitch, a wider
  dynamic sweep and a quieter action.

  *EP Mk.1* keeps its bench tuning and gains the two smallest loudest
  things: the damper pad now audibly LANDS when a key is let go, and the
  pickup finally rolls off like a passive coil into a real lead instead
  of handing the bus raw arithmetic. *EP Suitcase* is the same tines
  through their own amplifier - the stereo throb, five cycles a second,
  the two speakers trading the note - with the tremolo on a knob that
  works mid-note, because an amp's knobs act on whatever is ringing.

  *Clav* is a new mechanism, and the app's namesake in miniature: a
  string struck and HELD by a tangent, read by a pickup whose position is
  a knob - slide it and the comb moves, which is where the funk lives.
  Letting a key up chokes the string on yarn and spits the little clack
  every clav record is made of; there is a MUTE slider for the staccato
  the originals were famous for. *Clav Wah* is the same strings through
  an envelope filter that chases your touch - the pedal this instrument
  spent the seventies plugged into, with one knob for how hard it listens.

  *Reed 200* is a reed piano: hollow and woody played gently, and when it
  is leaned on the pickup plate hands the note a growing second harmonic
  and the tone GROWLS - measured on the bench riding from nothing at a
  feather to full snarl flat out, exactly the curve the real amp draws.
  Its tremolo throbs the level in mono, as that amp did. *Reed 140* is
  the earlier tube instrument: darker, dirtier, never entirely clean even
  at rest, with the growl arriving like a dimmer rather than a switch.

  Every sound keeps its knobs per strip and per patch, so switching away
  and back finds the instrument as it was left; sessions from before the
  new sounds open with their two pianos exactly where they were.

- **The plugin suite: four effects of Tangent's own.** Load them from the
  picker onto any channel, folder, bus or the master, as many instances as
  there are bays, exactly like a VST3 - state in the project, knobs
  automatable, each with a panel drawn in the timecode cluster's own glass:

  *Tangent EQ* - a visual equalizer, natural phase by design: the bell and
  shelf curves are matched to their analog prototypes instead of cramping
  at the top of the hearing range, and the display draws the response with
  THE SAME math the audio runs, over a live spectrum analyzer. Three bands;
  tap a handle to walk its filter shape, drag it where you want it, turn
  the wheel to narrow it, double-click to add or remove.

  *Tangent Imager* - a full-circle vectorscope and a correlation meter over
  three controls: width, an ensemble made of velvet noise that widens
  WITHOUT touching the mid - fold to mono and your track is still there -
  and a mono floor that keeps the lows centred.

  *Tangent Tape* - the record-and-repro path of a quarter-inch machine at
  its one speed: the head bump, the top rolling off, the subs tucked away,
  saturation that rounds attacks spectrally the way tape actually does
  (not a compressor wearing a costume), wow and flutter with a real
  transport's drift, and shaped hiss. Six knobs, no bells.

  *Tangent Comp* - dials, a gain-reduction needle on the glass, and nothing
  to hide behind. Threshold, ratio, attack, release, makeup, mix, and a
  key high-pass ready for the side-chain routing to come.

  All four live in **Tangent Labs**, the house's own folder in the picker -
  first in the list, wearing the cluster's colour, and opened FOR you
  exactly once, the first time you ever reach for a plugin slot.

- **Nine more of Tangent's own, and the shelf fills out.** The suite grows
  from four to thirteen:

  *Tangent Denoise* - two engines behind one Kernel selector, sharing one
  learned profile: play it a moment of the noise alone, and each of
  thirty-two bands learns where the floor lives. **Spectral Studio** is the
  shipped engine and the one the REDUCE dial is a promise about - a
  weighted overlap-add with a real gain per band, so asking for 12 dB
  removes 12 dB in every octave, and so that a bin's phase comes back
  exactly as it went in. It costs 21 ms of honest, reported latency (1024
  samples at 44.1 or 48 kHz, doubling with the rate), which delay
  compensation takes care of. **Spectral Live** is the same engine at half
  the frame, for tracking. **Bank** is the original thirty-two
  minimum-phase expanders: zero latency, bit-transparent with nothing
  learned, and the only one that can be - but its overlapping bells sum, so
  its dial is an approximation rather than a number. A profile belongs to
  the kernel that learned it, because the two do not measure the same
  quantity through it: switch engines after a learn and learn it again.
  What it learns saves with the project and rides presets.

  *Tangent Amp* - a warm, clean tube amp solved from the blackface-family
  schematic itself: the real tone stack (its mid scoop lands where the
  circuit puts it), a bright switch that behaves like the actual 120 pF
  cap (strongest at low volume, gone at full - because that is what the
  wiring does), a vibrato that is honestly an opto tremolo - neon lamp,
  lopsided pulse, mono to full stereo throb - and a power section that
  stays under 1% distortion until you lean on it. And underfoot: SEVEN
  PEDALS - real insert slots that take any plugin, VST3s included,
  arriving pre-boarded with the compressor, wah, a new
  screamer-family overdrive, chorus, a new opto tremolo pedal, echo and
  plate. The drive and trem also stand alone as their own pedals, and the
  chorus, echo and plate step out of the fixed chain to become loadable
  instances anywhere - the day their sentinels were declared for.

  *Tangent IR* - convolution with zero fuss: load an impulse response, or
  make your own without leaving the app. The **IR Wizard** (in Audio
  Setup) walks the loop: pick the outs and ins your hardware hangs off,
  set levels against live meters with test tones, run the sweep, and see
  the capture drawn on the glass with its usable decay and a distortion
  estimate - Save drops it in the new Tangent Media/IRs folder, already
  listed for the next load. It is honest about physics: gated, chorused
  or pitch-shifting programs cannot round-trip into a static IR, and the
  wizard says so before you waste a take.

- **Sidechain, done properly.** Route any track into a compressor's key
  input: the bay's right-click menu grows a SIDECHAIN... row on Tangent
  Comp and on any VST3 that reports a key input, opening a picker of your
  channels (folders included - a whole group can duck a bed). The key is
  the source's finished mix of the previous engine block - uniformly, for
  every source, which buys the property that matters: REORDERING TRACKS
  NEVER CHANGES THE MIX. A pinned test proves a key from above and a key
  from below squeeze identically, another proves the exact one-block
  arrival, and mutual keys (A ducks B while B ducks A) are legal and
  bounded - it is how broadcast beds breathe. Keyed bays wear a small dot
  on their chip; edges live in the project's own terms, so they survive
  track reorders and die with deleted tracks.

- **Presets, for every one of them.** A Presets folder under Tangent Media
  with a shelf per unit; save under a name from inside any panel, load from
  the same row, and share a sound by sharing the file. Drum kits join in:
  the single-file kit with every sample inside it now saves as `.kit`
  (old `.tgkit` files open forever), and the kit dialog starts at its own
  shelf.

- **Every slider now knows its way home.** Double-click was already how a
  band knob, a mixer fader, a drum knob and a keyboard knob reset; the
  effect panels' parameter rows and the routing dialog's level were the
  two that could not. A double-click on an effect row now clears the
  stored value so the row lands back on the DSP's own default - the same
  rule the panel's RESET button uses, never a number the UI made up - and
  the route level snaps back to unity. The rule from here on: if it
  slides, a double-click puts it back.

### Fixed

- **Tangent Keys: every knob audited against the bench, and the ones that
  lied fixed.** All sixty-four knobs across the eight sounds were traced
  from panel to physics and measured; six were wired wrong and two did
  nothing, and the two worst timbre defects both fell out of one line:

  *The pianos' STRETCH turned backwards* - the dispersion filter was
  inverted against string stiffness, so knob zero was the stiffest wire on
  the panel and Piano 2's softer concert wire came out MORE metallic than
  Piano 1, not less. The map is redesigned in delay terms (and the shipped
  default is preserved to the cent at middle C); the knob now sweeps from
  near-harmonic to honky-tonk the way its label says, the bass finally
  gets the stretch big strings actually have, and Piano 2 has the sweeter
  treble it always claimed. *The same inversion was the clav's mystery
  bell*: its thin wire turned into a huge coefficient that parked a
  long-ringing upper series up to 122 cents sharp - an enharmonic chime on
  the one instrument whose brightness should be all combs and harmonics.
  Its partials now sit within a few cents of the stretched series at every
  velocity, and a test pins them there.

  *The tine piano's BARK was an inverse volume knob* - the tick read the
  stock pickup gap while the compensation read the knob, so turning it up
  made fortissimo QUIETER than mezzo-forte and added no dirt at all. It
  now leans the tine into the magnet for real: waveform asymmetry 0.15 to
  0.76 across the travel at full velocity, loudness flat within a decibel.
  *The TONEBAR did nothing on either tine patch* (it was wired to the
  pianos' soundboard, which the electric instruments hand a hard zero);
  it now weighs and rings the actual bar - the sustain and the slow beat.
  *The Suitcase TREMOLO retuned the chime nine semitones* through a
  leftover mapping while it throbbed; the two are now separate, and the
  suitcase's bell finally matches the Mk.1's. *The reed piano had traded
  characters with the Rhodes* - almost pure sine at normal touch, its one
  overtone parked in the tine's bell register: the growl now arrives with
  the player's weight (second harmonic -29 dB at mezzo to -12 flat out,
  measured), the hammer knock is audible, the bar overtone moved low and
  short, and the 140 keeps a standing even-harmonic haze so it is never
  entirely clean even feathered - while the Mk.1 got its glassy bell back
  at a whisper. The quiet knobs got real travel too: the pianos'
  SOUNDBOARD opens to an audible wash (the centre is unchanged), the clav
  TONE is a proper two-pole rocker reaching down to 600 Hz, and the tine
  and reed VOICING now move the attack by whole decibels instead of
  fractions. Two new bench tests pin the clav to its harmonic series and
  the reed and tine to their opposite signatures, so the mechanisms can
  never converge again.

- **A project's folders now belong to the project.** The folder layout -
  which channel is a group, which sit inside it, which are collapsed - was
  kept only in the shared settings file, never in the project document. So
  every project opened wearing the PREVIOUS session's folder shape: a mixer
  that indented empty slots the project never grouped, and in the worst
  case a stale collapsed group that hid the next project's real tracks.
  Undo had the matching half of the disease - it restored every other fact
  about the desk while the folder flags kept whatever a gesture had just
  set. The three folder facts now travel in the document's desk snapshot
  like everything else: saving writes them, opening applies them, New
  starts flat, and undo puts them back. A document from before this - which
  says nothing about folders - opens flat rather than inheriting whichever
  skeleton the last session left behind; refold once, save, and it sticks
  to the project from then on. (Deleting a track inside a folder still
  leaves an indented empty slot on purpose - that hole is where a re-added
  track rejoins the group, and clearing it would sever every member below.)

- **The window no longer opens bigger than the screen it opens on.** Every
  band's height in Tangent is a fixed fraction of the window's width, so the
  layout is a one-parameter family - choose a width and the height is decided
  for you - and until now nothing compared that decided height to the display.
  On a laptop the stack ran off the bottom: the transport and the recorder sat
  under the taskbar, and no drag brought them back, because the window was
  exactly as tall as the layout said it had to be. On a FIRST launch it was
  worse, since with no monitor remembered yet the width fell back to a flat
  1300 points, which is wider than many laptop screens before the height is
  even considered.

  Tangent now learns how much of the display the OS actually leaves a window -
  the taskbar on Windows, the Dock and menu bar on macOS, and this window's own
  title bar, all measured rather than assumed - and narrows the layout until
  the whole stack fits. A window that is already too big for the screen it
  finds itself on is corrected on the first frame, so carrying a laptop to a
  smaller display, or opening a session built on a desktop, no longer hides
  the bottom of the app. Because every band scales with the width, the same
  step is what makes the recorder, the piano and the strips the right size for
  a small screen rather than a big layout with its feet cut off.

  Detached panels get the same treatment: a chord, fretboard, theory or
  recorder window whose remembered size came from a bigger display is brought
  down to the screen it actually opens on, rather than having its top-left
  corner pinned on screen and the rest of it hanging off the bottom.

- **"Default" is now a sample rate you can rely on.** It used to mean "let each
  stream open at its own device's rate", and on a machine whose microphone and
  output disagree - most Windows laptops, a 44.1 kHz webcam mic against a
  48 kHz output - that is two clocks: the writer drains the instrument's ring
  at the input's rate while the engine fills it at the output's, and the take
  drifts. The app could only warn about it. Default now resolves to ONE rate
  before either half opens, taken from the output device, and the input is
  asked for it explicitly; a device that genuinely cannot reach it still falls
  back exactly as before. Nobody should have to discover that the setting
  named Default is the one that cannot be trusted.

- **The mixer's sixteen-over-sixteen fold answers to the space it has**, on
  every platform, instead of to a window flag. The fold was gated on the
  window being fullscreen or maximised, which is never reported on macOS
  (egui #3494) - so the fold was unreachable there however large the display -
  and IS reported for a laptop maximised against a taskbar, where it folded
  into two rows with no room for either. The height test beside that flag was
  set at less than half of what a strip needs, so it was the flag doing all
  the work. The flag is gone, the threshold has been measured against a real
  strip, and a test fails if a folded strip ever loses a control.

- **A drum track built before kits were per-lane opened silent.** A project
  whose drumpad sat on any channel but the first handed its one saved kit to
  channel 0 - often a piano track, where nothing strikes a pad - and the drum
  track came up with a hundred and twenty-eight empty pads under a full
  arrangement. Nothing had been lost: the samples were in the file the whole
  time, on the wrong lane. The kit now lands on the channel whose first bay
  actually holds the drumpad, which is the same question the pad-plugin
  migration has been asking of the same desk for months.

- **The Dock icon was too big on macOS, and rebuilding never fixed it.** The
  bundle's icon was correct and had been for weeks; the running app was
  replacing it. On macOS the icon handed to the window system becomes the
  Dock tile of the running app, and Tangent was handing over its artwork
  full-bleed while the bundle's copy is padded onto Apple's grid - so Tangent
  drew about a quarter larger than every neighbour, every launch, and no
  amount of clearing icon caches could help. The runtime icon is now padded
  from the same constant the bundle uses, and it is no longer squeezed
  through the 128-pixel ceiling that exists for X11, so it is sharp on a
  Retina Dock as well.

- **Choosing a keyboard sound did not visibly change anything** until the
  panel was closed and reopened. The row lit up only on the next open, and
  the knobs under it went on showing the sound that was no longer loaded.

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
