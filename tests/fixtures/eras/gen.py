#!/usr/bin/env python3
"""Writes tests/fixtures/eras/*.json: synthetic files in every desk shape the
app has written, built from the frozen era tables. Deterministic."""
import json, os, sys
OUT = sys.argv[1]
DRUMS = "@tangent-drums"; DX7 = "@tangent-dx7"

def strips(ch):  # (drums, click, fx, master) strip indices of a C-channel era
    return ch, ch + 1, ch + 2, ch + 3

def desk_rows(n_strips, legacy_click=None, legacy_master=None):
    """colors/names over n_strips+1 (master last), sends over n_strips."""
    colors = [100 + i for i in range(n_strips + 1)]
    names = ["s%d" % i for i in range(n_strips + 1)]
    sends = [round((i + 1) / 100, 2) for i in range(n_strips)]
    if legacy_click is not None: colors[legacy_click] = 8
    if legacy_master is not None: colors[legacy_master] = 5
    return colors, names, sends

def bays(length, put):
    v = [""] * length
    for (seat, bay), p in put.items():
        v[seat * 3 + bay] = p
    return v

def write(name, obj):
    # One top-level key per line, each value compact: readable in a diff
    # and not ten thousand lines of "" per file.
    lines = []
    for k, v in obj.items():
        if k == "desk":
            inner = [" %s: %s" % (json.dumps(dk), json.dumps(dv, separators=(",", ":"))) for dk, dv in v.items()]
            lines.append("%s: {\n%s\n}" % (json.dumps(k), ",\n".join(inner)))
        else:
            lines.append("%s: %s" % (json.dumps(k), json.dumps(v, separators=(",", ":"))))
    with open(os.path.join(OUT, name), "w") as f:
        f.write("{\n" + ",\n".join(lines) + "\n}\n")

# ---------------------------------------------------------------- settings
# v12: 8 channels, BACKING at 8, click 9, fx 10, master 11; 36 bays.
c, n, s = desk_rows(11, legacy_click=9, legacy_master=11)
put = {(i, 0): "/x/s%d.vst3" % i for i in range(12)}
put[(0, 0)] = DX7
put[(11, 2)] = "/x/s11b2.vst3"
write("settings-v12.json", {
    "settings_version": 12, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": bays(36, put),
    "strip_muted": (1 << 9) | (1 << 2), "strip_soloed": (1 << 10) | (1 << 5),
    "channel_kinds": [1, 0, 0, 2, 0, 0, 0, 0], "channel_inputs": ["midi:all", "", "", "0", "", "", "", ""],
})

# v17: 8 channels with drums at 8, sixteen pads at 12..27: the 84-bay sub-era.
c, n, s = desk_rows(11)
put = {(i, 0): "/x/s%d.vst3" % i for i in range(12)}
put[(0, 0)] = DRUMS
put[(12 + 0, 0)] = "/x/pad0.vst3"; put[(12 + 2, 1)] = "/x/pad2.vst3"; put[(12 + 15, 2)] = "/x/pad15.vst3"
write("settings-v17-pads16.json", {
    "settings_version": 17, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": bays(84, put),
    "strip_muted": 1 << 9, "strip_soloed": 1 << 3,
    "channel_kinds": [1, 1, 0, 0, 0, 0, 0, 1],
})

# v18: 8 channels, 128 pads at 12..139; the drumpad on channel 4 (a pre-lanes
# drum channel, so the legacy pad block follows it).
c, n, s = desk_rows(11)
put = {(i, 0): "/x/s%d.vst3" % i for i in range(12)}
put[(4, 0)] = DRUMS
put[(12 + 0, 0)] = "/x/pad0.vst3"; put[(12 + 40, 1)] = "/x/pad40.vst3"; put[(12 + 127, 2)] = "/x/pad127.vst3"
write("settings-v18.json", {
    "settings_version": 18, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": bays(140 * 3, put),
    "strip_muted": (1 << 8) | (1 << 2), "strip_soloed": 1 << 10,
    "channel_kinds": [1, 1, 0, 0, 1, 0, 0, 0],
})

