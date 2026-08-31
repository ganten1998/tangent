Tangent
=====

Tangent draws all 88 piano keys and lights them from your MIDI keyboard in real
time. As you play, it names what you are playing: triads and sevenths through
altered dominants, slash chords, rootless jazz voicings, scales and modes. It
records, mixes and arranges what you play too, and one press writes the audio,
the MIDI and a video of the window itself.

Everything is in the right-click menu. That is the entire interface.


First launch
------------

macOS

  The easy way is the installer: open Tangent-macos.pkg and follow it. If you
  took the .dmg or the .zip instead, drag Tangent.app into your Applications
  folder.

  That is all. Tangent is signed with a Developer ID certificate and notarized by
  Apple, with the ticket stapled to the app, so it opens on a double-click with
  no security prompt and without being online.

  Requires macOS 11 (Big Sur) or later. The build is universal: Apple Silicon
  and Intel, one download.

Windows

  Run Tangent-windows-setup.exe and follow it. It installs the app, puts it in
  the Start menu, and includes an uninstaller.

  The installer is not code-signed, so SmartScreen shows "Windows protected
  your PC" the first time. Click "More info", then "Run anyway". Windows
  remembers the choice.

Linux

    tar -xzf tangent-<version>-linux-<arch>.tar.gz
    cd tangent-<version>-linux-<arch>
    ./install.sh

  That installs Tangent into your home directory, no root needed, and prints
  how to undo it (./install.sh --uninstall). Or skip installing entirely:
  `chmod +x tangent && ./tangent` runs it from the folder.

  It asks once whether you want desktop integration: the menu entry and the
  icon, which are what put Tangent in your application menu and in launchers
  like rofi, wofi and dmenu. Say yes and you can start it by name from a
  launcher as well as by typing `tangent`. --desktop and --no-desktop answer
  the question in advance, for a scripted install.

  Everything the recorder needs ships in the tarball: the video encoder is
  bundled as tangent-ffmpeg, found automatically as long as it sits beside the
  tangent binary. Filming a take also wants a Vulkan driver, which nearly every
  desktop already has; if yours does not, install.sh notices and prints your
  distribution's one-line fix (mesa's lavapipe works on any GPU).

  MIDI goes through ALSA, so libasound.so.2 needs to be present: package
  libasound2 on Debian/Ubuntu, alsa-lib elsewhere. It already is on essentially
  every desktop install. PipeWire systems work unchanged: your keyboard still
  shows up as an ordinary ALSA sequencer port. Wayland and X11 are both
  supported. Requires glibc 2.30 or newer.

  LV2 plugin hosting needs liblilv-0.so.0, and unlike everything above, without
  it Tangent does not start at all. It arrives with PipeWire or with ffmpeg, so
  it is already there on Debian, Fedora, Arch, Void, openSUSE, Mint, Pop and
  every Ubuntu flavour. The one desktop that ships without it is stock Ubuntu
  GNOME:

      sudo apt install liblilv-0-0

  install.sh checks for it and prints your distribution's command if it is
  missing.

  Your keyboard has to be plugged in and switched on before Tangent can see it,
  because ALSA only creates a port for hardware that is actually there.

  Audio out of the box goes through ALSA, which on a PipeWire desktop means
  PipeWire, with nothing to set up. If you run a pro-audio session, Settings ->
  Audio Setup has a second entry: JACK. Pick it and Tangent becomes an ordinary
  client of the graph, under the name "Tangent", so its ports show up in
  qjackctl, Carla or Patchance next to everything else and can be patched into
  a DAW. It runs on the server's own period and sample rate, so the buffer-size
  and sample-rate controls have nothing left to do while it is selected; the
  Audio Status line reports what the server is actually running.

  Pick it to be part of a session, not to chase a smaller number: measured on
  the machine this was developed on, JACK was not faster than the ordinary ALSA
  path, and the reasons are written up in the project's notes.

  What it does fix outright is WHICH interface you record. Through ALSA on a
  PipeWire machine there is only one device Tangent can open, and it follows
  whatever your desktop's default input happens to be, so changing that
  default in your system settings silently changes what Tangent records.
  Through JACK the ports carry your interface's own name and stay where you put
  them.

  One thing works differently under JACK, and it is the JACK way round rather
  than a limitation: the input picker shows a single row, "Tangent_in", because
  that is what Tangent is on the graph. WHICH interface feeds it is decided by
  the patchbay, not by the picker. Tangent connects itself to the first
  hardware capture ports it finds when it starts, so there is sound to record
  straight away; repatch it in qjackctl and the change is immediate. Under
  ALSA the picker still lists your interfaces by name, exactly as before.

  This works against real jackd and against PipeWire's JACK layer equally.
  Most distributions install the latter by default now, sometimes as a separate
  package (pipewire-jack on Debian/Ubuntu, pipewire-jack-audio-connection-kit on
  Fedora). Nothing is required: libjack is opened only if you choose JACK, so a
  machine without it is not missing anything and the entry falls back to ALSA
  rather than leaving you in silence.

  If you picked JACK and got ALSA back, PipeWire's libjack is installed
  somewhere the dynamic loader does not look; some distributions keep it out
  of the default path deliberately so that it does not shadow a real jackd.
  Starting Tangent as `pw-jack tangent` is the supported way round that, and it
  is what the same distributions tell you to do for every other JACK
  application.

  Installing by hand instead:

    install -Dm755 tangent         ~/.local/bin/tangent
    install -Dm755 tangent-ffmpeg  ~/.local/bin/tangent-ffmpeg
    install -Dm644 tangent.desktop ~/.local/share/applications/tangent.desktop
    install -Dm644 tangent.png     ~/.local/share/icons/hicolor/128x128/apps/tangent.png

  The fonts in fonts/ are already embedded in the binary. They ship as loose
  files for license compliance, and in case you want them installed system-wide.


