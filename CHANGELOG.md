# Changelog

All notable changes to Tangent are documented in this file. Entries before 2.3.0
refer to the app under its former name, Ivory.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions 1.x are the original Python/Qt app; 2.x is the Rust rewrite.

## [Unreleased]

### Added

- **Send to Device: the master out on a second output device too.** While
  Tangent plays through your interface, the same mix can also go to a
  virtual cable (VB-Audio's CABLE Input on Windows, BlackHole or Loopback on
  a Mac), so OBS, Discord or anything else that records an input can pick it
  up as a microphone. It sits beside Stream to OBS in the recorder band's
  cog, under SEND, and opens a window to pick the device and switch it on.

  On Windows the send always opens the cable through WASAPI, so it works
  while the interface itself plays through ASIO. The two devices keep their
  own clocks, and the send trims its rate by a fraction of a cent to stay in
  step with the cable; when the two run at different sample rates it
  converts between them, and the window says so, since matching the rates
  gives the cleanest copy. If the cable goes away or stops, the send turns
  itself off with a notice and the interface plays on untouched; Retry in
  the window starts it again. The cog's SEND chip shows what the send is
  doing: on, missing, stopped or failed.

- **Buses and returns, as many as a session needs, on every tier.** New Bus
  and New Return, on a track's right-click, add a row at the end of the
  desk that is not a track: it takes no clips and no input, hears no
  MIDI, and never counts toward the free desk's tracks. A bus has a fader,
  pan, mute, solo, six bays, automation, a colour and a name; it runs its
  plugins once over whatever arrives, and it can be routed on or be a
  sidechain source. Any track sends to any number of buses and returns
  from its Routing window (Send to a Bus or Return, or Send to a New
  Return), each send at its own level and after the fader unless you flip
  it to before, and a post-fader send is offered for automation by its bus's
  name. Soloing a track keeps the returns it feeds audible, and a soloed
  bus hears what feeds it.

- **Routes have no limit.** The routing table held 64 routes and refused
  the 65th; it holds as many as the desk makes now. The pickers grey out a
  destination that would feed a signal back into itself, and a loop saved
  by an earlier version still opens and sounds as it did.

- **The desk is as long as its tracks.** The mixer and the arrangement draw
  the tracks that exist and one place to add the next: a plus after the
  last strip in the mixer, and a row under the last lane in the arrangement.
  The plus's own background carries on past it, across to the click in the
  mixer and down to the bottom of the arrangement, and keeps its colour as
  tracks come and go, so a desk shorter than the window never ends in bare
  panel.
  Ctrl/Cmd+T, the header's plus and the mixer's plus add a track after the
  selection, or at the end with nothing selected, and dropping media below
  the last lane makes a track for it. Both views scroll, Shift+wheel zooms
  out as far as 64 strips or lanes at once, and the routing and sidechain
  pickers filter as you type and name each track. At the free desk's limit
  the plus is drawn as a padlock and opens the Supporter Key dialog.

### Changed

- **A supporter key now takes the track limit away, and the free app holds
  32 tracks.** Without a key the desk holds up to 32 tracks: MIDI and audio
  tracks and folders count. With one there is no limit but the machine's.
  Every key already sold does the same, since keys have no expiry and cover
  every version. Buses, returns, sends and routes have no limit for anyone
  and are never counted. The limit only ever stops a new track: a project
  with more than 32 tracks opens, plays, edits, exports and saves whole
  without a key, and adding a track to it asks for one. Ctrl/Cmd+T at the
  limit says so; it used to do nothing. The About card names the desk an
  install has, and the Supporter Key dialog, the README and the store page
  say what a key does; they said it unlocked nothing.

- **Remove Track takes the row away.** It used to clear the track and
  leave an empty slot where it stood; the row goes now and the tracks below
  close up. Its plugins' settings are put by first, so Undo brings the track
  back whole, and an amp's pedalboard leaves with its track.

- **Reordering tracks moves no plugin.** Every plugin, route, send,
  sidechain key and automation row belongs to its track now rather than to
  a position on the desk, so dragging a track to a new place is one change
  and nothing reloads. It used to hand each moved plugin back and forth
  with the audio engine, which on a desk full of plugins could take seconds.

- **Settings and projects are saved in a new format, and the old files are
  kept.** The first time this version saves over a settings file, a project
  or a template written by an earlier one, it keeps the original beside it,
  once: `settings.pre-v23.json`, `<project>.pre-v8`, and the drum kits'
  `drumpad.json.pre-v8`. Plugin settings are copied to new file names that
  follow their track, and the old files are never renamed or deleted.
  Earlier versions of Tangent cannot open the new files; putting the kept
  originals back gives an earlier version its project as it was.

- **While the arrangement records or a realtime export runs, a new track
  goes at the end.** Inserting, removing or reordering tracks, or changing
  a track's kind or input, waits until it is over, with a line saying why:
  the recording captures each track by its row, and the export writes each
  one by its row.

- **The installers go straight to installing Tangent.** The macOS installer
  no longer opens on a list of components with only Tangent in it, and the
  Windows installer no longer has a components page. Their pages, and the
  Linux `install.sh --help`, describe Tangent alone.

- **The metronome panel's BEATS and NOTE are typed fields.** Press the
  number between the minus and the plus and type any beat count from 1 to
  32, over a note value of 1, 2, 4, 8, 16 or 32, so 7/8, 11/16, 15/32 and
  3/1 are a few keys away. Enter or Tab sets it and Escape leaves it as it
  was; pressing anywhere else sets what was typed. A number a signature
  cannot have, like 33 beats or a note of 3, is turned down and the
  signature stays as it was. NOTE's four buttons became a minus, the field
  and a plus, like BEATS above it, and its steps reach all six note values.
  Each change is one step of undo, and the timeline's bars, the click and
  the count-in follow it, while the song plays too.

### Fixed

- **Routes follow their tracks.** Moving a track left every route to and
  from it on the row it came from, so after a reorder a track could be
  routed into somebody else's audio, and removing a track left routes aimed
  at its row. Routes, sends and sidechain keys name their tracks now, so a
  reorder, an insert or a removal carries them, and a removed track takes
  its routes with it.

- **A drum track's pad effects move with it.** Reordering a drum track left
  the effect plugins on its pads behind on the row it came from.

- **Old settings files keep the drum bus's and the master's polarity.** A
  file from the 16-channel builds that had the drum bus or the master
  phase-inverted opened with them upright, and the inversion moved onto
  tracks that never asked for it.

- **Files from the 24-channel builds open with their buses and master
  where they belong.** The builds of 31 August that had a 24-channel desk
  had no step to lift their files onto the wider one, so the buses' and
  the master's settings landed on tracks.

- **Old projects reopen the plugins on their buses, master and drum pads
  as they were saved.** A project from the 8- or 16-channel builds reopened
  those plugins at their default settings, because its plugin state files
  were named for a desk that had since grown; they are read by the desk
  they were written for now.

- **Save As writes the desk you see.** It wrote the desk as it stood at
  the last save, so a track added since then was missing from the copy
  while the copy's kit file had it.

- **Removing a drum track no longer deletes its kit before you save.** The
  kit file was rewritten at once, so Don't Save after a removal reopened
  the project without the kit. It is written with the project on the next
  save now, and Undo before then brings it back.

- **A crash part-way through a save no longer leaves plugins at their
  defaults.** The plugin settings a save owes a project are recorded before
  the project file is written and paid at the next launch, on every road
  that saves: Save, Save As, New, switching projects and Quit.

- **The launch clean-up no longer deletes a folder that holds anything
  Tangent did not put there.** It removed any folder in the projects folder
  whose name began "Untitled " whole, so a saved project filed in a folder
  of your own called "Untitled Folder", or saved inside an Untitled
  session's folder, was deleted at the next launch with its recordings.
  Only folders named exactly "Untitled" or "Untitled" and a number are
  cleared now, and only when everything in them is Tangent's own; Don't
  Save and New follow the same rule.

- **New keeps the recordings in the Untitled session it replaces.**
  Answering Don't Save to New, or pressing New on a session with nothing
  unsaved (a take undone off the timeline is still on disk), deleted that
  Untitled folder, recordings and all, while Open and Quit kept it. New now
  starts the next session beside it, and asks first when no save prompt
  was shown.

- **Opening a template from File > Open no longer puts the last project
  at risk.** The template was shown standing in the previous project's
  folder, so the next Save moved that project's folder to the new name and
  wrote the template over its arrangement, and the two sessions' plugin
  settings were mixed. A template opened that way now becomes a fresh
  Untitled session, as New makes one, and a first save never moves a
  folder that already holds a saved project.

- **Reset Everything no longer empties the open project's file.** It wrote
  the reset desk (every instrument, plugin, track name, fader and route
  gone) over the document of the project that was open. It now discards
  the open session's unsaved changes, as the save prompt's Discard does,
  and leaves an earlier Untitled folder with recordings in it alone.

- **A recording running when you quit, start a new project or open
  another is kept.** Quitting threw an arrangement recording away
  unwritten, without a question on a session with nothing else unsaved,
  and New or Open let it run on into the next project, where it landed at
  Stop. The recording now ends in its own session first: quitting asks
  about it, Open keeps it in the project it was recorded in, and New asks
  before replacing the session.

- **Resetting the default template asks before replacing an unsaved
  session.** Settings > Reset > The Default Template replaced the open
  session with a fresh one without the save prompt, losing unsaved edits
  and plugin settings. It asks first now, as New does.

- **Don't Save keeps a drum kit built in an unsaved session.** Answering
  Don't Save deleted an Untitled folder the session had created even when
  a drum kit had been built in it. Such a folder is now kept, as one
  holding a recording is.

- **A new session plays its template's own audio.** A template's audio
  clips and frozen tracks were silent in every session started from it,
  drawn with no waveform and missing from an export, until the session was
  saved under a name. A template saved from such a session lost those files,
  and a session made from it left them behind once it was saved or written
  to disk. They play from the start now, and they come along whenever the
  session or the template is written. Save Kit and Duplicate in a new
  session take the template's samples and audio with them too, and undoing
  the deletion of a template clip brings its audio back.

- **Undoing a track move puts its plugins back with their settings.**
  Undoing a drag, Insert Track, Paste Track or a grouping into a folder
  brought the plugins on every track that moved back at their factory
  settings, or with a neighbour's where two tracks held the same plugin.
  Redoing a swap of two tracks did the same, and so did changing the
  buffer size after a move. Undo and redo now carry the plugins exactly as
  the move did.

- **Changing the buffer size keeps every plugin's settings.** Changing the
  buffer size, the sample rate or the output, an output that dropped out
  and came back, or closing the band reloaded every plugin at its settings
  from the last save, or at its factory settings if it had never been
  saved. What each plugin is playing is now written down first.

- **Moving a track no longer changes a saved project until you save.**
  Dragging, inserting or grouping tracks rewrote the project's saved plugin
  settings at once, so leaving with Don't Save, or a crash, reopened the
  moved tracks with each other's settings or at their factory settings.

- **A moved plugin opens its own window.** After moving a track, opening
  one of its plugins could open the plugin that used to sit in that slot
  instead, now on another track, under the new plugin's name.

- **New and Reset Template start clean.** A plugin replaced or deleted in
  the previous session could come up in the new one with that session's
  settings, where the same plugin sat in the same slot.

- **Undoing a drum pad swap brings each pad's effects back with their
  settings.** Carrying a pad onto another swaps the two pads' effect bays
  too, and an undo or redo of the swap brought each pad's effects back at
  their factory settings, or with the other pad's where both held the same
  plugin.

- **Holding Delete no longer removes a track after deleting automation
  points.** A held Delete or Backspace deleted the chosen points, row or
  clips, and the key's repeat then removed the selected track too, without
  asking. A separate press with nothing chosen still removes the selected
  track.

- **Removing a track takes its automation and settings with it.** A track
  added where one was removed inherited its automation curves and armed
  rows, its pan, polarity, icon and drum kit. Copy and Paste Track now
  carry the track's automation too.

- **A track's polarity flip moves with it.** Insert Track Below, Paste
  Track Below, a drag or a grouping into a folder carried every other
  setting of the tracks that moved and left the polarity switch on the row
  it was pressed on, so a flipped track played the right way up and the
  track that took its row played upside down.

- **Automation rows armed or bypassed in one project no longer carry into
  the next.** Opening or creating a project kept the previous project's
  armed and bypassed rows on the same track numbers, so an armed row could
  write into the new project unseen. And undoing a track move now takes a
  bypassed row back with its curve, where it used to leave the bypass on
  the row the track had moved to and the curve playing.

- **Splitting an audio clip keeps the quantize, gain and stretch on both
  halves.** The right half of a split came back unquantized, at unity gain,
  unstretched and reading the wrong part of the file, and stopped following
  the tempo. It is now the same take as the left half, cut.

- **A second quantize, or a new stretch algorithm, is heard at once.**
  Applying a quantize again at another swing, window or grid could go on
  playing and gluing the previous one while an export used the new one, and
  changing a quantized clip's stretch algorithm kept the old one.

- **A take quantized again after its project was reopened keeps its new
  fit.** A take saved with the Slice or Stretch fit, reopened and quantized
  again with Auto, played Auto until it was saved, and then reopened with
  the old fit.

- **A quantized bass, guitar or vocal stays in tune.** Small corrections
  were resampled, bending each corrected note's pitch by up to a third of a
  semitone; the stretch algorithm is now chosen once for the whole take and
  never resamples.

- **A flam at the end of a take no longer stretches everything after it.**
  The audio after a quantized take's last hit took that last correction's
  speed, up to four times slower; it now plays as recorded.

- **Zooming no longer shows a quantized take unquantized.** With any clip
  in the session still missing its waveform, a zoom or scroll redrew a
  quantized take from the original audio, its hits off the grid, until the
  next edit.

- **The quantize window's grid sizes are named in the song's meter.** In
  3/4 or 6/8 each chip quantized one note value finer than it said (the
  default 1/8 gave sixteenths). The chips are now spelled as the
  arrangement spells its grid, and follow a meter change while the window
  is open.

- **The quantize window no longer stalls on a folder of takes or reads
  stale attacks.** With more than eight takes asked for, every move of the
  detect slider analysed them all again; after a change of device rate, or
  a new scratch take under an old name, Q could use the previous file's
  attacks.

- **Changing the tempo no longer garbles looped MIDI clips.** A tempo
  change, or stretching clips from their edge, retimed a part's notes but
  not its loop, so every pass restarted in the wrong place, and the sustain
  pedal was left behind. The loop and the pedal now move with the notes,
  and a stretched audio clip's loop with its audio.

- **In 3/4, 6/8 and 7/8, the arrangement's snap lands on the grid lines it
  draws,** and the arrow keys no longer skip lines. 4/4 is unchanged.

- **The MIDI editor's grid follows the song's tempo and meter changes.**
  Its lines, its snap, step entry and the arrow keys were spaced by the
  tempo dial and the time signature, so after a tempo or meter change they
  no longer matched the arrangement's bars or the click. They follow the
  song's tempo and meter now, and so does a quantize, in the MIDI editor
  and on audio.

- **With triplets or swing, every bar line is a grid line.** Triplets and
  swung pairs were counted from the start of the song, so in a bar holding
  an odd number of them (quarter-note triplets in 3/4, swung eighths in
  7/8) every other downbeat fell between two lines and a note played on it
  was quantized off the beat. Each run now starts again at the bar line,
  in the quantize and in the MIDI editor.

- **Step entry in the MIDI editor works at every tempo with the magnet
  on.** At most tempos, repeated Enter stacked notes on one grid line and
  the right arrow could get stuck on the line it stood on. Enter and both
  arrows now walk one grid line per press.

- **In a loop trimmed at its start, Enter, Backspace and paste in the MIDI
  editor act where the playhead line is drawn in every pass;** at the top
  of a later pass a note landed further into the pass and the song could
  jump forward. An Enter that writes nothing no longer moves the playhead.

- **A tempo drag on the metronome panel is one undo step,** at the
  release; it made one per BPM it passed. A tempo typed or dragged on an
  empty song is now its own undo step too.

- **A click on the metronome panel over the mixer presses once.** Every
  row of it was pressed twice, so the beats and the count-in moved two at
  a time and the count-in's "carries on" switch flipped and flipped back.
  The panel over the arrangement was not affected.

- **A second Cmd/Ctrl+M closes the metronome panel.** In the arrangement
  the chord opened the panel and pressing it again did nothing, so the
  panel could only be closed with the mouse. A beat count or note value
  being typed into the panel is set as it closes.

- **The varispeed range keeps what you type into it.** Clicking outside
  the range panel, on its X, on RESET or on the other end threw away a
  number being typed into MIN or MAX. It is set first now, as the
  signature's fields set theirs, and clicking back into the field keeps
  typing; Escape still leaves the range as it was. Right-clicking another
  knob while typing no longer leaves an invisible field holding the
  keyboard. Each change to the range, typed or by RESET, is one step of
  undo.

- **An Analog track's dry stem lines up with the others.** In a dry stems
  export it came out 68 samples (1.4 ms at 48 kHz) behind every other stem
  and the mix, by the delay the Analog reports for its own sound. It now
  leaves on the same frame as its processed stem.

- **A chosen output that is missing plays through the system's own.** With
  an output chosen in Setup that was unplugged or would not open, every
  system but ASIO stayed silent and reopened the audio path every few
  seconds. Tangent now plays through the system's own output and says so
  once. Your choice is kept, and on macOS and Windows it is tried again the
  moment it is plugged back in; on Linux and FreeBSD, when the band is
  reopened.

- **Plugging the interface back in during an ASIO fallback brings ASIO
  back.** The stand-in was still running when the reopen came, so the input
  failed and ASIO only returned after the band was closed and reopened. A
  plugin dialled in while the stand-in played comes back on the interface
  as it was left. A plug during a take or an arrangement record no longer
  stops its monitoring and its instrument: the switch waits until the
  record is over.

- **Choosing an audio system in Setup is safer.** Pressing the system that
  is already chosen, as you might during an ASIO fallback to get back to
  it, no longer forgets the output chosen for it. Pressing a different
  system clears the OUTPUT row, where a press could save a device the new
  system cannot open; its outputs are listed the next time Setup opens.

- **Each project's exports land in a folder of their own.** With the
  Export dialog's folder left empty, every project exported into one
  Exports/project folder, so a mix exported from one project replaced
  another project's file of the same name. Exports now go to a folder
  named for the project, and the dialog's DIRECTORY row shows it before
  you export. Earlier exports stay in Exports/project.

- **Save As Template no longer saves the project you are in.** Saving a
  template from a saved project first saved that project, so an unsaved
  experiment was written into it and could no longer be discarded. The
  template is written from the session as it stands, with the project's
  media copied in as before, and the project is left as it was last saved.

- **A take whose audio output fails halfway through says so, and when.**
  The rest of the file was silence, or the file simply stopped, and the
  take was filed as ordinary. A realtime export whose output fails now
  ends with an error instead of hanging on "Exporting..." until Stop, and
  a failed export no longer leaves broken stems in the stems folder: it
  removes what it wrote and leaves the folder as it was.

- **Closing the band or losing the audio output during a count-in no longer
  leaves the tempo and undo locked.** Undo, redo and every tempo control
  stayed refused, with nothing saying why, until the band was reopened.
  The count now ends with the engine it was counting on.

