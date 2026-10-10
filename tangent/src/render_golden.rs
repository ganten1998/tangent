//! **Render goldens: what today's engine sounds like, to the bit.**
//!
//! The engine is about to be rebuilt around boxed lanes in a slot spine, and
//! the promise that rebuild makes is that no desk which exists today changes
//! by a single sample. A promise like that has to be measured before
//! anything moves, so this pins it: two dozen desks of built-in units only
//! (no third-party plugin can be on a build machine), each rendered offline
//! through today's public `Engine` API, hashed (FNV-1a over the output's f32
//! BITS, so a change of one ulp is a different hash), and compared against
//! `tests/fixtures/renders/golden.json`.
//!
//! The desks cover what the rebuild touches: folders nested three deep, a
//! route chain, a cycle, a hardware route, sends through Return 0's
//! built-ins, a drum lane with pad FX, the DX7, Keys and the sampler,
//! automation on gain, pan, send, mute, the master and a built-in parameter,
//! a latency unit (PDC), solo inside a folder, phase, a frozen lane, wet and
//! dry stems, a sidechain key, and the grain and fork renders on and off.
//!
//! **aarch64 macOS only for the hashes.** Floats are deterministic per
//! platform, not across them: the x86 build fuses differently and its libm
//! is another libm. Off that target the golden test is ignored, and the
//! invariants beside it (every desk sounds, every desk renders the same
//! twice, the fork renders what the single walk rendered) run everywhere.
//!
//! `TANGENT_GOLDEN_WRITE=1` regenerates the file instead of comparing. Do it
//! only for a change that is MEANT to change the sound, and say which in the
//! commit.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tangent_record::audio::Timebase;
use tangent_ui::automation::{AutoParam, Automation, Curve, Point, Shape};
use tangent_ui::recorder::Desk;
use tangent_ui::routing::{Route, Routes, Target};

use crate::builtin_fx::{
    CHORUS_PATH, COMP_PATH, DENOISE_PATH, EQ_PATH, PLATE_PATH, SAMPLER_PATH, TRIM_PATH,
};
use crate::instrument::{
    ClipNote, Engine, PadKit, Placed, PlacedMidi, SamplerSample, CHANNELS, MASTER_AUTO_LANE,
    SAMPLER_ROOT_KEY,
};

/// The rate every desk renders at.
pub const RATE: u32 = 48_000;
/// The block the goldens are rendered in.
const BLOCK: usize = 512;
/// Blocks rendered stopped first, so every hand-off ring (kits, voices,
/// samples, plugins) has delivered before the song starts.
const SETTLE: usize = 8;
/// Blocks hashed: about half a second, long enough for reverb tails, the
/// comp's release and a pad's decay to carry state through many blocks.
const HASHED: usize = 48;

/// An offline engine with today's defaults made explicit: nothing armed, no
/// lane listening to live MIDI, nothing sending, the master at unity.
pub fn offline() -> Engine {
    let e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
    e.set_armed(0);
    e.set_midi_off(u64::MAX);
    e.set_desk(&quiet_desk());
    e.set_master_gain(1.0);
    e
}

/// Nothing sends, nothing is muted, nothing is soloed.
pub fn quiet_desk() -> Desk {
    // Each array as wide as the desk's own type says: no width named here.
    Desk {
        send: std::array::from_fn(|_| 0.0),
        muted: std::array::from_fn(|_| false),
        soloed: std::array::from_fn(|_| false),
    }
}

/// Seconds of seeded stereo noise. A different seed per lane so two lanes
/// never cancel or sum into something a mask bug could hide behind.
pub fn noise(seconds: f64, seed: u32) -> Arc<tangent_record::decode::Clip> {
    let frames = (seconds * f64::from(RATE)) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut x: u32 = seed | 1;
    for _ in 0..frames {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let n = (x as f32 / u32::MAX as f32).mul_add(2.0, -1.0);
        samples.push(n * 0.25);
        samples.push(n * 0.21);
    }
    Arc::new(tangent_record::decode::Clip {
        samples,
        rate: RATE,
        source: PathBuf::from("render-golden-noise.wav"),
    })
}