Connecting a MIDI keyboard
--------------------------

Plug it in, start Tangent, play. Tangent connects on its own at startup, preferring
ports whose names contain "USB-MIDI", then "Scarlett" or both "USB" and "MIDI",
and otherwise simply taking the first port it finds.

If you connected after launching, or you have more than one device:
right-click > "Select MIDI Input...", choose the port, click OK. Tangent does not
reconnect by itself if a device is unplugged mid-session: plug it back in and
pick it from that list again.

From a terminal:

    tangent -l                   list the available MIDI input ports
    tangent -p "Digital Piano"   connect to one specific port

(on macOS that binary lives at /Applications/Tangent.app/Contents/MacOS/tangent)

No keyboard to hand? Right-click > "Enable Keytoggle", then click keys on the
drawn piano to hold and release notes. Chord naming behaves exactly the same.


When Tangent names a chord differently than you would
---------------------------------------------------

Two menu items, and they do different things.

"Teach Chord Name..." pins your name to the exact voicing you are holding. Type
it, click OK, and that is what Tangent calls it from then on. Tick "Apply in all
keys" and the name follows that shape into every key. It is greyed out when you
are not holding anything, because it acts on what you are playing.

"Correct Chord Name..." teaches a general preference instead. It lists the
readings Tangent actually weighed for this voicing, with their scores; pick the
one you would rather see and click "Learn". Tangent then leans that way
everywhere, so similar voicings shift too: about one chord in ten, sometimes
in unrelated keys. That is the feature working, not a fault. If the name you
want is not in the list, Tangent never considered it and cannot be argued into
it, so pin it with "Teach Chord Name..." instead. The first correction that lands
switches Chord Learning on, which also reactivates any earlier corrections.

"Manage Taught Chords..." lists everything you have pinned, with a Delete button
beside each, shows whether learning is on and what it has picked up, and offers
"Forget Learning", which restores stock chord naming exactly. "Disable Chord
Learning" in the menu silences learning without erasing anything.


Settings
--------

Colors, dark mode, window size (50%–200%), borderless mode, flats versus sharps,
and the detachable chord display all live in the right-click menu, and save
themselves the moment you change them. "Reset Settings to Default" puts
everything back.

Tangent keeps two files, at the same path on every platform:

    ~/.config/tangent/settings.json    appearance and window preferences
    ~/.config/tangent/overrides.json   taught chords and learned leanings

On Windows that folder is C:\Users\<you>\.config/tangent\. Deleting one of these
files resets that part of Tangent and nothing else.


Privacy
-------

Tangent makes no network connections and collects no data. Your settings and your
taught chords stay on your machine. There is no telemetry, no update check, and
no account. The one link in the app, the author's site in the About box,
hands the address to your browser if you click it, and that is the whole of it.


If something looks wrong
------------------------

  "Tangent is already running"
      Only one copy runs at a time. Close the other window; if there is no
      window, quit the leftover process.

  No chord names appear
      Chord detection may be switched off. Right-click > "Enable Chord
      Detection".

  Keys never light up
      Tangent is probably on the wrong port. Right-click > "Select MIDI
      Input..." and pick your keyboard.


Licenses
--------

Tangent is MIT-licensed; see LICENSE. The Rust libraries it links against are
listed in THIRD-PARTY-LICENSES. The bundled Courier Prime fonts are
Copyright 2015 The Courier Prime Project Authors, licensed under the SIL Open
Font License 1.1; see OFL.txt.

  macOS     inside the app: right-click Tangent.app > Show Package Contents >
            Contents/Resources
  Windows   next to tangent.exe
  Linux     in this folder; OFL.txt is in fonts/

THANKS

  Omer, for the rounded macOS app icon.
  Hatsu, for extensive Windows testing and feedback.

Tangent is pay-what-you-can, and nothing is a perfectly good amount. Thanks for
playing.
