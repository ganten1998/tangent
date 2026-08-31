//! Track routing: where a channel goes instead of, or as well as, the master.
//!
//! Two destinations, and they behave differently on purpose:
//!
//! - a **hardware output** takes a COPY of the channel out of the box on its
//!   own pair of jacks (3/4, 5/6, a single mono jack), which is how somebody
//!   feeds a headphone amp, a second interface, or a stage monitor. The
//!   channel stays in the master mix; the jacks are extra.
//! - a **channel route MOVES the channel.** It comes off the master and is
//!   folded into the destination channel instead, through whatever is loaded
//!   on it — a submix. The routing dialog says exactly this ("Routing to a
//!   channel MOVES it there"), and the engine enforces it by closing the
//!   channel's direct path to the master for as long as such a route exists
//!   (`route_direct` in `instrument.rs`). For a COPY — the classic reverb
//!   feed — the strip's SEND knob into the effects bus is the copy; calling
//!   both of them "send" is how you end up expecting to hear a track twice.
//!
//! **No duplicates**, by the owner's word: the same channel to the same
//! destination twice is two faders fighting over one path, and nobody means
//! that. [`Routes::add`] refuses it rather than silently merging.

use serde_json::{Map, Value};

/// How many routes the desk carries at most.
///
/// The engine's routing table is a fixed array of atomics read on the audio
/// thread (`MAX_ROUTES` in `instrument.rs`, which must stay this number), and
/// `set_routes` publishes only this many entries. Sixty-four is four routes
/// per channel on a sixteen-channel desk, past anything a person builds by
/// hand — the ceiling exists so the audio thread never allocates, not to
/// ration anybody.
pub const MAX_ROUTES: usize = 64;

/// Where a route sends the audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Device output channels, zero-based. `stereo` sends the channel's own
    /// left and right to `first` and `first + 1`; mono sums them into `first`
    /// alone — a mono jack is one jack, and sending only the left channel to
    /// it would silently drop half of anything panned.
    Hardware { first: usize, stereo: bool },
    /// Another desk channel, by index. This one MOVES the channel there —
    /// see the module docs; the copy is the SEND knob, not a route.
    Channel(usize),
}

impl Target {
    /// What the strip under the button says. Hardware is named the way an
    /// interface names its jacks — one-based, because no interface in the
    /// world prints "0" on its back panel.
    pub fn label(self) -> String {
        match self {
            Target::Hardware { first, stereo: true } => format!("{}/{}", first + 1, first + 2),
            Target::Hardware { first, stereo: false } => format!("Out {}", first + 1),
            Target::Channel(n) => format!("Ch {}", n + 1),
        }
    }

    /// The device channels this target touches, for the overlap check.
    fn hardware_span(self) -> Option<(usize, usize)> {
        match self {
            Target::Hardware { first, stereo } => {
                Some((first, if stereo { first + 1 } else { first }))
            }
            Target::Channel(_) => None,
        }
    }
}

/// One route: a source channel, a destination, and how much of it goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Route {
    /// The desk channel this leaves FROM.
    pub from: usize,
    pub target: Target,
    /// 0..=1, the strip's own fader. Unity by default: a route that arrived
    /// silent would read as a route that does not work.
    pub gain: f32,
}

impl Route {
    pub fn new(from: usize, target: Target) -> Self {
        Self {
            from,
            target,
            gain: 1.0,
        }
    }
}

/// Every route on the desk, in the order they were added.
///
/// A flat list rather than a per-channel map: it serialises as one array, it
/// iterates in one pass in the audio callback, and "every route from channel
/// 3" is a filter rather than a lookup that can be missing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Routes(pub Vec<Route>);

