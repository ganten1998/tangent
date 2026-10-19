# Tangent 5.0.0 announcement

For existing users: supporters by email, and the same text for a post.
Written from scratch for 5.0 (owner, 2026-10-12): it tells a supporter that
Tangent is now a DAW before it tells them what changed, and names no
third-party product.

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

"Replies come to me": the deployed `MAIL_REPLY_TO` is keys@tangentdaw.org,
which Porkbun forwards to a read inbox (owner, 2026-10-12). Past keys were
sent from the old sender domain, so the body offers a resend on reply
instead of naming the address.

---

Subject: Tangent 5.0 is out

Hi,

Tangent 5.0 is out: record, arrange and mix on macOS, Windows, Linux and
FreeBSD. Free with up to 32 tracks; your key removes the limit.

You bought a key for a program that named the chords you played and could
record them. This release is the one where it becomes a DAW.

You can record audio and MIDI into it, arrange clips on a timeline, edit
notes in a piano roll, and mix on a desk with buses, returns, sends, folders
and automation. It hosts your VST3 plugins (and LV2 on Linux and FreeBSD) and
tells them the song's tempo and transport. It also comes with its own
instruments, among them modelled pianos and electric pianos, an analog-style
synth and a sampler, and a shelf of effects from EQ and compression to tape,
reverb, a tube amp and a bitcrusher. Tangent Insert puts your outboard gear
in an insert slot and keeps the session in time with it. Exports can pick
their tracks and their range. FreeBSD is now a supported platform too.

It still names the chord you are playing, and a take can still film the
window.

What your key does now: the free app holds up to 32 tracks, and your key
removes that limit. Every key ever sold does this, so there is nothing for
you to do. Buses, returns, sends and routes have no limit for anyone.

Two things to know before you upgrade from 4.x:

1. Tangent as a VST3 plugin inside another DAW is gone. The 5.0 installers
   remove the old Tangent.vst3 on macOS and Windows; on Linux, delete
   ~/.vst3/Tangent.vst3 yourself. Tangent still hosts your plugins.

2. 5.0 keeps its settings in a new folder and does not read the ones 4.x
   kept, so your settings, taught chord names and key start fresh. Your
   projects and recordings are not touched. After upgrading, paste your key
   again (right-click anywhere, Support Tangent..., Activate). It is in the
   email your key first came in. If you cannot find it, reply to this email
   or to your Gumroad receipt and I will send it again.

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

On Windows every program in the download is signed. SmartScreen may still say
"Windows protected your PC" the first time; click More info, then Run
anyway.

Thank you for supporting it. Replies come to me.

Ganten
