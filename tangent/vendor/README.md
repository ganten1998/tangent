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

So `Panel::hold_directory` says it again once the restore has happened: three
one-shot `NSTimer`s at 30, 80 and 180 ms, added to the run loop in the COMMON
modes because a modal panel spins in `NSModalPanelRunLoopMode` and a timer in
the default mode alone would not fire until the panel had closed. Also
measured: re-asserting on the next run-loop turn is too early and still moves;
30 ms holds. Three shots inside 200 ms covers a slower machine and is far
quicker than anyone can navigate a panel, so it can never take a folder away
from a hand that chose it. It is on the SAVE panel only — the open panel holds
what it is given.

## Dropping this copy

Delete `tangent/vendor/rfd`, remove the `rfd` line from the root
`[patch.crates-io]`, and confirm on macOS that a Save As opens in the Projects
folder rather than at home. The INFO line above is still emitted and still says
which directory was asked for. Note that dropping this copy brings BOTH faults
back, and the second one is invisible to that log line — the app will still
report handing over the right directory while the panel opens somewhere else.