impl Routes {
    /// Add a route unless the exact same one already exists, or the desk is
    /// already carrying [`MAX_ROUTES`].
    ///
    /// Returns whether it was added, so the caller can say why nothing
    /// happened. "The exact same one" is source AND destination: channel 3 to
    /// 3/4 and channel 3 to 5/6 are two different routes, and channel 4 to
    /// 3/4 is a third.
    ///
    /// The cap is refused HERE, not merely clamped downstream: the engine's
    /// `set_routes` publishes only the first `MAX_ROUTES` entries to the
    /// audio thread, so a sixty-fifth route accepted into this list would be
    /// stored, saved, drawn on the strip — and silently never sound. A fader
    /// that moves and does nothing, forever, is exactly the failure `prune`'s
    /// docs warn about, so the refusal happens where the caller can still say
    /// no out loud.
    pub fn add(&mut self, route: Route) -> bool {
        if self.0.len() >= MAX_ROUTES {
            return false;
        }
        if self.0.iter().any(|r| r.from == route.from && r.target == route.target) {
            return false;
        }
        self.0.push(route);
        true
    }

    /// Every route leaving one channel, with its index in the whole list so a
    /// strip can address itself back.
    pub fn from(&self, channel: usize) -> Vec<(usize, Route)> {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, r)| r.from == channel)
            .map(|(i, r)| (i, *r))
            .collect()
    }

    /// **Drop every route pointing INTO one channel.**
    ///
    /// Called when a track becomes a folder, because a group cannot be a send
    /// destination: the fold that would carry the arriving audio has already
    /// run by the time a send is gathered, so the audio is processed and then
    /// goes nowhere. Rather than leave a route that silently drops what it
    /// carries, the route goes — and folders are not offered in the picker in
    /// the first place. See `TangentApp::routing_dialog`.
    ///
    /// Returns how many were dropped, so a caller can tell somebody.
    pub fn drop_into(&mut self, channel: usize) -> usize {
        let before = self.0.len();
        self.0.retain(|r| r.target != Target::Channel(channel));
        before - self.0.len()
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.0.len() {
            self.0.remove(index);
        }
    }

    pub fn set_gain(&mut self, index: usize, gain: f32) {
        if let Some(r) = self.0.get_mut(index) {
            r.gain = gain.clamp(0.0, 1.0);
        }
    }

    /// Drop routes that name channels or device outputs that no longer exist.
    ///
    /// **Called when the device changes, and it must not be silent about
    /// scale.** Somebody who unplugs an 18-out interface and plugs in a
    /// 2-out one has lost every route past the second jack; keeping them
    /// would mean a fader that moves and does nothing, forever.
    pub fn prune(&mut self, device_outs: usize, channels: usize) -> usize {
        let before = self.0.len();
        self.0.retain(|r| {
            if r.from >= channels {
                return false;
            }
            match r.target {
                Target::Hardware { first, stereo } => {
                    let last = if stereo { first + 1 } else { first };
                    last < device_outs
                }
                Target::Channel(n) => n < channels && n != r.from,
            }
        });
        before - self.0.len()
    }

    /// Whether anything routes to hardware — the flag the render loop reads
    /// to decide whether to keep per-channel audio at all.
    pub fn any_hardware(&self) -> bool {
        self.0
            .iter()
            .any(|r| matches!(r.target, Target::Hardware { .. }))
    }

    pub fn to_json(&self) -> Value {
        Value::Array(
            self.0
                .iter()
                .map(|r| {
                    let mut m = Map::new();
                    m.insert("from".into(), Value::from(r.from as i64));
                    match r.target {
                        Target::Hardware { first, stereo } => {
                            m.insert("kind".into(), Value::String("hardware".into()));
                            m.insert("first".into(), Value::from(first as i64));
                            m.insert("stereo".into(), Value::Bool(stereo));
                        }
                        Target::Channel(n) => {
                            m.insert("kind".into(), Value::String("channel".into()));
                            m.insert("channel".into(), Value::from(n as i64));
                        }
                    }
                    m.insert("gain".into(), Value::from(f64::from(r.gain)));
                    Value::Object(m)
                })
                .collect(),
        )
    }

    /// Read routes back. A malformed entry is skipped rather than failing the
    /// whole list — the same forgiveness the rest of the settings file gets.
    pub fn from_json(v: &Value) -> Self {
        let Value::Array(items) = v else {
            return Self::default();
        };
        let mut out = Self::default();
        for it in items {
            let Value::Object(m) = it else { continue };
            let Some(from) = m.get("from").and_then(Value::as_i64) else {
                continue;
            };
            let gain = m
                .get("gain")
                .and_then(Value::as_f64)
                .unwrap_or(1.0)
                .clamp(0.0, 1.0) as f32;
            let target = match m.get("kind").and_then(Value::as_str) {
                Some("hardware") => {
                    let Some(first) = m.get("first").and_then(Value::as_i64) else {
                        continue;
                    };
                    Target::Hardware {
                        first: first.max(0) as usize,
                        stereo: m.get("stereo").and_then(Value::as_bool).unwrap_or(true),
                    }
                }
                Some("channel") => {
                    let Some(n) = m.get("channel").and_then(Value::as_i64) else {
                        continue;
                    };
                    Target::Channel(n.max(0) as usize)
                }
                _ => continue,
            };
            // `add`, not `push`: a hand-edited file with the same route twice
            // must not produce two faders over one path.
            out.add(Route {
                from: from.max(0) as usize,
                target,
                gain,
            });
        }
        out
    }
}