- **Tracks can no longer be inserted, pasted below, grouped or dragged out
  of a folder during an export or an arrangement recording.** These moved
  track rows under a running export or recording, so stems and recorded
  clips landed on the wrong tracks. They are refused now with a note
  saying why, as dragging tracks already was, and during a realtime
  export Cmd/Ctrl+F no longer makes or dissolves a folder, which took its
  bus off its tracks halfway through the file.

- **The count-in holds together.** A take started during a count-in the
  red button had joined began at once, mid-count, with no head in its
  take.json; it begins on the downbeat. Stop and the red button silenced
  a count-in that a take was recording into its file; they no longer do.
  Pause or Space takes back a Play pressed during a count-in, and the Play
  button lights while it waits for the downbeat. A tempo typed into the
  Export dialog no longer changes the tempo during a count-in.

- **take.json says where a take really began.** For a take whose song
  starts on its count-in's downbeat, `timeline_rolling` came out true or
  false by chance. A realtime export's take.json carried the previous
  take's count-in head and the playhead from before the export moved to
  its start, and a take with its count-in recorded into it, started with
  no audio engine, claimed a head for a count that never sounded. And for
  a take with its count-in recorded into it, `timeline_rolling` now says
  whether the song started on the count's downbeat, with
  `timeline_start_seconds` where it started, `head_seconds` into the file.

- **Pressing the cassette just as a realtime export finishes no longer
  turns the new take into the export.** That take was filed as the
  export's file and the real export was left in Takes.

- **Flipping a track's polarity during a take is heard and recorded at
  once.** The switch lit during a take or its count-in but changed nothing
  until the take ended, when the flip landed in the middle of playback.

- **An armed input stays out of the speakers when monitoring is off,
  wherever it goes.** Routed to a bus, it was removed as if it went
  straight to the master, so with the bus turned down it played at minus
  the difference, and with the bus muted at full level, inverted; on a
  track inside a folder it was heard through the folder's send to the
  effects bus. An armed input on a folder track itself was turned down
  twice by the folder's fader, and flipping the folder's polarity flipped
  it back; it plays at the folder's level now, and stays out of the
  speakers too. A plugin on the bus it is routed to is still not fully
  removed (the speakers hear what that plugin changes), as with a plugin
  on a folder.

- **A bus's fader and mute affect what is routed into it while the song is
  stopped.** With the transport parked, or in a session with no audio
  clips, muting a bus or pulling its fader changed nothing routed into it.

- **The Analog's arpeggiator keeps its step at the end of a loop that has a
  tempo change in it.** Just before each wrap it was given a beat measured
  back from the start of the loop, so it fired a stray step or dropped
  one, and an arp on a track being monitored live ran a buffer early. It
  is told the song started or stopped in the buffer where it did, not a
  buffer before or after.

- **Starting and stopping no longer, now and then, play a moment from the
  wrong place.** Stopping could play a moment from the start point and
  leave the playhead a little past it, and playing from a new place could
  first play a moment from where the playhead had been.

- **A new Denoise bay on the Bank kernel cuts as soon as it finishes
  learning.** It faded its reduction in over about a quarter of a second,
  while the spectral kernels applied theirs at once.

- **A drag always ends when you let go.** A clip dragged onto the open MIDI
  editor and let go there never finished, and the next press on a track's
  switch or a toolbar button could jump the clip to the pointer; a mixer
  fader went on following the pointer after a dialog that appeared
  mid-drag had closed. Every drag now ends at its release, as one undo
  step, even when a dialog hid the release; and a drag that Tab, Shift+1,
  2 or 3, or Esc cuts short is its own step, where a drum pad knob or a
  pad's trim handle let go by Esc had none.

- **Undoing a reset no longer takes the window out of full screen.**
  Undoing any reset asked the window for a size, which left full screen
  or unmaximised it. Only undoing a reset that resizes the window (Reset
  Everything, or Reset Layout and Windows) puts its size back now.

- **Pressing the MIDI editor's loop bar without moving it no longer snaps a
  recorded loop to the grid.**

- **The quantize window keeps what you were trying.** Pressing Q with the
  window open put every dial back to the last applied settings, so the
  next Apply used the old ones; it now only re-aims the window. Q on a
  track with no audio quantized a take still selected on another track;
  that take is let go now. The window's top line follows a selection made
  in the MIDI editor, where it went on saying "all 47 notes" while Apply
  quantized the ones picked.

- **Applying a quantize no longer covers the screen.** Every Apply put up a
  "Quantizing" cover for at least 0.4 seconds and ignored clicks
  meanwhile. With the window open its detections are already made, so
  Apply acts at once; a take it still has to analyse keeps the cover.

- **The quantize window no longer shows a move it will not make**: with one
  attack left in a clip, the ticks showed it moving while Apply kept the
  old quantize. And it lets the app rest when a selected take's file is
  missing, instead of redrawing every tick on every frame.

- **Normalize measures what a clip plays.** A looped, stretched or quantized
  clip was measured over parts of its file it never plays, so a take could
  come up by almost nothing, or past the -1 dBFS normalize aims for.

- **One key press with a pedal window open over an amp acts once.** With
  child windows off, T skipped a theme and Space sent the transport two
  commands.

- **The export and glue no longer stretch a clip past the length playback
  allows.** A clip whose stretch would make it longer than 20 minutes
  plays unstretched; an export, glue or normalize made soon after opening
  could still render it, a huge allocation. They play it unstretched too
  now.

- **A stretched take keeps its length on screen while its quantize
  renders.** After opening, a quantize Apply or an undo, a stretched and
  quantized take was briefly drawn at its unstretched length.

- **Glue says no where it cannot glue.** MIDI glue welded clips on a frozen
  track, which plays its render and keeps its clips for the thaw; it now
  refuses, as audio glue does. Cmd/Ctrl+G on audio clips in a build with
  no audio engine did nothing; it says why.

- **File names made from clip and export names are safe on Windows.** A
  trailing dot could survive, a name of dots became a hidden file, and a
  name like "con" made a file Windows treats as a device. And a glued file
  can no longer take the name of a missing clip's file in another case,
  which made that clip play the glued audio.

- **A plugin's automation stays with that plugin.** Replacing a plugin left
  its automation driving whichever of the new plugin's knobs shared its
  parameter numbers. Replacing now removes it (Undo brings both back),
  moving or swapping plugins on the mixer takes each one's automation
  along, and automation kept after removing a plugin waits for that same
  plugin to return.

- **A quick tap on an automation row's empty room moves the playhead
  there.** A trackpad tap, whose press and release arrive together, did
  nothing.

- **Choosing automation nodes lets go of the selected clips and row, and
  chooses only nodes you can see.** Nodes chosen with the box, by
  Shift-clicking in a new node or by pasting left a clip selection
  standing beside them, and a clip on a frozen track then stopped Delete
  from removing the nodes. The box, dragged out over the track headers,
  also chose every node back to the start of the song, out of view.

- **An automation row's dial changes the level it shows.** When a curve
  opened on a step, turning or double-clicking the dial changed the level
  after the step instead.

- **The first node of an automation lane stays at the start of the song
  when dragged**, where a slightly sideways drag with the magnet off moved
  it off the start and it could then be deleted. Only the node you hold is
  drawn larger, and only while you hold it. Cmd/Ctrl+A on an empty track
  slot no longer opens the automation menu, where a row ticked drew a
  curve the next track made there inherited.

- **Reset Layout and Windows resizes the window to its opening size at
  once**, as Reset Everything does, and undoing it puts the size back. Its
  confirmation no longer promises the window's position, which it never
  moved.

- **A count-in set in the metronome panel steps on at the COUNT-IN chip.**
  A 3-bar count-in (or 5 to 8 bars) set there went to off at one press of
  the chip; it now steps to the next length up, and goes round to off only
  past 4 bars.

- **The settings panels are modal to the keyboard and the right button
  too.** With one open, a right-click opened an effect panel or the band
  menu on top of it, and in reader mode the band's controls still answered
  Tab and Enter underneath. A RECORDING panel left open when a realtime
  export started stays drawn now, instead of turning invisible and eating
  the next click. Under a take, the TAKE panel greys the rows it will not
  change.

- **The settings cog is named for what it opens**: a screen reader
  announced it as "Take settings", and it is "Settings - audio and media"
  now. The SIG field under the cog and in the red button's panel shows a
  caret while you type into it.

- **When ASIO cannot play, Tangent says why.** The fallback asked whether
  the interface was plugged in even when its driver had refused the buffer
  size or rate; it now names the driver's reason and points to Setup,
  where changing the buffer or rate tries ASIO again.

- **Setup's OUTPUT row lights the output that is actually playing.** A
  saved name that matched a renamed device loosely, or a blank one, played
  that device or the system's own while no chip was lit.

- **A short name given to an interface stays with that interface, chosen
  or not.** With no input chosen, unplugging the interface moved its short
  name onto the microphone the session fell back to. Short names typed
  from now on remember their interface; one typed before keeps the old
  behaviour until it is typed again.

- **Double-clicking a knob in the Keys panel puts it back to the sound's
  own value**, as its tooltip always said; it jumped the knob to the
  pointer instead.

- **A full-speed export and a freeze play the tracks the sampler plays.** A
  MIDI track played by the built-in sampler came out of a full-speed
  export, its stems and a freeze as silence; the export now loads each
  sampler's file as the desk does.

- **A failed export leaves its folder as it found it.** When a file of a
  full-speed export could not be finished (on a full disk, say), the export
  said it had failed but left its partial files where the last export's
  had been. It now writes aside and puts the files in place only once every
  one is finished, and an export whose file could not even be created no
  longer leaves "Exporting..." on screen. The dialog's realtime export
  delivers only the stems it wrote, and each export says once where its
  files went.

- **MIDI export and glue write what a MIDI clip plays.** Notes hidden past
  the end of a trimmed clip that does not loop were written into the
  exported file, and gluing that clip made them sound; a note ringing past
  a clip's end was written at its full length, though playback stops it at
  the clip's end; and a clip's sustain pedal stayed down after the clip,
  for the rest of the file when nothing lifted it. All three now match
  playback, and so does the pedal where two clips on one track overlap:
  it stands at the higher of the two clips' levels and comes up only where
  neither holds it, where a half pedal in one used to lower a full pedal
  the other was holding.

- **A recording still counting in when you start a new project, open
  another or quit no longer begins in the next one.** It began on its
  downbeat in the new session, on its armed tracks, with the tempo and
  undo locked until then; leaving a session now calls the count off, as
  pressing the red button again does. Resetting the default template ends
  a running recording first, in the session it was made in, which then
  asks about saving it, as New does.

- **Undoing a track's removal or a plugin's replacement brings its
  automation rows back as they were.** The curves came back with their
  rows closed, their arms gone and a bypassed curve playing; the rows now
  reopen as before, and Redo closes them again, including a row opened
  after the undo, which used to stay open (and armed, if it was) for the
  next track or plugin put there to write into. A row kept for a plugin
  you took out is named for that plugin, for example "EQ (unloaded):
  parameter 2", rather than "bay 1 parameter 2".

- **After a tempo or meter change, the MIDI editor's nudge, duplicate and
  resize move a note by the grid step where the note is**, not where the
  playhead is, and a note drawn with the magnet off is that step long, so
  notes stay on the lines. The arrange view's arrow keys and the LCD's bar
  and beat drags walk the song's own bars and beats there too.

- **Stretching a selection by a clip's edge no longer shrinks an untouched
  audio clip in it to a sliver.** A clip of the selection not touched since
  it was dropped came out a twentieth of a second long.

- **Setup's OUTPUT row always offers "the system's own".** When Tangent
  could not list the outputs, the row said "none reported" with nothing to
  press.

- **Normalising says why where it cannot.** In a build without the audio
  engine, Cmd/Ctrl+Shift+N on an audio clip did nothing and said nothing.

- **The quantize message counts the clip's own attacks.** "Quantized N of M
  attacks" counted every attack in the take's whole file, so a split or
  trimmed clip was told of hits it does not play.

- **On macOS, waiting for a lost audio output to come back costs next to
  nothing.** While the output was down, Tangent checked for it every two
  seconds by building a CoreAudio output unit, about 20 ms of work each
  time; it reads the device's properties instead.

- **The arrangement's shortcut card names the right button's clip
  gestures**: a right-click on a clip opens its menu, and a right-drag
  across a lane's empty room chooses the clips it crosses.

- **Tracks can no longer be removed during a realtime export or an
  arrangement recording.** Removing a track mid-export took its clips and
  plugins out of the file halfway through, and a folder's bus off its
  tracks, with undo unavailable until the export ended; during a recording
  it could delete the track being recorded, or make the recording capture
  another track's input. Remove Track, Delete, the strip's close button
  and Cut now refuse with a note saying why. Thawing a frozen track,
  adding or removing a route, changing a sidechain's key and Reset > The
  Desk changed a realtime export's file partway through in the same way,
  and The Desk could empty the tracks an arrangement recording captures;
  they are refused too.

- **New, Open, the template resets and Reset > Devices and Folders wait
  for a take's count-in, or an export, to finish.** Starting a new project
  or opening another while a take was counting in applied that project's
  tempo under the count, so the click sped up or slowed down under the
  take; during a realtime export it replaced the project being exported.
  Resetting Devices and Folders puts the tempo, the time signature and the
  count-in back along with the devices, so it retimed the click, or the
  export, the same way. They are refused now with a note saying why, and
  so is the Undo in the box that reports a reset, which puts the tempo and
  the signature back too; the box comes back, so its Undo can be pressed
  once the count or the export is over. Quitting is not affected.

- **A frozen track sounds right in a full-speed export.** A full-speed
  export, its stems and a freeze of a track a frozen one feeds rendered the
  frozen track from its own clips with its effects switched off: dry audio,
  silence through a plugin instrument, and none of the frozen sound. They
  now play its frozen file, as playback and the realtime export always
  did; if that file is missing, the track is silent, as it is on the desk,
  and the export says so.

- **The sampler keeps its sample in the project.** It remembered a sample
  by where the file lay, so a take dragged onto it before a session's first
  save went missing once the session was saved, a template's clip dropped
  on it kept playing the template's own file, and the sample went missing
  when its folder or the project moved. A session started from a template
  whose sampler plays one of the template's own files lost that file when
  it was first saved, duplicated or saved as a template. The sampler now
  copies its file into the project's media folder, as the drum pads do,
  and the file travels with the project; projects saved before play the
  files they name, as they always did.

- **Loading a sampler preset loads its sample, and a Tangent IR preset its
  impulse.** A sampler preset brought back its settings and left whatever
  sample was loaded; it now loads the sample it was saved with, copied
  into the project, so a preset saved in one project plays the same in any
  other. A preset that names a file which is not audio loads its settings
  and says so, and copies nothing. A preset for the convolution unit
  brought back its knobs and left the impulse that was loaded; it now
  loads the impulse it was saved with, and says so when that file is no
  longer where the preset names it.

- **A settings file from 4.81 or earlier with a band popped out opens at
  the right size.** With no window size saved, Tangent opened the window as
  a strip the height of the piano and then drew every band into it; it now
  opens at the size its bands come to. The popout windows behind it, which
  nothing could open since 4.82.0, are gone, and their settings are carried
  along unread.

- **The README.txt in every download describes the app as it is.** It sent
  you to menu rows that are keys now (Keytoggle is K, dark mode D, sharps
  or flats P) or that sit in categories (Chords, Window, Colors, Reset),
  offered window sizes from 50% to 200% where there are three, gave the
  Windows settings folder with mixed slashes, said a keyboard unplugged
  mid-session is never picked up again (MIDI tracks hear every device and
  pick a replugged one up on their own), and said Tangent makes no network
  connections at all, where Stream to OBS, once you turn it on, sends to
  the machine you name (this one, unless you change it). The readmes and
  the font licence notes in every download name Terminess, the typeface
  Tangent opens in, as the default.

- **A second export or freeze pressed while one is running is refused and
  says why.** A realtime export pressed during another sent the running
  export's files to its own folder, under its own name; pressing Export
  during an export or a freeze took the running one's "Exporting..."
  message off the screen and said nothing, and a freeze pressed during
  either did the same; a realtime export pressed during a full-speed export
  or a freeze ran alongside it. Each is refused now with a note, and the
  running one keeps its message and its progress. An export or a freeze
  pressed during a recording or a count-in, the red button during a
  realtime export, and the cassette while an export writes its files also
  say why they did nothing.

- **About tells the truth about the build it is in.** It names the desk the
  install has, a 32-track desk or unlimited tracks with a supporter key,
  where it gave a channel count of 16; it names the plugin
  formats the build hosts, VST3 on macOS and Windows and LV2 and VST3 on
  Linux and FreeBSD, where it said LV2/VST3 everywhere; and a build without
  the recorder no longer claims to record or host plugins.

- **The warning for an armed input that recorded nothing says where to
  look**: the recorder band's cog, then AUDIO SETUP. It sent you to an
  Audio Status window that does not exist, after a run of stray spaces.

### Added

- **Thirteen new themes, thirty-two in all.** Each is named for an object
  and dressed in its colours, and each fills a gap the nineteen left:
  Urushi (a black lacquer box with cinnabar and gold), Paperback (a shelf
  of mid-century paperbacks), Malachite (banded emerald and rose quartz),
  Sodium (a night bus at 3 a.m.), Kiln (a terracotta page and a soot
  band), Lapis (ultramarine and gold leaf), Chartres (a rose window in
  black lead), Overprint (a pink and blue riso zine, the fourth light
  desk), Woodblock (Prussian blue on washi with one vermilion seal),
  Toffee (a cast-resin radio with a jade dial), Confetti (a Memphis
  sideboard), Lamplight (a green glass desk lamp) and Ditto (a
  spirit-duplicator handout). Five of them keep their ground dark, as
  Amber and Phosphor do. The existing themes and their slots are
  unchanged, so a saved desk opens exactly as it was, and every new theme
  carries its own palette of 32 track colours.

- **The Themes menu fits a short window.** A hover menu taller than the
  screen now shrinks its rows to fit, as the main menu already did,
  instead of running off the bottom.