/// A noise clip on `lane` for the whole render.
fn clip_on(lane: usize) -> Placed {
    let clip = noise(1.0, 0x9e37 + lane as u32 * 7919);
    let len = clip.frames() as u64;
    Placed {
        clip,
        strip: lane,
        start: 0,
        offset: 0,
        len,
        fade_in: 0,
        fade_out: 0,
        gain: 0.5,
    }
}

fn clips(e: &mut Engine, lanes: &[usize]) {
    e.set_clips(lanes.iter().map(|&l| clip_on(l)).collect());
}

fn seat(e: &mut Engine, seat: usize, bay: usize, unit: &str, state: Option<&[u8]>) {
    e.load_insert(seat, bay, Some(Path::new(unit)), state)
        .unwrap_or_else(|err| panic!("{unit} did not load into seat {seat}: {err}"))
        .unwrap_or_else(|| panic!("{unit} loaded but seat {seat} bay {bay} reports no tenant"));
}

/// A built-in's state blob with its first `values` set.
fn state(unit: &str, values: &[(usize, f32)]) -> Vec<u8> {
    let spec = crate::builtin_fx::spec_for(unit).expect("a built-in");
    let mut v = spec.defaults();
    for &(i, x) in values {
        v[i] = x;
    }
    crate::builtin_fx::state_text(spec, &v, &[]).into_bytes()
}

fn notes(strip: usize, keys: &[(u64, u64, u8)]) -> PlacedMidi {
    PlacedMidi {
        strip,
        start: 0,
        len: u64::from(RATE) * 2,
        notes: Arc::new(
            keys.iter()
                .map(|&(at, dur, key)| ClipNote {
                    at,
                    dur,
                    key,
                    vel: 110,
                })
                .collect(),
        ),
        pedal: Arc::new(Vec::new()),
    }
}

fn routes(list: &[(usize, Target, f32)]) -> Routes {
    Routes(
        list.iter()
            .map(|&(from, target, gain)| Route { from, target, gain })
            .collect(),
    )
}

fn folders(pairs: &[(usize, i32)]) -> [i32; CHANNELS] {
    let mut p = [-1i32; CHANNELS];
    for &(child, parent) in pairs {
        p[child] = parent;
    }
    p
}

fn curve(lane: usize, param: AutoParam, points: &[(f64, f64)]) -> Curve {
    Curve::sanitised(
        lane,
        param,
        points
            .iter()
            .map(|&(at_s, value)| Point {
                at_s,
                value,
                shape: Shape::Linear,
            })
            .collect(),
    )
}

/// The three-insert chain the bench measures, on four lanes.
fn inserts_desk() -> Engine {
    let mut e = offline();
    clips(&mut e, &[0, 1, 2, 3]);
    for lane in 0..4 {
        seat(&mut e, lane, 0, EQ_PATH, None);
        seat(&mut e, lane, 1, COMP_PATH, None);
        seat(&mut e, lane, 2, PLATE_PATH, None);
    }
    e
}

/// One desk: a name, how to build it with and without the one feature it is
/// there to pin, and whether its stems are hashed too.
///
/// **The "without" twin is what makes a golden worth having.** A desk whose
/// feature silently stopped doing anything would still render, still repeat
/// itself, and still match a golden recorded after the break. So every desk
/// that pins a feature is also rendered with that feature switched off, and
/// the two must differ: the feature reaches the sound.
pub struct GoldenDesk {
    pub name: &'static str,
    pub build: fn(bool) -> Engine,
    /// Whether `build(false)` is a real twin. A desk that pins no single
    /// feature (the plain desk, the insert chain) has none.
    pub twin: bool,
    pub stems: bool,
}

fn plain(_: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[0, 1, 2, 31]);
    e
}

fn folders_nested(on: bool) -> Engine {
    // 0 holds 1, which holds 2, which holds 3; 4 sits in 1; 5 is loose.
    let mut e = offline();
    clips(&mut e, &[0, 3, 4, 5]);
    if on {
        assert!(e.set_folders(folders(&[(1, 0), (2, 1), (3, 2), (4, 1)])));
    }
    seat(&mut e, 1, 0, COMP_PATH, None);
    seat(&mut e, 2, 0, EQ_PATH, None);
    e.set_channel_gain(0, 0.8);
    e
}

