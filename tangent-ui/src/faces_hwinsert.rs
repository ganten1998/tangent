//! The face of **Tangent Insert**: a bay whose effect is outboard gear.
//!
//! Four parts, top to bottom. The glass, which says whether the loop is up
//! and what it costs in time (the latency the bay reports to delay
//! compensation, in milliseconds and samples, and whether a ping measured
//! it). The two sides of the loop, SEND and RETURN, each a mono or stereo
//! choice and a channel stepped through what the device actually has. The
//! knobs, as the suite's own rows. And PING, which fires a click through the
//! loop and measures where it comes back.
//!
//! **What this file cannot see.** The engine is behind the firewall, so the
//! jacks arrive as the bay's extra state lines (`send_first`, `ret_width`...)
//! and the live picture as lines prefixed [`LIVE`], both on
//! `BuiltinFace::extra`; [`HwLive`] is the one spelling of the second, written
//! by the host and read here. The face never sets a jack itself: it asks
//! ([`HwAsk`]), and the host checks the ask against the device, writes the
//! bay's state and reloads it.
//!
//! **The master's jacks are not offered.** Output 1/2 carries the mix (1 on a
//! mono device), and a send there would put the bay's signal on the speakers
//! and the whole mix into the box, so the stepper walks past them. A saved
//! project that names them anyway is refused by the engine and the glass
//! says so.

use crate::face::{self, FaceStyle, LcdTier};
use crate::faces::{FaceAction, FaceLive};
use crate::fonts;
use egui::{Align2, FontId, Pos2, RichText, Vec2};

/// The sentinel, spelled again for this crate (the engine's
/// `builtin_fx::HWINSERT_PATH` is the other spelling; a test pins them).
pub const HWINSERT_PATH: &str = "@tangent-insert";

/// What the picker and the window title call it.
pub const HWINSERT_NAME: &str = "Tangent Insert";

/// The line under the title.
pub const HWINSERT_ABOUT: &str = "your outboard gear, in the bay, in time";

/// The prefix of the live lines the host adds to a bay's pushed extra
/// state. Never saved: the bay's own symbols do not start with it, and the
/// engine keeps only the symbols the unit claims.
pub const LIVE: &str = "live.";

/// A jack choice as the wire carries it: the first channel, zero-based, and
/// whether it is a pair. `None` is "not chosen".
pub type JackPick = Option<(u16, bool)>;

/// What the face asks of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwAsk {
    /// Put one side of the loop on these jacks (`send` true for the send,
    /// false for the return), or on none.
    Route { send: bool, jacks: JackPick },
    /// Fire a click through the loop and measure where it comes back.
    Ping,
}

/// **The loop as the face shows it this frame**, written by the host and
/// carried on the bay's pushed extra lines (see [`HwLive::to_lines`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HwLive {
    pub connected: bool,
    /// The link's sentence: "Connected." or what is wrong and what to do.
    pub status: String,
    /// What the bay reports to delay compensation, frames.
    pub latency: u32,
    /// The loop that figure assumes, frames.
    pub loop_frames: u32,
    /// Whether a ping measured it at today's rate and buffers.
    pub measured: bool,
    pub pinging: bool,
    /// Blocks whose return had not arrived in time since the bay loaded.
    pub slips: u32,
    /// The last ping's answer: the loop it measured, or why it could not.
    pub answer: Option<Result<u32, String>>,
    pub rate: u32,
    pub out_channels: u32,
    pub in_channels: u32,
    /// Every other loop's (send, return), for the shared-jack warning.
    pub others: Vec<(JackPick, JackPick)>,
    /// The outputs the desk's hardware routes play a track on: the master
    /// is not the only thing a send can land on top of.
    pub routed: Vec<(u16, bool)>,
}

fn pick_text(p: JackPick) -> String {
    match p {
        None => "-".to_owned(),
        Some((first, stereo)) => format!("{first}{}", if stereo { "s" } else { "m" }),
    }
}

fn pick_parse(s: &str) -> JackPick {
    let s = s.trim();
    let (num, stereo) = match s.strip_suffix('s') {
        Some(n) => (n, true),
        None => (s.strip_suffix('m')?, false),
    };
    Some((num.parse().ok()?, stereo))
}

