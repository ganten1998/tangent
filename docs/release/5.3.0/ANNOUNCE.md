# Tangent 5.3.0 announcement

For existing users: supporters by email, and the same text for a post.
Names no third-party product. The 5.1 and 5.2 emails were never sent, so
this one covers everything since 5.0. It goes out the way 5.0.0's did
(docs/release/5.0.0/ANNOUNCE.md has the commands): through the fulfil
service's `announce` command, a dry run unless `--send` is given, which the
owner runs himself, and only after the release is public and
`scripts/check-store-links.sh` is green from a signed-out machine.

```sh
flyctl ssh console -a tangent-fulfil -C 'tangent-fulfil announce --subject "Tangent 5.3 is out" --body /tmp/body.txt'          # dry run
flyctl ssh console -a tangent-fulfil -C 'tangent-fulfil announce --subject "Tangent 5.3 is out" --body /tmp/body.txt --send'   # owner only
```

---

Subject: Tangent 5.3 is out

Hi,

Tangent 5.3 is out. It is the first note from me since 5.0, so here is
everything new since then.

Tangent Tape Rework: warm analog tone, no frills, just results. Tape is a
tape machine now, at four speeds from 3.75 to 30 inches per second, each
with its own playback EQ, head bump and hiss. Wow, Flutter, Bias, Age and
Damage do what they would on a real machine, and a deep wow range gives
you a worn cassette's warble. Right-click the PITCH CONT dial and Play can
spin the whole desk up from a standstill, and Stop wind it down, pitch and
all.

Tangent Keys Rework: a complete upgrade to the modeling means a full suite
of classic keyboard sounds: two grands, two tine pianos, two clavs, two
reed pianos and an electric grand, each with presets of its own. Try them
all, or mess around with the bonus instrument: a Nylon Guitar. A new
session now opens on the Intimate Grand.

Everything in time. When a plugin adds delay, Tangent holds every track
back to match, and now the click, what you record and the keys you play
live all stay on that one clock: the click lands on the beat, a part lands
where you played it, and an armed instrument answers your keys at once.
Every note of the built-in instruments lands on its own sample, so what
you hear is what you export.

Hear the master while you track. Monitoring through Tangent now plays every
plugin across the master, so a Tape or a reverb there is in your headphones
while you record.

Your key works as before, so there is nothing to do.

Download (these links always fetch the current version):

  macOS 11 or later, Apple Silicon or Intel
  https://github.com/ganten1998/tangent/releases/latest/download/Tangent-macos.pkg

  Windows 10 or later
  https://github.com/ganten1998/tangent/releases/latest/download/Tangent-windows-setup.exe

  Linux x86_64
  https://github.com/ganten1998/tangent/releases/latest/download/tangent-linux-x86_64.tar.gz

  Everything else (Linux aarch64, FreeBSD, the .dmg and .zip, checksums, the
  full release notes):
  https://github.com/ganten1998/tangent/releases/latest

On Windows every program in the download is signed. SmartScreen may still
say "Windows protected your PC" the first time; click More info, then Run
anyway.

Thank you for supporting it. Replies come to me.

Ganten
