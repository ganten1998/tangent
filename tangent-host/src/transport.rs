//! **Where the song is, told to a hosted plugin.**
//!
//! Until this module every VST3 `process` call went out with
//! `processContext: null`, and an LV2 plugin was never sent a
//! `time:Position`. A plugin asked for the tempo and was told nothing, so
//! Superior Drummer's "follow host" sat at its own default, a tempo-synced
//! delay echoed at whatever it assumed, and an arpeggiator or a drum machine
//! ran free of the song. Nothing failed: a null context is legal, and a
//! plugin that gets none simply falls back.
//!
//! [`Transport`] is the one fact both formats are built from: the timeline
//! position of the FIRST frame of the block being processed, with the tempo
//! and the signature in force there. The engine fills it per rack call from
//! its own published anchor (see `rack_transport` in
//! `tangent/src/instrument.rs`), so a lane rendered a grain ahead of the
//! playhead carries its own position rather than the playhead's, and an
//! offline export carries the position it is rendering rather than "now".
//!
//! Both builders here are pure functions of a `Transport` and allocate
//! nothing, so they are safe on the audio thread and testable without a
//! plugin.

use vst3::Steinberg::Vst::ProcessContext;
use vst3::Steinberg::Vst::ProcessContext_::StatesAndFlags_ as F;

/// The song's position and pulse at the first frame of one block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transport {
    /// The transport is rolling: the position advances by the block length.
    pub playing: bool,
    /// The arrangement is recording (a punch is live on some lane).
    pub recording: bool,
    /// Timeline position in samples at the device rate. Stopped, the parked
    /// playhead.
    pub sample: i64,
    /// The same position in QUARTER notes, read off the tempo map.
    pub quarters: f64,
    /// The quarter on which the bar holding `quarters` began.
    pub bar_start: f64,
    /// That bar's index, from 0.
    pub bar: i64,
    /// Quarter-note beats per minute at `quarters`, ramps included.
    pub tempo: f64,
    /// The signature in force in that bar.
    pub numerator: i32,
    pub denominator: i32,
    /// The loop, in quarters, when one is set. Set means ACTIVE here: the
    /// engine has no "defined but switched off" loop.
    pub cycle: Option<(f64, f64)>,
}

impl Default for Transport {
    /// Stopped at the top of a 4/4 song at 120. What a plugin is told before
    /// the engine has published anything, which is still a valid tempo.
    fn default() -> Self {
        Self {
            playing: false,
            recording: false,
            sample: 0,
            quarters: 0.0,
            bar_start: 0.0,
            bar: 0,
            tempo: 120.0,
            numerator: 4,
            denominator: 4,
            cycle: None,
        }
    }
}

impl Transport {
    /// The tempo, made safe to hand to a plugin: a non-finite or
    /// non-positive one would be a divide-by-zero inside somebody's LFO.
    fn safe_tempo(&self) -> f64 {
        if self.tempo.is_finite() && self.tempo > 0.0 {
            self.tempo
        } else {
            120.0
        }
    }

    fn safe_sig(&self) -> (i32, i32) {
        (self.numerator.max(1), self.denominator.max(1))
    }
}

/// An all-zero `ProcessContext`, the starting point [`fill_context`] writes
/// over. A const so an instance can hold one without `unsafe`.
pub const EMPTY_CONTEXT: ProcessContext = ProcessContext {
    state: 0,
    sampleRate: 0.0,
    projectTimeSamples: 0,
    systemTime: 0,
    continousTimeSamples: 0,
    projectTimeMusic: 0.0,
    barPositionMusic: 0.0,
    cycleStartMusic: 0.0,
    cycleEndMusic: 0.0,
    tempo: 0.0,
    timeSigNumerator: 0,
    timeSigDenominator: 0,
    chord: vst3::Steinberg::Vst::Chord { keyNote: 0, rootNote: 0, chordMask: 0 },
    smpteOffsetSubframes: 0,
    frameRate: vst3::Steinberg::Vst::FrameRate { framesPerSecond: 0, flags: 0 },
    samplesToNextClock: 0,
};