- **Render ahead: the CPU headroom of a big buffer at the latency of a
  small one.** A new row in the audio setup panel, beside the buffer size,
  set to Auto by default. A settings file written by a development build
  from before Auto was the default keeps Off until the row is changed.

  A track's effects are normally run once per audio callback, and most of
  what a plugin costs is paid per call rather than per sample: ninety-six
  reverbs cost about 765 microseconds before a single frame is rendered.
  With this on, a track's effects run in longer stretches instead, with the
  stretches staggered across tracks so each callback refills a share of the
  desk rather than all of it.

  **The audio is not delayed.** A stretch is rendered earlier in wall
  clock for the same positions in the song, so the sound comes out when it
  always did. What does arrive later is the desk's answer to a CHANGE: an
  automation edit or a clip edit lands on a rendered-ahead track up to the
  chosen number of milliseconds late, and up to about twice that, since a
  track may already have its next stretch rendered while it plays this
  one. A knob turned in a plugin's own window lands with the track's next
  stretch. A track whose plugins carry automation is not rendered ahead,
  so that automation plays on time. Faders, sends, mute and solo, and
  their automation, are applied at the moment the audio is played and are
  unaffected.

  Measured here on thirty-two tracks carrying three FabFilter Pro-R 2
  reverbs each - ninety-six of them - with other work running on the
  machine, so the figures are if anything gloomy:

  | buffer | off | at 5 ms | at 10.7 ms |
  |---|---|---|---|
  | 32 frames | every block late | 2 late in 4200 | 28 late |
  | 64 frames | 2607 late in 4200 | none late | none late |
  | 128 frames | none late | none late | none late |

  Twelve seconds of that desk takes 4.9 seconds to render at a 64-frame
  buffer with this on, against 12.4 with it off.

  This is about PLAYING. Bouncing a mix is a separate path, unaffected by
  any of it, and it got its own speed-up below.

  **Auto is the setting to use.** It works the length out from the buffer
  size every callback - eight buffers' worth, up to 10.7 ms - and measured
  against every fixed value at every buffer size it lands on the best one
  and is never worse. A fixed value is still there if you want one.

  **What it does not affect: anything you export.** Bouncing a mix and
  freezing a track build their own renderer and never render ahead, so a
  file you make is the same file whether this is on or off.

  **On a sampled instrument you may hear a different take.** Starting
  playback with this on perturbs the first moments enough that an
  instrument which varies itself between notes - most sampled pianos and
  keyboards do - picks a different set of samples from then on. Measured
  on a five-track Rhodes session: a decibel and a half either way over
  fifteen seconds, in both directions depending on where playback started.
  It is a different performance of the same part, not a wrong one, and it
  is the same thing those instruments do between any two takes.

  **A 32-frame buffer becomes possible at all**, which it was not before:
  without this, every single block on that desk was late. At 32 frames the
  5 ms setting is the better one and 10.7 ms is not, which is the one place
  the largest value is not the best: the longer stretches cannot be spread
  finely enough across so short a buffer.

  Armed tracks, tracks listening to live MIDI, folders, send destinations,
  sidechained tracks and anything being auditioned always run on the
  callback's own block, so tracking and monitoring are untouched.
  Instrument tracks are rendered ahead like the rest.

- **Bouncing and freezing now use every core.** They rendered on one,
  because the code believed an export was "already running flat out on
  every core it can reach" and it was not. Depending on how much work your
  tracks carry it is 1.5 to 3.5 times faster - most on tracks with real
  instruments and reverbs, least on a desk of simple EQs.

  The file is the same file. The tracks are summed in the same order
  whether or not the work was spread, and two of the owner's own songs
  rendered both ways differ less than the same song rendered twice the old
  way.

  It also works where spreading live playback across cores can fail
  silently. Playing live needs a real-time scheduling grant from the
  operating system; a bounce asks for no such permission and is never
  affected. On Windows and macOS an app can ask for itself, so there is
  nothing to arrange. On Linux and FreeBSD the grant is given at install
  time, and both installers now ask for it - see below.

  A bounce uses every core, not just the fast ones. Playing live can only
  use the fast cores - a slow one holds up the whole block and makes
  things worse - but nothing is waiting on a bounce block by block, so a
  slow core doing some of the work beats it doing none. On this machine
  that is the difference between a bounce taking 3.8 seconds and 1.9.

- **A session opens on the view you left it on.** Save a song with the
  arrangement showing and it comes back on the arrangement; save it on the
  mixer and it comes back on the mixer. It travels in the project file
  beside the theme, so it follows the song rather than the machine.

- **The Linux and FreeBSD installers ask for real-time audio scheduling.**
  Spreading a song's tracks across cores while it plays needs permission
  from the operating system, and without it Tangent quietly does all the
  work on one thread - it does not warn, it just sounds like a slower
  computer. macOS and Windows let an app ask for itself. Linux and FreeBSD
  do not: it is granted once, at install time, by something running as root.

  On Linux the installer adds `@audio - rtprio 95` in
  `/etc/security/limits.d/` and puts you in the `audio` group. On FreeBSD it
  loads `mac_priority` and puts you in the `realtime` group. Both print what
  they did, both are undone by `--uninstall`, and `--no-rt` skips it if you
  would rather arrange it yourself. Neither asks for locked memory, which
  Tangent does not use.

  Log out and back in for a new group to take effect.

- **Quantize, for audio takes as well as MIDI.** Q opens the quantize
  window, in the arrangement and in the MIDI editor alike. With the editor
  open it moves notes: the selected ones, or every note in the clip when
  none is selected, each keeping its length and staying inside its clip.
  Otherwise it moves the attacks in the selected audio clips, and any MIDI
  clips in that selection are left alone. In the MIDI editor Q used to snap
  the selected notes straight onto the editor's grid; it opens the window
  there too now.

  It is a window you leave open, not a question to answer: select a clip,
  press Apply, listen, select another. Apply does not close it. Its top
  line says what Apply would act on ("2 audio clips", "all 47 notes in this
  clip") and follows the selection as it changes, and Space still plays and
  stops the song while it is up.

  It has a grid of its own rather than borrowing the view's (a grid size,
  triplet, and a swing amount where 50% is straight), so trying a sixteenth
  quantize does not change what the arrangement draws. STRENGTH is how far
  each note or hit travels: 100% puts it on the line, and less keeps some
  of what was played. WINDOW leaves alone anything further than that from
  a line, which is what stops a grace note, a flam or the second half of a
  roll being collapsed onto the beat it decorates; at 0 it is off and
  everything moves. The window opens on the settings you last applied, and
  they are saved with the session.

  **Audio is quantized inside the clip, not by cutting it up.** The take's
  attacks are found and moved onto the grid, and each attack plays back
  exactly as it was recorded while only the sustain after it is stretched.
  On a test recording of clicks, stretching through the attacks put every
  hit 6.5 ms late; keeping them puts each within half a millisecond of the
  line. The *detect* slider sets how readily something counts as an
  attack. The fit decides how a moved piece of audio gets where it
  belongs: *stretch* fills the distance, and *slice* moves it untouched, so
  a hit moved later leaves a gap in front of it and one moved earlier cuts
  short the sound before it, as a split would. The *auto* fit, the default,
  listens to the material: percussive takes are sliced and sustained ones
  stretched, decided once for the whole clip. The quantize is an edit on
  the clip, so it undoes, it saves, and the waveform moves with it. It is
  rendered in the background the way a stretch is, and the clip is silent
  until the render lands.

  **While the window is open, the waveform shows its answer.** Every attack
  it finds in the clips it would act on gets an amber tick, with a fainter
  one where the grid will pull it, and both follow the dials as they turn,
  so a detection setting is something you can see before you commit to it.
  A take that is already quantized wears its ticks on its hits, and a
  looped clip is marked in every pass.

  **Q aims at a track.** Click a track's header and press Q, and that
  track's audio clips are selected for it (a folder stands for the tracks
  inside it), unless the clips already selected are all on tracks you
  picked.

  **Not yet right everywhere.** Quantizing a multi-mic kit moves each mic
  on its own: each take is quantized from its own attacks, so the bleed
  between a drum folder's mics stops lining up and can flam.

- **Audio clips glue.** Cmd/Ctrl+G welds audio clips the way it has always
  welded MIDI ones: two or more on one track, or one that repeats, become
  one clip. They are rendered exactly as they play (trim, loops, fades,
  gain, a quantize and a stretch included) into a new 32-bit float WAV in
  the project's media folder, and replaced by one plain clip that loops
  over its own length and plays the same samples they did. Like a MIDI
  glue it takes one track and one kind of clip at a time, and it is
  refused on a frozen track. With no audio engine running, as when the
  recorder band is closed, it asks you to open the band first.

- **Automation nodes make steps, and move as a set.** Two nodes may now
  share an instant, which is a step: the curve arrives at the first and
  leaves from the second. A node could not sit on a neighbour's time
  before, and a drag that passed over one destroyed it. The very start of
  the song is the one place a step cannot be heard, so a node put down on
  the first node there gives it its value instead.

  Drag across an automation row's empty room and a box chooses every node
  inside it; a press that does not move is still the click that places the
  playhead. Dragging a chosen node carries all of them by the distance the
  hand travels, and a click alone moves nothing. A group dragged against
  the start of the song or the top or bottom of the row stops there as a
  shape instead of piling onto one spot. Shift-click on a node still adds
  it to the choice or takes it back out. Delete or Backspace removes the
  chosen nodes, all but a node at the very start of the song, which holds
  the row's level; it used to remove the selected track. Copy, cut and
  paste work on them: a paste lands at the playhead, snapped when the
  magnet is on, on the row you chose, pressed or copied from, replaces the
  nodes in the span it covers, and arrives chosen. Nodes copied from
  several rows paste back onto their own rows, and a paste onto a mute row
  arrives as switches. The clip menu's Delete and Copy act on the
  clip even while nodes are chosen, and undo, or opening or closing a row,
  lets the choice go rather than leave it on the wrong nodes.

- **Cmd/Ctrl+A opens the selected track's automation menu**, the same as
  the wrench on its header. In the MIDI editor it still selects every note.

- **A built-in effect's window follows its automation.** The knobs,
  readings and EQ curve of an open Tangent effect show the automation's
  value at the playhead, and move with it as the song plays. It is display
  only: saving, or storing a preset, keeps the setting you made by hand. A
  knob with automation on it follows its curve rather than the mouse, so a
  drag never writes the curve's passing value into your setting (moving an
  EQ band whose frequency is automated changes only its gain); a value
  typed into its reading still sets it, and Init or the EQ's reset puts it
  back to its default.

- **Mute and solo take the record arm's alt and shift.** A plain click
  toggles and carries the selected strips with it. Alt-click makes that
  strip the only one muted or soloed, and alt-click on the strip that is
  already alone clears them all. Shift-click toggles that one strip and
  leaves the selection out of it. Exclusive solo is the one most worth
  having: with several tracks soloed, hearing just one used to mean
  clicking each of the others off.

- **Audio Setup chooses the output.** A new OUTPUT row beside SYSTEM lists
  the outputs the system offers, with "the system's own" first. It matters
  most under ASIO, which has no default device: Tangent opened whichever
  ASIO driver came first in Windows' list, so a machine with FL Studio
  ASIO or ASIO4ALL installed beside a real interface opened the wrapper,
  and nothing in the app could say otherwise. The output belongs to the
  computer, not the song, so projects do not carry it; changing the audio
  system forgets it, since a device name from one system means nothing to
  another, and so does resetting Devices and Folders. Under ASIO the row
  reads Windows' list of installed drivers without loading any, so it is
  complete even while one is playing; a driver whose hardware is not
  plugged in is listed too, and choosing it plays through the system's own
  audio and says so.

  Under ASIO, "the system's own" is the first driver in Windows' list
  that answers, which can be a wrapper such as ASIO4ALL, and Setup says so
  under the row; choose the interface by name.

- **A count-in being recorded into the take shows its beat in red** on the
  band's display.

### Fixed

- **A track could lose its waveform and never get it back.** The clip stayed
  drawn in the arrangement with nothing in it, and changing the buffer size
  made it worse rather than better.

  When a file will not decode, that answer is remembered so the app does not
  try it again on every frame. Changing the audio device empties every cache
  and re-reads every file in the session at once - and it was leaving those
  refusals in place, so a file that lost that race once was skipped for the
  rest of the session. Each later buffer change cleared the cache and left
  the refusal standing, which is why it looked like the change caused it.

  The refusals are forgotten with the caches now. The largest file in a
  session is the likeliest to lose that race, which is why it tended to be
  the same track every time.


- **A frozen track's effects now look and behave frozen.** They are already
  in the rendered file, so swapping, bypassing or deleting one changed
  nothing anybody could hear until the track thawed - and the press landed
  anyway. It is refused now, with a line saying how to thaw, exactly as a
  frozen track's clips have always refused an edit. Copying an effect off a
  frozen track still works, because that changes nothing on it.

  The frost over the bays was being painted UNDER the plugin's name and the
  loaded tab, in both the mixer and the arrangement's track header, so a
  frozen track's effects went on reading as live. It goes on last now, over
  the bays and the input button, which are the two things on a frozen track
  that do nothing until it thaws.


- **A track's effects could be run over the same audio twice.** Only with
  render-ahead on. A reverb or a compressor handed the same stretch of
  sound a second time carries the wrong tail out of it. Effects with
  nothing to remember, and tracks with no effects at all, were unaffected,
  which is why the first round of tests missed all of this.

  Five ways in, all of them found by measurement and all of them fixed:

  - at a setting whose length was not a whole number of audio buffers,
    which the 5 ms setting is at most buffer sizes (up to 14 dB wrong);
  - when a track stopped being rendered ahead while the music played,
    which is what arming it, punching in, or opening a plugin's window on
    it does (up to 16 dB wrong);
  - when the render-ahead setting itself was changed while the music
    played, including turning it off;
  - when the sound card handed over a buffer longer than the render-ahead
    setting;
  - and at a loop's end, where a lap could come out silent or, on the
    second lap onward, plainly wrong.

  Looping is also why the setting now costs a little more than it did: the
  track's rendered-ahead sound is dropped at every loop end and rebuilt on
  the other side, because sound rendered past the loop's end belongs to a
  bar that is not played next.

- **The Analog instrument sounded different at different buffer sizes.**
  Not a little: on a track of notes it rendered a 128-frame buffer and a
  512-frame one to sounds differing by more than the signal itself. It only
  showed when notes start and stop inside a buffer, which is why held
  chords and every effect were unaffected, and why it went unnoticed.

  The synth cuts its work into short chunks and was measuring them from the
  start of whatever buffer the device handed over. With no notes that is
  harmless, because the boundaries land in the same places either way. A
  note arriving mid-buffer cuts a chunk short and restarts the count there,
  and from that point two buffer sizes put their boundaries in different
  places - so everything worked out once per chunk was worked out a
  different number of times.

  The chunk grid is absolute now, so where it falls depends on where the
  notes are and on nothing else. Two smaller things went with it: the slow
  random drift of the oscillators was stepping once per chunk however long
  the chunk was, and the parameter smoothing was moving a full chunk's
  worth however short the chunk was.

  It was found while building render-ahead, which cuts a track's audio into
  stretches that are not the buffer's and so could not tell the two apart.

- **A session with a piano in it no longer opens with a quiet high C
  playing.** Every hosted instrument is warmed up at load: it is played a
  test note and listened to, because several will render silence for
  seconds after they report being ready and there is no way to ask them.
  The note is a C7, and the drain that clears it afterwards counted a block
  as quiet once it fell below the level that answers "is this instrument
  making a sound at all" - which is the wrong question, and its answer is a
  quiet note rather than silence. The instance then crossed to the audio
  thread still decaying, so the note was heard on every open of a session
  with a piano in it and printed into renders of one.

  The drain now waits for real silence, and the instrument is flushed first
  by the same means a panic button uses, which turns out to clear it in a
  single block. The measured level a rack is handed went from -40 dBFS to
  -180, and loading each bay got faster, not slower, because the drain
  finishes at once instead of waiting out eight blocks of decay.

- **A take made after the audio engine restarted was silent.** Changing the
  buffer size or the sample rate, or the automatic recovery after the
  output goes quiet, rebuilds the audio engine, and from then on every take
  recorded digital silence: the new engine's connection to the take
  recorder was lost in the handover. It is kept now. A take that starts
  with no working connection still records silence, but it now ends with
  an error that says why. Measured on the Linux machine that found it: a
  take after the engine was rebuilt peaks at -17.5 dB, the same as the
  output.

- **A dry stem now carries the instrument that plays it.** A MIDI track
  played by an instrument in its bays (the Analog, the sampler, any hosted
  instrument) rendered its dry stem silent, and so was left out of a dry
  export altogether: the dry copy was taken before the bays ran, and such
  an instrument makes its sound inside them. An instrument in any of the
  track's six bays writes the dry stem too now.

- **The count-in's last click sounded twice.** The count-in and the
  metronome both played the downbeat where one hands over to the other, a
  sample apart. The metronome lets the count-in's click stand now, and the
  song's first click after the count-in is no longer lost with the
  playhead parked between two beats.

- **The Analog's arpeggiator follows the song's tempo.** With the transport
  stopped it ran at 120 bpm whatever the song said, until play had been
  pressed once: on a 90 bpm song a held chord at 1/8 gave four notes a
  second instead of three. It also worked its beat out from the tempo knob
  alone, so on a song with a tempo ramp it drifted further from the click
  every bar; it follows the tempo map now. And the ARP switch acts on a
  chord you are already holding: switching it on starts the pattern,
  switching it off hands the chord back to the keys, and moving the switch
  between its on positions keeps the pattern's place.

- **An armed input is no longer heard bare while its plugins load.** A rack
  fills one plugin at a time, so for a second or two after a session
  opened, a guitar meant to go through an amp reached the speakers as the
  bare DI, the loudest thing in the room. While the racks are being
  rebuilt (a session opening or a new one starting, or the audio engine
  restarting, as it does for a new buffer size or sample rate), monitoring
  now waits until every plugin an armed audio input is heard through has
  loaded: its own, its folders', the first three bays of any track it is
  routed to, and the effects bus's when it sends there. A
  plugin you add by hand does not touch monitoring. And a denoiser that
  has just loaded applies its full reduction from its first moment instead
  of fading in from none. On Open or New, a bay still playing the previous
  session's plugin until it reloads is heard through rather than held, so
  a folder's Denoise no longer lets through the noise it removes, at full
  level, while a project loads.

- **"Untitled" folders stopped piling up.** Save As Template on a session
  you had never saved wrote that session into Projects/Untitled before
  copying it, every time. The template is written straight from memory
  now, always with its drum kit, and no Untitled folder is left behind;
  the template it replaces is kept until the new one is complete, and the
  session then counts as saved, since the template holds it. "Don't Save"
  means it: answering it on New, Open or Quit no longer writes the
  session, or its plugin settings, back into the project anyway, and the
  Project window's New and Open now ask before leaving an unsaved session
  at all. An Untitled folder this session created is removed when you
  answer Don't Save, provided it holds no saved clips, no recording and no
  drum kit built in it, instead of pushing the next session to
  "Untitled 2". New, Open and Quit all keep one that holds a recording.
  The launch clean-up no longer mistakes Finder's .DS_Store files, freeze
  renders or copies of the template's own samples for work. And a new
  session takes the template's drum kit rather than the last project's.

- **The band's controls no longer vanish in a smaller window.** At 900
  points wide (the window at 100% on a 1440-point screen) the transport
  icons shrank until the settings cog was too small to draw while its
  button stayed live, and between 794 and 889 points the send knobs, the
  master, the pitch dial and the tempo knob dropped out together. Icons
  keep a floor size now, a knob's caption gives way before the knob does,
  and every control is drawn at every width from 560 to 2600 points.

- **Reset Everything resizes the window straight away.** It put the
  window's size back to 100% in the settings and left the window as it was
  until the next launch. It resizes it at once now, and undoing the reset
  puts the window back.

- **The settings panel behaves.** DONE no longer also places the playhead
  in the song under it, a signature typed into SIG is kept when you click
  away, and Escape backs out one layer at a time. On Linux, AUDIO SETUP
  and the strips name a USB interface by its maker and model rather than
  by its PipeWire node. And a short name given to an input you chose stays
  with that interface: with it unplugged, the default input standing in
  for it is no longer shown under its name.

- **Audio Setup shows the audio system that is actually running.** It
  marked a system only once the setting named one, and the setting is
  empty until you choose, so a fresh install playing through WASAPI lit
  neither of the row's choices. During an ASIO fallback it marks the
  system standing in.

- **An ASIO driver's refusal says why.** A buffer size the driver would
  not take, or buffers it could not create, reached the log as a device
  that was not available, which was the one thing that was not true, and
  the real reason went to a console a Windows app does not have. The
  driver's own reason is in the log now.

