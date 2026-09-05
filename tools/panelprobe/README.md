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

What they found on 2026-09-05, and what `tangent/vendor/rfd` does about it, is
written up in `tangent/vendor/README.md` under "The third change". In short: a
folder under `~/Documents` is refused by the save panel and it falls back to
home whatever `NSOSPLastRootDirectory` says; `panel:didChangeToDirectoryURL:`
reports the move; and a `setDirectoryURL:` said after that call holds.

Each program writes to its own preference domain (`panelprobe`, `paneltime`),
never Tangent's.