impl HwLive {
    /// The lines the host appends to a bay's pushed extra state.
    pub fn to_lines(&self) -> Vec<(String, String)> {
        let mut out = vec![
            ("connected", u8::from(self.connected).to_string()),
            ("status", self.status.clone()),
            ("latency", self.latency.to_string()),
            ("loop", self.loop_frames.to_string()),
            ("measured", u8::from(self.measured).to_string()),
            ("pinging", u8::from(self.pinging).to_string()),
            ("slips", self.slips.to_string()),
            ("rate", self.rate.to_string()),
            ("out", self.out_channels.to_string()),
            ("in", self.in_channels.to_string()),
            (
                "others",
                self.others
                    .iter()
                    .map(|(s, r)| format!("{},{}", pick_text(*s), pick_text(*r)))
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
        ];
        out.push((
            "routed",
            self.routed.iter().map(|p| pick_text(Some(*p))).collect::<Vec<_>>().join(";"),
        ));
        match &self.answer {
            Some(Ok(n)) => out.push(("answer", n.to_string())),
            Some(Err(why)) => out.push(("refused", why.clone())),
            None => {}
        }
        out.into_iter().map(|(k, v)| (format!("{LIVE}{k}"), v)).collect()
    }

    /// Read [`HwLive::to_lines`] back out of a bay's extra lines; `None`
    /// when the host pushed none (no engine, or not a hardware insert).
    pub fn from_lines(extra: &[(String, String)]) -> Option<Self> {
        let get = |k: &str| {
            extra
                .iter()
                .rev()
                .find(|(s, _)| s.strip_prefix(LIVE) == Some(k))
                .map(|(_, v)| v.as_str())
        };
        let num = |k: &str| get(k).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        let flag = |k: &str| get(k) == Some("1");
        get("status")?;
        let others = get("others")
            .unwrap_or("")
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| {
                let (s, r) = e.split_once(',').unwrap_or((e, "-"));
                (pick_parse(s), pick_parse(r))
            })
            .collect();
        let answer = match (get("answer"), get("refused")) {
            (Some(n), _) => n.parse().ok().map(Ok),
            (None, Some(why)) => Some(Err(why.to_owned())),
            _ => None,
        };
        Some(Self {
            connected: flag("connected"),
            status: get("status").unwrap_or("").to_owned(),
            latency: num("latency"),
            loop_frames: num("loop"),
            measured: flag("measured"),
            pinging: flag("pinging"),
            slips: num("slips"),
            answer,
            rate: num("rate"),
            out_channels: num("out"),
            in_channels: num("in"),
            others,
            routed: get("routed")
                .unwrap_or("")
                .split(';')
                .filter_map(pick_parse)
                .collect(),
        })
    }
}

/// The bay's jacks, read off its extra state lines (the engine's
/// `units::hwinsert::state_lines` writes them).
pub fn routes_of(extra: &[(String, String)]) -> (JackPick, JackPick) {
    let get = |k: &str| extra.iter().rev().find(|(s, _)| s == k).map(|(_, v)| v.as_str());
    let side = |s: &str| {
        let first = get(&format!("{s}_first"))?.trim().parse::<u16>().ok()?;
        let stereo = get(&format!("{s}_width")).and_then(|w| w.trim().parse::<u16>().ok()) == Some(2);
        Some((first, stereo))
    };
    (side("send"), side("ret"))
}

fn last(p: (u16, bool)) -> u16 {
    p.0 + u16::from(p.1)
}

fn overlaps(a: (u16, bool), b: (u16, bool)) -> bool {
    a.0 <= last(b) && b.0 <= last(a)
}

/// The jacks the master plays on: 1/2, or 1 on a mono device.
pub fn main_outs(out_channels: u32) -> Option<(u16, bool)> {
    match out_channels {
        0 => None,
        1 => Some((0, false)),
        _ => Some((0, true)),
    }
}