- **One gesture, one undo.** After normalising and then quantizing a take,
  one Ctrl+Z undid both, because the normalise had no undo step of its
  own. The same fault folded these into whatever you did next: adding a
  track; dragging the arrangement header's knobs; the mute, solo, arm,
  polarity and theory switches, in the mixer and the arrangement alike;
  dragging the band's master fader, send knobs and varispeed, and a
  double-click that resets a knob; the metronome panel's division,
  count-in and click-sound rows, and the switches that record the click
  and the count-in into a take; choosing a DX7 cartridge, a DX7 patch, the
  default bank or a Keys sound; and collapsing a folder. The opposite fault
  took several presses to undo one thing. With the magnet off, a
  two-second marker drag pushed 120 steps into a history that holds 100.
  Carrying a time-signature change along the ruler left two steps for
  every bar it crossed, the first of each pair a song with that change
  missing. Dragging a plugin to another bay left a step with the plugin in
  both. A multi-file import left one step per file, and deleting a track
  left one for each plugin on it plus one for its clips. Grouping tracks
  into a folder, inserting a track or pasting one left a step with every
  track shifted down and nothing in the gap. Each is one step now.

  So are these, which still folded into whatever came next: the MIDI
  editor's magnet, grid, triplet, swing and humanize; the master's MONO
  and a plugin's bypass, from the mixer, either FX menu, a shift-click on
  a bay or the plugin's own BYPASS button; switching the click on or off;
  each change in an effect's parameter panel; a time signature typed into
  SIG or stepped with the metronome panel's BEATS and UNIT, and a typed
  take name, taken when you leave the field; a turned Keys knob, when you
  let go; and flats, the staff's key, the guitar's tuning, capo and capo
  style, a transpose (a held arrow is one step), the view on show and a
  theme saved under a new name. Two more took two steps: a Shift-added
  automation or tempo node dragged into place, and an audio quantize
  Apply that also changed a setting in the window, whose second step only
  put the setting back. Choosing the amp took eight, one per pedal of the
  board it arrives wearing, each leaving the newest pedal switched on; it
  is one now, and redoing past it no longer seeds the board again or
  throws the rest of the redo away. Double-clicking a Keys knob or an
  effect panel's slider to put it back is one step too, where one Ctrl+Z
  used to stop at the value the first click had jumped it to.

- **Undoing a Paste FX, a bay swap or a delete brings the plugin back with
  the settings it was playing at.** It came back at its settings from the
  last save, or from the last time its window was closed, or at its factory
  settings if it had never been saved; and dragging another plugin into a
  bay a deleted one had left could lose the deleted one's settings
  outright. A long session no longer forgets them all at once, either: the
  store that keeps them emptied itself after 32.

- **The Stream to OBS window keeps what you typed however it closes.**
  Escape or the window's close button threw away a typed identifier and
  host, and a stream that was switched on went on sending to the old ones.
  And the MIDI Inputs window follows devices being plugged in and out while
  it is open, keeping the boxes you had ticked.

- **The MIDI editor's playhead loops with the clip.** Inside a looping clip
  the line kept going past the pass the roll draws, while the sound looped
  correctly. It wraps with the loop now, and inserting, pasting, splitting,
  Backspace, the arrows and the zoom all act where the line is drawn; the
  navigator strip, which shows the whole clip, still shows where the song
  is. The loop bar counts from where the loop does, so on a clip trimmed
  from its front it stands where the loop turns over, and with the magnet
  on, dragging it keeps the loop a whole number of grid steps long. Not
  yet on a looping part trimmed from its front: in the first moments of
  each later pass, which the roll draws before the clip's head, inserting,
  pasting and Backspace act at the head instead, and the song can jump
  forward.

- **The edge of the track headers waits before it resizes.** It took the
  slightest movement as a resize, so a click meant to send the playhead
  home moved the edge. It waits for 8 points of travel now, and a click
  that stays inside them always sends the playhead home.

- **Render ahead's Auto could be chosen and never stuck.** The panel stored
  it as Off; it is kept now.

- **File names made from clip and export names are safer on Windows.**
  `< > : " | ? *` and control characters become `-`, and trailing dots are
  trimmed: Windows refuses the first in a file name and drops the second.

- **Two small ones on the band:** the cassette's hubs keep an edge at the
  band's smallest size, and the Sandstone and Celadon hearts have a dark
  border.

### Changed

- **A session carries its theme.** The theme and the channel palette were
  settings of the machine, so a project opened wearing whatever the last
  one left behind. They travel in the document now, beside the desk, the
  rack and the editor's feel that moved there before them. Changing a theme
  is a session edit and undoes like one.

  What travels is the theme's NAME, not its colours, which is what the app
  already meant by a theme: hand-edit a colour and you have a modified
  theme, and saving it under a name is how you keep it. A project written
  before this opens on whatever theme is live and records it at the next
  save, and a project naming a theme that has since been deleted opens on
  the default.


- **The IR reverb costs less than half what it did, and the reverbs on a
  desk stop landing on the same block.** Two separate faults, both in the
  convolver every IR bay is built on.

  It transformed four blocks of audio per step where two would do. Audio is
  real, and a complex transform of a real signal wastes half of itself, so
  the left and right channels now ride together through one transform in
  each direction and come apart afterwards by symmetry. It also worked out
  its own rotation angles inside the innermost loop, five hundred times a
  block, for angles that are the same on every block of every instance for
  the life of the program; they are worked out once now. Measured on this
  machine, per step: a cabinet-length response 36.3 to 17.0 microseconds, a
  room 37.2 to 17.2, a two-second hall 83.3 to 66.4.

  The second fault cost nothing on average and everything at the peak. Every
  convolver in the app started its staging buffer at zero, so every IR bay
  on the desk crossed its partition boundary on the same block and the
  entire cost of all of them landed in one block out of four, with three
  quiet blocks around it. Dropouts are decided by the worst block, not the
  average one. Each instance now starts at its own offset, so the same work
  is spread across the whole period. The delay the unit reports is
  unchanged, to the sample, at every offset, and a transport stop no longer
  puts them all back in step.

  Together, on this machine, a desk of thirty-two tracks carrying three
  half-second reverbs each, ninety-six in all, at 128 frames:

  | | before | after |
  | --- | --- | --- |
  | worst block in a hundred | 5489 us | 1137 us |
  | that as a share of the deadline | 206% | 43% |
  | blocks that missed the deadline, of 2000 | 500 | 0 |

  One block in four was late before, which is a dropout every eleven
  milliseconds for as long as the reverbs are running. None is late now. At
  64 frames the same desk went from 388% of its deadline to 50%. With the
  parallel render also in hand the ninety-six bays sit at 15% of the
  deadline at 128 frames and 19% at 64.

- **A plugin that crashes can no longer take the audio with it.** If
  anything went wrong while a worker was processing one track, the main audio
  thread was left waiting for that track to be finished, with nothing that
  could ever satisfy it, spinning until the app was killed. The track is now
  marked finished on every way out, including that one, so the block
  completes and the app keeps playing.

- **A race in the parallel render is closed.** A worker reads what it is
  meant to do once per round and then loops asking for the next track. If it
  was interrupted between finishing one track and asking again, the main
  audio thread could meanwhile finish the round, move on, and hand out the
  next round's work; the interrupted worker would then take one of those
  tracks and run it against instructions that no longer existed. One audio
  buffer is several rounds, so the two were microseconds apart. Each round is
  now stamped, and a worker holding a stale one is given nothing.

- **The parallel render works on Windows, Linux and FreeBSD.** Spreading a
  block's plugin work across cores only ever engaged on macOS, because it
  refuses to run on threads the system has not promised to schedule on time,
  and the only way of asking for that promise it knew was the macOS one.
  Windows is asked through the audio scheduling service every host on that
  platform uses, and Linux and FreeBSD through the real-time scheduler, at a
  priority deliberately below the audio callback itself. Where the system
  refuses, which on a Linux desktop without the right permission it will,
  nothing forks and the engine runs exactly as before. Type-checked for all
  three targets from here; not yet run on any of them.

- **The parallel render follows the machine it is on.** The pool that
  spreads a block's plugin work across cores was fixed at four workers,
  which is right for the five performance cores it was built on and wrong
  on anything larger: the same four faced ten on an M4 Max and more on an
  Ultra, so the machines with the most silicon left the most of it idle. It
  asks the machine now. Nothing changes on this Mac.

- **A plugin seated past the third bay gets the same treatment as one in
  it.** The app asks the open plugin windows three questions once a frame:
  whether a preset just moved the plugin's delay, whether a window has just
  closed so its settings can be saved, and whether to keep drawing, which on
  Linux is what actually runs the plugin's own window. It asked only over the
  first hundred and eight bays, which is the tracks' first three slots and
  the master. Everything else, the second rack behind a folder and all seven
  pedals in front of an amp, was outside the question. A plugin there could
  go out of time alignment after a preset click, lose its settings on close,
  and on Linux stop responding. It is asked over every slot now.

- **A note held when you press stop is now actually told to stop.** The
  engine sends a note-off for every note being held when the transport stops
  or the playhead jumps, and none of them were being delivered. A DX7
  survived because a separate sweep silences DX7 instances directly; that
  sweep reaches no Keys instrument, no Analog and no third-party plugin, so
  those kept ringing.

  Underneath it, the record of what was sounding was a single list keyed by
  pitch with two unrelated owners, the live keyboard and clip playback. A
  clip note ending on a pitch cleared the record of a key you were holding on
  that same pitch, and that key then got no note-off at all. Each track keeps
  its own record now.

- **One track's dense chord no longer delays another track's note.** What was
  sounding lived in one list for the whole desk with a fixed cap, filled from
  track 1 upward, so two busy tracks could push a third track's note a block
  late. There is no shared pool any more.

- **Every track now reads its own audio, instead of the desk keeping one
  block of everything.** The timeline used to be assembled once per block
  into a single buffer holding every track, which each track then copied its
  own row out of. A track asks for its own audio directly now, for whichever
  block is being worked on. Nothing about the sound changes and the whole
  clip test suite says so, to the sample: placement, trim, fades, pan, mute,
  solo, punch-in, the route through a track's plugins, and what the master
  hears of it.

  Two things come out of it. Assembling the timeline used to cost the same
  whether or not any clip was near the playhead, and a single looped clip is
  expanded into as many as four thousand pieces, so a long arrangement paid
  for all of them on every block; it now costs one comparison each. And this
  is the change that makes processing tracks ahead of time possible at all.
  Measured on this machine, of the work a track costs in a block, the share
  that could be done in advance went from nothing to all of it on an audio
  desk, and to 62% on the largest of the projects here, the rest of which is
  MIDI and is the next piece of work.

- **Track delay compensation is applied where the audio is used, not where
  it is made.** Every track carries two small delay lines that hold it back
  so it lands in time with tracks whose plugins are slower. They used to run
  as the last step of processing a track. They now run as the first step of
  mixing it in. Nothing about the sound changes, to the sample, and the whole
  test suite says so, including the four alignment tests and the one that
  compares grouped and ungrouped playback. What changes is that a delay line
  can no longer be advanced by processing that is later discarded, which is
  the first piece of groundwork for processing tracks ahead of time.

- **The audio panel can show what a look-ahead would have been worth, and
  the answer is nothing.** Set `TANGENT_PROBE=1` and a line appears under the
  render load giving how much of each block's track processing could have been
  done in advance, off the deadline, the way some other programs do it. It is
  a counter and nothing else: it processes nothing early, starts no thread and
  changes no sound, and it is absent from every run that does not ask for it.

  Measured over this machine's own projects and over twenty-three test desks
  including one carrying ninety-six commercial reverbs, the figure is zero
  every time. A track can only be worked on in advance if its audio has a
  future to read, and the source that feeds a track is one block deep for the
  whole desk, so any track that is processing at all is a track that cannot
  be run ahead. It also overlaps almost exactly with the change that stopped
  processing tracks with nothing to play: the work that would have been moved
  off the deadline is the work that is no longer being done.

- **The render load meter can tell one number from another.** It kept its
  distribution in octaves, so 524 and 1048 microseconds were the same
  reading, which put every figure that decides whether a desk is fast enough
  inside a single bucket. It keeps eighths of an octave now, which is about
  nine percent, at the cost of a shift and a mask per block.

- **A track with nothing to play stops being processed.** While the
  transport was rolling, every track with a plugin on it and a clip anywhere
  in the song ran its whole chain on every block, over silence, for the gaps
  between its clips and for every part it does not play on. Muting a track
  saved almost nothing, because a plugin costs the same on silence as on
  signal. A track now rests when it has nothing under the playhead and has
  stopped making sound, and wakes for its next clip. Measured on this
  machine at 128 frames, sixteen tracks with an EQ, compressor and plate
  each, ten of them idle: 149 to 64 microseconds a block.

  Four things keep a track awake, and each answers a way this is easy to get
  wrong. Something under the playhead, tested by span so a rest inside a
  take is still the take playing. Its own measured output, so a reverb tail
  or a delay repeat keeps its track running until it falls below -80 dBFS,
  which is what stops a tail being cut off mid-song and is also why the rest
  needs no fade: nothing audible is ever taken away. Being the destination
  of a send, because such a track's plugins are clocked a second time by the
  send pass and are handed their sidechain there. And any knob turn, preset
  or automation point that arrived for it this block.

- **Tangent's own Drive and Tape stop being the most expensive plugins on
  the machine.** The performance sweep measured both above every commercial
  plugin it tested beside them, which is not a place a built-in effect
  should be. Both run four-times oversampling through a pair of filters, and
  those filters walked their delay line with a split and a chain that no
  compiler can vectorise, adding one tap at a time into one accumulator.
  Every sample is written twice now so the window is always contiguous, and
  the taps are summed eight at a time into independent accumulators, which
  is the shape the Analog synth's own filter has had all along. Measured on
  this machine over sixteen lanes at 128 frames, per instance: Drive 91.5 to
  37.7 microseconds, Tape 63.9 to 24.2. The unfixed Drive desk went a block
  over its deadline during the measurement; the fixed one does not.

- **The chorus, the echo and the plate stop recomputing filter poles per
  sample.** Each is handed one frame at a time and worked out its own
  coefficients from a knob that had not moved, which is a libm call per
  sample: three in the chorus, two in the tape echo, one in the plate. They
  are remembered on the value that produced them now, so a knob still takes
  effect on the sample it is turned and the arithmetic is otherwise the same
  call on the same argument. The chorus's gate had three more that are fixed
  times at a fixed rate, and those are worked out once when it is built.
  Measured over sixteen lanes: chorus 61.5 to 46.9 microseconds a block, a
  quarter off. The echo moved about four percent, which is inside the
  measurement's own noise.

- **Two smaller ones in the same pass.** The DX7's sine did a libm remainder
  call six times per voice per sample, which is 4.6 million a second at full
  polyphony, and now rounds instead. And the Analog's arpeggiator allocated
  a vector on the audio thread every thirty-two frames for as long as a key
  was held, which is the one thing that thread may never do.

- **The audio panel says what the render is costing.** A new line under the
  dropout count: what the last block cost as a percentage of its deadline,
  and the worst block since the stream opened, amber past seven tenths. Two
  numbers rather than one because the average is the misleading one, which
  is the whole point of a real-time meter: a desk can sit at four percent
  and still glitch, since what the room hears is the block that missed and
  not the mean of the ones that made it. Nothing in the engine could be
  ranked or proven without this, and every other DAW has had it for years.
  The measurement costs 38 nanoseconds a block, which is 0.0014 percent of
  a 128-frame deadline at 48 kHz.

- **The desk renders on several cores instead of one.** Every lane's
  instruments, pedalboard, plugin chain and delay line used to run one after
  another inside the CoreAudio callback, so a block cost the sum of
  everything the desk held while the other fourteen cores did nothing. The
  lane pass is three walks now: one that decides, which touches the state
  the whole desk shares and stays a single walk; one that renders, which
  touches nothing but a lane's own seats and its own row of output, and
  which forks across a pool of worker threads; and one that folds, in
  ascending lane order. Measured on this machine at 128 frames with the
  fan-out forced on and no device open: sixteen lanes of EQ, compressor and
  plate went from 145 to 66 microseconds a block, sixteen denoisers from 183
  to 76, and eight lanes of the Analog synth at eight voices each from 523
  to 160. The sum is bit-identical either way, which is what the ascending
  fold buys, and a test asserts it sample for sample rather than to a
  tolerance.

  **The workers are real-time threads or there are no workers.** Each one
  takes a mach time-constraint policy derived from the block the device
  granted and joins that device's own IO workgroup; a worker that fails
  either does not count, and a pool that is short renders serially instead.
  That is not caution. Fifteen ordinary threads measured worse than the
  single walk they would replace, one of them taking a whole deadline in a
  single block. Off macOS, and on any device that publishes no workgroup,
  nothing is ever ready and the engine renders exactly as it did.
  `TANGENT_FANOUT=0` turns it off.

- **Ten themes: seven replaced and three retuned.** The owner's list, in
  his words: Valentine keeps its slot with more red, "like the original TUI
  application"; Sandstone keeps its slot with a darker sand, because "even
  light mode" was "too harsh"; Centennial keeps its slot with a slightly
  darker recorder. Studio, Aurora, Blueprint and Porcelain were replaced
  with "something more inspired and not yet covered by the rest of the
  themes"; Dusk and Iris were "too close" to each other; Nocturne was "also
  just Navi". In their slots now: Ochre, the pigment as the band, with
  cornflower and indigo; Reed, the reed electric piano this app models, in
  the avocado the 200 wore; Zinc, the patina metal and the only violet in
  the set, with brass for what is held; Celadon, the glaze, cool where
  Sandstone is warm; Prism, charcoal with one colour per control; Storm,
  slate and the bolt, the only theme built on yellow; and Amber, the other
  phosphor console beside the green one. Every theme in the set was colour
  checked in both modes against the whole suite before it shipped, which is
  what the owner asked for: legible the first time.

  No slot moved. A theme's position is the index a saved desk keeps as its
  palette choice, so each new theme took the retired one's row and each
  retired run kept every byte it had in the strip table; a desk that painted
  a track from one of them still paints it the same colour. The seven new
  palettes were appended whole, and the velocity lane's colours were
  re-solved for all nineteen themes against each theme's own roll.

- **The denoiser's spectral kernels open and close like an instrument, not a
  gate.** Measured on the owner's take against a render he prefers: the old
  kernel wrote every band's gain fresh every 5 ms frame and snapped a band from
  the floor to unity in four frames whenever a note landed in it, so every bit
  of room noise inside that band came up with the note and dropped out after
  it - the "digital" sound, as a gain moving 12 dB in 25 ms and back at every
  word. Three changes, each measured on that take: a band's gain is now a
  TARGET that a one-pole follows in dB, up at ATTACK and down at RELEASE, both
  live on the spectral kernels for the first time (they were "Bank only");
  the presence is gated on how far a band reads over its own learned print,
  so a chair opens a band part way and a note all the way, with SMOOTH as the
  width of that knee; and a new TREBLE BOOST lifts what survives above about
  3 kHz and never the residual. New defaults, the owner's numbers: attack
  10 ms, release 100 ms, smooth 0 (the softest knee), treble boost 2 dB. On
  the take the largest single-frame gain jump fell from 4.2 dB to 1.7 dB
  (the reference: 3.6), quiet events in the 500 Hz-2 kHz region went from
  -3.8 dB to -5.2 dB (reference -6.9), signal frames are untouched below
  3 kHz and +1.2 dB above, and the gap residual is about 2 dB shallower than
  before at the same REDUCE, which is the cost of the release holding each
  word's tail - the reference pays it too, from a deeper floor; turn REDUCE
  up to have both. The bank keeps its own ratio, open time and readings.
  `TANGENT_DN_*`: an ignored test, `render_a_take_through_the_denoiser`,
  renders any WAV through the unit from a bay's saved print, and reproduces
  the plugin's own output to within 3e-3, so the next change is measured on
  a real take too.

