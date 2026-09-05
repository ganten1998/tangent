# Vendored `rfd`

`rfd/` is [`rfd` 0.15.4](https://github.com/PolyMeilex/rfd) (MIT) with **one
change** and nothing else. Upstream is otherwise untouched, deliberately: a
diff this small is reviewable, and it can be dropped the day the fix lands
upstream.

## Why

The macOS save panel opened at the HOME directory however real the directory
Tangent handed it.

`src/backend/macos/file_dialog/panel_ffi.rs`, `Panel::set_path`, builds
`<starting directory>/<suggested file name>` and passes that to
`setDirectoryURL:` with `isDirectory: true`:

```rust
let path = if let (Some(name), true) = (file_name, path.is_dir()) {
    let mut path = path.to_owned();
    path.push(name);                       // .../Projects/Untitled copy
    path
} else { path.to_owned() };
let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), true);
self.panel().setDirectoryURL(Some(&url));
```

That path does not exist — it is where the file is ABOUT to be written. AppKit
discards a `directoryURL` it cannot open and the panel falls back to whatever
`NSSavePanel` last remembered, which for a fresh user is home.

Three things make it hard to see. It bites only when a file name is set AND
the directory exists, so Open panels are fine and Save panels are not. It looks
INTERMITTENT, because the remembered directory is sometimes the right one
anyway — the owner reported the panels "reverted AGAIN to home directory",
having worked the build before. And it produces no error of any kind: no
return code, no log line, nothing on screen.

It was settled by adding a log line on Tangent's side, which proved the app was
handing macOS a real directory:

```
panel: Save the project as opens at /Users/<user>/Documents/Tangent Media/Projects - the directory exists
```

## The change

`set_path` no longer appends the file name; it sets the directory it was
given. The suggested name is unaffected — every builder that passes a
`file_name` to `set_path` calls `set_file_name` two lines later, which is
`setNameFieldStringValue:` and is the correct way to fill that field.

## The second change, and the one that actually fixed it

The first change was necessary and not sufficient: with a real directory
reaching `setDirectoryURL:`, the save panel STILL opened at home, and the owner
reported it again as a per-build wildcard — "never has been consistently
applied".

It was settled by measurement rather than argument. A small AppKit program
reads the panel's own `directoryURL` back, before it is shown and again after,
and cancels itself so nobody has to click:

```
SAVE  before show  ~/Documents/Tangent Media/Projects
      AFTER  show  /Users/<user>                       *** MOVED ***
OPEN  before show  ~/Documents/Tangent Media/Projects
      AFTER  show  ~/Documents/Tangent Media/Projects  HELD
```

`NSSavePanel` accepts the URL and then moves; `NSOpenPanel`, in the same
program on the same directory, holds it. That is exactly the split the owner
saw — Open Project and the kit chooser behaved, Save As and Save As Template
did not — and it is why it looked intermittent: a save panel restores the
app's remembered location (`NSOSPLastRootDirectory`, a bookmark in the app's
own preference domain) OVER the one it was handed, and whether that looks
wrong depends on where it last was. Varying the file name, the shielding
window level, and the order the two are set in changed nothing; all four
variants moved.

So `Panel::hold_directory` says it again once the restore has happened. Also
measured: re-asserting on the next run-loop turn is too early and still moves;
30 ms holds. Four shots inside 320 ms cover a slower machine and are far
quicker than anyone can navigate a panel, so it can never take a folder away
from a hand that chose it. It is on the SAVE panel only — the open panel holds
what it is given.

**And it goes on the main QUEUE, which the first attempt got wrong.** That
attempt used `NSTimer`s in `NSRunLoopCommonModes`, on the reasoning that a
modal panel spins in `NSModalPanelRunLoopMode`. It holds in a standalone
AppKit program — measured, three shots, all three fired — and it did NOT hold
inside Tangent, where the panel runs as a sheet nested inside winit's own
event loop. The app logged that it handed the panel a real directory and the
panel still opened at home:

```
panel: Save Kit opens at ~/Documents/Tangent Media/Presets/Tangent Drums - the directory exists
```

Which run-loop modes are common under a nested event loop is not a thing to
reason about from a vendored dialog crate. The main queue has no such
question: it is drained while a modal panel is up regardless of mode, and it
is what the original measurement used.

## The third change: the panel says when it moves, and is put back

The second change did not hold either, and the owner reported it for the
fourth time. The app's log for that morning has it handing over the right
folder — `panel: Save the project as opens at ~/Documents/Tangent Media/Projects
- the directory exists` — and the app's own preference domain has the panel
closing at `/Users/<user>`.

Measured again on 2026-09-05, with a standalone program opening exactly this
panel as a sheet and reading `directoryURL` back:

* `~/Documents`, `~/Desktop`, `~/Music` and `/private/tmp/probedir` all HELD;
  `~/Documents/Tangent Media/Projects` MOVED to home. A folder UNDER
  `~/Documents` is refused.
* It moved with `NSOSPLastRootDirectory` pointing at home, pointing at the
  wanted folder, and deleted — all three. So the remembered location, which
  the second change's note blamed, is not what does it.
* `panel:didChangeToDirectoryURL:` fires for the move, once, with the home
  directory, before any hand has touched the panel.
* `setDirectoryURL:` said again AFTER that call holds — read back three
  seconds later, still there.
* The main queue is blocked during the panel's own setup: every block queued
  for 10 through 320 ms ran together at 325 ms. In the probe that fell after
  the move, which is why the timed shots "held" there; in the app they fired
  before it and changed nothing.

So `Panel::_steer` is an `NSOpenSavePanelDelegate` (`Steer` in
`panel_ffi.rs`): on a change to a directory other than the one the panel was
built with, within three seconds of building it and at most twice, it says the
directory again from a main-queue block 30 ms on. The timed shots stay as a
fallback and are now GUARDED — they re-assert only when the panel is not
already where it was told — so neither mechanism can take a folder away from a
hand that chose one.

Both leave a line in Tangent's log file (`~/.config/tangent/tangent.log`,
which keeps INFO) when they act: `panel: the save panel moved itself to
/Users/<user> 340 ms after opening; putting it back on ... (repair 1 of 2)`
from the delegate, and `panel: the 320 ms shot found the save panel at ...`
from a shot. A fifth report with neither line means the panel moved without
telling its delegate; with the delegate's line and the panel still at home,
the re-assert is what stopped holding. Either way the next hunt starts with
evidence rather than a theory.

## Dropping this copy

Delete `tangent/vendor/rfd`, remove the `rfd` line from the root
`[patch.crates-io]`, and confirm on macOS that a Save As opens in the Projects
folder rather than at home. The INFO line above is still emitted and still says
which directory was asked for. Note that dropping this copy brings ALL THREE faults
back, and the second and third are invisible to that log line — the app will
still report handing over the right directory while the panel opens somewhere
else.