fn route_chain(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[0, 1]);
    if on {
        e.set_routes(&routes(&[
            (1, Target::Channel(2), 0.7),
            (2, Target::Channel(3), 0.5),
        ]));
    }
    seat(&mut e, 3, 0, PLATE_PATH, None);
    e
}

fn route_cycle(on: bool) -> Engine {
    // 1 and 2 feed each other, 4 feeds itself, and both cycles drain into
    // 3, which reaches the master: everything that hangs off a cycle runs in
    // the tail, one block late.
    let mut e = offline();
    clips(&mut e, &[0, 1, 2, 4]);
    let mut list = vec![
        (1, Target::Channel(2), 0.6),
        (2, Target::Channel(3), 0.5),
        (4, Target::Channel(3), 0.5),
        (2, Target::Channel(5), 0.0),
    ];
    if on {
        list.push((2, Target::Channel(1), 0.4));
        list.push((4, Target::Channel(4), 0.3));
    }
    e.set_routes(&routes(&list));
    e
}

fn route_hardware(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[1, 2]);
    if on {
        e.set_routes(&routes(&[(
            1,
            Target::Hardware {
                first: 0,
                stereo: true,
            },
            0.8,
        )]));
    }
    e
}

fn sends_return0(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[0, 1]);
    let mut desk = quiet_desk();
    if on {
        desk.send[0] = 0.7;
        desk.send[1] = 0.3;
    }
    e.set_desk(&desk);
    e.set_effects(crate::effects::Sends {
        reverb: 0.6,
        delay: 0.4,
        chorus: 0.5,
        phaser: 0.5,
        ..crate::effects::Sends::default()
    });
    e.set_fx_return(0.9);
    e
}

fn drums_pad_fx(on: bool) -> Engine {
    let mut e = offline();
    let mut kit = PadKit::default();
    kit.pads[0] = Some(noise(0.1, 0xd00d));
    kit.pads[1] = Some(noise(0.05, 0xbeef));
    assert!(e.set_pad_kit(6, kit));
    e.set_drum_strips(1 << 6);
    e.set_channel_gain(6, 1.0);
    if on {
        seat(
            &mut e,
            tangent_ui::recorder::pad_rack_index(6, 0),
            0,
            COMP_PATH,
            Some(&state(COMP_PATH, &[(0, 0.5), (1, 1.0)])),
        );
    }
    e.set_midi_clips(vec![notes(
        6,
        &[(0, 2_400, 0), (6_000, 2_400, 1), (12_000, 2_400, 0)],
    )]);
    e
}

fn dx7(on: bool) -> Engine {
    let mut e = offline();
    // Without the voice the lane is an ordinary MIDI lane with nothing to
    // play it, so the twin is the clip on a lane with no instrument, and a
    // loose audio lane keeps the twin from being silence.
    clips(&mut e, &[9]);
    if on {
        e.set_builtin_strips(1 << 2);
        e.sync_instrument_lanes(1 << 2, 0);
    }
    e.set_channel_gain(2, 0.3);
    e.set_midi_clips(vec![notes(2, &[(0, 12_000, 48), (6_000, 12_000, 55)])]);
    e
}

fn keys(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[9]);
    if on {
        e.sync_instrument_lanes(0, 1 << 1);
        e.set_keys_strips(1 << 1);
    }
    e.set_channel_gain(1, 0.8);
    e.set_midi_clips(vec![notes(1, &[(0, 9_000, 60), (3_000, 9_000, 64)])]);
    e
}

fn sampler(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[9]);
    seat(&mut e, 3, 0, SAMPLER_PATH, None);
    if on {
        assert!(e.set_sampler_sample(
            3,
            Some(SamplerSample {
                clip: noise(0.5, 0x5a5a),
                name: "golden.wav".to_owned(),
                source: PathBuf::from("golden.wav"),
                root_key: SAMPLER_ROOT_KEY,
            })
        ));
    }
    e.set_channel_gain(3, 0.5);
    e.set_midi_clips(vec![notes(
        3,
        &[
            (0, 9_000, SAMPLER_ROOT_KEY),
            (4_000, 9_000, SAMPLER_ROOT_KEY + 7),
        ],
    )]);
    e
}