- **A take records and nothing else; the transport stays yours.** Pressing
  the cassette used to roll the song, refuse Play and Pause, leave Space
  doing nothing, ignore the loop and make Stop end the take: the opposite
  of what a take is for when you are filming a walkthrough of a project.
  Now a take is started and ended by the cassette, never by the transport,
  and it no longer starts the song: the song plays when you play it, as it
  does with no take running. Record, play, stop, pause and the cassette
  stay where they are on the band, and Stop, Space, auditioning a chord,
  undo and redo and moving tracks all work while a take is recording. A
  realtime export is the one exception: it owns the transport while it
  runs.

  A take and the red button share one count-in: a take started during the
  red button's count joins it, the red button pressed during a take's
  count joins that one, and Play during either starts the song on the
  downbeat. A take's `take.json` records whether the song was rolling when
  it began (`timeline_rolling`), which says whether
  `timeline_start_seconds` places its first frame; for a take with its
  count-in recorded into it, whether the song started on the count's
  downbeat, `head_seconds` into the file.

- **The take's settings are on the cassette.** Right-click it for the TAKE
  panel: the take's name and folder, count-in, signature and export, and
  the take's switches. Right-click the red button, when no take is
  running, for a RECORDING panel with just the count-in and the signature.
  The cog keeps the machine (Audio Setup, hearing the input, the project
  and plugin doors, where things are kept) with the count-in and signature
  as well. A row is the same size in all three.

- **In the arrangement, Cmd/Ctrl+M opens the metronome panel; showing and
  hiding the master row moved to Cmd/Ctrl+Shift+M.** The panel opens in the
  middle of the arrangement, and a second press closes it. It carries the
  count-in now, counted in bars ("off", "1 bar", "2 bars"), and IN TAKE
  chooses whether the count stops before the take begins or carries on
  into it, recorded at the head of the file. A count lasts its bars at
  every click division, with one click a beat.

- **During a count-in or an export the tempo is held.** In a count-in the
  tempo knob dims and TAP refuses; while an export renders, both leave the
  band's row. The metronome panel's tempo row and the tempo lane refuse a
  change until the count is over or the export has finished, and a screen
  reader hears the held controls as unavailable. Undo and redo wait as
  well, since a step back can change the tempo.

- **A session carries the arrangement's own grid and window.** The magnet,
  the grid, whether the tempo lane and the master row are showing, the
  width of the track headers, how many lanes fit the window and the
  quantize window's settings are saved in the project now, beside the
  metronome, tempo and signature that already were. Opening a project
  saved this way no longer leaves the timeline wearing the last session's
  choices; one saved before this opens with the timeline as it was last
  left, and records its own at the next save. Like the theme, changing one
  is an edit to the song: it undoes, and it marks the project unsaved. The
  lane count travels with the height of the window it was chosen in, so on
  a display of another size it gives about the same lane height rather
  than the same number of lanes. Not yet for the header width and the lane
  count while they are on Auto, as on a fresh install: a project saved on
  Auto opens on whatever the window already had.

- **A clip's waveform shows its gain.** Normalising a take changed how it
  sounded and not how it looked. The waveform is drawn at the clip's level
  now, held inside its lane when the clip is lifted past full scale.

- **On Windows, every program Tangent installs is signed.** Windows 11's
  Smart App Control checks each program every time it starts and blocks an
  unsigned one it does not know, with no override, not even for an
  administrator: a tester's installed Tangent stopped opening the day Smart
  App Control switched itself on. The app, its bundled video encoder, the
  installer, the helpers inside it and the uninstaller are all signed now,
  under the author's own name, and neither a Windows release nor a tester
  build can be published with any of them unsigned.
  SmartScreen may still say "Windows protected your PC" on the first signed
  releases, because that warning goes on reputation, which a new signature
  has to earn.

- **The Windows download's video encoder is FFmpeg 8.1.3** (it was 8.1.2).

- **The supporter's heart moves at half the speed.**

### Fixed

- **The right-click menu stops jumping to the top on a second display.**
  The cause was never in the menu's logic. egui tells the app how big the
  monitor is and never where it is, so the code treated the screen as the
  box from the origin to that size. That is true of one display. Window
  positions on macOS are global across the whole desktop, so a window on a
  display arranged below or to the right of the primary one sits outside
  that box, every "does this run off the edge" test answers yes, and the
  clamp does what it was told: it moves the menu, and its submenu, to the
  primary display's top corner. Every time, on the external display, and
  never on the built-in.

  The host asks the operating system which display the window is on and
  hands the menu that rectangle, so a menu on any display is now kept on
  that display rather than pulled onto another. Where the host cannot say,
  which is every platform but macOS, the clamp applies only when the menu's
  position is inside the box it was given, since that is the only case where
  the box is known to be the right screen.

  Two other things found in the same code and fixed along the way. A dwell
  asked for its whole length again on every frame, so after the window
  learned to rest between frames a 140 ms wait took 248 ms and a 60 ms one
  took 88; each is asked for by what is left of it now. And a hand inside
  the triangle between where it started and an open panel is read as
  travelling rather than choosing, which is what native menus do, so the
  rows it crosses on the way no longer open.

- **An export no longer loses the third drum track onward.** A kit crosses
  to the engine on a handoff ring that held two, which was enough for the
  live desk because it retries a refused kit on the next frame until it
  lands. The export path does not: it hands over every drum lane in one
  pass before a single block is rendered, and never looked at the answer.
  So a project with three or more drum tracks rendered a file in which the
  third onward were silent, discovered whenever somebody listened to the
  bounce. The ring is as deep as the sampler's now, which is one slot per
  channel and two spare.

- **A frozen track stops being processed twice.** Freezing renders a lane
  through everything it holds and plays the result as an ordinary audio
  clip, so the file already contains all six insert bays and the whole
  pedalboard. The stand-down that is supposed to switch those off asked
  whether a SEAT was frozen, which is true of a lane's first three bays and
  false of its second three and its seven pedal slots: those kept running,
  over a file they had already been baked into, and kept reporting their
  latency as well, so the lane also sat late against the rest of the mix by
  their lookahead. It asks which LANE a seat belongs to now.

  **This changes the sound of a track you have already frozen**, the moment
  the project opens, if it carries anything in bays 4 to 6 or an amp with
  pedals. That is the sound the freeze was meant to give you, and it is not
  the one you have been hearing. A track with three bays or fewer and no amp
  was always correct and is untouched.

- **Automation on a plugin insert reaches the plugin.** It never has. The
  host files a block's automation points into the parameter pool the plugin
  reads, and then cleared that pool before calling the plugin, so the count
  it reported was zero and every curve was silently inert: the knob moved on
  screen, the project saved the curve, and the sound never changed. Built-in
  and LV2 inserts were unaffected, because their parameters take another
  road. The clear happens first now. Anyone who has drawn automation on a
  VST3 insert will hear it start working, which is the correct behaviour and
  is still a change worth knowing about before opening an old project.

- **The dropout count is no longer blind on macOS.** The panel read two
  counters, and both are structurally silent on a Mac: one is the PipeWire
  graph's, which is Linux only, and the other is cpal's, which never sees an
  xrun through CoreAudio. So the figure was always "unknown" on the platform
  the app ships on, while the HAL's own overload listener sat there counting
  and reaching nothing but a log line. It reaches the panel now. "Unknown"
  still means nobody could watch, which is the honest answer on a device
  that refuses the listener, and a real zero from a counter that can see is
  still rendered as a zero.

- **An export no longer freezes the window before its first sample.** Seven
  engine setters wait for the audio thread to hand back what they replaced,
  giving up after 250 ms. An offline engine has no audio thread, so every
  one of those waits ran to the full quarter second and then shrugged, and
  an export furnishes every seat it holds before it renders anything: at
  forty-eight seats that is twelve seconds of a frozen window with the
  progress bar at zero and nothing to say why. The wait is skipped where
  there is no callback to wait for. Nothing is leaked by it, because the
  renderer's own teardown drops whatever the wait did not collect, which is
  what the wait's own note already said.

- **A noise print learned on Spectral Studio or Live now works on Bank.**
  The spectral kernels freeze each band at the geometric mean of a
  windowed periodogram, and the bank gates on a different measurement of
  the same noise, so a spectral print read as a bank knee sat in the middle
  of the noise. A spectral pass now runs the bank's own analysers as well
  and stores a bank knee beside the print, persisted as `k0..k31` next to
  `t0..t31`; a print from before this gates the bank at its thresholds as
  it always did, and re-learning gives it knees. The unit is called Tangent
  Denoise now, without "(Experimental)".

- **The band's peak lamp lights again with the limiter in circuit.** The
  clip latch was taken after the master's limiter, which holds the master
  under the ceiling, so it could never light. It now also answers for a
  sample at the ceiling going INTO the limiter.

- **The master limiter ships out of circuit, with a Circuit row on its
  right-click face to put it back.** Out, the engine is sent the top of the
  dial whatever the dial says, and the dial is drawn dark. Projects and
  settings from before this have no circuit choice and open with the
  limiter out.

- **The theory view opens light by default.** The arrangement field and
  the MIDI editor still open dark; a settings file that says otherwise
  still wins.

- **The right-click menu's submenus answer on time again, and a panel no
  longer lights a row the hand has not touched.** When the window learned
  to rest between frames (5.0.0-dev, 2026-09-15) the menu's own asks became
  the only frames it got, and egui takes its predicted frame time off every
  ask: a 140 ms dwell asked for again on the frame that fell 16 ms short
  took two rounds, 248 ms, and a 60 ms rest 88. Each is now asked for by
  what is left of it. The submenu panel, which on the desktop is its own
  window that reports the last pointer position it saw, now lights no row
  until the hand has moved inside it, and starts over for every submenu
  it is refilled for.

- **Every plugin face's reading is a field, and Shift makes its slider
  fine.** Click a reading to type a value - "-12", "440", "1.5k", "35%",
  "off" - and Enter lands the knob where the reading says those words; the
  host reads the words on the unit's own scale, so the field shows the
  number typed. Escape, or the focus going elsewhere, changes nothing.
  Shift held during a drag moves the knob a quarter as far, the ratio the
  Analog and sampler dials already turn at, and a press made with Shift
  down no longer knocks the knob to the press point first.

- **Splitting a repeating MIDI clip no longer leaves the tail empty.** A
  clip whose loop is shorter than its length keeps every note inside its
  first pass, and the blade handed the tail only the notes at or past the
  cut in source time, which past the first pass is none of them. A
  repeating clip's tail now takes every note and pedal edge, the same loop
  and an offset at the cut, and plays on from where the head stopped; a
  clip that does not repeat still divides at the blade as before.

- **A chosen track count carries its density to another display.** A
  lane count set with Shift+wheel was applied as a number wherever the
  window went, so a dozen chosen on a 1440-point external display were a
  dozen thin lanes on the laptop's 982. The count now remembers the
  timeline height it was chosen at and, on a pane of materially different
  size, gives the same lane height instead: twelve on the external is eight
  on the laptop, and eight on the laptop is twelve back on the external.
  The mixer's strip count does the same with the desk's width. A count from
  an older settings file, which recorded no height, is taken as it is until
  it is next changed.

- **An armed strip inside a muted folder no longer comes out of the
  speakers.** A grouped strip's live input reaches the mix through its
  folder's fold, but what the room kept out of a withheld input was the raw
  input, so with the folder muted, or merely turned down, the difference
  came out of the speakers inverted: the take was silent and the room was
  not. What the room keeps out now follows the folder chain's gain and pan,
  so a muted folder keeps its armed child out of the room as well as out of
  the take.

- **An armed strip is in the take whatever the mute does, and the band's
  VU follows it.** The take is the desk, and the desk dropped a strip nobody
  could hear, armed or not, so the VU over a vocal being recorded
  armed-and-muted sat at rest while the arrangement recorded it just fine,
  the same hole the strip meters had. Arm is the record switch now and it
  beats the mute in the take: the strip goes in through its rack, its fader
  and the master's limiter, which is what the needle reads. The mute, or a
  solo elsewhere, still keeps it out of the speakers, with monitoring on or
  off, exactly as monitoring-off withholds a live input.

- **Empty tracks are no longer exported as stems.** A lane counted as
  "sounding" if it merely held a plugin, so the stems folder carried a file of
  digital silence for every instrument track with nothing on it - four of
  them in the owner's export. A stem that never receives one sample off
  digital zero is removed when the render finishes, in both the realtime and
  the offline bounce, and the notice says how many were skipped.

- **A record-armed track is metered even while muted.** It records muted just
  fine, and its meter read nothing, which said the opposite. An armed strip -
  audio or MIDI - now meters its INPUT, the signal the take is getting,
  whatever the fader and mute do to what the room hears; unarmed strips
  meter as before.

- **The loading splash no longer loads behind the export window.** The
  cover is painted in the main viewport, and the export window is a child
  window the operating system keeps in front of it, so a decode or a write
  that started under a render showed the render window over an app that
  seemed frozen. The cover is now mirrored into that window as well.

### Added

- **Track freezing.** Cmd/Ctrl+Shift+F on the selected track, or Freeze
  Track on the track's right-click menu, renders the track through its insert
  rack, post-insert and pre-fader, into the project's media folder and
  plays that file in its place while the rack and any built-in instrument
  on it stand down: the plugins keep their state and pay nothing. Fader,
  pan, mute, solo and sends stay live, so the render leaves them out. The
  frozen track is painted under frost with a snowflake, its clips are shown
  and not editable, and the same key or row thaws it. The freeze travels in
  the project and rides undo. Cmd/Ctrl+F still makes a folder.

- **The arrow keys walk the playhead.** In the arrange view, Left and Right
  move the playhead a grid step, onto the grid when snap is on, and a bar
  with Shift. In the MIDI editor they are the playhead whether or not a
  note is selected; Enter leaves the note it wrote selected, so the bare
  arrows had stopped being the playhead the moment a note went in. Nudging
  the selection, in time and in pitch, moved to Alt+arrows.

- **Backspace in the MIDI editor steps back.** It deletes the selected
  note, or with nothing selected the note that ended last before the
  playhead, and puts the playhead where it stood before that note was
  written, so a run of Enter presses can be walked back the way it was
  typed. Delete still deletes without moving anything.

- **FreeBSD is a release platform.** `scripts/release.sh` on a FreeBSD host
  runs `build-freebsd.sh`, checks the `.pkg` and both offline bundles the way
  every other artifact is checked - both manifests, the binaries, the desktop
  entry, the licence texts, the declared `alsa-lib`, `alsa-plugins` and `lilv`,
  no GhostBSD base dependency, the right version - and `publish-github.sh`
  uploads them. pkg spells a prerelease its own way (`5.0.0-rc.1` is
  `5.0.0.r1`), so the three scripts share one `sed` and `check.sh` holds the
  copies byte-identical. Until now the port built and shipped by hand, outside
  every checksum and upload glob; the decision whether it ships is made.

### Fixed

- **The FreeBSD package and bundles now carry the licence texts.** Every
  other artifact ships `LICENSE`, `OFL.txt`, `THIRD-PARTY-LICENSES` and the
  `font-licenses/` folder; the FreeBSD `.pkg` installed none of them and the
  bundles carried only `LICENSE`. The OFL requires its text to accompany the
  fonts embedded in the binary, so this was a licence gap, not a cosmetic one.
  The `.pkg` installs them under `share/licenses/tangent/`, the bundles carry
  them at the top level, and `release.sh` refuses either without them - which
  is what found it: the new checks, run against the artifacts already in
  `dist/`, failed on exactly those files and nothing else.

- **An open band with no camera no longer holds the window at 30 fps.** The
  band asks the capture thread for a preview whenever it is open, and the
  frame ledger read "a preview is wanted" as "pictures are arriving", so a desk
  with the recorder showing and no camera selected at all was held at
  `LIVE_TICK` for as long as the band was open, collecting frames from a thread
  that did not exist. Found last, after every other pin below had been removed
  and `IVORY_PACE_PROBE` still read `host_live=[camera]` on every second with
  nothing selected. The ledger now counts the camera only while a capture is
  actually running. `IVORY_PACE_PROBE` prints `host_live=[...]`, the host's
  reasons for staying awake, so the next one of these names itself.

- **The heart no longer sets the pace of the whole window.** The animated
  heart - the supporter's palette wave and the cheater's crackle alike - asked
  for a repaint 33 ms out on every frame it was drawn, and since egui keeps the
  MINIMUM asked, that one line beat every resting cadence the rest of the app
  had negotiated: with nothing playing, the owner's desk painted fifty-six
  frames a second for as long as it was open, about half a core, for a sprite
  seven pixels wide. Found by adding `asked_by=` to `IVORY_PACE_PROBE`, which
  prints egui's own record of every call site that asked for a frame that
  second; it named this one on every frame. Every resting test had passed,
  because the test fixture turns the heart off - the one animator this machine
  always has was the one no test ever saw. The heart now asks for 100 ms (its
  wave is stepped and advances one band every 375 ms; the swirl turns once in
  ten seconds) and only while the window is focused and not minimised; unseen,
  it still moves at the idle poll, for free. The analyzer answers to the same
  rule, since a noise floor moves its bins by more than its settling threshold
  on every frame. `IVORY_PACE_PROBE` now also prints the smallest delay the
  app's ledger and the host's ledger each asked for in the second.

- **An idle desk rests, whatever is loaded on it.** Two more reasons a
  window with nothing playing kept painting, both the same shape as the VU
  needle below. The host's meter ledger asked "is anything above the floor",
  which is true of every desk with a plugin in it for as long as the project
  is open: an idle rack's noise floor sat a few LEDs up the ladder, wandering
  by less than a pixel, and the window painted it thirty times a second all
  afternoon. It now asks whether the meters' PICTURE would draw differently -
  a level that has not moved one LED, a hold line that has not moved one LED,
  a lamp and a lane bit unchanged - and rests when it would not, with a grace
  so a transient is followed down its decay every frame. And a lane carrying
  an instrument never parked its rack at all, the class of the plugin deciding
  that at load, so the owner's the modelled piano ran its physical model over nothing
  for the life of the session. A voice now parks on a stricter rule than an
  effect: nothing above -80 dBFS at its pre-fader output, its own instrument
  reporting no voice where it can be asked, and no note or control event, for
  thirty seconds rather than the effects' eight - the extra margin is for a
  patch that swells from silence. A control event wakes a parked lane the way
  a note always did, so a sustain-down that arrives while the lane is parked
  is not the one message its instrument never received.

