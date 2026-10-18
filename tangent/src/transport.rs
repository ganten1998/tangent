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
//! is derived and safe to re-assert every frame. The one thing that keys off
//! its edges is the tape motor (`crate::motor`), on the audio thread, and only
//! while `Shared::motor_ok` lets it.
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
    /// Whether the transport rolls. **The only thing that decides whether
    /// the song is meant to roll.** With tape start and stop on, the engine
    /// keeps a stopping tape rolling until it has stopped
    /// (`Engine::motor_coasting`); that is the stop finishing, not a second
    /// switch.
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
    /// **The engine's tape was coasting to a stop at the last push**
    /// (`Engine::motor_coasting`), so the push after it carries where the
    /// tape came to rest back once more. See `push`.
    coasting: bool,
    /// **The pending locate is Stop's own return to the cursor**: set by
    /// `stop`, cleared by every other locate. While the engine has not
    /// applied it, the tape may still be on its way to the coast that holds
    /// it, and is drawn where it is (`shown_s`).
    returning: bool,
}

impl Transport {
    pub fn new() -> Self {
        Self {
            pos_s: 0.0,
            generation: 1,
            playing: false,
            cursor_s: 0.0,
            coasting: false,
            returning: false,
        }
    }

    /// Put the playhead somewhere, now. Legal while rolling — that is the
    /// point of the generation — and legal with no engine, where it simply
    /// waits to be published.
    pub fn locate(&mut self, seconds: f64) {
        self.pos_s = seconds.max(0.0);
        self.cursor_s = self.pos_s;
        self.generation += 1;
        self.returning = false;
    }

    /// Move the playhead WITHOUT moving the edit cursor — the return-to-
    /// cursor stop uses it, or stopping twice would walk the cursor.
    ///
    /// **And an arrangement record pressed while the tape coasts to a stop**
    /// pins the record's start with it (`start_arr_record`): the engine
    /// applies that seek as the motor snaps to speed, and the edit cursor
    /// stays where the hand put it.
    pub fn locate_playhead(&mut self, seconds: f64) {
        self.pos_s = seconds.max(0.0);
        self.generation += 1;
        self.returning = false;
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
        self.returning = true;
    }

