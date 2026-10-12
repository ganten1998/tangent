# ganten.neocities.org card for Tangent 5.0.0

Replaces the Tangent `<article>` in
`~/Dropbox/Projects/Web/ganten-site/projects.html` (lines 131 to 178 on
2026-10-12). The live card is stale in four ways:

- it says `v3.0.0` and dates itself 2026-08-14;
- it describes Tangent "inside your DAW, as a VST3 plugin", which is retired
  (owner ruling 2026-10-10: remove every user-facing mention);
- every link uses the old slug `github.com/ganten1998/ivory`, which only works
  through GitHub's rename redirect (and a deleted or recreated repository
  loses that redirect);
- it says nothing about the 32-track free desk or what a key does.

Rules for this copy: no em dashes, no mention of the Konami code (owner call
9), no third-party product names, and the key facts said the way every other
surface says them. Written from scratch for 5.0 (owner, 2026-10-12): the card
used to describe a chord display that also records, and it now describes a
DAW.

Publish it only after the 5.0.0 release is public and
`scripts/check-store-links.sh` is green from a signed-out machine. Then
`bash upload.sh` in `ganten-site/` (Dropbox strips the exec bit). If the old
screenshots are replaced, move retired files out of `media/` first and prune
them with `bash prune-remote.sh <path>`, because Neocities uploads never
delete.

**Screenshots:** the card leads with two new shots and keeps the theory
image already on the site. Before uploading, copy
`docs/img/screenshot-arrangement.png` and `docs/img/screenshot-mixer.png`
from this repository to `ganten-site/media/apps/tangent-arrangement.png` and
`ganten-site/media/apps/tangent-mixer.png` (1600x900 PNGs). `upload.sh`
sends them with the page; the card's `src` paths are
`media/apps/tangent-arrangement.png` and `media/apps/tangent-mixer.png`.
`media/apps/tangent-dark.png` is no longer used by the card: move it out of
`media/` and prune it with `bash prune-remote.sh media/apps/tangent-dark.png`
if nothing else on the site uses it.

---

## Short version (one paragraph, for a list or a social post)

Tangent 5.0: record, arrange and mix on macOS, Windows, Linux and FreeBSD.
Tangent is now a DAW. Record audio and MIDI, arrange
and edit it, and mix with buses, returns, sends, folders and automation,
using your own VST3 or LV2 plugins or the instruments and effects it comes
with. It still names the chord you are playing. Free with up to 32 tracks;
a supporter key removes the limit.

---

## The card (paste-ready HTML)

```html
        <article class="post">
          <header class="post__head">
            <h2 class="post__title">
              <a href="https://github.com/ganten1998/tangent" target="_blank" rel="noopener">Tangent</a>
            </h2>
            <span class="post__status" data-status="live">released</span>
            <time class="post__date" datetime="2026-10-12">2026 &middot; v5.0.0</time>
          </header>
          <div class="post__body">
            <p>
              Record, arrange and mix on macOS, Windows, Linux and FreeBSD.
              Free with up to 32 tracks; a supporter key removes the limit.
            </p>
            <img src="media/apps/tangent-arrangement.png" alt="Tangent's arrangement: drum, piano and audio tracks with MIDI and audio clips and a volume automation lane, under the recorder band" loading="lazy">
            <p>
              Tangent is a DAW. It records audio and MIDI, lays clips out on
              a timeline, edits notes in a piano roll and mixes them on a desk
              with buses, returns, sends, folders and automation. It hosts VST3 plugins everywhere and
              LV2 on Linux and FreeBSD, and it ships its own instruments and
              effects: modelled keys, an FM synth, an analog-style synth, a
              sampler, a drum machine, and a shelf of effects from EQ and
              compression to tape, reverb and a tube amp.
            </p>
            <img src="media/apps/tangent-theory.png" alt="Tangent naming a C major 13, with the circle of fifths, a Tonnetz and the harmonic triangles above a piano keyboard and a guitar neck" loading="lazy">
            <p>
              It started as a program that names the chord you play, and it
              still does: as you play, it writes the chord on a staff, places
              it on a guitar neck and draws it on the circle of fifths. A take
              can film the window with your camera in it, and outboard gear
              can sit in an insert slot, measured and in time with the rest
              of the session.
            </p>
            <img src="media/apps/tangent-mixer.png" alt="Tangent's mixer: a strip per track with inserts, routing, sends and faders, a folder, a return and the master" loading="lazy">
            <p>
              Written in Rust and MIT licensed. Buses, returns, sends and
              routes have no limit for anyone, free or not. It makes no
              network connections of its own: no telemetry, no update check,
              no account.
            </p>
            <p>
              <a href="https://github.com/ganten1998/tangent/releases/latest" target="_blank" rel="noopener">Download</a><span class="sep">&middot;</span><a href="https://github.com/ganten1998/tangent" target="_blank" rel="noopener">Source</a><span class="sep">&middot;</span><a href="https://ganten.gumroad.com/l/ivory" target="_blank" rel="noopener">Support it</a>
            </p>
            <p class="post__tags">
              <span class="tag">rust</span>
              <span class="tag">egui</span>
              <span class="tag">daw</span>
              <span class="tag">audio</span>
              <span class="tag">midi</span>
            </p>
          </div>
        </article>
```

Notes:

- "no network connections of its own" is deliberate: Stream to OBS sends
  audio to the address you give it once you switch it on, and a plugin you
  load is its maker's program. The README's Privacy section says the same.
- The owner decided (2026-10-12) to make the GitHub repository public at
  publish time. Until it is, the Download and Source links are 404s for
  every visitor, so upload this card only after it is public (CHECKLIST.md,
  D1).