/// **Every choice a side may step to**, in order: the jacks of that width
/// the device has, pairs on odd-numbered jacks (1/2, 3/4) the way front
/// panels print them, and for the send never the master's own.
pub fn choices(send: bool, stereo: bool, channels: u32) -> Vec<(u16, bool)> {
    let mains = if send { main_outs(channels) } else { None };
    let step = if stereo { 2 } else { 1 };
    (0..channels)
        .step_by(step)
        .filter_map(|c| u16::try_from(c).ok())
        .map(|c| (c, stereo))
        .filter(|p| u32::from(last(*p)) < channels)
        .filter(|p| mains.is_none_or(|m| !overlaps(*p, m)))
        .collect()
}

/// One step through a side's choices from `now`, `by` +1 or -1, through
/// "none" at either end so a side can be emptied again.
pub fn step(send: bool, now: JackPick, stereo: bool, channels: u32, by: i32) -> JackPick {
    let list = choices(send, stereo, channels);
    if list.is_empty() {
        return None;
    }
    // "None" sits before the first choice.
    let at = now.and_then(|p| list.iter().position(|q| *q == p)).map_or(0, |i| i as i32 + 1);
    let n = list.len() as i32 + 1;
    let next = (at + by).rem_euclid(n);
    (next > 0).then(|| list[(next - 1) as usize])
}

/// The same side, the other width: the jacks it is on kept as near as the
/// new width allows, or the first that fits.
pub fn rewidth(send: bool, now: JackPick, channels: u32) -> JackPick {
    let stereo = !now.is_some_and(|p| p.1);
    let list = choices(send, stereo, channels);
    let want = now.map_or(0, |p| if stereo { p.0 & !1 } else { p.0 });
    list.iter()
        .copied()
        .find(|p| p.0 == want)
        .or_else(|| list.first().copied())
}

fn label(p: JackPick, dir: &str) -> String {
    match p {
        None => "none".to_owned(),
        Some((first, true)) => format!("{dir} {}/{}", first + 1, first + 2),
        Some((first, false)) => format!("{dir} {}", first + 1),
    }
}

/// Milliseconds for a frame count, to the hundredth.
pub fn ms(frames: u32, rate: u32) -> String {
    format!("{:.2} ms", f64::from(frames) * 1_000.0 / f64::from(rate.max(1)))
}

/// One side's row: its name, the width button, and the stepper.
fn side_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    send: bool,
    now: JackPick,
    channels: u32,
) -> Option<FaceAction> {
    let mut out = None;
    let (name, dir) = if send { ("SEND", "Out") } else { ("RETURN", "In") };
    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(64.0, 20.0),
            egui::Label::new(RichText::new(name).font(bold(11.0)).color(st.s.ink)),
        );
        let stereo = now.is_some_and(|p| p.1);
        let width = format!("{name} {}", if stereo { "STEREO" } else { "MONO" });
        let hover = if send {
            "mono sends the sum of both sides at -6 dB; stereo sends left and right on a pair"
        } else {
            "a mono return feeds both sides; a stereo one keeps them apart"
        };
        if ui
            .add(egui::Button::new(RichText::new(width).font(bold(9.5)).color(st.s.ink)).fill(st.s.raised))
            .on_hover_text(hover)
            .clicked()
        {
            out = Some(FaceAction::HwInsert(HwAsk::Route { send, jacks: rewidth(send, now, channels) }));
        }
        let back = format!("< {}", dir.to_uppercase());
        let fwd = format!("{} >", dir.to_uppercase());
        if ui
            .add(egui::Button::new(RichText::new(back).font(bold(9.5)).color(st.s.ink)).fill(st.s.raised))
            .clicked()
        {
            out = Some(FaceAction::HwInsert(HwAsk::Route {
                send,
                jacks: step(send, now, stereo, channels, -1),
            }));
        }
        ui.add_sized(
            Vec2::new(80.0, 20.0),
            egui::Label::new(RichText::new(label(now, dir)).font(bold(11.0)).color(st.s.ink)),
        );
        if ui
            .add(egui::Button::new(RichText::new(fwd).font(bold(9.5)).color(st.s.ink)).fill(st.s.raised))
            .clicked()
        {
            out = Some(FaceAction::HwInsert(HwAsk::Route {
                send,
                jacks: step(send, now, stereo, channels, 1),
            }));
        }
    });
    out
}