# v19: sixteen channels (19 sends), drums 16, click 17, fx 18, master 19,
# pads 20..147, master-extra 148, channel c's pads at 149 + (c-1)*128.
# The drum bus and the master are phase-inverted.
c, n, s = desk_rows(19, legacy_click=17, legacy_master=19)
put = {(i, 0): "/x/s%d.vst3" % i for i in range(20)}
put[(0, 0)] = DRUMS; put[(1, 0)] = DX7
put[(19, 2)] = "/x/s19b2.vst3"
put[(20 + 0, 0)] = "/x/pad0.vst3"; put[(20 + 40, 1)] = "/x/pad40.vst3"; put[(20 + 127, 2)] = "/x/pad127.vst3"
put[(148, 1)] = "/x/master-extra.vst3"
put[(149 + 2 * 128 + 5, 0)] = "/x/c3pad5.vst3"
write("settings-v19.json", {
    "settings_version": 19, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": bays((149 + 2 * 128 + 5 + 1) * 3, put),
    "strip_muted": (1 << 17) | (1 << 2), "strip_soloed": (1 << 18) | (1 << 5),
    "strip_phase": (1 << 16) | (1 << 19),
    "channel_kinds": [1, 1, 2, 1] + [0] * 12,
})

# v20 at 27 sends: the 24-channel window. drums 24, click 25, fx 26, master
# 27, pads 28..155, master-extra 156, channel c's pads 157 + (c-1)*128..3100.
c, n, s = desk_rows(27)
put = {(i, 0): "/x/s%d.vst3" % i for i in range(28)}
put[(0, 0)] = DRUMS; put[(1, 0)] = DX7
put[(27, 2)] = "/x/s27b2.vst3"
put[(28 + 0, 0)] = "/x/pad0.vst3"; put[(28 + 40, 1)] = "/x/pad40.vst3"; put[(28 + 127, 2)] = "/x/pad127.vst3"
put[(156, 1)] = "/x/master-extra.vst3"
put[(157 + 2 * 128 + 5, 0)] = "/x/c3pad5.vst3"
put[(3100, 2)] = "/x/c23pad127.vst3"
write("settings-v20-27.json", {
    "settings_version": 20, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": bays(3101 * 3, put),
    "strip_muted": (1 << 25) | (1 << 2), "strip_soloed": (1 << 26) | (1 << 21),
    "strip_phase": (1 << 24) | (1 << 27) | (1 << 3),
    "channel_kinds": [1, 1] + [0] * 20 + [2, 1],
})

# v20 at 35 sends: thirty-two channels, a trimmed dense array (pre-v21), with
# a folder-extra and a pedal seat at the far end.
c, n, s = desk_rows(35)
put = {(i, 0): "/x/s%d.vst3" % i for i in range(36)}
put[(0, 0)] = DRUMS; put[(1, 0)] = DX7
put[(36 + 40, 1)] = "/x/pad40.vst3"; put[(164, 1)] = "/x/master-extra.vst3"
put[(165 + 30 * 128 + 127, 0)] = "/x/c31pad127.vst3"
put[(4133 + 5, 2)] = "/x/folder5-extra.vst3"
put[(4165 + 3 * 7 + 6, 1)] = "/x/c3pedal6.vst3"
write("settings-v20.json", {
    "settings_version": 20, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": bays((4165 + 3 * 7 + 6 + 1) * 3, put),
    "strip_muted": (1 << 33) | (1 << 2), "strip_soloed": (1 << 34), "strip_phase": (1 << 32) | (1 << 35),
    "channel_kinds": [1, 1] + [0] * 29 + [2],
})

# v21: the head (108) and the sparse tail.
head = bays(108, {(0, 0): DRUMS, (1, 0): DX7, (32, 0): "/x/drumbus.vst3", (35, 2): "/x/s35b2.vst3"})
write("settings-v21.json", {
    "settings_version": 21, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": head,
    "strip_inserts_tail": {str(76 * 3 + 1): "/x/pad40.vst3", str(4138 * 3 + 2): "/x/folder5-extra.vst3"},
    "strip_muted": 1 << 33, "strip_soloed": 0, "strip_phase": 1 << 35,
})