/// Write `t` into a VST3 `ProcessContext`, in place.
///
/// `continuous` is the instance's own count of samples processed since it
/// was activated, which is exactly what `continousTimeSamples` means: a
/// clock that never jumps on a locate or a loop wrap and keeps running while
/// the transport is stopped.
///
/// **What is claimed valid, and what is not.** Tempo, signature, musical
/// position, bar position and continuous time are real, so their flags are
/// set every block, stopped or rolling. System time, SMPTE, chord and the
/// MIDI clock are not tracked, so their flags stay clear and their fields
/// zero: a plugin that sees a flag set is entitled to believe the field.
pub fn fill_context(ctx: &mut ProcessContext, t: &Transport, rate: f64, continuous: i64) {
    let mut state = F::kTempoValid
        | F::kTimeSigValid
        | F::kProjectTimeMusicValid
        | F::kBarPositionValid
        | F::kContTimeValid;
    if t.playing {
        state |= F::kPlaying;
    }
    if t.recording {
        state |= F::kRecording;
    }
    let (cycle_a, cycle_b) = match t.cycle {
        Some((a, b)) if b > a && a.is_finite() && b.is_finite() => {
            state |= F::kCycleActive | F::kCycleValid;
            (a, b)
        }
        _ => (0.0, 0.0),
    };
    let (num, den) = t.safe_sig();
    *ctx = ProcessContext {
        state: state as u32,
        sampleRate: rate,
        projectTimeSamples: t.sample,
        systemTime: 0,
        continousTimeSamples: continuous,
        projectTimeMusic: finite_or_zero(t.quarters),
        barPositionMusic: finite_or_zero(t.bar_start),
        cycleStartMusic: cycle_a,
        cycleEndMusic: cycle_b,
        tempo: t.safe_tempo(),
        timeSigNumerator: num,
        timeSigDenominator: den,
        ..EMPTY_CONTEXT
    };
}

fn finite_or_zero(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

// ── LV2 ──────────────────────────────────────────────────────────────────────

/// The URIDs a `time:Position` object is spelled in, mapped once per plugin
/// instance off the audio thread (the URID map takes a lock).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeUrids {
    pub object: u32,
    pub long: u32,
    pub int: u32,
    pub float: u32,
    pub position: u32,
    pub frame: u32,
    pub speed: u32,
    pub bar: u32,
    pub bar_beat: u32,
    pub beat_unit: u32,
    pub beats_per_bar: u32,
    pub beats_per_minute: u32,
}

impl TimeUrids {
    /// The URIs, NUL-terminated, in field order. The caller maps each one
    /// through its own URID map, which is the only one the plugin knows.
    pub const URIS: [&'static [u8]; 12] = [
        b"http://lv2plug.in/ns/ext/atom#Object\0",
        b"http://lv2plug.in/ns/ext/atom#Long\0",
        b"http://lv2plug.in/ns/ext/atom#Int\0",
        b"http://lv2plug.in/ns/ext/atom#Float\0",
        b"http://lv2plug.in/ns/ext/time#Position\0",
        b"http://lv2plug.in/ns/ext/time#frame\0",
        b"http://lv2plug.in/ns/ext/time#speed\0",
        b"http://lv2plug.in/ns/ext/time#bar\0",
        b"http://lv2plug.in/ns/ext/time#barBeat\0",
        b"http://lv2plug.in/ns/ext/time#beatUnit\0",
        b"http://lv2plug.in/ns/ext/time#beatsPerBar\0",
        b"http://lv2plug.in/ns/ext/time#beatsPerMinute\0",
    ];

    /// Build from `map`, called once per URI in [`TimeUrids::URIS`] order.
    pub fn mapped(mut map: impl FnMut(&std::ffi::CStr) -> u32) -> Self {
        let mut ids = [0u32; 12];
        for (id, uri) in ids.iter_mut().zip(Self::URIS) {
            // The table above is all NUL-terminated literals; a malformed one
            // maps to 0, which no plugin will match, rather than panicking.
            *id = std::ffi::CStr::from_bytes_with_nul(uri).map_or(0, &mut map);
        }
        let [object, long, int, float, position, frame, speed, bar, bar_beat, beat_unit, beats_per_bar, beats_per_minute] =
            ids;
        Self {
            object,
            long,
            int,
            float,
            position,
            frame,
            speed,
            bar,
            bar_beat,
            beat_unit,
            beats_per_bar,
            beats_per_minute,
        }
    }
}

/// Bytes in a `time:Position` object BODY as [`position_body`] lays it out:
/// the 8-byte object header and seven properties of 24 bytes each (an 8-byte
/// property header, an 8-byte atom header and a value padded to 8).
pub const POSITION_BODY: usize = 8 + 7 * 24;

