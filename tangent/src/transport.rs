//! The transport: one owner for the playhead, in seconds.
//!
//! # Why the HOST owns it, and the engine only answers
//!
//! The engine is dropped and rebuilt whenever the audio path changes — a
//! buffer size, a sample rate, an audio system, the band closing — and a
//! position that lived only in `Shared` silently returned to 0:00 mid-session
//! with nothing host-side to restore it from. So the playhead's home is here,
//! in SECONDS, because seconds survive a device that changes rate; the engine
//! holds the sample-exact copy and this struct reconciles the two.
//!
//! # The generation, and why a locate is an event
//!
//! `set_track_playing` was a LEVEL re-asserted sixty times a second, and the
//! audio thread found its edges — which made a locate inexpressible: a bare
//! "seek to N" store could not be told from the same value re-asserted. The
//! locate is now a GENERATION-stamped command (`Engine::set_transport`), the
//! callback acks the generation it has applied, and the level (`set_rolling`)
//! is derived and safe to re-assert because nothing keys off its edges.
//!
//! The generation is this struct's monotonic counter, starting at 1 and never
//! reset — deliberately, so a freshly built engine (whose applied generation
//! is 0) never matches and is re-located by the first push. That is the whole
//! of engine-rebuild recovery.
//!
//! # What the readout shows
//!
//! [`Transport::position_s`] promises exactly one live authority at a time:
//! the REQUESTED position until the engine acks it, the engine's sample clock
//! after, and the last known truth — never a surprise 0:00 — when there is no
//! engine at all. The gap it closes is small and visible: without it, a click
//! on the waveform drew the playhead at the old position for a frame (~16 ms)
//! and then snapped, which is the gesture the whole feature is judged by.

use crate::instrument::Engine;

/// The playhead, the play switch, and the bookkeeping that keeps the engine's
/// copy honest. One per app.
pub struct Transport {
    /// The playhead in seconds from 0:00. The one owner.
    pos_s: f64,
    /// Monotonic locate counter. Starts at 1 so a fresh engine (ack 0) always
    /// re-locates on the first push.
    generation: u64,
    /// Whether the transport rolls. **The only thing that decides it.**
    ///
    /// A take used to be ORed in: the session's rolling state made the engine
    /// roll whatever this said, so pressing TAKE played the song and nothing
    /// could stop the song without ending the take. The take is capture only
    /// now - a film of the window and the desk - and the transport is this
    /// switch's, under a take exactly as without one. `docs/ARRANGEMENT.md`
    /// said as much from the start: "Recording is TWO separate functions,
    /// never entangled."
    playing: bool,
    /// **The EDIT CURSOR: the position the last explicit locate chose** —
    /// a ruler click, a lane click, a seek. Play rolls from it; STOP
    /// returns to it, the owner's third and final ruling on the pair: the
    /// music always starts again from where the hand last pointed.
    cursor_s: f64,
}

impl Transport {
    pub fn new() -> Self {
        Self {
            pos_s: 0.0,
            generation: 1,
            playing: false,
            cursor_s: 0.0,
        }
    }

    /// Put the playhead somewhere, now. Legal while rolling — that is the
    /// point of the generation — and legal with no engine, where it simply
    /// waits to be published.
    pub fn locate(&mut self, seconds: f64) {
        self.pos_s = seconds.max(0.0);
        self.cursor_s = self.pos_s;
        self.generation += 1;
    }

    /// Move the playhead WITHOUT moving the edit cursor — the return-to-
    /// cursor stop uses it, or stopping twice would walk the cursor.
    fn locate_playhead(&mut self, seconds: f64) {
        self.pos_s = seconds.max(0.0);
        self.generation += 1;
    }

