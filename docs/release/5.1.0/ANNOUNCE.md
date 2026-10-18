# Tangent 5.1.0 announcement

For existing users: supporters by email, and the same text for a post.
Names no third-party product. It goes out the way 5.0.0's did
(docs/release/5.0.0/ANNOUNCE.md has the commands): through the fulfil
service's `announce` command, a dry run unless `--send` is given, which the
owner runs himself, and only after the release is public and
`scripts/check-store-links.sh` is green from a signed-out machine.

```sh
flyctl ssh console -a tangent-fulfil -C 'tangent-fulfil announce --subject "Tangent 5.1 is out" --body /tmp/body.txt'          # dry run
flyctl ssh console -a tangent-fulfil -C 'tangent-fulfil announce --subject "Tangent 5.1 is out" --body /tmp/body.txt --send'   # owner only
```

---

Subject: Tangent 5.1 is out

Hi,

Tangent 5.1 is out, and it rebuilds Tangent Keys, the modelled keyboard
that comes with it.

Every Keys sound was re-voiced against measurements of a reference
instrument: how each partial decays, how the level follows your touch, the
ring around a note, the release. There are now ten sounds:

- Intimate Grand and Concert Grand (Piano 1 and Piano 2 until now), built
  again from the strings up. Each note has three strings coupled at the
  bridge, a hammer graded by key and velocity, and strings that ring in
  sympathy with what you play. Both half-pedal, hold a sostenuto and shift
  for the una corda.
- EP Mk.1 and EP Suitcase, the tine pianos, dry and through their amp.
- Clav and Clav Wah.
- Reed 200 and Reed Amp (Reed 140 until now: the reed piano through its
  amplifier, with the cabinet's slap and throb and a room).
- Electric Grand, new: a stage grand's short strings heard through pickups.
- Nylon Guitar, new: six nylon strings, each note on the string a hand
  would choose, the open strings ringing along. Every key plays, even past
  the guitar's own range.

Every sound saves presets of its own now, and the Keys window opens big
enough to show the ten sounds, every knob and the presets at once. The ten
also sit together in level, so moving from one to another no longer means
reaching for the fader. And a named chord now spells its notes from its
root on the staff, the Tonnetz and the circle of fifths.

Two things to know before you upgrade:

1. A mix balanced on the 5.0 Keys sounds may want its faders looked at:
   the two grands play louder than they did (7 dB at fortissimo), EP
   Suitcase a little quieter, the two clavs louder.

2. A track saved on Piano 1, Piano 2 or Reed 140 opens on the Intimate
   Grand, the Concert Grand or Reed Amp, with the knobs it had.

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