- **An idle window no longer paints sixty frames a second.** The VU needles
  chase the meters through a one-pole, which APPROACHES its target and never
  arrives, and the test deciding whether they were still moving asked whether
  they were exactly there, to within `1.0e-4` - about -80 dBFS, four orders of
  magnitude below anything a VU face can draw. A desk with anything loaded in
  it is never digitally silent, its noise floor wanders every frame, and so
  every frame re-opened a gap the test could see and the screen could not. The
  needles asked for a repaint 16 ms out, and because `request_repaint_after`
  keeps the MINIMUM asked, that one request beat every resting cadence the
  rest of the app had negotiated. The window ran at 59 frames a second for as
  long as it was open, with nothing playing, at about half a core. A needle
  within a quarter of a pixel of its reading now SNAPS to it and stops asking;
  the ballistics are untouched.

  Measured on the owner's machine with the modelled piano and Brusfri loaded and the
  transport stopped: `IVORY_PACE_PROBE=1` read `fps=59 p99=18.3ms`, which is
  the 16 ms request plus the frame it buys, and `TANGENT_FRAME_LOG=1` priced
  that frame at `update 1.0/1.1/1.1 | cpu(prev) 2.7/3.0/3.1`. The resting
  policy had a test for an idle desk already; it rested its meters at exactly
  zero, which is a reading no real desk ever gives.

### Added

- **A session carries its rack, its levels and its feel.** The eight knobs
  across the recorder's band (reverb, delay, chorus, phaser, the two filters,
  the output gain and the limiter) and every parameter row behind them lived
  in the settings file alone, which made each of them a property of the
  MACHINE rather than of the song: opening any project wore whatever reverb
  the last one was using, and there was no way to leave a mix set up and come
  back to it. The same was true of the backing track's level, the mono fold,
  the loudness window, the varispeed dial, the click's division and sound,
  whether the click prints into the take, the transpose, the count-in, the
  take name, the MIDI editor's magnet, grid, swing, triplet and humanize, and
  the staff's key signature with the guitar's tuning and capo. All of them
  ride the document's desk now, captured on every save and applied on an
  explicit open, beside the faders and bays that already did. The owner: "I
  need the desk settings (all send knobs in recorder and levels) to be saved
  per session. They currently don't count in the parameters set by a session
  file. Everything should be session dependent."

  A document that does not carry a key keeps the LIVE value, which is the
  rule the desk's earlier late arrivals already answer to: nothing an old
  project sounds like changes because a field was added, and a project opened
  from before this wears no asterisk it did not earn.

- **Shift+Up/Down shows the notes it takes.** The selection was right and
  invisible: `selected` is sorted by contract because the roll asks a binary
  search whether each note it draws is picked, and the walk APPENDED rather
  than inserting in order. The moment the note taken in had a lower index than
  one already held (which in a chord is most of the time, since notes are
  indexed in the order they were written and the one above is as often older
  as newer), the search could no longer find it and the note drew unselected.
  It appeared later, all at once, whenever some other operation rebuilt the
  selection in order. The owner: "it just doesn't show a visual indicator as
  notes are added in a stacked note/chordal situation. it only shows after an
  operation like pasting the selection."

- **An arrow's audition is held long enough to be a pitch.** A cursor step
  sounded its note and set no deadline, and a preview with no deadline is read
  as one a hand is holding, so with no button down it was released on the
  very next frame. A click rather than a note, and the key it lit blinked. It
  is held for four tenths of a second now, and the step after it releases it
  first, so walking the keyboard cannot leave a chord ringing behind. The
  owner: "make the arrow audition a bit longer unless interrupted by another
  audition."

- **A sounding key is visible.** The editor auditions on three roads (a
  press on the piano column, an arrow that moves the pitch cursor, a note
  written or grabbed) and not one of them drew anything: the sound came out
  and the picture said nothing, so a player walking up the keyboard on the
  arrows had to count rows, and a press that produced no sound looked exactly
  like one that had not registered. A key that is sounding now lights amber in
  the piano column and washes its lane all the way across the roll, because
  the eye editing a part is on the notes and not on a strip eighty points away
  at the far left. The owner: "we need proper visual feedback for arrow and
  click keyboard auditioning in the midi editor."

- **The MIDI editor's grid chip reads "1/4" rather than the note glyph**, the
  same numerals the arrange toolbar has used since the owner asked for them
  there. The two spellings had been kept apart on purpose (he asked about
  ARRANGE, and a shared helper would have moved this chip in the same edit
  without anyone deciding to), with a test pinning the editor's glyph in both
  directions until he said otherwise. He has: "Replace the note glyph with 1/4
  in midi editor."

- **Right-click a note for its own menu**, and two keys beside it. Copy,
  Cut, Paste, Delete, Mute, Join and Split: the owner's list. Right-click
  used to erase the note on the spot; Delete is one of the rows now, one
  press further in and named. **M mutes the selection** from the keyboard, the
  menu's row on a key. A right-click on a note outside the selection
  selects it first, and one on a note already in it leaves the selection
  alone, so a menu opened on one note of a chord acts on the chord. **J
  joins and S splits**, the two the owner asked to have on keys as well; over
  the roll both stand down from their app-wide meanings, the way H and Z
  already do.

  **A muted note is written but not played.** It keeps its place, its length,
  its pitch and its velocity, is still there to be dragged and selected, and
  is skipped by every walk that turns a clip into sound, so it is not the
  same as deleting and undoing. It draws hollow rather than dimmed, because
  this roll already paints velocity as colour and a washed-out note means
  something else here. A flag rather than velocity zero, so unmuting gives
  back the velocity the note had.

  Split cuts at the playhead and only the notes the playhead stands inside,
  never a sliver: a selection the playhead misses is left where it is rather
  than being cut somewhere it did not ask for.

- **The MIDI editor can be driven from the keyboard, start to finish.** The
  owner: "a system where-in proper arrow controls can be used to create and
  edit midi clips intuitively, and from the beginning to the end."

  **Enter writes a note** at the playhead on the pitch cursor, and walks the
  playhead past it, so Enter pressed four times writes four notes in a row
  rather than four in a stack, and a melody is a run of Enter with the odd Up
  or Down in it. The roll has a playhead for the time axis and now a cursor
  for the pitch axis, which the mouse never needed because a hand carries
  both at once.

  **The bare arrows do one of two jobs and the selection decides which**,
  the grammar the rest of the app already uses. With a note in hand they
  edit it: a step along, a semitone up, and the pitch cursor comes with it so
  the next note starts where the hand left off. With nothing in hand they
  drive: the playhead along the clip, the cursor up the keyboard, sounding
  each note it lands on.

  **Shift splits by axis.** Left and right are the note's length; up and down
  take the note above or below into the selection, which is how a chord is
  built a note at a time: a candidate has to overlap the selection in time,
  so it climbs the chord rather than reaching across the bar. **Ctrl/Cmd+Alt
  with up and down** moves the selection an octave. **Escape lets go** of the
  selection, and closes the editor on the second press, which is the order
  somebody in trouble presses it in anyway, and it is what puts the bare
  arrows back on the playhead without reaching for the mouse.

- **Shift+H humanizes the selection in the MIDI editor.** The chord hangs a
  small menu under the pointer with two amounts (TIMING, a percentage of one grid
  step, and VELOCITY, a percentage of the velocity each note already has)
  and HUMANIZE (or Enter) applies them to the selected notes. Two
  independent amounts because the two are not the same wish: a drum part
  often wants its velocities moved and its timing left where it is, and a
  held pad the reverse. Velocity moves by a proportion rather than a fixed
  number so the part keeps its shape: a ghost note shifts by a few and an
  accent by a dozen, and nothing is ever pushed to silence. The amounts are
  remembered, so the second part costs one press. Notes stay wholly
  inside their clip.

  **The shift is not a preference.** Bare H is held for the shortcut card and
  the editor has a card of its own, so taking the letter outright cost the one
  surface whose card is hardest to do without. Cmd+H was the obvious sidestep
  and is not available: winit installs the standard macOS application menu,
  whose Hide item carries Cmd+H (and Cmd+Shift+H for Hide Others), and AppKit
  eats both before the app sees a key, so bound there, Humanize would have
  hidden Tangent.

- **Insert Track, on a channel's right-click.** A new empty track opens
  directly under the one clicked and the rows beneath shift down a place.
  A row opened inside a folder joins that folder, or the group would be
  split in two by a track that is not in it. The owner: "Need a rightclick
  option for channels to insert track."

- **BYPASS FX in the insert menu.** Right-click a loaded insert bay, on the
  desk or in the timeline's header, and the first row is BYPASS FX: the
  plugin stays loaded, keeps its settings and its window, and the signal
  goes past it. Over a bay that is already out the same row reads UNBYPASS
  FX, so the row always says what the press will do. Shift+click on the chip
  did this already and nothing on screen said so. The bay's chip wears the
  amber tab either way. The owner: "add 'Bypass FX' option to the right click
  menu for inserts."

- **A track listens to several named MIDI devices, on a channel of its own.**
  The input chip's MIDI row opens a dialog with a checkbox per device and a
  channel chooser, for that track. Tick two keyboards and the track hears both;
  tick none and it hears every device, including one plugged in later, which is
  the default and what the app has always done. The channel is one of the
  sixteen or all of them.

  This was a lie the app had been telling. Each track's chip already named a
  MIDI port, and the app opened exactly ONE device app-wide, chosen from a
  global menu row nothing on the desk mentioned, so two tracks could not have
  separate keyboards and every chip said they could. The owner found it while
  setting up two tracks with two inputs.

  The chip says what is true now, in a grammar that names the axis it is
  talking about: `MIDI: ALL DEVICES`, `MIDI: Keystation 88 +1 CH 3`. A device a
  track remembers and which is not plugged in right now is kept, not dropped
  (unplugging a keyboard and plugging it back in costs nothing), and it is
  marked, on the chip with a `!` and in the dialog with "not connected".

  Two tracks may ask for the same device and channel, and both hear it. That is
  what this desk already does everywhere else (two armed instruments both
  sound), and layering a piano and a pad from one keyboard is the ordinary
  reason to want it.

  Recording follows the same rule, so a take of two players lands each
  performance on its own track rather than both on both. The global menu's
  "Select MIDI Input..." now opens the selected track's dialog; there is no
  app-wide MIDI device left to pick.

  Settings written by this build stay readable by an older one: `channel_inputs`
  still says what it always said, and the sets ride beside it in an additive key.

- **The MIDI editor has a pedal lane.** The chip marked PED, at the pencil's
  shoulder, swaps the lane along the foot of the editor between velocity and
  the sustain pedal. The pedal is drawn as a step: a shelf where it is held
  and a riser where it moves, never a slope, because a controller holds its
  value and a line sloping between two edges would be a picture of an
  interpolation nothing performs. The shelf is filled down to the floor, so
  where the damper is down reads from across the room, and it takes its colour
  from the same gradient a velocity does: a half-pedal is a different colour
  from a full one.

  Drag in the lane to paint. The height of the hand is the value, which is what
  lets one gesture do both jobs: along the top it paints the damper down,
  along the floor it paints it up, because zero is a pedal nobody is pressing.
  There is no separate erase to go and find. A press that does not travel
  writes a single edge, "from here, this much". Strokes are bounded: what stood
  after the span goes on standing, so painting four beats of damper in the
  middle of a pedalled passage does not silently lift the pedal for the rest of
  the clip.

  One lane and not a new row: the editor is already a tool strip, a ruler, the
  roll and a lane, and every row added to that stack is height the roll loses.
  Sostenuto and soft ride above the damper as thin rails (a part that uses
  them should not look like a part that does not), and they survive an edit
  untouched.

- **A looping clip shows where every pass ends, audio and MIDI alike.** The
  clip stops being a rectangle: a triangular divot is taken out of its top and
  bottom edge at each turn of the loop, so a one-bar part dragged over four
  bars reads as four bars of one part instead of as one green box, and a
  four-bar drum take pulled over sixteen shows its four passes through the
  waveform. Nothing is drawn ON the clip (the material is taken AWAY), so it
  says the same thing on a near-black theme and on a bone one with no colour to
  choose. The bite is a tenth of the lane deep, which is the band the waveform
  and the note miniature are both kept clear of, and it is dropped when a pass
  is too narrow to place: a hundred notches a point apart is a moiré claiming
  to be structure, not a picture of a loop. An empty MIDI item that has not
  been played into yet gets it too, which is exactly when it is most useful.

- **Glue flattens a clip that repeats, on its own.** Gluing used to need two
  clips; one is enough now if it loops, and it welds that clip's own repeats
  into it. A one-bar part playing C D E F, stretched until it shows C D E F /
  C D, becomes a clip holding those six notes as one pass with no seam: what
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
  becomes four repeats of one bar; lengthening stops at the clip's own end,
  because the clip's length belongs to the timeline (see "The loop bar
  stops at the clip instead of stretching it"). It follows the SNAP
  chip like everything else in the editor, so a loop lands on the grid the
  notes in it landed on, and it will not be pulled shorter than one grid line.
  A note that ends on the turn keeps its own edge: notes win on the roll, and
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
  off. The whole thing takes no height from the roll (it lives in room the
  strip was not using), and steps aside on an editor too narrow to hold both
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
  hidden, so the input stream is always open and the log never once said so.

- **`TANGENT_ACTIVITY=1` asks macOS to treat the process as latency-critical.**
  Opt-in and off by default, so behaviour on every macOS version is unchanged
  unless somebody is deliberately testing it.

- **Stems export asks which stems: dry, or processed.** The export dialog's
  SOURCE row is joined by a STEMS row with two chips. *Dry* writes each lane
  before its own inserts, for a remix somewhere else, and is what the STEMS
  row comes up on when SOURCE moves to stems; pick *Processed* and it stays
  picked while stems stay on. *Processed* writes each lane through its own
  insert rack: the mix's own sound, one file per lane. The rule is the same
  at every strip: a dry stem is that
  strip's own bays taken out and nothing else's, so a group's dry stem is the
  group before its bus compressor with its children exactly as they reach the
  mix. The two write to different folders (`stems/` and `stems-dry/`), so a
  second export cannot overwrite the first with different-sounding files under
  identical names. Choosing dry does not move one sample of the master.

- **Drag to the edge and the view follows.** Holding a drag against the edge
  of a view now scrolls it that way, on all three surfaces that needed it: the
  MIDI editor's roll in all four directions, the arrangement's lanes sideways
  along the song and downward through the track stack, and the mixer's desk
  across its channels. Things can be moved beyond what is currently on screen
  without resizing or zooming out first: a note to a pitch off the top, a
  clip to the ninth track, a channel past the eighth column. One rate law
  serves all three, measured in points per SECOND rather than per frame, so
  the same hold covers the same ground whether the window is painting at 240
  frames a second or ticking over at twenty; it ramps from a slow floor at the
  zone's inner lip to a ceiling at the rail and holds that ceiling however far
  past the edge the hand goes. The gesture asks for its own frames, so it
  keeps moving while the hand is perfectly still. View gestures (the alt-drag
  pan, the navigator) are exempt: a view that auto-scrolled while being panned
  would accelerate away from the hand steering it.

- **Nothing waits in silence any more.** Every operation that used to freeze
  the window with no explanation now says what it is doing. A survey traced
  seventy-six of them. The ones that can be counted show a real meter
  (loading a project counts its plugins, a pasted track counts its bays, an
  export counts its racks), and the ones that cannot name what they are
  waiting on instead, by the plugin's own name where there is one. The overlay
  obeys the rule the frozen frame forces on it: it is painted on the frame
  BEFORE the blocking call, because a call that blocks produces no frames to
  paint in. It waits a moment before appearing, so an operation that turns out
  to be quick never flashes anything, and it holds a minimum time so a fast one
  does not blink. It also gives up on an operation that never returns, rather
  than owning the screen forever.
- **Messages moved to the top-left corner and stopped interrupting.** A themed
  notification that lasts three to five seconds and can be clicked away.
  Messages that are *conditions* (a missing MIDI port, a device that
  disappeared) stand until the condition clears rather than expiring, and a
  dismissal sticks without silencing the next genuine occurrence.
- **Silverface**, the nineteenth theme: the faceplate of the Dual Showman
  Reverb, with the red jewel pilot lamp as the master.

### Changed

- **Every launch opens the default session.** Tangent no longer reopens the
  project that was on screen when it quit; it stands on a fresh copy of the
  default template every time, and the last session is a File > Open away.
  A scratch stays on disk for that panel if it holds an audio recording, a
  drum kit built in it or anything Tangent did not put there, or was saved
  with clips in it; one this session created that holds none of them is
  deleted, with its imported copies and plugin settings, when you answer
  Don't Save on New, Open or Quit. The
  owner: "make it so every launch opens the default session, and doesn't
  open the previously open session."

- **A project opens in the time its audio takes to read, not in the time
  forty child processes take to spawn.** Every take used to go out to
  `afconvert` (ffmpeg elsewhere), through a temporary WAV and back in, one
  file per frame on the UI thread; and once the notes' 250 ms deadline fired,
  the reload decoded every file still waiting in a single frame and the
  warm-up queue then decoded them all a second time. A take is now read
  natively when its header is the plain PCM the recorder writes at the
  engine's own rate (anything else, a resampled file, an MP3, a stream with
  no length, a take cut short, still goes to the child), each file is decoded
  exactly once, refusals are remembered instead of re-spawned on every edit,
  and the work runs on a small crew of decode threads while the window keeps
  painting. Measured on forty-five one-minute takes: 1.7 s of serial child
  processes became 0.17 s of wall time off the UI thread. A missing file is
  no longer asked for again on every zoom step.

- **Zoom and pan draw from a picture built once.** A clip's waveform was
  recomputed from its samples whenever the view moved more than a twentieth
  of the window, and that recompute also rebuilt the engine's schedule and
  waited on the audio thread twice, which is the hitch and the smear a zoom
  used to carry. Each decoded file now carries a min/max pyramid, built once
  beside the decode, and the timeline reads it at any zoom directly: no
  deadband, no sample walk, no engine push. The waveform is one shape per
  clip instead of one per pixel column. The panel also lays itself out once
  per frame instead of once per clip (it had been rebuilding the whole layout
  three times for every clip in the document, on screen or not), computes
  its colours once per lane, cuts a label with one text layout instead of
  one per candidate length, and builds accessibility nodes only on a frame
  that installs them.

- **A stretched clip renders once the hand stops.** Dragging a clip's edge
  used to render the whole stretched file on the UI thread for every ratio
  the pointer passed through. The render is now a job for the decode crew,
  started when the ratio settles; until it lands the clip is silent and
  drawn from a stand-in, and the busy cover says so. A ratio that would
  render past the decoder's ceiling plays the clip unstretched with a notice
  instead of allocating gigabytes.

- **The window rests.** An idle desk repainted twenty to thirty times a
  second forever. Now every animator (the transport, live meters, a
  notification, the busy cover, the splash, a drag, the camera, the
  analyzer with signal) asks for its own frames, and a still window asks for
  none. `IVORY_PACE` still forces the old cadence; `IVORY_PACE_PROBE=1`
  shows the frame rate; `TANGENT_FRAME_LOG=1` logs where each second of
  frames went, section by section.

- **Settings writes coalesce.** A fader drag, a wheel notch or a held key
  used to write the whole settings file synchronously on every step. Every
  repeating gesture now owes one write, paid when the hand stops and always
  paid before quit, a project switch, or the window losing focus. Explicit
  saves, the project path and the kit sidecar still write at once.

- **Undo holds the document, not its printout.** Each undo step stored the
  whole document as pretty-printed JSON and re-parsed it to travel back. A
  step is now the structure and its words, compared exactly, so a hundred
  steps of a real project cost a tenth of what they did and an open no
  longer serialises the document twice to start the history. Opening writes
  the settings once, and undo and redo do not write them at all.

