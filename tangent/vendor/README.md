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

## Dropping this copy

Delete `tangent/vendor/rfd`, remove the `rfd` line from the root
`[patch.crates-io]`, and confirm on macOS that a Save As opens in the Projects
folder rather than at home. The INFO line above is still emitted and still says
which directory was asked for.