    /// The green button. Starts the audition from the playhead; pressed while
    /// auditioning, PAUSES — the playhead stays put, and the next press picks
    /// up from there. Going back to the edit cursor is the stop button's job,
    /// which is what makes the pair a tape transport rather than two spellings
    /// of stop.
    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    /// The square. Ends the audition and RETURNS TO THE EDIT CURSOR —
    /// wherever the hand last pointed the playhead. Play from bar 9, stop
    /// at bar 12: the playhead is back at 9, and play plays 9 again. Pause
    /// is the button that stays put.
    pub fn stop(&mut self) {
        self.playing = false;
        self.locate_playhead(self.cursor_s);
    }

    /// Whether the audition is running.
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// The playhead, for the readout and the waveform.
    ///
    /// One authority at a time: the pending locate until the engine has
    /// applied it, the sample clock after, the remembered position with no
    /// engine. `rate` is the engine's own, asked at the call site so this
    /// module never caches a number the device owns.
    pub fn position_s(&self, engine: Option<&Engine>, rate: f64) -> f64 {
        match engine {
            Some(e) if e.transport_acked(self.generation) && rate > 0.0 => {
                e.transport_position() as f64 / rate
            }
            _ => self.pos_s,
        }
    }

    /// Reconcile once a frame.
    ///
    /// The locate is published BEFORE the level rises, and `run_transport`
    /// loads the generation again after the level and rolls only on a pair
    /// it read whole, so the two arrive as one fact: a callback that meets
    /// the push between its own loads holds a chunk on the new place. While
    /// the generation was loaded once, before the level, such a callback read
    /// the old generation with the level already up and rolled one block
    /// from the old position. On the way down the level falls BEFORE the
    /// return-to-cursor locate (see `stores`): a callback between the two
    /// stores is a stopped one at the old position, and `set_transport`'s
    /// Release carries the fall, so a callback that Acquires the new
    /// generation cannot read the level up.
    /// The locate used to go first, and a callback landing between the two
    /// stores - or reading the Relaxed level stale - rolled one block at the
    /// cursor and left the playhead there.
    ///
    /// No session in the signature any more: see `playing` for why a take has
    /// no say in whether the transport rolls.
    pub fn push(&mut self, engine: Option<&Engine>) {
        let Some(e) = engine else {
            return;
        };
        let rate = f64::from(e.output().sample_rate).max(1.0);
        let acked = e.transport_acked(self.generation);
        for store in self.stores(acked, e.transport_position(), rate).into_iter().flatten() {
            match store {
                Store::Locate { generation, frames } => e.set_transport(generation, frames),
                Store::Rolling(rolling) => e.set_rolling(rolling),
            }
        }
    }

    /// **What `push` stores, in the order it stores them**, for an engine
    /// that has (`acked`) or has not applied this generation and whose clock
    /// stands at `engine_frames`. A callback can land between any two, so
    /// the order is the contract, and a list is what lets a test put a
    /// callback between them.
    fn stores(&mut self, acked: bool, engine_frames: u64, rate: f64) -> [Option<Store>; 2] {
        let rolling = self.playing;
        // While rolling and acked, the engine's clock is the truth — carried
        // back into seconds continuously, so an engine that dies mid-roll is
        // survived by the position it had reached, not by where it started.
        if rolling && acked {
            self.pos_s = engine_frames as f64 / rate;
        }
        let locate = (!acked).then(|| Store::Locate {
            generation: self.generation,
            frames: (self.pos_s.max(0.0) * rate) as u64,
        });
        // **On the way down the level falls FIRST.** A callback between the
        // two stores is then a stopped one at the old position, and the
        // locate's Release carries the fall with it, so a callback that
        // Acquires the new generation reads the level down. The return to
        // the cursor went first, and a callback between the two rolled a
        // block from the cursor. On the way up the locate still goes first:
        // see `push`.
        if rolling {
            [locate, Some(Store::Rolling(true))]
        } else {
            [Some(Store::Rolling(false)), locate]
        }
    }
}

