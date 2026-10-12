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
9), and the key facts said the way every other surface says them.

Publish it only after the 5.0.0 release is public and
`scripts/check-store-links.sh` is green from a signed-out machine. Then
`bash upload.sh` in `ganten-site/` (Dropbox strips the exec bit). If the old
screenshots are replaced, move retired files out of `media/` first and prune
them with `bash prune-remote.sh <path>`, because Neocities uploads never
delete.

---

## Short version (one paragraph, for a list or a social post)

Tangent 5.0: play a MIDI keyboard and Tangent names the chord, writes it on a
staff, shows it on a guitar neck and records the take as audio, MIDI and
video. Around it is a recording DAW with buses, returns, hardware inserts,
layered instruments and plugin hosting. Free for macOS, Windows, Linux and
FreeBSD with up to 32 tracks; a supporter key takes the limit away.

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
              Play a chord. Tangent names it. All 88 keys light as you play
              them while a weighted-scoring engine reads the voicing, from
              plain triads through altered dominants, rootless jazz voicings
              and slash chords. It writes the chord on a staff, shows it on a
              guitar neck where a hand would actually play it, and draws it on
              the circle of fifths and a Tonnetz.
            </p>
            <img src="media/apps/tangent-theory.png" alt="Tangent naming a C major 13 with a 6 and a 9, showing the circle of fifths, a Tonnetz and the harmonic triangles above a piano keyboard and a guitar neck" loading="lazy">
            <p>
              Then it records the take: one press writes the audio, the MIDI
              and a video of the window itself, camera included. Since 5.0 it
              is a recording DAW too: an arrangement, a MIDI editor, a mixer
              with buses and returns, hardware inserts for outboard gear,
              layered instruments, automation, and VST3 and LV2 plugins that
              follow the song's tempo.
            </p>
            <img src="media/apps/tangent-dark.png" alt="Tangent in dark mode naming an altered dominant chord, on an ebony fingerboard" loading="lazy">
            <p>
              Written in Rust, free and MIT licensed, for macOS, Windows,
              Linux and FreeBSD. The free app holds up to 32 tracks, and a
              supporter key takes the track limit away; buses, returns,
              sends and routes have no limit for anyone. It makes no network
              connections of its own: no telemetry, no update check, no
              account.
            </p>
            <p>
              <a href="https://github.com/ganten1998/tangent/releases/latest" target="_blank" rel="noopener">Download</a><span class="sep">&middot;</span><a href="https://github.com/ganten1998/tangent" target="_blank" rel="noopener">Source</a><span class="sep">&middot;</span><a href="https://ganten.gumroad.com/l/ivory" target="_blank" rel="noopener">Support it</a>
            </p>
            <p class="post__tags">
              <span class="tag">rust</span>
              <span class="tag">egui</span>
              <span class="tag">midi</span>
              <span class="tag">daw</span>
              <span class="tag">chord detection</span>
            </p>
          </div>
        </article>
```

Notes:

- "no network connections of its own" is deliberate: Stream to OBS sends
  audio to the address you give it once you switch it on, and a plugin you
  load is its maker's program. The README's Privacy section says
  the same.
- The owner decided (2026-10-12) to make the GitHub repository public at
  publish time. Until it is, the Download and Source links are 404s for
  every visitor, so upload this card only after it is public (CHECKLIST.md,
  D1).
