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
    /// The locate is published BEFORE the level rises, which ORDERS the two
    /// without making them one fact: `run_transport` loads the generation
    /// before the level, so a callback can still read the old generation with
    /// the level already up and roll one block from the old position. On the
    /// way down the return-to-cursor locate is stored BEFORE the level falls,
    /// so a callback landing between the two stores - or reading the Relaxed
    /// level stale after Acquiring the new generation - rolls one block at
    /// the cursor. Publishing `set_rolling(false)` first closes the falling
    /// half.
    ///
    /// No session in the signature any more: see `playing` for why a take has
    /// no say in whether the transport rolls.
    pub fn push(&mut self, engine: Option<&Engine>) {
        let rolling = self.playing;
        let Some(e) = engine else {
            return;
        };
        let rate = f64::from(e.output().sample_rate).max(1.0);
        // While rolling and acked, the engine's clock is the truth — carried
        // back into seconds continuously, so an engine that dies mid-roll is
        // survived by the position it had reached, not by where it started.
        if rolling && e.transport_acked(self.generation) {
            self.pos_s = e.transport_position() as f64 / rate;
        }
        if !e.transport_acked(self.generation) {
            let frames = (self.pos_s.max(0.0) * rate) as u64;
            e.set_transport(self.generation, frames);
        }
        e.set_rolling(rolling);
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
