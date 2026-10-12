Tangent
=======

Tangent is a DAW: you record audio and MIDI into it, arrange clips on a
timeline, edit notes in a piano roll, and mix with buses, returns, sends,
folders and automation. It hosts VST3 plugins (and LV2 on Linux and FreeBSD)
and comes with its own instruments and effects. As you play, it also names
the chord under your hands, and a take can film the window with your camera
in it.

It is free with up to 32 tracks. A supporter key removes the limit; buses,
returns, sends and routes never count toward it.


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

  In a release build the installer, the app, its video encoder and the
  uninstaller are signed under the author's name, which is what Windows 11's
  Smart App Control asks of a program it does not know before it will run
  it. SmartScreen goes on reputation, which a new signature has to earn, so
  it may still show "Windows protected your PC" the first time: click
  "More info", then "Run anyway". Windows remembers the choice.

Linux

  The tarball is tangent-<version>-linux-<arch>.tar.gz, or under its
  version-free name tangent-linux-<arch>.tar.gz; either way the folder it
  unpacks to carries the version:

    tar -xzf tangent-<version>-linux-<arch>.tar.gz
    cd tangent-<version>-linux-<arch>
    ./install.sh

  That installs Tangent into your home directory, no root needed, and
  ./install.sh --uninstall takes it all back out. Or skip installing entirely:
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
  PipeWire, with nothing to set up. If you run a pro-audio session, Settings
  (the cog on the recorder band) -> Audio Setup has a second entry: JACK. Pick
  it and Tangent becomes an ordinary client of the graph, under the name
  "Tangent", so its ports show up in qjackctl, Carla or Patchance next to
  everything else and can be patched into a DAW. It runs on the server's own
  period and sample rate, so the buffer-size and sample-rate controls have
  nothing left to do while it is selected; the INPUT and OUTPUT lines at the
  top of Audio Setup report what the server is actually running.

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


Your first session
-----------------

Tangent opens on a new session with tracks ready to play. Three views share
the window, and Tab moves between them: the keyboard view (the piano, the
chord names and the theory band), the arrangement, and the mixer. The
recorder band stays where it is in all three, with the transport, the
metronome, the project's name and the cog for Settings and Audio Setup.

  Space          play and stop (in the keyboard view, it sounds the lit chord)
  Cmd/Ctrl+R     record into the arrangement, on every armed track
  Cmd/Ctrl+T     new track
  H (hold)       every shortcut for the view you are in

The cassette on the recorder band starts a take, which can also save a video
of the window beside its audio and MIDI.

Everything that is not on screen is in the right-click menu. Right-click a
track for New Bus, New Return, Freeze Track and its routing. The plus on an
empty insert slot loads a plugin, a built-in instrument or a built-in effect.

Audio Setup (the cog on the recorder band) picks the audio system, the
device, the sample rate and the buffer size. On Windows, pick ASIO there if
your interface has an ASIO driver.

Click the project's name on the recorder band for New, Open, Save and
Export. Export writes a mixdown or stems as WAV or MP3, for the whole song,
the loop or a range you type, with only the tracks you tick.


Connecting a MIDI keyboard
--------------------------

Plug it in, start Tangent, play. The MIDI tracks a new session starts with
listen to every device, so any keyboard that is plugged in is heard on the
armed track. A session with no MIDI track opens one port, preferring ports
whose names contain "USB-MIDI", then "Scarlett" or both "USB" and "MIDI", and
otherwise the first port it finds.

A device plugged in after launch, or unplugged and plugged back in, is picked
up by itself as long as a track listens to it. If one stays silent after being
plugged straight back in, leave it out for a few seconds and plug it in again.
To choose which devices a track listens to, select it, then right-click >
"Select MIDI Input...", tick the ones it should hear (none ticked means every
device) and click Done.

From a terminal:

    tangent -l                   list the available MIDI input ports
    tangent -p "Digital Piano"   connect to one specific port

(on macOS that binary lives at /Applications/Tangent.app/Contents/MacOS/tangent)

No keyboard to hand? Press K for Keytoggle, then click keys on the drawn piano
to hold and release notes.


When Tangent names a chord differently than you would
---------------------------------------------------

Right-click the piano and look under Chords.