- **Documents and settings stop carrying thirteen thousand empty seats.**
  The desk's insert map was written whole, one entry per possible bay, which
  was most of a small project's bytes and the settings file's. A document
  now writes its strips' bays as a head and everything past them as a
  sparse tail (`arrangement_version` 7, `settings_version` 21). Every
  earlier shape still opens. A build from before this change that opens a
  new document keeps the strips' bays and drops the far seats on its own
  save, which is the same loss any earlier format step accepted.

- **Only an ARMED track hears the keyboard.** Selecting a MIDI track used to
  open it to live notes as well, on the reasoning that trying an instrument out
  should not begin by arming a track. What that cost in the room was arm
  reading as a lie: an armed track sitting silent while an unarmed one sounded
  and metered, because selection is quiet and sticky: every click on a header
  sets it, and nothing on the strip says it also opens a MIDI gate. The lane
  answering the keyboard was not the lane wearing the R. Arm is now the whole
  of it, and the price is paid knowingly: auditioning an instrument means
  arming its track first.

  The MIDI editor is untouched and is still the road that needs no arm: while
  it is open, its clip's lane is the only lane that hears, armed or not.

- **Every clip is a loop from the moment it is made, audio as well as MIDI.**
  A drawn MIDI item and an imported `.mid` were already pinned at their own
  length and looping from birth; a recorded take, a split's tail and every
  audio clip in the app were not, so the same right-edge drag laid down another
  pass on one and stopped at the end of the file on the next. It now lays down
  another pass on all of them. A loop as long as the clip it sits on is one
  pass and sounds like nothing at all (no repeat, no seam, no change to any
  session), so what this actually buys is that pulling any clip's right edge
  repeats it, which is the reason anybody records four bars of a groove. The
  loop can be turned off per clip from the clip's own right-click menu.

  An imported audio file is the one clip that cannot be given its loop where it
  is made, because its length is the decoder's fact and not the app's: it gets
  both numbers together the first time the file is decoded. The consequence for
  a project saved before this, worth stating plainly: an audio clip still
  carrying the whole-file length gains a loop the first time it is opened here,
  and nothing about it sounds different, but its right edge now pulls out into
  repeats. A clip whose loop has been turned off keeps it off: that door only
  opens once.

- **Right-click a clip and untick Loop to stop it repeating.** The clip menu
  has a Loop row with a checkmark beside it when the clip repeats: press it and
  the loop goes, press it again and it comes back over the clip's whole length.
  Turning it off leaves the clip exactly where and as long as it was: a clip
  pulled out to four passes and then unlooped stays four bars on the timeline
  and plays its material once, because the length is the timeline's business
  and this row has no say in it. Like every other row in that menu it acts on
  the whole selection, so a marquee over eight takes and a press on one of them
  unloops all eight. Choosing how long a pass is remains the MIDI editor's loop
  bar; this row is only the on and the off.

- **Glue makes one clip that loops over the whole of what was glued.** It used
  to be the escape hatch from the loop (the way to turn several repeating
  items into one long pass), and it is now the same rule as everything else:
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
  learn. Cmd is the platform's command finger (Ctrl off macOS), and it is
  already what the editor's rubber band uses. The item is still born pinned at
  the length it was drawn and looping from that moment, notes or no notes, so
  the first pull of its right edge lays down a second pass.