# v22: v21 plus the per-track MIDI device sets.
write("settings-v22.json", {
    "settings_version": 22, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": head,
    "strip_inserts_tail": {str(4138 * 3 + 2): "/x/folder5-extra.vst3"},
    "strip_muted": 1 << 2, "strip_soloed": 1 << 32, "strip_phase": 1 << 33,
    "channel_kinds": [1, 1, 1], "channel_inputs": ["midi:all", "midi:all", "midi:Keystation 88"],
    "channel_midi_inputs": {"2": {"devices": ["Keystation 88"], "channel": 3}},
})

# ---------------------------------------------------------------- DAW blobs
# 3.x: a v11 blob (five slots, four inputs, track, click, fx; master at 12).
inserts = [""] * (13 * 3)
inserts[0] = "/x/Pro-R 2.vst3"; inserts[2 * 3] = "/x/Pro-C 3.vst3"; inserts[12 * 3] = "/x/Pro-L 2.vst3"
write("daw-3x.json", {
    "settings_version": 11,
    "plugin_slots": ["/x/Pianoteq 8.vst3", None, "/x/Diva.vst3", None, None],
    "plugin_gains": [0.8, 1.0, 0.6, 1.0, 1.0], "input_gains": [0.9, 0.7, 1.0, 1.0],
    "record_extra_inputs": ["uid-4"],
    "strip_colors_v2": [3, 0, 7, 0, 0, 16, 21, 0, 0, 23, 0, 0, 5],
    "strip_names": ["Piano", "", "Lead", "", "", "Mic", "Amp", "", "", "", "", "", "Sum"],
    "strip_sends": [1.0, 1.0, 0.4, 1.0, 1.0, 0.0, 0.2, 0.0, 0.0, 0.1, 0.0, 0.5],
    "strip_muted": 32, "strip_inserts": inserts,
})
# 4.x: a v16 blob, eight channels with the drum bus and sixteen pads.
c, n, s = desk_rows(11)
put = {(0, 0): DX7, (8, 1): "/x/drumbus.vst3", (11, 0): "/x/Pro-L 2.vst3", (12 + 3, 0): "/x/pad3.vst3"}
write("daw-4x.json", {
    "settings_version": 16, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": bays(84, put), "strip_muted": 1 << 8, "strip_soloed": 1 << 1,
    "channel_kinds": [1, 2, 0, 0, 0, 0, 0, 0], "channel_inputs": ["", "0", "", "", "", "", "", ""],
})
# 5.x: a 5.0 blob, v19 at sixteen channels.
c, n, s = desk_rows(19)
put = {(0, 0): DRUMS, (1, 0): DX7, (16, 0): "/x/drumbus.vst3", (19, 0): "/x/Pro-L 2.vst3",
       (20 + 36, 1): "/x/kick.vst3", (149 + 128 + 2, 2): "/x/c2pad2.vst3"}
write("daw-5x.json", {
    "settings_version": 19, "strip_colors_v2": c, "strip_names": n, "strip_sends": s,
    "strip_inserts": bays((149 + 128 + 2 + 1) * 3, put),
    "strip_muted": 1 << 17, "strip_soloed": 1 << 4, "strip_phase": 1 << 16,
    "channel_kinds": [1, 1, 1] + [0] * 13,
})

# ---------------------------------------------------------------- documents
def doc(version, desk, automation, clips=None):
    d = {"arrangement_version": version, "desk": desk}
    if automation is not None:
        d["automation"] = [{"lane": l, "param": p, "points": [{"at": 0.0, "v": v}, {"at": 4.0, "v": v / 2}]}
                           for (l, p, v) in automation]
    d["clips"] = clips or [{"lane": 1, "kind": "midi", "start_s": 0.0, "len_s": 2.0, "notes": []}]
    return d

def legacy_desk(ch, n_strips, inserts, extra=None):
    colors, names, sends = desk_rows(n_strips)
    d = {"kinds": [1, 1] + [0] * (ch - 2), "names": names, "colors": colors, "inserts": inserts,
         "sends": sends, "muted": 0, "soloed": 0, "gains": [1.0] * ch, "pans": [0.0] * ch,
         "inputs": ["midi:all"] + [""] * (ch - 1), "tempo_bpm": 96.0}
    d.update(extra or {})
    return d