/// The hardware destinations a device offers, as the picker lists them.
///
/// Stereo pairs first, then the mono jacks: a pair is what most people want
/// and a mono jack is the exception, so the common answer is at the top. Pairs
/// are consecutive and even-aligned — 1/2, 3/4, 5/6 — because that is how
/// every interface labels its own outputs, and inventing 2/3 would be a pair
/// no manual mentions.
pub fn hardware_targets(device_outs: usize) -> Vec<Target> {
    let mut out = Vec::new();
    let mut first = 0;
    while first + 1 < device_outs {
        out.push(Target::Hardware { first, stereo: true });
        first += 2;
    }
    for ch in 0..device_outs {
        out.push(Target::Hardware {
            first: ch,
            stereo: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_route_twice_is_refused() {
        let mut r = Routes::default();
        let hw = Target::Hardware { first: 2, stereo: true };
        assert!(r.add(Route::new(3, hw)));
        assert!(!r.add(Route::new(3, hw)), "the same route was added twice");
        assert_eq!(r.0.len(), 1);
        // A different source to the same jacks is a different route, and a
        // real thing to want: two channels feeding one headphone amp.
        assert!(r.add(Route::new(4, hw)));
        // And the same source to different jacks.
        assert!(r.add(Route::new(3, Target::Hardware { first: 4, stereo: true })));
        assert_eq!(r.0.len(), 3);
    }

    #[test]
    fn a_send_names_the_channel_the_owner_would_name() {
        // The owner's example: "pro r could be on 9 and i could send 3 to 9".
        let mut r = Routes::default();
        assert!(r.add(Route::new(2, Target::Channel(8))));
        assert_eq!(r.0[0].target.label(), "Ch 9", "channels are one-based to a human");
        assert_eq!(
            Target::Hardware { first: 2, stereo: true }.label(),
            "3/4",
            "outputs are named the way an interface prints them"
        );
        assert_eq!(Target::Hardware { first: 0, stereo: false }.label(), "Out 1");
    }

    #[test]
    fn a_channel_cannot_send_to_itself() {
        // Feedback with extra steps. Pruning is where it is caught, because
        // that is the one place that sees the whole desk.
        let mut r = Routes::default();
        r.add(Route::new(5, Target::Channel(5)));
        let dropped = r.prune(8, 16);
        assert_eq!(dropped, 1);
        assert!(r.0.is_empty());
    }

    #[test]
    fn routes_past_the_end_of_a_smaller_interface_are_dropped() {
        let mut r = Routes::default();
        r.add(Route::new(0, Target::Hardware { first: 0, stereo: true }));
        r.add(Route::new(1, Target::Hardware { first: 6, stereo: true }));
        r.add(Route::new(2, Target::Channel(9)));
        // An 18-out interface unplugged, a 2-out one plugged in.
        let dropped = r.prune(2, 16);
        assert_eq!(dropped, 1, "the 7/8 route should be gone");
        assert_eq!(r.0.len(), 2);
        // The channel send survives: it never touched the interface.
        assert!(r.0.iter().any(|x| x.target == Target::Channel(9)));
    }

    #[test]
    fn the_picker_lists_pairs_before_mono_jacks_and_aligns_them() {
        let t = hardware_targets(4);
        assert_eq!(t[0], Target::Hardware { first: 0, stereo: true });
        assert_eq!(t[1], Target::Hardware { first: 2, stereo: true });
        // Then the four mono jacks.
        assert_eq!(t[2], Target::Hardware { first: 0, stereo: false });
        assert_eq!(t.len(), 2 + 4);
        // No 2/3: pairs are even-aligned, as every interface labels them.
        assert!(!t.contains(&Target::Hardware { first: 1, stereo: true }));
        // A mono interface offers exactly one jack and no pair at all.
        assert_eq!(hardware_targets(1), vec![Target::Hardware { first: 0, stereo: false }]);
    }

    #[test]
    fn routes_round_trip_through_the_settings_file() {
        let mut r = Routes::default();
        r.add(Route::new(2, Target::Hardware { first: 2, stereo: true }));
        r.add(Route::new(3, Target::Channel(8)));
        r.set_gain(1, 0.42);
        let back = Routes::from_json(&r.to_json());
        assert_eq!(back.0.len(), 2);
        assert_eq!(back.0[0].target, Target::Hardware { first: 2, stereo: true });
        assert_eq!(back.0[1].target, Target::Channel(8));
        assert!((back.0[1].gain - 0.42).abs() < 1e-6);
    }

    #[test]
    fn a_hand_edited_file_with_a_duplicate_yields_one_route() {
        let json: Value = serde_json::from_str(
            r#"[{"from":1,"kind":"hardware","first":2,"stereo":true,"gain":1.0},
                {"from":1,"kind":"hardware","first":2,"stereo":true,"gain":0.5},
                {"from":1,"kind":"nonsense"}]"#,
        )
        .expect("valid json");
        let r = Routes::from_json(&json);
        assert_eq!(r.0.len(), 1, "the duplicate and the junk should both be gone");
        assert!((r.0[0].gain - 1.0).abs() < 1e-6, "the FIRST one wins");
    }

    #[test]
    fn only_hardware_routes_ask_the_render_loop_for_extra_work() {
        let mut r = Routes::default();
        assert!(!r.any_hardware());
        r.add(Route::new(0, Target::Channel(3)));
        assert!(!r.any_hardware(), "a send needs no per-channel device buffer");
        r.add(Route::new(0, Target::Hardware { first: 2, stereo: true }));
        assert!(r.any_hardware());
    }

    #[test]
    fn a_sixty_fifth_route_is_refused_rather_than_stored_and_never_sounding() {
        // The engine publishes only the first MAX_ROUTES entries to the audio
        // thread, so anything held past that would be a route the settings
        // file remembers and the desk never plays. `add` must say no while
        // somebody is still there to hear it.
        let mut r = Routes::default();
        for i in 0..MAX_ROUTES {
            assert!(
                r.add(Route::new(i % 16, Target::Channel(16 + i))),
                "route {i} is under the cap and distinct, it should fit"
            );
        }
        // Not a duplicate of anything above — the refusal is the cap alone.
        assert!(
            !r.add(Route::new(0, Target::Hardware { first: 0, stereo: true })),
            "route MAX_ROUTES + 1 must be refused"
        );
        assert_eq!(r.0.len(), MAX_ROUTES);
        // Taking one off makes room again: the cap is a ceiling, not a latch.
        r.remove(0);
        assert!(r.add(Route::new(0, Target::Hardware { first: 0, stereo: true })));
        assert_eq!(r.0.len(), MAX_ROUTES);
    }

    #[test]
    fn a_gain_is_always_a_fraction() {
        let mut r = Routes::default();
        r.add(Route::new(0, Target::Channel(1)));
        r.set_gain(0, 9.0);
        assert!((r.0[0].gain - 1.0).abs() < 1e-6);
        r.set_gain(0, -3.0);
        assert!(r.0[0].gain.abs() < 1e-6);
    }
}
