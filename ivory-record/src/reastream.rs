//! ReaStream network audio **sender** — Tangent's master out onto the wire that
//! Cockos's ReaStream plug-in reads, so OBS (or REAPER) running the real
//! ReaStream in *receive* mode picks up Tangent's audio over localhost.
//!
//! Tangent hosts VST3 and ReaStream ships VST2 only, so Tangent can never LOAD
//! the plug-in — but ReaStream is a network protocol, not just a plug-in, and
//! the two ends need only share the wire format and an identifier string. This
//! speaks the sender half of that protocol; the receiver stays the genuine
//! ReaStream, wherever it is hosted.
//!
//! **Wire format**, verified against `niusounds/reastream-android`
//! (`ReaStreamPacket.java`) and the `reastream.txt` notes, little-endian
//! throughout:
//!
//! ```text
//!   off  size  field
//!   0    4     magic 'M','R','S','R'
//!   4    4     packet size (u32)  = 47 + data_bytes
//!   8    32    identifier, zero-padded, final byte always 0
//!   40   1     channel count (1..=64)
//!   41   4     sample rate (u32)
//!   45   2     block length (u16) = BYTES of sample data, max 1200
//!   47   ...   f32 samples, NON-interleaved: [all ch0][all ch1]...
//! ```
//!
//! There is a MIDI packet too (magic `'m','R','S','R'`); a send source does not
//! need it, so it is not built here.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// ReaStream's default UDP port. The receiver listens here unless told otherwise.
pub const REASTREAM_PORT: u16 = 58710;

/// The identifier ReaStream uses when the field is left alone, on both ends.
pub const DEFAULT_IDENTIFIER: &str = "default";

const MAGIC_AUDIO: [u8; 4] = [b'M', b'R', b'S', b'R'];
const ID_LEN: usize = 32;
/// Everything before the samples: magic + size + id + nch + rate + blocklen.
const AUDIO_HEADER_BYTES: usize = 4 + 4 + ID_LEN + 1 + 4 + 2;
/// The protocol's ceiling on one packet's sample data (`reastream.txt`).
const MAX_BLOCK_BYTES: usize = 1200;

/// Frames of a `channels`-wide block that still fit under [`MAX_BLOCK_BYTES`].
fn frames_per_packet(channels: usize) -> usize {
    // bytes = frames * channels * 4  <=  MAX_BLOCK_BYTES
    (MAX_BLOCK_BYTES / 4 / channels.max(1)).max(1)
}

/// Build one ReaStream audio packet from **planar** samples: all of channel 0,
/// then all of channel 1, and so on — `planar.len()` must be `channels * frames`.
///
/// Pure and byte-exact so the wire format can be pinned by a test rather than a
/// receiver on another machine.
pub fn encode_audio_packet(
    identifier: &str,
    channels: u8,
    sample_rate: u32,
    planar: &[f32],
) -> Vec<u8> {
    let data_bytes = planar.len() * 4;
    let mut p = Vec::with_capacity(AUDIO_HEADER_BYTES + data_bytes);
    p.extend_from_slice(&MAGIC_AUDIO);
    p.extend_from_slice(&((AUDIO_HEADER_BYTES + data_bytes) as u32).to_le_bytes());
    // 32 bytes, zero-padded, and the final byte stays 0 no matter how long the
    // name is — the receiver reads a C string out of this field.
    let mut id = [0u8; ID_LEN];
    let src = identifier.as_bytes();
    let n = src.len().min(ID_LEN - 1);
    id[..n].copy_from_slice(&src[..n]);
    p.extend_from_slice(&id);
    p.push(channels);
    p.extend_from_slice(&sample_rate.to_le_bytes());
    p.extend_from_slice(&(data_bytes as u16).to_le_bytes());
    for s in planar {
        p.extend_from_slice(&s.to_le_bytes());
    }
    p
}

/// Where and how a [`ReaStreamSender`] streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaStreamConfig {
    /// The name both ends match on. Empty falls back to [`DEFAULT_IDENTIFIER`].
    pub identifier: String,
    /// The receiver's host — `127.0.0.1` for OBS on the same machine.
    pub host: String,
    /// The receiver's UDP port, normally [`REASTREAM_PORT`].
    pub port: u16,
}