/// One lane of noise and one automation curve on it (or not).
fn automated(on: bool, lanes: &[usize], auto: fn() -> Curve) -> Engine {
    let mut e = offline();
    clips(&mut e, lanes);
    e.set_effects(crate::effects::Sends {
        reverb: 0.6,
        ..crate::effects::Sends::default()
    });
    if on {
        e.set_automation(Automation::from_curves(vec![auto()]));
    }
    e
}

fn auto_gain(on: bool) -> Engine {
    automated(on, &[0], || {
        curve(0, AutoParam::Gain, &[(0.0, 0.2), (0.4, 0.9)])
    })
}

fn auto_pan(on: bool) -> Engine {
    automated(on, &[0], || {
        curve(0, AutoParam::Pan, &[(0.0, 0.0), (0.4, 1.0)])
    })
}

fn auto_send(on: bool) -> Engine {
    automated(on, &[0], || {
        curve(0, AutoParam::Send, &[(0.0, 0.0), (0.3, 1.0)])
    })
}

fn auto_mute(on: bool) -> Engine {
    automated(on, &[0, 1], || {
        Curve::sanitised(
            0,
            AutoParam::Mute,
            vec![
                Point {
                    at_s: 0.0,
                    value: 0.0,
                    shape: Shape::Hold,
                },
                Point {
                    at_s: 0.2,
                    value: 1.0,
                    shape: Shape::Hold,
                },
            ],
        )
    })
}

fn auto_master(on: bool) -> Engine {
    automated(on, &[0, 1], || {
        curve(MASTER_AUTO_LANE, AutoParam::Gain, &[(0.0, 0.9), (0.4, 0.3)])
    })
}

fn auto_builtin_param(on: bool) -> Engine {
    let mut e = automated(on, &[0], || {
        let id = crate::builtin_fx::spec_for(TRIM_PATH).expect("trim").params[0].id;
        curve(
            0,
            AutoParam::Plugin { bay: 0, id },
            &[(0.0, 0.2), (0.4, 0.9)],
        )
    });
    seat(&mut e, 0, 0, TRIM_PATH, None);
    e
}

fn pdc(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[0, 1]);
    // The twin keeps a unit in the bay with no latency, so the difference is
    // the compensation and not merely the denoise's own colour on lane 0.
    seat(&mut e, 0, 0, if on { DENOISE_PATH } else { EQ_PATH }, None);
    e
}

fn solo_in_folder(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[3, 4, 5]);
    assert!(e.set_folders(folders(&[(3, 2), (4, 2)])));
    let mut desk = quiet_desk();
    desk.soloed[4] = on;
    e.set_desk(&desk);
    e
}

fn phase(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[0, 1]);
    if on {
        e.set_strip_phase(1 << 1);
    }
    e
}

fn frozen(on: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[0, 1]);
    seat(
        &mut e,
        0,
        0,
        TRIM_PATH,
        Some(&state(TRIM_PATH, &[(0, 0.9)])),
    );
    seat(
        &mut e,
        1,
        0,
        TRIM_PATH,
        Some(&state(TRIM_PATH, &[(0, 0.9)])),
    );
    if on {
        e.set_frozen(1 << 0);
    }
    e
}

/// Stems on, wet or dry. The twin of the dry desk is the wet one.
fn stems(dry: bool) -> Engine {
    let mut e = offline();
    clips(&mut e, &[0, 2]);
    seat(&mut e, 0, 0, CHORUS_PATH, None);
    e.set_stems(true);
    e.set_stems_dry(dry);
    e
}

fn stems_wet(_: bool) -> Engine {
    stems(false)
}

fn sidechain_key(on: bool) -> Engine {
    // Lane 2 keys the master's comp and reaches nothing else: it is routed
    // into lane 15, whose fader is shut. Lane 1 is the mix.
    let mut e = offline();
    let master = crate::instrument::MASTER_SEAT;
    seat(
        &mut e,
        master,
        0,
        COMP_PATH,
        Some(&state(COMP_PATH, &[(0, 0.5), (1, 1.0), (2, 0.0), (3, 0.0)])),
    );
    if on {
        e.set_insert_keys(&[(master, 0, 2)]);
    }
    e.set_routes(&routes(&[(2, Target::Channel(15), 1.0)]));
    e.set_channel_gain(15, 0.0);
    clips(&mut e, &[1, 2]);
    e
}