    /// **The generation of the latest locate**, the one the next `push`
    /// publishes: what a realtime pass ties its land to
    /// (`Engine::land_gains_at`), so it lands the block that plays from
    /// there and not whichever callback comes first.
    pub fn generation(&self) -> u64 {
        self.generation
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
    ///
    /// **The REQUESTED position, for the host's own logic**: a record's start
    /// and finish, the take's facts, an export's end. Through a Stop's coast
    /// (`Engine::motor_coasting`) that is the cursor the stop returns to,
    /// unacked until the tape has stopped; what is DRAWN then is
    /// [`Transport::shown_s`].
    pub fn position_s(&self, engine: Option<&Engine>, rate: f64) -> f64 {
        match engine {
            Some(e) if e.transport_acked(self.generation) && rate > 0.0 => {
                e.transport_position() as f64 / rate
            }
            _ => self.pos_s,
        }
    }

    /// **The playhead as it is drawn, and as the hand edits it**: the
    /// engine's own clock while Stop's return waits on a moving tape,
    /// otherwise [`Transport::position_s`].
    ///
    /// One authority at a time, extended by one case: through a coast the
    /// line follows the tape as it slows, and every action at the playhead
    /// (a split, a paste, an automation point, the readouts, a nudge) acts
    /// at the line the user sees; when a Stop's coast ends, the line jumps
    /// to the cursor and they act there. A Pause's coast needs no case of
    /// its own: it holds no locate, so `position_s` is the engine's clock.
    ///
    /// **From the frame after the press, not from the first pass that
    /// coasts.** The band draws before the frame's push (`fill_recorder_state`
    /// runs before `after_frame`, and Stop lands in `after_frame`'s request
    /// loop, after the push), so the frame after a Stop draws before the
    /// engine has been told anything: the tape still shows Running and the
    /// return is unacked. Asked only while the tape coasted, that frame drew
    /// the requested position, the cursor, and the next ones the coasting
    /// tape again, a blink at every Stop, longer with a callback longer than
    /// a pass. So while the pending locate is Stop's own return and the
    /// engine has not applied it, a tape that coasts, or that the host lets
    /// move and is not at rest, is drawn where it is. With the motor off a
    /// Stop draws the cursor at once, as it always has; on a desk where the
    /// engine keeps the motor down itself (a tracking desk, a hardware loop)
    /// the line waits at the stopped tape for the frame before the ack. Any
    /// other locate, a ruler click during a coast among them, is drawn where
    /// it was asked at once, which is the lag `position_s` was built to
    /// remove.
    pub fn shown_s(&self, engine: Option<&Engine>, rate: f64) -> f64 {
        match engine {
            Some(e) if rate > 0.0 && self.returning && !e.transport_acked(self.generation) && Self::moving(e) => {
                e.transport_position() as f64 / rate
            }
            _ => self.position_s(engine, rate),
        }
    }

    /// **The engine's tape is moving, or may still be**: coasting, or not at
    /// rest with the host letting the motor move (`Engine::motor_armed`), so
    /// a Stop it has not yet read may yet coast.
    fn moving(e: &Engine) -> bool {
        use crate::motor::MotorShow;
        e.motor_coasting() || (e.motor_armed() && !matches!(e.motor_show(), MotorShow::Parked | MotorShow::Holding))
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
        // **A tape coasting to a stop is still the truth**, and so is where
        // it came to rest: carried back while it coasts and once when it
        // settles, so an engine rebuilt after a Pause is re-located where
        // the tape stopped and not where Pause was pressed. A Stop's coast is
        // not acked (its return is held), so this never overwrites the
        // cursor.
        let coasting = e.motor_coasting();
        if acked && !self.playing && (coasting || self.coasting) {
            self.pos_s = e.transport_position() as f64 / rate;
        }
        self.coasting = coasting;
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

    /// An offline engine whose tape motor is armed, 0.5 s up and 0.7 s down,
    /// and the transport that drives it.
    fn armed() -> (Engine, Transport) {
        let e = Engine::offline(48_000, tangent_record::audio::Timebase::new())
            .expect("an offline engine needs no device");
        e.set_tape_motor(true, 0.5, 0.7);
        (e, Transport::new())
    }

    /// **The shown playhead follows a coasting tape, and the cursor holds
    /// the host's logic.**
    ///
    /// Through a Stop's coast the tape is still moving and its return is
    /// held, unacked: what is DRAWN is where the tape is (`shown_s`, the
    /// engine's clock, climbing), and what the host's own logic reads is the
    /// position it asked for, the cursor (`position_s`), so a record started
    /// or a take placed then is placed at the cursor. When the tape stops,
    /// both are the cursor.
    ///
    /// Driven in `DesktopApp::update`'s own order from the frame after the
    /// press: the band draws (`fill_recorder_state`), then the frame's push,
    /// then the callbacks until the next frame, one 256-frame pass a frame
    /// or six (a 33 ms frame at 48 kHz holds about six). The press lands in
    /// a frame's request loop, after its push, so the first frame after it
    /// is drawn before the engine has heard of it: the line must not blink
    /// to the cursor there and back onto the tape, as it did whenever only
    /// a coasting tape was drawn where it is.
    #[test]
    fn the_shown_playhead_follows_a_coasting_tape_and_the_cursor_holds_the_logic() {
        for per_frame in [1usize, 6] {
            let what = format!("{per_frame} passes a frame");
            let (mut e, mut t) = armed();
            let rate = 48_000.0;
            t.locate(1.0);
            t.toggle_play();
            let mut out = vec![0.0f32; 2 * 256];
            let frame = |e: &mut Engine, t: &mut Transport, out: &mut [f32]| {
                t.push(Some(&*e));
                for _ in 0..per_frame {
                    e.render_offline(out);
                }
            };
            for _ in 0..40 {
                frame(&mut e, &mut t, &mut out);
            }
            let pressed = e.transport_position() as f64 / rate;
            // The press, in this frame's request loop, after its push.
            t.stop();
            let mut drawn = Vec::new();
            let mut coasted = false;
            for _ in 0..400 {
                drawn.push(t.shown_s(Some(&e), rate));
                assert!((t.position_s(Some(&e), rate) - 1.0).abs() < 1e-9, "{what}: the host's logic left the cursor mid-coast");
                frame(&mut e, &mut t, &mut out);
                coasted |= e.motor_coasting();
                if coasted && !e.motor_coasting() {
                    drawn.push(t.shown_s(Some(&e), rate));
                    break;
                }
            }
            assert!(coasted, "{what}: the stop did not coast");
            let home = drawn
                .iter()
                .position(|s| (s - 1.0).abs() < 1e-9)
                .unwrap_or_else(|| panic!("{what}: the drawn playhead never came home to the cursor"));
            assert!(home > 10, "{what}: the line came home {home} frames after the press, before the tape could have stopped");
            assert!(
                drawn[..home].iter().all(|s| *s >= pressed - 1e-9),
                "{what}: the line was drawn behind the tape before it came home: {:?}",
                &drawn[..home.min(6)]
            );
            assert!(drawn[..home].windows(2).all(|w| w[1] >= w[0]), "{what}: the drawn playhead went backwards through the coast");
            assert!(
                drawn[home..].iter().all(|s| (s - 1.0).abs() < 1e-9),
                "{what}: the drawn playhead left the cursor after it came home: {:?}",
                &drawn[..drawn.len().min(6)]
            );
            assert!((t.shown_s(Some(&e), rate) - 1.0).abs() < 1e-9, "{what}: the drawn playhead is not at the cursor after the stop");
            assert!((t.position_s(Some(&e), rate) - 1.0).abs() < 1e-9);
        }
    }

    /// **A click during a coast is drawn where it lands, at once.**
    ///
    /// A ruler click while the tape coasts to a stop is a locate
    /// (`R::Locate`), which also cuts the coast (`Engine::cut_motor`) so the
    /// engine applies it after the house's 10 ms declick rather than when the
    /// coast would have ended. It is not Stop's return, so from the very
    /// frame after the click the line is drawn where it was asked, as every
    /// click is (`position_s`), and not at the coasting tape until the cut
    /// lands. Frame by frame in `update`'s order, the click landing in a
    /// frame's request loop.
    #[test]
    fn a_click_during_a_coast_is_drawn_where_it_lands_at_once() {
        let (mut e, mut t) = armed();
        let rate = 48_000.0;
        t.locate(1.0);
        t.toggle_play();
        let mut out = vec![0.0f32; 2 * 256];
        for _ in 0..40 {
            t.push(Some(&e));
            e.render_offline(&mut out);
        }
        t.stop();
        for _ in 0..20 {
            t.push(Some(&e));
            e.render_offline(&mut out);
        }
        assert!(e.motor_coasting(), "the stop did not coast, so nothing was proved");
        t.locate(3.0);
        e.cut_motor();
        for k in 0..30 {
            let drawn = t.shown_s(Some(&e), rate);
            assert!((drawn - 3.0).abs() < 1e-9, "{k} frames after the click the line was drawn at {drawn}");
            t.push(Some(&e));
            e.render_offline(&mut out);
        }
        assert!(e.transport_acked(t.generation()) && !e.motor_coasting(), "the click never landed");
    }

    /// **A Pause's coast is remembered where the tape stopped** (the
    /// review's C11).
    ///
    /// Pause holds no return, so the tape comes to rest a coast past the
    /// press. The host's own copy is carried back while it coasts and once
    /// more when it settles, so an engine rebuilt after the Pause (a device
    /// change, the band reopening) is re-located where the tape stopped and
    /// not where Pause was pressed: up to 0.39 of the stop time apart.
    #[test]
    fn a_pause_coast_is_remembered_where_the_tape_stopped() {
        let (mut e, mut t) = armed();
        t.locate(1.0);
        t.toggle_play();
        let mut out = vec![0.0f32; 2 * 256];
        for _ in 0..40 {
            t.push(Some(&e));
            e.render_offline(&mut out);
        }
        let pressed = e.transport_position();
        t.toggle_play();
        for _ in 0..400 {
            t.push(Some(&e));
            e.render_offline(&mut out);
        }
        assert!(!e.motor_coasting());
        let rested = e.transport_position();
        assert!(rested > pressed + 10_000, "the pause did not coast");
        let remembered = t.position_s(None, 0.0);
        assert!(
            (remembered * 48_000.0 - rested as f64).abs() <= 1.0,
            "the host remembers {remembered} s, the tape stopped at {rested}"
        );
        // A fresh engine, pushed afterwards, is located there.
        let mut fresh = Engine::offline(48_000, tangent_record::audio::Timebase::new())
            .expect("an offline engine needs no device");
        t.push(Some(&fresh));
        fresh.render_offline(&mut out);
        assert!(
            (fresh.transport_position() as f64 - rested as f64).abs() <= 1.0,
            "a rebuilt engine was located at {}, not where the tape stopped ({rested})",
            fresh.transport_position()
        );
    }

    /// **Pinning the playhead leaves the cursor where the hand put it** (the
    /// review's C12).
    ///
    /// A record pressed while the tape coasts pins its own start
    /// (`start_arr_record`), and that pin must not become the place the next
    /// Stop returns to: the cursor is the hand's alone.
    #[test]
    fn pinning_the_playhead_leaves_the_cursor_where_the_hand_put_it() {
        let mut t = Transport::new();
        t.locate(1.0);
        t.toggle_play();
        t.locate_playhead(5.0);
        assert!((t.position_s(None, 0.0) - 5.0).abs() < 1e-9, "the pin did not move the playhead");
        t.stop();
        assert!(
            (t.position_s(None, 0.0) - 1.0).abs() < 1e-9,
            "Stop returned to {} rather than the cursor the hand put at 1.0",
            t.position_s(None, 0.0)
        );
    }
}