# v4 at 11 sends, the 84-bay pad sub-era.
put = {(0, 0): DRUMS, (1, 0): DX7, (8, 0): "/x/drumbus.vst3", (9, 1): "/x/click.vst3",
       (11, 2): "/x/master.vst3", (12 + 2, 0): "/x/pad2.vst3", (12 + 15, 1): "/x/pad15.vst3"}
write("doc-v4-11.json", doc(4, legacy_desk(8, 11, bays(84, put), {"muted": (1 << 9) | 1, "soloed": 1 << 10}),
                             [(0, "gain", 0.5), (16, "gain", 0.8)]))
# v5 at 18 sends: sixteen channels, no drum bus (click 16, fx 17, master 18), 57 bays.
put = {(0, 0): DRUMS, (1, 0): DX7, (16, 0): "/x/click.vst3", (17, 1): "/x/fx.vst3", (18, 2): "/x/master.vst3",
       (15, 0): "/x/c15.vst3"}
write("doc-v5-18.json", doc(5, legacy_desk(16, 18, bays(57, put), {"muted": 1 << 16, "soloed": (1 << 17) | (1 << 3)}),
                             [(3, "pan", 0.25), (16, "gain", 0.9), (16, "mute", 1.0)]))
# v5 at 19 sends: sixteen channels with the drum bus; dense to channel 3's pad 5.
put = {(0, 0): DRUMS, (1, 0): DX7, (16, 0): "/x/drumbus.vst3", (17, 0): "/x/click.vst3",
       (19, 2): "/x/master.vst3", (20 + 40, 1): "/x/pad40.vst3", (148, 0): "/x/master-extra.vst3",
       (149 + 2 * 128 + 5, 2): "/x/c3pad5.vst3"}
write("doc-v5-19.json", doc(5, legacy_desk(16, 19, bays((149 + 2 * 128 + 6) * 3, put),
                                           {"muted": (1 << 17), "soloed": (1 << 16) | (1 << 2), "tracks_added": 7}),
                             [(2, "gain", 0.3), (16, "gain", 0.7)]))
# v6 at 27 sends: the 24-channel window, master curves at 24.
put = {(0, 0): DRUMS, (1, 0): DX7, (23, 0): "/x/c23.vst3", (24, 0): "/x/drumbus.vst3", (26, 1): "/x/fx.vst3",
       (27, 2): "/x/master.vst3", (28 + 127, 0): "/x/pad127.vst3", (156, 1): "/x/master-extra.vst3",
       (157 + 2 * 128 + 5, 2): "/x/c3pad5.vst3"}
write("doc-v6-27.json", doc(6, legacy_desk(24, 27, bays((157 + 2 * 128 + 6) * 3, put),
                                           {"muted": (1 << 25) | (1 << 23), "soloed": 1 << 26,
                                            "tracks_added": (1 << 23) | 3, "armed": 2}),
                             [(23, "gain", 0.6), (24, "gain", 0.4), (24, "pan", 0.1)]))
# v6 at 35 sends: thirty-two channels, master curves at 32.
put = {(0, 0): DRUMS, (1, 0): DX7, (31, 0): "/x/c31.vst3", (32, 0): "/x/drumbus.vst3", (35, 2): "/x/master.vst3",
       (36 + 40, 1): "/x/pad40.vst3"}
write("doc-v6-35.json", doc(6, legacy_desk(32, 35, bays(80 * 3, put),
                                           {"muted": 1 << 33, "soloed": 1 << 31, "tracks_added": (1 << 31) | 3, "armed": 1 << 31}),
                             [(31, "gain", 0.6), (32, "gain", 0.4)]))
# v7: the head and the sparse tail, phase and the late fields.
head = bays(108, {(0, 0): DRUMS, (1, 0): DX7, (32, 0): "/x/drumbus.vst3", (35, 2): "/x/master.vst3"})
d7 = legacy_desk(32, 35, head, {"muted": 1 << 2, "soloed": 0, "phase": (1 << 32) | (1 << 35),
                                "tracks_added": 3, "armed": 2,
                                "inserts_tail": {str(76 * 3 + 1): "/x/pad40.vst3", str(4138 * 3 + 2): "/x/folder5-extra.vst3"}})
write("doc-v7.json", doc(7, d7, [(1, "gain", 0.5), (32, "gain", 0.25), (32, "fx:4:7", 0.5)]))
print("ok")