/// **A shared jack, said out loud.** Two loops on one output put both bays'
/// signals into one box; two on one input feed both bays the same return.
/// Neither is refused (a splitter is a real patch) but neither should be a
/// surprise.
pub fn warnings(
    send: JackPick,
    ret: JackPick,
    others: &[(JackPick, JackPick)],
    routed: &[(u16, bool)],
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(s) = send {
        if others.iter().any(|(o, _)| o.is_some_and(|o| overlaps(o, s))) {
            out.push(format!("{} is also another Tangent Insert's send.", label(send, "Out")));
        }
        if routed.iter().any(|r| overlaps(*r, s)) {
            out.push(format!("{} also carries a track's hardware route.", label(send, "Out")));
        }
    }
    if let Some(r) = ret {
        if others.iter().any(|(_, o)| o.is_some_and(|o| overlaps(o, r))) {
            out.push(format!("{} also returns to another Tangent Insert.", label(ret, "In")));
        }
    }
    out
}

/// The face.
pub fn draw_hwinsert(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let mut out = None;
    let hw = HwLive::from_lines(&live.face.extra).unwrap_or_else(|| HwLive {
        status: "No audio device is running.".to_owned(),
        ..Default::default()
    });
    let (send, ret) = routes_of(&live.face.extra);

    // ── the glass: the link and the time ──
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), egui::Sense::hover());
    face::lcd_glass(ui.painter(), rect, st);
    let pad = 8.0;
    face::lcd_text(
        ui.painter(),
        Pos2::new(rect.left() + pad, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        if hw.connected { "CONNECTED" } else { "DISCONNECTED - PASSING DRY" },
        11.0,
        LcdTier::Primary,
        st,
    );
    let time = if hw.pinging {
        "PINGING...".to_owned()
    } else {
        format!(
            "{}{}  {} samples  {}",
            if hw.measured { "" } else { "~" },
            ms(hw.latency, hw.rate),
            hw.latency,
            if hw.measured { "measured" } else { "unmeasured" }
        )
    };
    face::lcd_text(
        ui.painter(),
        Pos2::new(rect.left() + pad, rect.top() + 32.0),
        Align2::LEFT_CENTER,
        &time,
        11.0,
        LcdTier::Primary,
        st,
    );
    face::lcd_text(
        ui.painter(),
        Pos2::new(rect.left() + pad, rect.top() + 48.0),
        Align2::LEFT_CENTER,
        if hw.measured {
            "delay compensation lines the loop up"
        } else {
            "estimated from the buffers: PING to measure"
        },
        9.5,
        LcdTier::Secondary,
        st,
    );
    ui.add_space(4.0);
    ui.label(RichText::new(&hw.status).font(FontId::new(10.0, fonts::courier())).color(st.s.ink));
    ui.add_space(4.0);

    // ── the two sides ──
    face::keep(&mut out, side_row(ui, st, true, send, hw.out_channels));
    face::keep(&mut out, side_row(ui, st, false, ret, hw.in_channels));
    for w in warnings(send, ret, &hw.others, &hw.routed) {
        ui.label(RichText::new(w).font(FontId::new(10.0, fonts::courier())).color(st.s.faint));
    }
    ui.add_space(6.0);

    // ── the knobs ──
    for id in 0..=2 {
        face::keep(&mut out, crate::faces::row(ui, st, &live.face, id));
    }
    face::keep(&mut out, crate::faces::step_row(ui, st, &live.face, 3, None));
    ui.add_space(6.0);

    // ── the ping ──
    ui.horizontal(|ui| {
        let bold = FontId::new(10.0, fonts::courier_bold());
        let word = if hw.pinging { "PINGING" } else { "PING" };
        let hit = ui
            .add_enabled(
                hw.connected && !hw.pinging,
                egui::Button::new(RichText::new(word).font(bold).color(st.s.ink)).fill(st.s.raised),
            )
            .on_hover_text(
                "send a click through the loop and measure where it comes back; \
                 set the unit dry or bypassed for the cleanest answer",
            )
            .on_disabled_hover_text("the loop has to be connected to ping it");
        if hit.clicked() {
            out = Some(FaceAction::HwInsert(HwAsk::Ping));
        }
        let said = match &hw.answer {
            Some(Ok(n)) => format!("loop {n} samples ({})", ms(*n, hw.rate)),
            Some(Err(why)) => why.clone(),
            None => String::new(),
        };
        if !said.is_empty() {
            ui.label(RichText::new(said).font(FontId::new(10.0, fonts::courier())).color(st.s.ink));
        }
    });
    if hw.slips > 0 && hw.connected {
        ui.label(
            RichText::new(format!(
                "The input fell behind {} time{}: if the timing sounds off, PING again.",
                hw.slips,
                if hw.slips == 1 { "" } else { "s" }
            ))
            .font(FontId::new(10.0, fonts::courier()))
            .color(st.s.faint),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_lines_round_trip() {
        let live = HwLive {
            connected: true,
            status: "Connected.".to_owned(),
            latency: 556,
            loop_frames: 300,
            measured: true,
            pinging: false,
            slips: 2,
            answer: Some(Ok(300)),
            rate: 48_000,
            out_channels: 18,
            in_channels: 18,
            others: vec![(Some((2, true)), None), (None, Some((5, false)))],
            routed: vec![(6, true), (3, false)],
        };
        assert_eq!(HwLive::from_lines(&live.to_lines()), Some(live.clone()));
        let refused = HwLive { answer: Some(Err("no click".to_owned())), others: vec![], routed: vec![], ..live };
        assert_eq!(HwLive::from_lines(&refused.to_lines()), Some(refused));
        assert_eq!(HwLive::from_lines(&[]), None);
        assert!(HwLive::default().to_lines().iter().all(|(k, _)| k.starts_with(LIVE)));
    }

    #[test]
    fn the_send_never_offers_the_main_outs_and_steps_through_none() {
        // Eight outputs: pairs 3/4, 5/6, 7/8 and never 1/2.
        assert_eq!(choices(true, true, 8), vec![(2, true), (4, true), (6, true)]);
        assert_eq!(choices(true, false, 4), vec![(2, false), (3, false)]);
        // The return may use any input.
        assert_eq!(choices(false, true, 4), vec![(0, true), (2, true)]);
        // A stereo device's two outputs are both the mix, a mono device's
        // one is; a third output is the first free one.
        assert!(choices(true, false, 2).is_empty());
        assert!(choices(true, false, 1).is_empty());
        assert!(choices(true, true, 2).is_empty());
        assert_eq!(choices(true, false, 3), vec![(2, false)]);
        // Stepping walks the list and "none" at the ends.
        assert_eq!(step(true, None, true, 8, 1), Some((2, true)));
        assert_eq!(step(true, Some((6, true)), true, 8, 1), None);
        assert_eq!(step(true, None, true, 8, -1), Some((6, true)));
        assert_eq!(step(true, Some((2, true)), true, 8, -1), None);
        // A width change keeps the jacks as near as it can.
        assert_eq!(rewidth(true, Some((5, false)), 8), Some((4, true)));
        assert_eq!(rewidth(true, Some((4, true)), 8), Some((4, false)));
        assert_eq!(rewidth(false, None, 4), Some((0, true)));
    }

    #[test]
    fn a_shared_jack_is_said() {
        let others = vec![(Some((2, true)), Some((4, false)))];
        let w = warnings(Some((3, false)), Some((4, true)), &others, &[]);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(warnings(Some((6, true)), Some((0, true)), &others, &[]).is_empty());
        let routed = warnings(Some((6, true)), None, &[], &[(7, false)]);
        assert_eq!(routed, vec!["Out 7/8 also carries a track's hardware route.".to_owned()]);
    }

    #[test]
    fn the_jacks_read_off_the_state_lines() {
        let extra = vec![
            ("send_first".to_owned(), "2".to_owned()),
            ("send_width".to_owned(), "2".to_owned()),
            ("ret_first".to_owned(), "5".to_owned()),
            ("ret_width".to_owned(), "1".to_owned()),
        ];
        assert_eq!(routes_of(&extra), (Some((2, true)), Some((5, false))));
        assert_eq!(routes_of(&[]), (None, None));
        assert_eq!(ms(480, 48_000), "10.00 ms");
    }
}