/// The body of an `atom:Object` of type `time:Position` for `t`, ready to
/// be the payload of an event whose atom type is `urids.object`.
///
/// The fields are the ones every LV2 host that sends time sends (Ardour,
/// Carla, jalv): `frame`, `speed` (1 rolling, 0 stopped), `bar`, `barBeat`
/// (beats of the signature's unit into the bar, not quarters), `beatUnit`,
/// `beatsPerBar` and `beatsPerMinute`. The last is the QUARTER-note tempo,
/// which is what the tempo map holds and what those hosts send beside a
/// `beatUnit` of the signature's denominator.
///
/// A fixed array, built on the stack: nothing here allocates.
pub fn position_body(t: &Transport, urids: &TimeUrids) -> [u8; POSITION_BODY] {
    let mut out = [0u8; POSITION_BODY];
    let mut at = 0usize;
    let mut put = |bytes: &[u8]| {
        if let Some(dst) = out.get_mut(at..at + bytes.len()) {
            dst.copy_from_slice(bytes);
        }
        at += bytes.len();
    };
    // LV2_Atom_Object_Body: id (0 = blank), otype.
    put(&0u32.to_ne_bytes());
    put(&urids.position.to_ne_bytes());
    let (num, den) = t.safe_sig();
    let bar_beat = finite_or_zero((t.quarters - t.bar_start).max(0.0)) * f64::from(den) / 4.0;
    // (key, atom type, value bytes, value size). Every value is padded to 8.
    let props: [(u32, u32, [u8; 8], u32); 7] = [
        (urids.frame, urids.long, t.sample.max(0).to_ne_bytes(), 8),
        (urids.speed, urids.float, pad4(if t.playing { 1.0f32 } else { 0.0 }), 4),
        (urids.bar, urids.long, t.bar.max(0).to_ne_bytes(), 8),
        (urids.bar_beat, urids.float, pad4(bar_beat as f32), 4),
        (urids.beat_unit, urids.int, pad4i(den), 4),
        (urids.beats_per_bar, urids.float, pad4(num as f32), 4),
        (urids.beats_per_minute, urids.float, pad4(t.safe_tempo() as f32), 4),
    ];
    for (key, ty, value, size) in props {
        // LV2_Atom_Property_Body: key, context (0), then the value's atom.
        put(&key.to_ne_bytes());
        put(&0u32.to_ne_bytes());
        put(&size.to_ne_bytes());
        put(&ty.to_ne_bytes());
        put(&value);
    }
    out
}

fn pad4(v: f32) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&v.to_ne_bytes());
    b
}

