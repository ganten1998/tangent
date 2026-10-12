# Tangent 5.0.0 announcement

For existing users: supporters by email, and the same text for a post.

How it goes out (from the supporter-licence notes; nothing here sends it):
Gumroad cannot email customers at these numbers, so the mail goes through the
fulfil service's `announce` command on the Fly machine, which dedupes by
address, is a dry run unless `--send` is given, and refuses an em dash. The
owner runs the `--send` himself. Send it only after the release is public,
`scripts/check-store-links.sh` is green from a signed-out machine, the
Gumroad page is re-enabled and the fulfil service is redeployed.

```sh
flyctl ssh console -a tangent-fulfil -C 'tangent-fulfil announce --subject "Tangent 5.0 is out" --body /tmp/body.txt'          # dry run
flyctl ssh console -a tangent-fulfil -C 'tangent-fulfil announce --subject "Tangent 5.0 is out" --body /tmp/body.txt --send'   # owner only
```

(Upload the body first by piping base64 into `base64 -d > /tmp/body.txt` on
the machine; a redeploy wipes `/tmp`, so upload after the deploy. The usage
is `tangent-fulfil announce --subject <text> --body <file> [--ledger <path>]
[--send]`, tools/tangent-fulfil/src/main.rs:454.)

"Replies come to me" holds only if the deployed service has `MAIL_REPLY_TO`
unset or pointing at a read inbox (default `tangentdaw@gmail.com`,
main.rs:352).

---

Subject: Tangent 5.0 is out

Hi,

Tangent 5.0 is out, and it is the biggest release since the rewrite.

What is new, in short: buses and returns, as many as you want; Tangent
Insert, which puts your outboard gear in an insert bay and keeps the session
in time with it; several instruments on one track, all playing; plugins that
follow the song's tempo and transport (Superior Drummer and synced delays
included); exports that pick their tracks, stems or range, with stems named
after their tracks; a send of the master to a virtual cable for OBS or
Discord; a bitcrusher; audio quantize; track freezing; thirty-two themes; and
FreeBSD as a supported platform.

What your key does now: the free app holds up to 32 tracks, and your key takes
that limit away. You do not need to do anything for that; every key ever
sold does it. Buses, returns, sends and routes have no limit for anyone.

Two things to know if you are coming from 4.x:

1. Tangent as a VST3 plugin inside another DAW is retired. The 5.0 installer
   removes the old Tangent.vst3. Tangent still hosts your VST3 and LV2
   plugins.

2. 5.0 keeps its settings in a new folder and removes the old
   ~/.config/ivory folder the first time it starts. That folder held your
   settings, the chord names you taught Tangent, and your key. Your projects
   and recordings are not affected. After upgrading, paste your key again
   (right-click anywhere, Support Tangent..., Activate). It is in the email
   your key first came in, from this same address; if you cannot find it,
   reply and I will send it again. If you taught
   Tangent chord names you want to keep, copy the ~/.config/ivory folder
   somewhere safe before you start 5.0, and see the release notes for where
   to put it back.

Download (these links always give you the current version):

  macOS 11 or later, Apple Silicon or Intel
  https://github.com/ganten1998/tangent/releases/latest/download/Tangent-macos.pkg

  Windows 10 or later
  https://github.com/ganten1998/tangent/releases/latest/download/Tangent-windows-setup.exe

  Linux x86_64
  https://github.com/ganten1998/tangent/releases/latest/download/tangent-linux-x86_64.tar.gz

  Everything else (Linux aarch64, FreeBSD, the .dmg and .zip, checksums, the
  full release notes):
  https://github.com/ganten1998/tangent/releases/latest

On Windows every program in the download is signed now. SmartScreen may still
say "Windows protected your PC" the first time; click More info, then Run
anyway.

Thank you for supporting it. Replies come to me.

Ganten