- **Alt-click a record button to arm that track and nothing else; shift-click to
  build a set.** The one-lane gesture was on shift, which is the multi-select
  key everywhere else on this desk and therefore meant the opposite of what the
  rest of the app teaches. Alt takes it over, and it ignores the strip selection
  the way no other control does: spreading it would arm the very tracks the
  gesture exists to stand down. Shift touches only the lane under it, leaving
  the strip selection out of it, and flips it (see "Both arm modifiers can
  undo themselves"); a plain click toggles the lane and carries the selected
  strips with it. Both views, since
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
  question (save before closing, flatten the tempo, the three destructive
  confirmations) is untouched, as are the file and folder pickers and the
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

- **Soloing a track inside a folder no longer mutes the whole desk.** The
  rule that decides what a solo leaves audible walked only UP the folder
  tree: a channel is heard if it, or a group it sits in, is the thing
  soloed. Nothing asked whether a group had a soloed track INSIDE it, so
  the folder itself fell through as not-heard, its fold took a gain of zero,
  and because a foldered lane reaches the master through its group and
  nowhere else, the soloed track silenced itself along with everything. A
  group is heard when anything in it is soloed; its own mute still wins,
  because nothing overrides a group being switched off. The owner: "track 7s
  solo is a mute for everything."

- **A drum kit inside a folder answers to the group.** The pad pass asked
  the flat question, which knows nothing about folders, so a kit on a lane
  inside a group went silent whenever anything else was soloed (including
  when its own group was the thing soloed), and a mute on the group never
  reached it. Same rule as every other lane now. Found by the test for the
  solo above rather than by anybody playing it.

- **The four-input cap is gone: every audio track can take an input.** The
  desk opened at most four capture inputs, handed out to audio channels in
  index order, so a fifth audio track could be bound, armed, metered and
  could never receive a sample: it simply read its input's name and stayed
  silent. The cap was a leftover from a band with four input strips; every
  channel has had its own input chip for a long time. There is no cap now,
  not even the desk's width: as many inputs open as there are audio tracks
  bound to one. It costs nothing: the capture rings already carry
  whole device frames, so a pick is a map from the stream's columns to a
  strip rather than a thing that sizes a buffer. The owner, on his fifth
  audio track: "I do not think the input cap is a good thing at all and it
  at least needs to be the size of the tracks."

- **A removed track takes its input with it.** Deleting a track cleared its
  kind, its name, its colour, its clips and its bays and left the interface
  channel it listened to sitting in `channel_inputs`: a state the desk's
  own model ("the input IS the kind") says cannot exist, and one nothing
  could repair by hand. The slot then drew as an empty plus; pressing it
  made a track that wore switches, whose R toggled and which could never
  monitor or record, because the kind was still none. Desks already carrying
  one are healed as they load: the orphan binding goes, rebinding is one
  press. The owner: "track 8 in a folder no longer works for record arming
  and monitoring ... it worked for another track then it now won't work at
  all."

- **Paste Track no longer destroys the track you aim it at.** The row was
  Paste Track (overwrite) and it took the lane it landed on, clips, bays and
  all. It said so on the label, which is the least a row like that can do,
  but a paste that costs a track is a paste people check twice before using.
  It is now Paste Track Below: the copy arrives on a row opened under the one
  clicked and everything beneath shifts down. The owner: "make paste track
  actually non-destructive, change it from the overwrite to add below
  inbetween selected track and track below it."

  The mixer's Cmd+V went the same way, and it had never had the guard the
  arrangement's has: aimed at a live track it overwrote silently, with no
  label anywhere to warn. Both surfaces now go through one decision: an
  empty slot receives the copy, a live track keeps its life and the copy
  lands under it.

- **The "That is a MIDI part, not a sample" message is gone.** It answered a
  question nobody asked, on a DRAG (as often a slip of the hand as an
  intention), and a drag that publishes nothing has already said so by not
  moving anything. The refusal stands; the sentence does not. The owner: "I'm
  not [sure] that message is actually useful in any way."

- **An explanatory message ages out on its own.** The app's helper answers
  (the ones a press asks for and nothing re-checks) borrowed the machinery
  built for CONDITIONS, which have no clock because they end when they stop
  being true. Nothing was ever going to lower "No MIDI track on the desk", so
  it sat in the corner until it was clicked, which is one more thing to do in
  the middle of whatever it interrupted. They leave after ten seconds now, and
  asking again restarts the clock. A real condition still stands until it
  clears: a device that has genuinely vanished is the one message that must
  not quietly leave. The owner: "Make the helper notifs also fade on their own
  after a set interval. These interfere with testing."

- **Enter no longer starts a take.** It began one from any surface, on the
  argument that the key which starts a performance must not be the one that
  ends it. But a bare, enormous, centre-of-the-keyboard key that rolls tape
  is a take started by a stray press, and the MIDI editor now wants Enter
  for writing a note. No key starts or ends a take now: the cassette's own
  button does both. Cmd/Ctrl+R is the arrangement's red button: it counts
  in and records the armed tracks, cancels a count-in, or stops a running
  record, and it works while a take is running. The owner: "Remove enter
  for record -- we already have command r."

- **The Humanize menu opens under the pointer, and is bigger.** It opened
  centred on the roll and wore the swing menu's rows, which are a list to pick
  from; these are tracks to drag a percentage along, and they have to be tall
  enough to aim at and wide enough that a point of travel is not four per
  cent.

- **A parked desk stops running its effect chains, so an idle project is
  silent.** Every loaded plugin ran every block, whatever the transport was
  doing, deliberately, so that a reverb tail is not cut in half at every gap
  or chopped the instant somebody presses stop. The cost of that was reckoned
  as "plugins process silence", which is free only for a plugin that is
  transparent on silence. An amp simulation is not: fed digital silence it
  emits the hiss and hum it exists to simulate. With an amp sim on a bass DI
  and a compressor behind it lifting that noise, a project sitting untouched
  with nothing armed and nothing playing put about -40 dBFS of buzz on the
  master for as long as it was open. The owner heard it three times before it
  had a name: "a chorusy buzz constant at around -40dB on the master meter."

  A lane's chain now stops after eight seconds of stopped transport, which is
  past the tail of any reverb anybody mixes with, and wakes instantly on play,
  on arm, on a note, on an audition. **A lane carrying an instrument never
  parks at all**: an effect fed nothing has nothing of its own to lose, but a
  voice may be holding a note, so the rule asks each plugin's own box whether
  it generates and stands down for the whole lane if any of them does. It also
  gives the CPU back: an idle desk of twenty plugins was running all of them.

- **A preset reaches the instrument, not its first sixteen values.** The
  queue a knob crosses to the audio thread held sixteen points a bay, and
  loading a preset is one point per knob all at once, so Tangent Analog,
  which publishes sixty-one, had sixteen of them applied and forty-five left
  standing at the patch before, and Tangent EQ lost nine of its twenty-five.
  Nothing showed it: the face, the host's ledger and a saved project all
  carry the full preset, because those are written on the host side and never
  cross that queue. Only the SOUND was a mongrel of two patches, which on a
  synth can drone. The owner found it as "presets require manual updates of
  values to register": nudging a knob afterwards sends a single point, and a
  single point always fitted.

  The cap was sized for automation curves, where a point that does not fit is
  genuinely re-sent by the next block; the knob road is drained destructively
  and had no such luck. It is now the widest built-in's own complement, and
  the drain folds repeats of one knob together so a bay can never be asked
  for more parameters than the unit in it has.

- **A trimmed MIDI loop fills its tail.** Every unroll counted passes as the
  clip's length over its loop, while placing each note at `at_s - offset_s`,
  so a clip trimmed from its front starts mid-loop and the walk ran out
  exactly the offset before the clip's end. Trim half a loop off the front
  and precisely the last half held no notes, in the sound, in the export and
  in the drawing. The offset is part of the span to cover. Audio loops were
  never affected: their passes place the clip's own start and hand the file
  offset to the player. The owner: "midi loops that are clipped leave a
  latter half that is empty."

- **A trimmed clip is notched where its loop actually turns over.** The
  divots along a looping clip's edges were laid a whole loop apart from the
  clip's LEFT EDGE, which is right only for a clip that begins on a
  boundary. Trim a quarter of a bar off the front (the same trim behind the
  empty tail above, and the same cause) and every notch sat a quarter of a
  bar late, over the middle of a pass, on the one mark whose whole job is to
  say where the loop restarts. The first pass of a trimmed clip is short by
  however far into the loop it starts; the rest follow a loop apart from
  there. A clip SHORTER than its own loop is notched too now, once, if its
  offset leaves a boundary inside it: that clip does turn over, and the old
  test called it unlooped and drew it flat.

  An AUDIO clip's notches do not move, because the same number means
  something else there: its trim chooses which seconds of the file play and
  every pass gets the same ones, so an audio loop turns over a whole loop in
  however far it has been trimmed. The seams read the clip's kind now.

- **Phosphor's and Nocturne's light mode is a light mode.** Those two are the
  only themes whose light ground is a near-black (it is what makes them what
  they are), so the piano has to derive a pale key for them and then put the
  keyboard the right way up out of it, which it has done since the last
  release. The page did not follow. It went on reading the near-black the theme
  declares, so light mode gave a bone keyboard butted against a black theory
  view with a visible seam between them, and the mode toggle changed nothing
  about the page at all: the same ground, taking the same pale ink, in both
  modes. Every other theme's page and keyboard are one continuous colour from
  the top of the circle of fifths to the bottom of the last natural, and those
  two are now as well. The page asks the piano what it painted rather than
  working the same answer out a second time from the same roles, so there is
  one decision and nothing for a second one to drift from. Nothing moves on the
  other sixteen themes: their naturals ARE their declared ground, which is
  exactly what the page used to return.

- **Phosphor's and Nocturne's dark keyboards stopped floating above their own
  page.** Those two are the only themes that ever named their own dark keys,
  and they named them back when nothing lifted a dark keyboard off a dark
  ground by itself. The lift has walked in both modes for a release now, so the
  two overrides had no work left to do and were doing damage instead: they held
  the naturals at 0.099 and 0.093 luminance over pages of 0.006 and 0.002,
  while all sixteen other themes paint their dark naturals exactly ON the page
  at 0.010 to 0.014. A keyboard is supposed to BE the page it sits on, with
  only the sharps standing off it. Those two were a mid slab hanging above the
  theory view with bone sharps on top, which is why the dark mode read bright
  and the seam between the band and the page was visible. They name their
  ground and nothing else now, like everybody else.
- **Opening a project with instruments on it took the better part of a
  minute, and almost all of it was one wait nobody was waiting for.** Every
  hosted instrument was warmed up before it was allowed into its rack, and
  the warm-up held a five second floor: five seconds of deliberate wall
  clock, per bay, one bay at a time, on the thread that draws the window. A
  nine channel session carrying seven of the same sampled Rhodes measured
  40.3 seconds to open, 35.0 of them inside that floor.

  The floor was costed for a different job. It exists so the recorder does
  not arm on a sampled piano that is still reading its samples off disk and
  record a silent take, and it was paid once, when the Recorder view opened,
  behind a view somebody had just asked for. Filling a rack is not that job:
  loading a plug-in into a bay arms nothing. So a bay load warms up under its
  own policy now, keeping every part of the test that decides anything and
  dropping the wait that follows it. The same seven bays come up in 6.8
  seconds, each one still proved to make a sound before it is handed over,
  and each one still handed over silent. What is left is very nearly all the
  plug-in's own: about a fifth of a second to construct each instance and
  another third to restore its preset.

  The launch splash covers the whole of it again as a result. It gives up
  after twelve seconds by design, so a session like this one used to lose its
  cover a third of the way in and leave a half drawn window sitting there for
  the rest.

- **A take that had finished decoding could sit behind the desk.** The window
  rests between animations now, so everything the app does after a frame has to
  ask for the frame that shows it. Five roads did not: a decoded take arriving,
  the last redraw of a zoom, a decoder's complaint, the last plug-in of a rack
  landing, and a click sound that would not load. Each asks now, and only when
  it really changed something, so a still window still rests.

- **"Loading audio" could take the desk hostage for the rest of the session.**
  Deleting the audio clips of a project whose takes were still decoding
  answered the loading debt while files nothing referenced any more were still
  in the queue; they came back, were dropped as unwanted, and nothing was left
  to lower the cover. Both quiet ends of the warm-up settle the meter now.

- **The last plug-in in a rack drew its face a frame late**, or not until
  something else asked for a frame: the loader asked for a repaint only while
  work was outstanding, and the bay that finishes the walk is the one where
  nothing is.

- **The EQ's end frequency labels stay on the glass.** "20k" was centred on a
  grid line that sits exactly on the right edge, so half the word hung past the
  panel and egui quietly clipped the "k" off. The half that stayed was worse
  than the half that went: a word only partly on the glass takes its contrast
  from the ground behind it instead, which put that label at 1.57:1 on the
  Sandstone theme against a floor of 1.6. Labels are measured and held inside
  the ends now, in whichever of the three shipped fonts is drawing them.

- **A seventeen-digit fraction survives a round trip.** The JSON reader
  rounded values such as `0.9999999999999999` to `1.0`, so a document with
  one of them was never equal to itself after a save, which is the class of
  fault where undo appears to do nothing and redo is gone. The reader is
  exact now.

- **A quit within the write delay lost the last fader.** The debounced
  settings write was never flushed on exit; it is drained on every road out.

- **A document second past the timeline's range no longer overflows it**, a
  looping MIDI clip is capped by the notes it would emit as well as its
  passes, and a seat named past the desk by a document cannot size the desk.

- **The sustain pedal is recorded into the clip and played back.** A part
  performed with the damper down came back dry: the take kept only the instants
  the KEYS came up, the clip had nowhere to say the pedal was still down, and
  playback dutifully cut every note at its key-up. All three pedals (sustain,
  sostenuto and soft) are now written into the clip as they were played, at
  their own values rather than as a switch, so half-pedalling survives; a pedal
  already down when the red button is pressed is restated at the take's start,
  so it is not released later by an event with no cause; and the clip's pedal
  reaches the instrument at playback, whether that is the built-in keyboard,
  the DX7, the analog, or a VST3 in a bay.

  The notes themselves are untouched. Stretching them to the pedal lift is the
  obvious cheap fix and it is the wrong one twice over: it throws away
  half-pedalling, and it destroys the difference between a chord rolled under
  the pedal and a chord held, which is a difference a pianist can hear.

  The pedal travels with the clip everywhere the notes do: split (the tail
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
  behind it is a release budget (eight notes at a tenth of a millisecond is a
  third of a 2.7 ms block), but `cargo test` builds unoptimised, so the figure
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
  passed close up behind it, which is what a single track has always done and
  what the arithmetic was written to match.

- **Phosphor's and Nocturne's keyboards were upside down.** Every other theme
  paints pale naturals and dark sharps in light mode, and inverts in dark mode.
  Those two did the opposite in each (light mode gave them a black keyboard and
  dark mode a pale one) because a theme's naturals are pinned to its light
  ground, and being dark-designed is exactly what those two are. Measured across
  the set, the sixteen others put their light-mode naturals between 0.61 and
  0.97 luminance; those two sat at 0.011 and 0.004. The polarity is decided by
  the piano now rather than inherited from the ground: the naturals are the pale
  key in light mode and the dark key in dark mode, on every theme. The two
  colours themselves are untouched; only which key wears which.

- **A held DX7 patch rings for the whole press.** The note stopped as soon as
  the pointer moved six points, which a hand holding a button still does, so it
  cut out part-way through a press that had not ended. The press is the gate
  now, as on a piano key. Dragging a patch into the staged bank sounds it on the
  way, which is the patch being chosen.

- **A recorded MIDI take now repeats when you pull its right edge, like a drawn
  one.** A MIDI item drawn on a lane has been a loop from birth (drag its edge
  and it lays down another pass), but a recorded take was born with no loop, so
  the same drag on the same kind of item revealed empty time instead. The two
  ways of getting a part onto a lane behaved differently at the one gesture
  everybody uses on a part they like. The take's own length is its loop now.
  Audio takes are untouched: there the file's remaining length is the limit, and
  repeating it would be a different feature.

- **A folder dragged past its own children turned into one of them.** A folder
  carries its rows, so dropping it below them names a target that is already
  inside the block being carried. The move itself is then a no-op (the block
  comes out and goes back where it was), but the membership decision that runs
  first is not: it asks which folder the target row belongs to, gets the dragged
  folder back, and writes that onto the folder itself. It stopped being a folder,
  became a child of itself, the group flattened around it, and the row that had
  been track 5 came back reading as track 6. That drop is refused whole now,
  since nothing about it could ever have been meaningful.

- **Phosphor's and Nocturne's pale piano keys stopped glaring.** The lift that
  makes those two themes' keyboards visible walked the pale key away from the
  naturals until the two were far enough apart, and toward pure white, which on
  exactly those themes it always reached, because their dark naturals sit at
  about 0.095 luminance and nothing can get far enough from that. So the walk
  spent every step, arrived at the screen's limit, and still fell short of the
  line it was aiming at. The pale key now stops at the bone this desk already
  paints with everywhere else: dimmer than any other theme's pale key, still
  more than five times the contrast of its own naturals, and no longer the
  brightest thing on screen. Themes that name their own pale key keep it.

- **The arrangement's little menus ignored the theme.** The clip menu, the FX
  menu, the input menu and the bay drag chip all painted a hardcoded cream with
  hardcoded near-black ink (the shipped theme's own colours, written out as
  literals), so on the other fifteen themes they were a pale card on a dark desk,
  and on the dark ones they glared. They derive their surface now, the way every
  other painted panel here does: the paper comes from the theme's face, moves
  itself when that face sits where neither ink would read, and the ink is a
  property of the paper rather than a colour borrowed from somewhere else.

- **Both arm modifiers can undo themselves.** Shift could add a lane to the
  armed set and not take one back out, and alt could make a lane the only armed
  one and not stand it down, so either gesture had to be abandoned mid-thought
  to be reversed. Shift now flips whatever it crosses and alt on the lane that is
  already alone disarms everything. A modified arm also touches one lane rather
  than the whole strip selection, which is what both modifiers are for; a plain
  press still carries the selection.

- **The master's FX bays had no right-click menu in the arrangement.** Every
  other rack on that view offers Delete FX and Replace FX; the master's six bays
  offered nothing, because the right-click matched the channel headers' bays
  alone and never the bus row's. A plugin could go into the master there and
  only come out by going to the mixer for it. Both of the master's racks answer
  now: its first three slots and its last three are seated far apart, so the
  menu resolves the seat the same way the left-click always has and cannot open
  pointed at a neighbour's plugin.

- **Analog could take the whole app down from the audio thread.** Letting go of
  a key in mono or unison falls back onto a key still held and, with
  multi-trigger on, re-attacks the whole voice stack. That road was the one
  place a voice was made to sound without being woken first, and waking is where
  a voice takes its filter lane, so any voice past the stack's old width, after
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
  button went past the bottom and the fix was to grow the window, which is not
  a rule, and the next row put it back off the edge, leaving a dialog whose only
  way out was Esc. The button is reserved at the foot now and the body scrolls
  above it, the same shape the Export dialog already uses, so a row added later
  can push the body into a scroll and can no longer push the way out off the
  screen. The reservation is measured against the real button under the real
  theme by a test, rather than being another number somebody has to remember.

- **The metronome's sound row said the same thing twice, and a chosen click
  could go missing.** With nothing chosen the two controls read "built-in" and
  "default": one naming the click you were on, one naming the click you would
  go back to, which are the same click, so neither said what pressing it would
  do. They are **Default** and **Custom** now, in that order, and Custom wears
  the chosen file's own name once there is one. And the file is now KEPT rather
  than pointed at: choosing a click copies it into Tangent's own folder and the
  setting names the copy, so renaming a folder or emptying a Downloads
  directory no longer drops the metronome silently back to the shipped click.
  One slot, replaced each time: this is a preference, not a library.

- **The DX7 picker opens lit on the patch the strip is playing.** It had been
  lighting the row for exactly one frame: the host's answer to the open, the
  same call that fills the list after a load, cleared the selection on the
  argument that belongs only to a freshly loaded bank. The row that is lit is
  now the row the strip's setting names, in both cases.

- **An audio system that opens and then plays nothing is set aside instead of
  being tried for ever.** A driver that refuses to open is easy: the open
  fails, Tangent falls back, and the band says why. The expensive case is a
  driver that says yes and does nothing: the stream is "running", the watchdog
  declares it dead two seconds later, the rebuild opens the same driver, it says
  yes again, and the app oscillates between silence and rebuilding for as long
  as anybody leaves it there. Nothing in that loop is an error, so nothing in it
  ever fell back. Windows has a stock example: Realtek ships a generic ASIO
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
  It marks what it is standing in for (the mark keeps the reprobe watching for
  the interface and carries the "back on ASIO" line when it returns), and it was
  also, wrongly, being asked whether a fallback was allowed at all. It cannot
  answer that: the mark is cleared in one place, and only once the chosen system
  is back, which the stand-in itself rules out. So it stayed set. Drop the
  engine for any reason (an output that went quiet, a band closed and reopened)
  and the rebuild restored ASIO, failed on it, found the mark still set,
  refused to fall back a second time, and spent its retries on hardware that was
  not there. Permanent silence, on a machine where choosing WASAPI by hand
  worked perfectly. The decision to fall back no longer consults the mark; the
  toast is still shown once per stand-in rather than once per rebuild.

- **A part recorded on an armed lane played back without the plugins it was
  recorded through.** There is one insert rack per channel and it is clocked
  once a block, so when the control room is monitoring an input it claims that
  channel's rack and the timeline's own pass stands aside. The claim was made
  for *any* armed channel, monitoring or not, and monitoring ships off, so the
  ordinary state of an armed channel was a rack claimed by a room that was not
  listening, whose entire output was then removed from the speakers again by
  the monitor gate. The lane stood aside for nothing and its audio went to the
  bus dry: record a part, leave the lane armed, press play, and the compressor
  or amp you recorded through is not on it. Nothing platform-specific: every
  desk did it. The rack now goes to whichever path can use it. If the room is
  listening it keeps it and takes the lane's own audio through it as well, so
  one run serves both. If the room is not listening and the lane holds audio,
  the timeline takes it. If the room is not listening and the lane holds
  nothing, the room keeps it exactly as before, so tracking through an amp sim
  while the interface does the monitoring still reaches the take wet.

- **The rate follower was building two AudioUnits a second, forever.** cpal's
  macOS `default_output_config` instantiates an AUHAL just to read a device's
  buffer range, and the poll that watches for a sample-rate change called it
  twice a second for the life of the process (21 ms median, 32 ms at the 95th
  percentile, measured) on a thread that exists to notice one number changing.
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
  the MIDI editor is gated by a hand (a press sounds it, the release quiets it)
  except this one. A double-click writes its note on the second *release*, so
  the note-on happens with the gesture already over and the release handler
  that would have closed the gate already returned past. The note-off was never
  sent at all: the note stood open until the next click anywhere in the roll.
  That is inaudible on every instrument that decays, which is how it survived:
  a piano, a Rhodes, a kit and most DX7 patches all fall silent on their own.
  Analog does not: its envelope ships sustaining at 0.8 and holds for exactly as
  long as the gate is open, so what was a silent leak everywhere else was an
  endless note there. The written note is given a length now (its own, so what
  you hear is what you drew), and underneath that, anything the editor is
  sounding with no deadline and no hand on it is let go of the next frame,
  which covers every early return in that file rather than only the one that
  was found.

- **A take's video was short by exactly the number of frames it repeated.** The
  pump has two clocks and until now it used both. `Compositor::frame` is
  pipelined one deep (it submits tick *n* and hands back tick *n − 1*), so a
  composited frame arrived carrying a timestamp one tick behind the pump's own
  counter, while a repeated frame, which composites nothing, carried the
  counter itself. Interleave the two and time runs backwards over every repeat:
  the encoder refuses a frame whose timestamp is not past the last one, counts
  it in a field that reached no manifest and no toast, and answers *fine*. One
  repeat cost exactly one real frame, invisibly, with the mp4 the only place it
  showed: a 10-second window take asked for 306 frames, repeated 43, and its
  file held 263. The file's clock is the encoder's own count now, which cannot
  run backwards by construction, and a frame the encoder will not take is
  counted as a frame the encoder dropped, so this class of fault can never be
  silent again.

- **A window take of a quiet moment ended on a red warning about a video with
  nothing wrong with it.** A screen capture delivers only when the screen
  changes, so a still window sends nothing and the take repeats the frame it
  already has: the picture is exactly right and no machine was late. Those
  repeats were counted beside the ones a machine that cannot composite in time
  makes, and every take that held still asked the owner to "lower the video
  size in Export", which could not have helped. They are counted apart now
  (`frames_still` in `take.json`), and only a real shortfall speaks.

- **The cursor wore a ring in every window take.** `capturesMouseClicks` is a
  different thing from capturing the pointer: it paints macOS's own click
  decoration, a hard ring that swells around the cursor on every press, into
  the frame, several times a bar over somebody playing, and burnt in with no
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
  meeting. A note's edges are spread wide so a hand can catch them and pull
  (seven points inside the note and four outside), and that band answered for
  double-clicks too, which have no meaning on a resize handle: eleven points
  around every note border where a double-click did nothing at all. And the
  band lands on the playhead, because a click with the pen up locates, snapped,
  and the double-click that follows writes its note from the same snapped grid
  line: the new note's border and the playhead are the same pixel. A click
  reads past the resize zones now: on the note it is the note, past it is the
  empty roll it looks like. The arrangement's clips had the same fault at their
  resize ends and fade corners, where a double-click would not open a MIDI item.

- **The IR wizard's level meter was frozen, and it opened pinned at full
  scale.** Two faults on the same readout, and each one hid the other. The
  host published the wizard's live picture only while the IR road was "doing
  something", and it decided that partly by reading the very meter the picture
  carries: a return at −200 dBFS made the host conclude nothing was happening
  and stop pushing. The stage-2 tone runs with an empty capture buffer, so
  during the whole of the level page that return level was the *only* thing
  that could keep the picture flowing: a loop bringing nothing back froze the
  wizard on whatever it last held, for the rest of the session. The case that
  went dark is exactly the case the page exists for. A dead loop is the one
  fault still fixable with the cable in your hand at the interface, the page
  has a sentence ready for it ("Nothing is coming back - check the loop's
  cables and the unit's bypass"), and that sentence is chosen by a number the
  host had decided not to send. Underneath it, the resting picture itself was
  wrong: `IrLive` derived its default, so four dBFS fields whose own
  documentation says −200 for silence rested at 0.0, and on a dBFS scale 0.0 is
  not "nothing", it is FULL SCALE. Every wizard is built from that default and
  then handed it, so a freshly opened level page drew its bar hard right and
  warned about clipping over a loop nobody had measured yet. The picture now
  follows the PANEL rather than the signal (an open wizard is pushed to every
  frame whatever the numbers say), silence rests at silence, and a road with no
  engine behind it publishes silence rather than going on showing levels off a
  device that is no longer open. The level page also carries the road's own
  phase line now, the way the sweep and result pages always have, so a tone
  asked for with no audio engine running says that instead of metering as a
  dead loop and sending somebody to check cables that are not the problem.

- **A full-speed export rendered every plugin at its last-saved knobs, not the
  ones the desk is playing.** An export builds a second engine and furnishes it
  by hand, and it filled every rack from the sidecar files on disk, which are
  written when a project is saved, when a plugin's own window is closed, when a
  session is replaced and at quit, and by nothing else. No knob writes them,
  and the suite's own units have no window whose closing writes them either. So
  a compressor dialled in on a track and exported before the next save was
  rendered by a mirror still wearing whatever that bay wore at the last save:
  its shipped defaults, if the bay had never been saved at all. On the master
  that reads as "the export sounds wrong". On stems it reads as something
  worse, and is how it was reported: the settings a unit sits at before
  somebody dials it in are precisely the ones that do nothing (a flat EQ, zero
  drive, a compressor still at 1:1), so a **Processed** stems export came out
  indistinguishable from the dry one, with every part of the dry/processed
  switch working correctly on audio that had been through a rack with nothing
  in it. An export now takes each bay's knobs off the desk, and falls back to
  the file only for a bay the running engine cannot answer for.

- **The busy overlay came up underneath the windows it was explaining.** It
  painted in the same egui band as the dialogs and one band below the export
  render window, so a "Preparing export" or "Writing files" cover could sit
  behind the very window whose work it was reporting. There is one rule now,
  stated once and used by both covers (this one and the launch splash): a cover
  paints above every band any surface of this app can ask for, because a cover
  is the last thing on screen before the app stops answering. And on the
  desktop, where a dialog is a window of its own that no paint order can reach
  over, an announced block now waits one frame longer: for a frame that no
  longer carries the window that asked for the work. A window is destroyed by
  not being drawn, and the pass that would not draw it was the pass that
  blocked, so pressing Export used to freeze the Export dialog on top of the
  progress cover for the whole length of the render.

- **A plugin on an audio track did nothing at all.** Not "processed silence":
  it did not run. An audio clip passed through its lane's fader, pan, mute,
  solo and send, and then went straight to the master having entered no insert
  rack anywhere in the engine; the lane's rack was reached only by MIDI notes
  and a live input, and a lane holding nothing but audio never got that far.
  A compressor dropped into an audio track's bay metered zero and changed
  nothing, in the speakers and in the exported file alike. Measured on a
  fixture with a constant clip thirty decibels over a 20:1 compressor's
  threshold, on that compressor's own lane: the master peaked at 0.8999999
  with the plugin loaded and 0.8999999 without it, bit for bit. It now peaks
  at 0.0285: thirty decibels of gain reduction that had been going nowhere.

  **This changes the mix of existing projects.** Any project with a plugin in
  an audio track's bay has been rendering as if that bay were empty, and will
  now render through it: the master, the stems and every bounce. An export
  made before this release is not reproducible after it. Nothing needs
  converting (the desk always showed the plugin, and the audio has finally
  caught up with what it said), but a mix balanced against a bay that was
  doing nothing will want another listen.

  The clips' separate delay compensation went with it. They used to hold
  themselves back by an arithmetic figure that reconstructed, per lane, what
  the lane would pay downstream; they now wait on the lane's own delay line by
  the lane's own formula, which is the one that already knew about groups and
  about an amp's seven pedal seats.

- **The window title was costing a sixth of every frame.** It asked whether the
  project was dirty once per frame, and asking meant serialising the entire
  document to JSON, 2.6 ms on a modest session. It is a structural fingerprint
  now, about thirty times cheaper, with the exact comparison kept everywhere a
  wrong answer could lose work.
- **The piano roll drew two circles per note in its velocity lane**, seventeen
  of them stacked in a column a fifth of a point wide. With the marks culled to
  what can actually be seen, a drum programme went from sixty-five percent of a
  frame to nineteen.
- **Opening a project made twenty-six thousand filesystem calls, twice**, walking
  every one of 4,389 seats when two of them held anything.
- **Consecutive strikes popped on every MIDI instrument**, worst under the
  pedal, because a re-struck key reused its voice and then reset it, clearing
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
  jazz grand voiced for solo playing, built as a digital waveguide: a delay
  line whose length is the pitch, a frequency-dependent loss filter for the
  decay, and an all-pass cascade for the stiffness that stretches a piano's
  partials sharp. **EP Mk.1** is a tine electric piano taken dry, and it is
  built a completely different way on purpose: a tine is a stiff bar, not a
  flexible string, so its modes are nowhere near harmonic and a waveguide
  cannot make them. It is modal instead: a bank of decaying sinusoids, each
  sitting where the metal actually puts it. Its velocity response is three
  mechanisms rather than a volume curve: a contact time that shortens with
  force and therefore feeds the high modes, a neoprene tip that stiffens as it
  compresses, and an electromagnetic pickup whose output goes as the inverse
  square of its gap. The last one is why hitting it hard *barks*: the transfer
  is lopsided only when the tine is swinging wide, so the distortion arrives
  with force and is simply absent when the note is quiet, which is something a
  waveshaper on the output could never do without dirtying the soft notes too.
  Measured on E4 across MIDI velocity 8 to 127, the bell runs from 48 dB under
  the fundamental to 18 dB under it, and waveform asymmetry from 0.017 to
  0.382, both monotonically. The whole module is pure `std` and adds no
  dependency, the same as the DX7's, so it compiles standalone on a bench that
  renders a scripted performance to a WAV in under a second, which is how all
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
  ring 256 of every 512 frames and *discarded the rest below every counter*:
  no xrun, no error, honest timestamps, a clean-looking stream at exactly half
  rate. The take's timeline is indexed by device frames, so a fifteen-second
  performance was written as a "7.5-second" take, and the desk's tap ring
  (drained at the halved pace) overflowed from the eighth second on, which is
  where the huge `frames_dropped` numbers actually came from. The input now
  sizes its ring to cover twice the graph's floor (the request still decides
  the period, so nobody's graph gets dragged coarser), and `takeprobe` (a new
  headless instrument that runs the app's own capture drain against a real
  device) measured the same stream at full rate with the fix and half rate
  without it. JACK takes were never affected: a JACK client's buffers are the
  graph's own cycle, so there is no ring to mismatch.

- **A take whose measured rate is far from its nominal one now says so out
  loud.** The rate fit measured the halving above perfectly (`epsilon_ppm:
  -499993` sat in every damaged manifest), and nothing read the number. A
  device-fed take with a fitted clock more than 10,000 ppm off nominal now
  raises a problem naming both rates: crystals lie by parts per million,
  delivery faults lie by factors, and there is nothing between.

### Added

- **JACK on Linux**, beside ALSA, in the Audio Setup panel's SYSTEM row, the
  row that has been a readout on every Linux build until now. Choosing it makes
  Tangent an ordinary client of the graph, named `Tangent`, so its ports appear
  in qjackctl, Carla or Patchance and can be patched into a DAW. It works
  against real jackd and against PipeWire's JACK layer equally, which is what
  nearly every desktop Linux is running.

  **It is offered for the routing, not for the latency.** The expectation was
  the opposite (a JACK client skips pipewire-alsa and the four-period ring
  `BUFFER_PERIODS` had to add for it), and the measurement did not support it:
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
  follows whatever the desktop's default source happens to be: the fault that
  once recorded a laptop microphone with an interface plugged in. Through JACK
  the ports carry the interface's real name.

  **It costs the artifact nothing.** libjack is `dlopen`ed on demand rather than
  linked, so `readelf -d` on the Linux binary gains no entry and a desktop with
  no pro-audio stack installed runs exactly what it ran before. No new crate
  enters the dependency graph either: `jack` was already there for the FreeBSD
  build. Choosing JACK on a machine where no server answers falls back to ALSA
  rather than leaving the app silent.

### Fixed

- **The Audio Status line no longer names a ring that does not exist.** The
  period and the ring were derived from `BUFFER_PERIODS`, a per-PLATFORM
  constant, where they are really a property of the HOST in use, so a Linux
  build had one number for two driver stacks. Under JACK that would have
  reported a 128-frame period for a 512-frame callback, next to a 2048-frame
  ring belonging to nobody. `BufferRule` now carries the distinction from the
  open, where the real host is known.
- **Under JACK the input picker no longer offers devices it cannot open.** The
  PipeWire node list is a workaround for cpal's ALSA host, and it REPLACES
  cpal's own list rather than joining it, so through JACK, where there is no
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
