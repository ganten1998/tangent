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

**Screenshot:** the card below keeps the two images already on the site.
Neither shows the arrangement or the mixer, which is what the card now
leads with. A shot of the arrangement with the mixer would serve it better;
if one is taken, put it in `media/apps/` and swap it in for the first image.

---

## Short version (one paragraph, for a list or a social post)

Tangent 5.0 is out, and Tangent is now a DAW. Record audio and MIDI, arrange
and edit it, and mix with buses, returns, sends, folders and automation,
using your own VST3 or LV2 plugins or the instruments and effects it comes
with. It still names the chord you are playing. Free for macOS, Windows,
Linux and FreeBSD with up to 32 tracks; a supporter key removes the limit.

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
              A DAW for recording, arranging and mixing. Tangent records
              audio and MIDI, lays clips out on a timeline, edits notes in a
              piano roll and mixes them on a desk with buses, returns, sends,
              folders and automation. It hosts VST3 plugins everywhere and
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
            <img src="media/apps/tangent-dark.png" alt="Tangent in dark mode naming an altered dominant chord, on an ebony fingerboard" loading="lazy">
            <p>
              Written in Rust, MIT licensed, for macOS, Windows, Linux and
              FreeBSD. It is free with up to 32 tracks, and a supporter key
              removes the track limit; buses, returns, sends and routes have
              no limit for anyone. It makes no network connections of its
              own: no telemetry, no update check, no account.
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