"Teach Chord Name..." pins your name to the exact voicing you are holding.
Type it and click OK, and Tangent calls that voicing by your name from then
on. Tick "Apply in all keys" and the name follows the shape into every key.

"Correct Chord Name..." teaches a general preference instead. It lists the
readings Tangent weighed for this voicing, with their scores; pick the one you
would rather see and click "Learn". Tangent then leans that way everywhere, so
similar voicings shift too, sometimes in unrelated keys. That is the feature
working. If the name you want is not in the list, Tangent never considered it,
so pin it with "Teach Chord Name..." instead.

"Manage Taught Chords..." lists what you have pinned, with a Delete button
beside each, and "Forget Learning" puts stock chord naming back exactly.
"Disable Chord Learning" in the menu silences learning without erasing
anything.


Settings
--------

Colours, themes and the window live in the right-click menu; T cycles the
thirty-two themes. Settings save themselves the moment you change them.
Projects do not: New, Open and Quit ask before they discard unsaved changes.
Reset, at the bottom of the menu, puts back one part at a time (colours and
theme, layout and windows, playing and theory, the desk, devices and folders,
the default template) or everything at once, and asks first.

Tangent keeps its settings in two files, at the same path on every platform:

    ~/.config/tangent/settings.json    every setting
    ~/.config/tangent/overrides.json   taught chords and learned leanings

On Windows that folder is C:\Users\<you>\.config\tangent\. Deleting one of
these files resets that part of Tangent and nothing else. Your projects live
wherever you save them.

Coming from Tangent 4.x: 5.0 does not read the old ~/.config/ivory folder and
removes it the first time it starts. Paste your supporter key again
(right-click > "Support Tangent..." > Activate); it is in the email it came in.


Privacy
-------

Tangent collects no data. It has no telemetry, no update check and no account,
and it checks a supporter key on your own machine. It makes no network
connection of its own except Stream to OBS (behind the cog on the recorder
band), which is off until you turn it on and then sends the master's audio
over UDP to the host you give it: this machine (127.0.0.1) unless you change
it. A plugin you load runs inside Tangent but is its maker's program, so what
it connects to is up to its maker. The two links in the app, the author's
site in the About box and the store page in "Support Tangent...", open in your
browser only if you click them.


If something looks wrong
------------------------

  "Tangent is already running"
      Only one copy runs at a time. Close the other window; if there is no
      window, quit the leftover process.

  No sound
      Open Audio Setup (the cog on the recorder band) and check the OUTPUT
      row: it lights the output that is actually playing.

  A keyboard is not heard
      Arm the track, and check what it listens to: right-click > "Select
      MIDI Input...", tick the keyboard, or untick everything, which means
      every device.

  No chord names appear
      Chord detection may be switched off. Right-click the piano > Chords >
      "Enable Chord Detection".


Licenses
--------

Tangent is MIT-licensed; see LICENSE. The Rust libraries it links against are
listed in THIRD-PARTY-LICENSES. The bundled Courier Prime fonts are
Copyright 2015 The Courier Prime Project Authors, licensed under the SIL Open
Font License 1.1; see OFL.txt. The other two typefaces, Terminess Nerd Font
Mono and JetBrains Mono, are under the same license; their license texts are
in font-licenses/, with those of the four fallback fonts the app embeds.

Where the download has a lame-licenses/ folder (macOS, FreeBSD), the app
contains LAME, the MP3 encoder, under the GNU LGPL: that folder holds its
licences and says how to rebuild Tangent with a changed LAME. Where it has
an ffmpeg-licenses/ folder (Windows, Linux), tangent-ffmpeg is an
unmodified GPL build of ffmpeg: that folder holds its licence and says
where its source is published.

  macOS     inside the app: right-click Tangent.app > Show Package Contents >
            Contents/Resources
  Windows   next to tangent.exe
  Linux     in this folder; OFL.txt is in fonts/

THANKS

  Omer, for the rounded macOS app icon of the 2.x era.
  Hatsu, for extensive Windows testing and feedback.

Tangent is free with up to 32 tracks, and a supporter key removes the limit:
https://ganten.gumroad.com/l/ivory
Source code, release notes and the issue tracker:
https://github.com/ganten1998/tangent