/// The insert chain with the grain render on: 10 ms, 480 frames, which is
/// not a whole number of 512-frame blocks, the case that once cost a shipped
/// bug (`a_grain_that_is_not_a_whole_number_of_blocks_is_the_same_audio`).
fn grains(_: bool) -> Engine {
    let e = inserts_desk();
    e.set_grain_ms(10.0);
    e
}

fn fork(_: bool) -> Engine {
    let mut e = inserts_desk();
    e.set_offline_fanout(true);
    e
}

fn grains_and_fork(_: bool) -> Engine {
    let mut e = grains(true);
    e.set_offline_fanout(true);
    e
}

/// Every desk, in the order the file lists them.
pub fn desks() -> Vec<GoldenDesk> {
    fn d(name: &'static str, build: fn(bool) -> Engine) -> GoldenDesk {
        GoldenDesk {
            name,
            build,
            twin: true,
            stems: false,
        }
    }
    fn lone(name: &'static str, build: fn(bool) -> Engine) -> GoldenDesk {
        GoldenDesk {
            name,
            build,
            twin: false,
            stems: false,
        }
    }
    vec![
        lone("plain", plain),
        lone("inserts", |_| inserts_desk()),
        d("folders-nested-3", folders_nested),
        d("route-chain", route_chain),
        d("route-cycle", route_cycle),
        d("route-hardware", route_hardware),
        d("sends-return0", sends_return0),
        d("drums-pad-fx", drums_pad_fx),
        d("dx7", dx7),
        d("keys", keys),
        d("sampler", sampler),
        d("auto-gain", auto_gain),
        d("auto-pan", auto_pan),
        d("auto-send", auto_send),
        d("auto-mute", auto_mute),
        d("auto-master", auto_master),
        d("auto-builtin-param", auto_builtin_param),
        d("pdc", pdc),
        d("solo-in-folder", solo_in_folder),
        d("phase", phase),
        d("frozen", frozen),
        GoldenDesk {
            name: "stems-wet",
            build: stems_wet,
            twin: false,
            stems: true,
        },
        GoldenDesk {
            name: "stems-dry",
            build: stems,
            twin: true,
            stems: true,
        },
        d("sidechain-key", sidechain_key),
        // The grain render and the fork are pinned by EQUALITY with the
        // plain insert desk, below, not by difference: neither may change
        // the sound at all (`the_fork_and_the_grain_are_one_sound`). Their
        // goldens still matter: a rebuild that broke both the same way would
        // keep them equal and move the hash.
        lone("grains-on", grains),
        lone("fork-on", fork),
        lone("grains-and-fork", grains_and_fork),
    ]
}

/// FNV-1a, 64-bit, over the little-endian bytes of each sample's bits.
fn fnv(h: &mut u64, samples: &[f32]) {
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            *h ^= u64::from(b);
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// One render of a desk: its hash, and the loudest sample it made.
pub fn render(desk: &GoldenDesk) -> (String, f32) {
    render_built(desk, true)
}

/// One render of a desk with its feature on or off.
fn render_built(desk: &GoldenDesk, on: bool) -> (String, f32) {
    let mut e = (desk.build)(on);
    let mut out = vec![0.0f32; BLOCK * 2];
    let mut stems = Vec::new();
    e.set_rolling(false);
    for _ in 0..SETTLE {
        out.fill(0.0);
        e.render_offline(&mut out);
        e.drain_stems(&mut stems);
    }
    stems.clear();
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut peak = 0.0f32;
    for _ in 0..HASHED {
        out.fill(0.0);
        e.render_offline(&mut out);
        fnv(&mut h, &out);
        peak = out.iter().fold(peak, |m, s| m.max(s.abs()));
        if desk.stems {
            e.drain_stems(&mut stems);
        }
    }
    if desk.stems {
        assert!(
            !stems.is_empty(),
            "{}: the stem tap delivered nothing",
            desk.name
        );
        fnv(&mut h, &stems);
    }
    (format!("{h:016x}"), peak)
}

/// `f` over every desk, three desks at a time, answers in desk order.
///
/// Three hands and not one per core: the gate runs this binary beside other
/// work, and each desk is independent (its own engine, built on the thread
/// that renders it, since an `Engine` is not `Send`).
fn over_desks<T: Send>(f: impl Fn(&GoldenDesk) -> T + Sync) -> Vec<(&'static str, T)> {
    let all = desks();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut got: Vec<(usize, T)> = std::thread::scope(|sc| {
        let hands: Vec<_> = (0..3)
            .map(|_| {
                sc.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(desk) = all.get(i) else { break };
                        mine.push((i, f(desk)));
                    }
                    mine
                })
            })
            .collect();
        hands
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    });
    got.sort_by_key(|(i, _)| *i);
    got.into_iter().map(|(i, t)| (all[i].name, t)).collect()
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/renders/golden.json")
}