fn pad4i(v: i32) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&v.to_ne_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rolling_at(quarters: f64) -> Transport {
        Transport {
            playing: true,
            recording: false,
            sample: 96_000,
            quarters,
            bar_start: 4.0,
            bar: 1,
            tempo: 100.0,
            numerator: 3,
            denominator: 4,
            cycle: None,
        }
    }

    #[test]
    fn a_rolling_block_says_so_with_every_field_it_claims() {
        let mut ctx = EMPTY_CONTEXT;
        fill_context(&mut ctx, &rolling_at(5.5), 48_000.0, 1234);
        let s = ctx.state;
        for f in [
            F::kPlaying,
            F::kTempoValid,
            F::kTimeSigValid,
            F::kProjectTimeMusicValid,
            F::kBarPositionValid,
            F::kContTimeValid,
        ] {
            assert_ne!(s & f as u32, 0, "flag {f} missing from {s:#x}");
        }
        for f in [F::kRecording, F::kCycleActive, F::kCycleValid, F::kSystemTimeValid, F::kSmpteValid, F::kClockValid, F::kChordValid] {
            assert_eq!(s & f as u32, 0, "flag {f} claimed without a value behind it");
        }
        assert_eq!(ctx.sampleRate, 48_000.0);
        assert_eq!(ctx.projectTimeSamples, 96_000);
        assert_eq!(ctx.continousTimeSamples, 1234);
        assert_eq!(ctx.projectTimeMusic, 5.5);
        assert_eq!(ctx.barPositionMusic, 4.0);
        assert_eq!(ctx.tempo, 100.0);
        assert_eq!((ctx.timeSigNumerator, ctx.timeSigDenominator), (3, 4));
        assert_eq!(ctx.samplesToNextClock, 0);
        assert_eq!(ctx.systemTime, 0);
    }

    #[test]
    fn stopped_keeps_the_tempo_and_the_parked_place() {
        let mut ctx = EMPTY_CONTEXT;
        let t = Transport { playing: false, ..rolling_at(5.5) };
        fill_context(&mut ctx, &t, 44_100.0, 0);
        assert_eq!(ctx.state & F::kPlaying as u32, 0);
        assert_ne!(ctx.state & F::kTempoValid as u32, 0, "a stopped host still has a tempo");
        assert_eq!(ctx.projectTimeSamples, 96_000);
        assert_eq!(ctx.projectTimeMusic, 5.5);
    }

    #[test]
    fn a_loop_and_a_take_are_flagged() {
        let mut ctx = EMPTY_CONTEXT;
        let t = Transport { recording: true, cycle: Some((8.0, 16.0)), ..rolling_at(9.0) };
        fill_context(&mut ctx, &t, 48_000.0, 0);
        assert_ne!(ctx.state & F::kRecording as u32, 0);
        assert_ne!(ctx.state & F::kCycleActive as u32, 0);
        assert_ne!(ctx.state & F::kCycleValid as u32, 0);
        assert_eq!((ctx.cycleStartMusic, ctx.cycleEndMusic), (8.0, 16.0));
        // An empty loop is no loop.
        let t = Transport { cycle: Some((8.0, 8.0)), ..t };
        fill_context(&mut ctx, &t, 48_000.0, 0);
        assert_eq!(ctx.state & (F::kCycleActive | F::kCycleValid) as u32, 0);
    }

    #[test]
    fn a_nonsense_tempo_or_signature_never_reaches_a_plugin() {
        let mut ctx = EMPTY_CONTEXT;
        let t = Transport { tempo: f64::NAN, numerator: 0, denominator: 0, ..rolling_at(1.0) };
        fill_context(&mut ctx, &t, 48_000.0, 0);
        assert_eq!(ctx.tempo, 120.0);
        assert_eq!((ctx.timeSigNumerator, ctx.timeSigDenominator), (1, 1));
    }

    fn urids() -> TimeUrids {
        let mut next = 100u32;
        TimeUrids::mapped(|_| {
            next += 1;
            next
        })
    }

    fn word(b: &[u8], at: usize) -> u32 {
        u32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
    }

    /// Walk the object the way `LV2_ATOM_OBJECT_FOREACH` does and return
    /// (key, type, size, value bytes) per property.
    fn props(body: &[u8]) -> Vec<(u32, u32, u32, Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 8;
        while at + 16 <= body.len() {
            let key = word(body, at);
            let size = word(body, at + 8);
            let ty = word(body, at + 12);
            let v = body[at + 16..at + 16 + size as usize].to_vec();
            out.push((key, ty, size, v));
            // lv2_atom_pad_size(sizeof(PropertyBody) + size)
            at += (16 + size as usize).div_ceil(8) * 8;
        }
        out
    }

    #[test]
    fn a_time_position_reads_back_as_lv2_walks_it() {
        let u = urids();
        // 6/8 at a dotted feel: the bar began at quarter 3, we are at 4.5,
        // which is 1.5 quarters = 3 eighths into the bar.
        let t = Transport {
            playing: true,
            recording: false,
            sample: 48_000,
            quarters: 4.5,
            bar_start: 3.0,
            bar: 1,
            tempo: 90.0,
            numerator: 6,
            denominator: 8,
            cycle: None,
        };
        let body = position_body(&t, &u);
        assert_eq!(word(&body, 0), 0, "blank object id");
        assert_eq!(word(&body, 4), u.position);
        let p = props(&body);
        assert_eq!(p.len(), 7);
        let get = |key: u32| p.iter().find(|x| x.0 == key).expect("property present").clone();
        let f32_of = |v: &[u8]| f32::from_ne_bytes(v[..4].try_into().unwrap());
        let (_, ty, _, v) = get(u.frame);
        assert_eq!(ty, u.long);
        assert_eq!(i64::from_ne_bytes(v[..8].try_into().unwrap()), 48_000);
        let (_, ty, _, v) = get(u.speed);
        assert_eq!(ty, u.float);
        assert_eq!(f32_of(&v), 1.0);
        let (_, _, _, v) = get(u.bar);
        assert_eq!(i64::from_ne_bytes(v[..8].try_into().unwrap()), 1);
        assert_eq!(f32_of(&get(u.bar_beat).3), 3.0, "barBeat is in eighths in 6/8");
        let (_, ty, _, v) = get(u.beat_unit);
        assert_eq!(ty, u.int);
        assert_eq!(i32::from_ne_bytes(v[..4].try_into().unwrap()), 8);
        assert_eq!(f32_of(&get(u.beats_per_bar).3), 6.0);
        assert_eq!(f32_of(&get(u.beats_per_minute).3), 90.0);
        // Stopped: speed 0, position kept.
        let body = position_body(&Transport { playing: false, ..t }, &u);
        let p = props(&body);
        assert_eq!(f32_of(&p.iter().find(|x| x.0 == u.speed).unwrap().3), 0.0);
    }
}
