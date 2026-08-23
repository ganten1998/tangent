//! Reading a Standard MIDI File into arrangement notes.
//!
//! Hand-rolled on purpose: this crate is firewalled from `ivory-record`
//! (and everything else with native ambitions), so the fifty lines of SMF
//! walking live here rather than pulling a parser through the wall. Format
//! 0 and 1, metrical division, running status, tempo map — the parts a
//! file dragged out of a DAW or a download actually uses. SMPTE division
//! is refused honestly rather than mis-timed.

use crate::arrangement::ArrNote;

/// What a `.mid` file becomes: clip-relative notes and the span they
/// cover, in the seconds everything else in the document speaks.
pub struct MidiImport {
    pub notes: Vec<ArrNote>,
    pub len_s: f64,
}

/// A big-endian reader that answers `None` past the end — every truncated
/// file lands in one refusal instead of a panic.
struct Cur<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    /// A variable-length quantity, the delta-time encoding.
    fn varlen(&mut self) -> Option<u64> {
        let mut v: u64 = 0;
        for _ in 0..4 {
            let byte = self.u8()?;
            v = (v << 7) | u64::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
}

/// One thing a track said, at an absolute tick.
enum Ev {
    On { key: u8, vel: u8 },
    Off { key: u8 },
    Tempo(u32),
}

/// Parse `bytes` as an SMF. `None` when it is not one this reader can be
/// honest about — wrong magic, SMPTE timing, truncation.
pub fn read(bytes: &[u8]) -> Option<MidiImport> {
    let mut c = Cur { b: bytes, at: 0 };
    if c.take(4)? != b"MThd" {
        return None;
    }
    let head_len = c.u32()? as usize;
    let _format = c.u16()?;
    let ntrks = c.u16()?;
    let division = c.u16()?;
    if division & 0x8000 != 0 {
        // SMPTE division: nobody drags these in, and guessing a tempo for
        // one would place every note wrong.
        return None;
    }
    let ppq = f64::from(division.max(1));
    c.take(head_len.checked_sub(6)?)?;

    // Every track's events, merged onto one absolute-tick line. The tempo
    // map is global — format 1 keeps it on track 0, and merging makes
    // that true of format 0 and of files that break the rule alike.
    let mut evs: Vec<(u64, Ev)> = Vec::new();
    for _ in 0..ntrks {
        if c.take(4)? != b"MTrk" {
            return None;
        }
        let len = c.u32()? as usize;
        let end = c.at.checked_add(len)?;
        if end > c.b.len() {
            return None;
        }
        let mut at: u64 = 0;
        let mut status: u8 = 0;
        while c.at < end {
            at = at.checked_add(c.varlen()?)?;
            let mut first = c.u8()?;
            if first & 0x80 != 0 {
                status = first;
                if status < 0xF0 {
                    first = c.u8()?;
                }
            } else if status == 0 {
                return None; // running status with nothing running
            }
            match status & 0xF0 {
                0x90 => {
                    let vel = c.u8()?;
                    let key = first & 0x7F;
                    if vel & 0x7F == 0 {
                        evs.push((at, Ev::Off { key }));
                    } else {
                        evs.push((at, Ev::On { key, vel: vel & 0x7F }));
                    }
                }
                0x80 => {
                    let _vel = c.u8()?;
                    evs.push((at, Ev::Off { key: first & 0x7F }));
                }
                // Two-byte channel messages whose second byte must go.
                0xA0 | 0xB0 | 0xE0 => {
                    let _ = c.u8()?;
                }
                // One data byte, already taken as `first`.
                0xC0 | 0xD0 => {}
                // System messages own the status byte whole — nothing was
                // pre-read as data, so their bytes all come fresh here.
                0xF0 => match status {
                    0xFF => {
                        let kind = c.u8()?;
                        let n = c.varlen()? as usize;
                        let data = c.take(n)?;
                        if kind == 0x51 && n == 3 {
                            let us = u32::from_be_bytes([
                                0, data[0], data[1], data[2],
                            ]);
                            evs.push((at, Ev::Tempo(us.max(1))));
                        }
                    }
                    0xF0 | 0xF7 => {
                        let n = c.varlen()? as usize;
                        c.take(n)?;
                    }
                    _ => {}
                },
                _ => return None,
            }
            // Meta and sysex CANCEL running status in a file; leaving it
            // armed would misread a malformed track instead of refusing.
            if status >= 0xF0 {
                status = 0;
            }
        }
        c.at = end;
    }
    evs.sort_by_key(|(at, e)| {
        // Tempo first, then offs, then ons at one tick: an off and an on
        // sharing a tick are a boundary, not an overlap.
        (*at, match e {
            Ev::Tempo(_) => 0u8,
            Ev::Off { .. } => 1,
            Ev::On { .. } => 2,
        })
    });

    // Ticks to seconds by walking the tempo segments once.
    let mut notes: Vec<ArrNote> = Vec::new();
    let mut open: Vec<(u8, u8, f64)> = Vec::new(); // key, vel, start_s
    let mut us_per_qn: f64 = 500_000.0;
    let (mut tick0, mut s0) = (0u64, 0.0f64);
    let mut len_s = 0.0f64;
    for (at, e) in evs {
        let s = s0 + (at - tick0) as f64 / ppq * us_per_qn / 1.0e6;
        match e {
            Ev::Tempo(us) => {
                us_per_qn = f64::from(us);
                tick0 = at;
                s0 = s;
            }
            Ev::On { key, vel } => open.push((key, vel, s)),
            Ev::Off { key } => {
                if let Some(i) = open.iter().position(|(k, ..)| *k == key) {
                    let (_, vel, from) = open.swap_remove(i);
                    notes.push(ArrNote {
                        at_s: from,
                        dur_s: (s - from).max(0.02),
                        key,
                        vel,
                    });
                    len_s = len_s.max(s);
                }
            }
        }
        len_s = len_s.max(s);
    }
    // A note the file never closed still deserves to exist.
    for (key, vel, from) in open {
        notes.push(ArrNote { at_s: from, dur_s: 0.25, key, vel });
        len_s = len_s.max(from + 0.25);
    }
    if notes.is_empty() {
        return None;
    }
    notes.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
    Some(MidiImport { notes, len_s })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-track SMF by hand: the test's own bytes, so the reader
    /// is measured against the spec and not against itself.
    fn smf(ppq: u16, track: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"MThd");
        b.extend_from_slice(&6u32.to_be_bytes());
        b.extend_from_slice(&0u16.to_be_bytes());
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&ppq.to_be_bytes());
        b.extend_from_slice(b"MTrk");
        b.extend_from_slice(&(track.len() as u32).to_be_bytes());
        b.extend_from_slice(track);
        b
    }

    /// **Two quarter notes at 120 bpm land at 0.0 and 0.5 seconds**, a
    /// half second long each — tempo event honoured, running status too.
    #[test]
    fn it_reads_notes_through_tempo_and_running_status() {
        let track = [
            // Δ0 meta tempo 500000 (120 bpm)
            0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20,
            // Δ0 note on C4 vel 100
            0x00, 0x90, 60, 100,
            // Δ480 note off (running status, vel 0)
            0x83, 0x60, 60, 0,
            // Δ0 note on E4
            0x00, 60 + 4, 100,
            // Δ480 off
            0x83, 0x60, 64, 0,
            // Δ0 end of track
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let got = read(&smf(960, &track)).expect("a plain file refused");
        assert_eq!(got.notes.len(), 2);
        assert!((got.notes[0].at_s - 0.0).abs() < 1.0e-9);
        assert!((got.notes[0].dur_s - 0.25).abs() < 1.0e-9);
        assert_eq!(got.notes[0].key, 60);
        assert!((got.notes[1].at_s - 0.25).abs() < 1.0e-9);
        assert_eq!(got.notes[1].key, 64);
        assert!((got.len_s - 0.5).abs() < 1.0e-9);
    }

    /// Garbage, truncation and SMPTE division are refusals, not panics.
    #[test]
    fn it_refuses_what_it_cannot_time() {
        assert!(read(b"not a midi file").is_none());
        assert!(read(&[]).is_none());
        let ok = smf(960, &[0x00, 0x90, 60, 100, 0x10, 0x80, 60, 0]);
        assert!(read(&ok).is_some());
        assert!(read(&ok[..ok.len() - 3]).is_none(), "truncated yet read");
        let mut smpte = ok.clone();
        smpte[12] = 0xE7; // division high byte with bit 15 set
        assert!(read(&smpte).is_none(), "SMPTE timing was guessed at");
    }
}