/// **Every desk renders to its recorded hash.**
///
/// A mismatch names every desk that moved, not just the first, because a
/// change that moves one desk usually moves a family and the family is the
/// diagnosis.
#[test]
#[cfg_attr(
    not(all(target_arch = "aarch64", target_os = "macos")),
    ignore = "float bits are pinned on aarch64 macOS only"
)]
fn every_desk_renders_its_golden() {
    let got: BTreeMap<String, String> = over_desks(|d| render(d).0)
        .into_iter()
        .map(|(name, h)| (name.to_owned(), h))
        .collect();
    let path = golden_path();
    if std::env::var("TANGENT_GOLDEN_WRITE").as_deref() == Ok("1") {
        let text = serde_json::to_string_pretty(&got).expect("serialise") + "\n";
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the fixture dir");
        std::fs::write(&path, text).expect("write the goldens");
        return;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (TANGENT_GOLDEN_WRITE=1 records it)",
            path.display()
        )
    });
    let want: BTreeMap<String, String> = serde_json::from_str(&text).expect("the goldens parse");
    let moved: Vec<String> = got
        .iter()
        .filter(|(k, v)| want.get(*k) != Some(v))
        .map(|(k, v)| format!("{k}: {v} (recorded {:?})", want.get(k)))
        .collect();
    assert!(
        moved.is_empty(),
        "render goldens moved:\n{}",
        moved.join("\n")
    );
    let stale: Vec<&String> = want.keys().filter(|k| !got.contains_key(*k)).collect();
    assert!(
        stale.is_empty(),
        "goldens recorded for desks that no longer exist: {stale:?}"
    );
}

/// **The goldens are worth pinning, on every arch**: each desk makes a
/// sound, renders the same twice, its feature reaches the sound (the twin
/// without it renders differently), and the fork renders what the single
/// walk rendered. A golden of a silent desk, of one that is not
/// deterministic, or of a feature that does nothing, would pin nothing.
#[test]
fn every_golden_desk_sounds_and_repeats_itself() {
    let hashes: BTreeMap<&'static str, String> = over_desks(|desk| {
        let (a, peak) = render(desk);
        assert!(peak > 1.0e-3, "{}: the desk is silent ({peak})", desk.name);
        assert!(
            peak.is_finite() && peak < 8.0,
            "{}: the desk blew up ({peak})",
            desk.name
        );
        let (b, _) = render(desk);
        assert_eq!(a, b, "{}: two renders of one desk differ", desk.name);
        if desk.twin {
            let (without, _) = render_built(desk, false);
            assert_ne!(
                a, without,
                "{}: the desk sounds the same without its feature",
                desk.name
            );
        }
        a
    })
    .into_iter()
    .collect();
    let names: std::collections::BTreeSet<_> = hashes.values().collect();
    assert_eq!(
        names.len(),
        hashes.len() - 3,
        "two desks that should differ rendered alike: {hashes:#?}"
    );
    assert_eq!(
        hashes["inserts"], hashes["fork-on"],
        "the fork changed the sound of the insert desk"
    );
    assert_eq!(
        hashes["inserts"], hashes["grains-on"],
        "the grain render changed the sound of the insert desk"
    );
    assert_eq!(
        hashes["grains-on"], hashes["grains-and-fork"],
        "the fork changed the sound of the grained desk"
    );
}
