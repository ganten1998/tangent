# panelprobe

Two twenty-line AppKit programs that open exactly the save panel `rfd` opens
in Tangent — `beginSheetModalForWindow:` on a plain window, then `runModal` —
and read the panel's own `directoryURL` back, so that "the save panel opens at
home" can be measured instead of argued about. They cancel themselves; nobody
has to click.

```sh
clang -fobjc-arc -framework AppKit -o panelprobe panelprobe.m
./panelprobe home                 # remember HOME as the last root, point at Projects
./panelprobe steer                # remember Projects itself
./panelprobe clear                # no remembered root at all
./panelprobe clear ~/Music        # any folder
./panelprobe clear ~/Documents/Tangent\ Media/Projects reassert   # say it again at 1.5 s

clang -fobjc-arc -framework AppKit -o paneltime paneltime.m
./paneltime                       # a timeline: directoryURL every few ms, the delegate's calls, one guarded re-assert
```

Two more, and these are the ones that settled it, because they PHOTOGRAPH the
panel (`screencapture`) instead of trusting `directoryURL`, which reads back
whatever was last set while the panel shows something else:

```sh
clang -fobjc-arc -framework AppKit -o panelapp panelapp.m
./panelapp ~/Documents/Tangent\ Media/Projects tag        # the app's own repair timing, photographed at 1.6 s
./panelapp ~/Documents/Tangent\ Media/Projects tag late   # ...and once more after a re-assert at 2.1 s
PROBE_EXPANDED=1 ./panelapp ...                           # ask for the expanded form
PROBE_LEGACY=1 ./panelapp ...                             # the deprecated setDirectory: instead

clang -fobjc-arc -framework AppKit -o openprobe openprobe.m
./openprobe ~/Documents/Tangent\ Media/Projects tag       # an OPEN panel in choose-a-folder mode with a Save As field
```

What they found on 2026-09-05: `NSSavePanel` refuses any folder under
`~/Documents` — new or old, 700 or 755, as a path, a `..` path, a firmlink
path, a file-reference URL or a symlink — and opens at home, whatever
`NSOSPLastRootDirectory` says; the delegate hears the move at ~280 ms; saying
the directory again changes the property and NOT the panel, early or late;
and `NSOpenPanel` on the same folder opens in it. That is why Tangent's save
panel on macOS is now an open panel: `macwin::save_panel`, and the story in
`tangent/vendor/README.md` under "The fourth change".

Each program writes to its own preference domain (`panelprobe`, `paneltime`),
never Tangent's.