impl Default for ReaStreamConfig {
    fn default() -> Self {
        Self {
            identifier: DEFAULT_IDENTIFIER.to_owned(),
            host: "127.0.0.1".to_owned(),
            port: REASTREAM_PORT,
        }
    }
}

/// A running sender: a thread that drains interleaved-stereo frames off the
/// engine's ring, packetizes them, and sends them over UDP to whatever receiver
/// its shared control names. Dropping it stops the thread.
///
/// The control is an `Option<ReaStreamConfig>` so that ONE sender, started once
/// per engine when the tap is first taken, serves every later change: `None`
/// stops sending (and discards what the ring holds so a re-enable starts
/// clean), `Some` (re)targets the socket when the host or port moves. The
/// once-only tap is never re-taken, and the real-time thread touches none of
/// this — the engine's `reastream_on` atomic gates the push, this only gates the
/// send.
pub struct ReaStreamSender {
    control: Arc<Mutex<Option<ReaStreamConfig>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ReaStreamSender {
    /// Start the sender thread over `frames` (interleaved stereo `L,R,L,R,...`
    /// at `sample_rate`), streaming to `config` when it is `Some`.
    pub fn start(
        mut frames: rtrb::Consumer<f32>,
        sample_rate: u32,
        config: Option<ReaStreamConfig>,
    ) -> Self {
        let control = Arc::new(Mutex::new(config));
        let stop = Arc::new(AtomicBool::new(false));
        let control_thread = Arc::clone(&control);
        let stop_thread = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("reastream-send".to_owned())
            .spawn(move || {
                let per_packet = frames_per_packet(2);
                let mut inter: Vec<f32> = Vec::with_capacity(per_packet * 2);
                let mut planar: Vec<f32> = vec![0.0; per_packet * 2];
                // The socket and the target it is currently connected to, so a
                // host or port change reconnects and nothing else does.
                let mut sock: Option<UdpSocket> = None;
                let mut target: Option<(String, u16)> = None;
                while !stop_thread.load(Ordering::Relaxed) {
                    let cfg = control_thread.lock().map(|g| g.clone()).unwrap_or(None);
                    let Some(cfg) = cfg else {
                        // Not streaming: throw away anything queued so a later
                        // enable does not open with stale audio, then idle.
                        while frames.pop().is_ok() {}
                        sock = None;
                        target = None;
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    };
                    if target.as_ref() != Some(&(cfg.host.clone(), cfg.port)) {
                        sock = UdpSocket::bind(("0.0.0.0", 0)).ok().and_then(|s| {
                            s.connect((cfg.host.as_str(), cfg.port)).ok().map(|_| s)
                        });
                        target = Some((cfg.host.clone(), cfg.port));
                    }
                    // Fill up to one packet, no more, so a backlog drains in
                    // packet-sized bites rather than one giant datagram.
                    while inter.len() < per_packet * 2 {
                        match frames.pop() {
                            Ok(s) => inter.push(s),
                            Err(_) => break,
                        }
                    }
                    if inter.len() < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    let frame_count = inter.len() / 2;
                    // Interleaved L,R -> planar [L block][R block].
                    for (f, pair) in inter.chunks_exact(2).enumerate() {
                        planar[f] = pair[0];
                        planar[frame_count + f] = pair[1];
                    }
                    let id = if cfg.identifier.is_empty() {
                        DEFAULT_IDENTIFIER
                    } else {
                        &cfg.identifier
                    };
                    let packet =
                        encode_audio_packet(id, 2, sample_rate, &planar[..frame_count * 2]);
                    if let Some(s) = &sock {
                        let _ = s.send(&packet);
                    }
                    inter.clear();
                }
            })
            .expect("spawn reastream sender");
        Self {
            control,
            stop,
            thread: Some(thread),
        }
    }

    /// Point the sender at a new receiver, or `None` to stop sending. Cheap and
    /// lock-guarded; the audio thread never touches this.
    pub fn set_config(&self, config: Option<ReaStreamConfig>) {
        if let Ok(mut g) = self.control.lock() {
            *g = config;
        }
    }
}

impl Drop for ReaStreamSender {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }
    fn read_u16(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }
    fn read_f32(b: &[u8], at: usize) -> f32 {
        f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    #[test]
    fn an_audio_packet_matches_the_reastream_layout_byte_for_byte() {
        // Two channels, three frames: planar is [L0 L1 L2 R0 R1 R2].
        let planar = [0.0f32, 0.25, -0.5, 1.0, -1.0, 0.75];
        let p = encode_audio_packet("default", 2, 48_000, &planar);

        assert_eq!(&p[0..4], b"MRSR", "audio magic");
        let data_bytes = 6 * 4;
        assert_eq!(read_u32(&p, 4) as usize, 47 + data_bytes, "packet size = header + data");
        // identifier: "default" then a run of zeros, and the 32nd byte is 0.
        assert_eq!(&p[8..15], b"default");
        assert!(p[15..40].iter().all(|&b| b == 0), "identifier zero-padded");
        assert_eq!(p[39], 0, "identifier's final byte is NUL");
        assert_eq!(p[40], 2, "channel count");
        assert_eq!(read_u32(&p, 41), 48_000, "sample rate");
        assert_eq!(read_u16(&p, 45) as usize, data_bytes, "block length is BYTES");
        // The samples, non-interleaved, in order.
        for (i, want) in planar.iter().enumerate() {
            assert_eq!(read_f32(&p, 47 + i * 4), *want, "sample {i}");
        }
        assert_eq!(p.len(), 47 + data_bytes, "no trailing bytes");
    }

    #[test]
    fn an_over_long_identifier_is_truncated_and_still_nul_terminated() {
        let name = "a-really-long-stream-identifier-well-past-thirty-two-bytes";
        let p = encode_audio_packet(name, 1, 44_100, &[0.0]);
        // 31 bytes of name at most, byte 31 (the 32nd) always 0.
        assert_eq!(&p[8..8 + 31], &name.as_bytes()[..31]);
        assert_eq!(p[8 + 31], 0, "the reserved final byte is never overwritten");
    }

    #[test]
    fn the_sender_puts_real_interleaved_frames_on_the_wire_as_planar() {
        // A real UDP receiver on loopback, the sender pointed at it, and two
        // stereo frames pushed through the ring — the whole thread path, not
        // just the encoder: interleave in, planar on the wire.
        let recv = UdpSocket::bind(("127.0.0.1", 0)).expect("bind receiver");
        recv.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let port = recv.local_addr().unwrap().port();

        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(1024);
        // (L,R) = (0.5,-0.5) then (0.25,-0.25), interleaved.
        for s in [0.5f32, -0.5, 0.25, -0.25] {
            tx.push(s).unwrap();
        }
        let cfg = ReaStreamConfig {
            identifier: "test".to_owned(),
            host: "127.0.0.1".to_owned(),
            port,
        };
        let _sender = ReaStreamSender::start(rx, 48_000, Some(cfg));

        let mut buf = [0u8; 2048];
        let n = recv.recv(&mut buf).expect("a packet arrived");
        let p = &buf[..n];
        assert_eq!(&p[0..4], b"MRSR");
        assert_eq!(&p[8..12], b"test");
        assert_eq!(p[40], 2, "stereo");
        assert_eq!(read_u16(p, 45), 16, "2 frames * 2 ch * 4 bytes");
        // Non-interleaved on the wire: [L0 L1][R0 R1].
        assert_eq!(read_f32(p, 47), 0.5);
        assert_eq!(read_f32(p, 51), 0.25);
        assert_eq!(read_f32(p, 55), -0.5);
        assert_eq!(read_f32(p, 59), -0.25);
    }

    #[test]
    fn a_stereo_block_never_exceeds_the_protocol_ceiling() {
        // 150 stereo frames = 300 floats = 1200 bytes, the max; not one over.
        assert_eq!(frames_per_packet(2), 150);
        let planar = vec![0.0f32; frames_per_packet(2) * 2];
        let p = encode_audio_packet("x", 2, 48_000, &planar);
        assert_eq!(read_u16(&p, 45) as usize, MAX_BLOCK_BYTES);
    }
}