/// **One store `push` makes**: see [`Transport::stores`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Store {
    /// `Engine::set_transport`: the locate, stamped with its generation.
    Locate { generation: u64, frames: u64 },
    /// `Engine::set_rolling`: the level.
    Rolling(bool),
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Stop lets the level fall BEFORE it returns to the cursor**, so a
    /// callback landing between the two stores is a stopped one. The locate
    /// went first, and a callback between them applied the return to the
    /// cursor and rolled one block from there: a block of the song heard
    /// after Stop, and the playhead left a block past the cursor, so the next
    /// Play started late. Driven store by store through `stores`, which is
    /// `push`'s own order, with a callback after each, on an offline engine.
    #[test]
    fn stop_lets_the_level_fall_before_it_returns_to_the_cursor() {
        let rate = 48_000u32;
        let mut e = Engine::offline(rate, tangent_record::audio::Timebase::new())
            .expect("an offline engine needs no device");
        // Play from the cursor at one second, and roll a while.
        let mut t = Transport::new();
        t.locate(1.0);
        t.toggle_play();
        let mut out = vec![0.0f32; 2 * 256];
        for _ in 0..20 {
            t.push(Some(&e));
            e.render_offline(&mut out);
        }
        assert!(e.transport_position() > 48_000 + 256, "the song never rolled from the cursor");
        // Stop, one store at a time, a callback after each.
        t.stop();
        let acked = e.transport_acked(t.generation);
        for store in t.stores(acked, e.transport_position(), f64::from(rate)).into_iter().flatten() {
            match store {
                Store::Locate { generation, frames } => e.set_transport(generation, frames),
                Store::Rolling(rolling) => e.set_rolling(rolling),
            }
            e.render_offline(&mut out);
        }
        assert_eq!(
            e.transport_position(),
            48_000,
            "a callback between Stop's two stores rolled a block from the cursor"
        );
    }

    /// **Wherever a callback meets `push`, it rolls only from where the host
    /// sent it.** It can meet a push between two of its stores, or in the
    /// middle of its own read of them: `run_transport` loads the generation
    /// and then the level, and the host's stores can land between the two.
    /// Play from a new place met there showed the callback the old
    /// generation with the level already up, and it rolled one block from
    /// where the playhead WAS, a moment of the wrong part of the song,
    /// before the locate landed. Play and Stop, each met after every store
    /// and with the whole push landing after each of the callback's loads
    /// (`instrument::meet_transport_loads_for_test`), on an offline engine:
    /// the callback that meets it, and the one after.
    #[test]
    fn wherever_a_callback_meets_a_push_it_rolls_only_from_where_it_was_sent() {
        use crate::instrument::{meet_transport_loads_for_test, TransportStores};
        use std::cell::Cell;
        use std::rc::Rc;
        const BLOCK: u64 = 256;
        let rate = 48_000u32;
        let cursor = u64::from(rate);
        let apply = |e: &Engine, store: Store| match store {
            Store::Locate { generation, frames } => e.set_transport(generation, frames),
            Store::Rolling(rolling) => e.set_rolling(rolling),
        };
        // `Between(k)`: the push's first `k` stores, the callback, the rest.
        // `Inside(n)`: all of them, landing after the callback's `n`th load.
        #[derive(Clone, Copy, Debug)]
        enum Meet {
            Between(usize),
            Inside(u32),
        }
        for play in [true, false] {
            for meet in [
                Meet::Between(0),
                Meet::Between(1),
                Meet::Between(2),
                Meet::Inside(1),
                Meet::Inside(2),
                Meet::Inside(3),
            ] {
                let mut e = Engine::offline(rate, tangent_record::audio::Timebase::new())
                    .expect("an offline engine needs no device");
                let mut out = vec![0.0f32; 2 * BLOCK as usize];
                // Play starts parked at two seconds; Stop, rolling from the
                // cursor at one.
                let mut t = Transport::new();
                if play {
                    t.locate(2.0);
                } else {
                    t.locate(1.0);
                    t.toggle_play();
                }
                for _ in 0..8 {
                    t.push(Some(&e));
                    e.render_offline(&mut out);
                }
                let was = e.transport_position();
                if play {
                    t.locate(1.0);
                    t.toggle_play();
                } else {
                    t.stop();
                }
                let acked = e.transport_acked(t.generation);
                let stores: Vec<Store> =
                    t.stores(acked, was, f64::from(rate)).into_iter().flatten().collect();
                match meet {
                    Meet::Between(k) => {
                        for &store in stores.iter().take(k) {
                            apply(&e, store);
                        }
                        e.render_offline(&mut out);
                        for &store in stores.iter().skip(k) {
                            apply(&e, store);
                        }
                    }
                    Meet::Inside(n) => {
                        let fired = Rc::new(Cell::new(false));
                        let (landed, all) = (Rc::clone(&fired), stores.clone());
                        meet_transport_loads_for_test(Some(Box::new(
                            move |loads: u32, host: &TransportStores<'_>| {
                                if loads != n || landed.get() {
                                    return;
                                }
                                landed.set(true);
                                for &store in &all {
                                    match store {
                                        Store::Locate { generation, frames } => {
                                            host.set_transport(generation, frames);
                                        }
                                        Store::Rolling(rolling) => host.set_rolling(rolling),
                                    }
                                }
                            },
                        )));
                        e.render_offline(&mut out);
                        meet_transport_loads_for_test(None);
                        // A read of fewer loads met it after the callback.
                        if !fired.get() {
                            for &store in &stores {
                                apply(&e, store);
                            }
                        }
                    }
                }
                let met = e.transport_position();
                e.render_offline(&mut out);
                let next = e.transport_position();
                let what = format!("{} met at {meet:?}", if play { "Play" } else { "Stop" });
                if play {
                    assert_ne!(
                        met,
                        was + BLOCK,
                        "{what}: the callback rolled a block from where the playhead was"
                    );
                    assert!(
                        [was, cursor, cursor + BLOCK].contains(&met),
                        "{what}: the callback left the playhead at {met}"
                    );
                    assert!(
                        [cursor + BLOCK, cursor + 2 * BLOCK].contains(&next),
                        "{what}: the callback after it left the playhead at {next}, not rolling from the cursor"
                    );
                } else {
                    assert_ne!(
                        met,
                        cursor + BLOCK,
                        "{what}: the callback rolled a block from the cursor after Stop"
                    );
                    assert!(
                        [was, was + BLOCK, cursor].contains(&met),
                        "{what}: the callback left the playhead at {met}"
                    );
                    assert_eq!(next, cursor, "{what}: the callback after it is not stopped at the cursor");
                }
            }
        }
    }

    /// **A note on the cursor sounds wherever a callback meets the press that
    /// plays from it.** The realtime export's own press (`start_bounce`:
    /// stop, locate to the span's head, play) pushes a locate and the level
    /// together, as does any frame that locates and plays at once. A
    /// callback that meets the push after the locate and before the level,
    /// or that sees the generation move between its own loads, holds one
    /// chunk stopped on the new place, and that chunk's render stamped the
    /// lanes' grain rings with the very span the next chunk rolls from. The
    /// next chunk was served it as it stood, so the note on the cursor never
    /// reached its instrument: a realtime export's first note was silent
    /// whenever a callback happened to meet its press there.
    ///
    /// One DX7 note on the cursor and nothing else on the desk, so a note
    /// that never reached its instrument is a silent render. That it sounds
    /// ONCE is `instrument::rolling_from_a_standstill`'s, which compares a
    /// render against the located one bit for bit.
    #[test]
    fn a_note_on_the_cursor_sounds_wherever_a_callback_meets_the_press_that_plays_from_it() {
        use crate::instrument::{meet_transport_loads_for_test, ClipNote, PlacedMidi, TransportStores};
        use std::cell::Cell;
        use std::rc::Rc;
        use std::sync::Arc;
        const LANE: usize = 2;
        let rate = 48_000u32;
        let apply = |e: &Engine, store: Store| match store {
            Store::Locate { generation, frames } => e.set_transport(generation, frames),
            Store::Rolling(rolling) => e.set_rolling(rolling),
        };
        #[derive(Clone, Copy, Debug)]
        enum Meet {
            Between(usize),
            Inside(u32),
        }
        for meet in [
            Meet::Between(0),
            Meet::Between(1),
            Meet::Between(2),
            Meet::Inside(1),
            Meet::Inside(2),
            Meet::Inside(3),
        ] {
            let mut e = Engine::offline(rate, tangent_record::audio::Timebase::new())
                .expect("an offline engine needs no device");
            e.set_builtin_strips(1 << LANE);
            e.sync_instrument_lanes(1 << LANE, 0);
            // Rendering ahead, as the app ships.
            e.set_grain_ms(-1.0);
            e.set_midi_clips(vec![PlacedMidi {
                pedal: Arc::new(Vec::new()),
                strip: LANE,
                start: 0,
                len: 4 * u64::from(rate),
                notes: Arc::new(vec![ClipNote { at: u64::from(rate), dur: 12_000, key: 60, vel: 110 }]),
            }]);
            let mut out = vec![0.0f32; 2 * 128];
            // Parked at two seconds, stopped, as the desk waits for the press.
            let mut t = Transport::new();
            t.locate(2.0);
            for _ in 0..8 {
                t.push(Some(&e));
                e.render_offline(&mut out);
            }
            // The export's press, which is a ruler click and Play as well:
            // the cursor at one second, on the note.
            t.stop();
            t.locate(1.0);
            t.toggle_play();
            let acked = e.transport_acked(t.generation);
            let stores: Vec<Store> = t
                .stores(acked, e.transport_position(), f64::from(rate))
                .into_iter()
                .flatten()
                .collect();
            match meet {
                Meet::Between(k) => {
                    for &store in stores.iter().take(k) {
                        apply(&e, store);
                    }
                    e.render_offline(&mut out);
                    for &store in stores.iter().skip(k) {
                        apply(&e, store);
                    }
                }
                Meet::Inside(n) => {
                    let fired = Rc::new(Cell::new(false));
                    let (landed, all) = (Rc::clone(&fired), stores.clone());
                    meet_transport_loads_for_test(Some(Box::new(move |loads: u32, host: &TransportStores<'_>| {
                        if loads != n || landed.get() {
                            return;
                        }
                        landed.set(true);
                        for &store in &all {
                            match store {
                                Store::Locate { generation, frames } => host.set_transport(generation, frames),
                                Store::Rolling(rolling) => host.set_rolling(rolling),
                            }
                        }
                    })));
                    e.render_offline(&mut out);
                    meet_transport_loads_for_test(None);
                    if !fired.get() {
                        for &store in &stores {
                            apply(&e, store);
                        }
                    }
                }
            }
            // And on, reconciled every callback as the app reconciles it.
            let mut peak = 0.0f32;
            for _ in 0..40 {
                t.push(Some(&e));
                out.fill(0.0);
                e.render_offline(&mut out);
                peak = out.iter().fold(peak, |m, s| m.max(s.abs()));
            }
            assert!(
                e.transport_position() > u64::from(rate),
                "{meet:?}: the song never rolled from the cursor"
            );
            assert!(peak > 0.01, "{meet:?}: the note on the cursor never sounded ({peak})");
        }
    }

    /// **Play pauses in place; only stop goes home.** The second press of
    /// play used to locate to 0:00, which made it a second stop button — a
    /// transport where you cannot stop to listen and carry on from there.
    #[test]
    fn play_pauses_in_place_and_stop_goes_home() {
        let mut t = Transport::new();
        t.locate(83.0);
        t.toggle_play();
        assert!(t.playing());
        t.push(None);
        t.toggle_play();
        assert!(!t.playing(), "the second press did not pause");
        t.push(None);
        assert!(
            (t.position_s(None, 0.0) - 83.0).abs() < 1e-9,
            "pausing moved the playhead to {}",
            t.position_s(None, 0.0)
        );
        t.stop();
        assert!(
            (t.position_s(None, 0.0) - 83.0).abs() < 1e-9,
            "stop moved the playhead away from where the music stopped"
        );
    }
}
