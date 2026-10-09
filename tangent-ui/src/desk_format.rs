//! **Format 2: the one desk serializer**, for settings.json, documents and
//! templates alike (design section 6 and Appendix D).
//!
//! **One writer, one reader, one switch.** The desk used to be written by
//! two serializers (the settings file's and the document's), each with its
//! own idea of which arrays were the desk and how wide they were, and each
//! old shape is recognised today only by its LENGTHS. Format 2 says what it
//! is (`"format": 2`), so a reader dispatches on that and never consults a
//! length. Both files write the same `desk` object through [`write`], and
//! every side effect of writing it (the stamps, the backups, the drumpad.json
//! keys, the pstate names, the `legacy_states` marker) reads [`WRITE_V8`],
//! so nothing is stamped 23 or 8 before the file really is format 2, and one
//! commit turns all of it on.
//!
//! **Errors never cost a file** (design 6.3). A dangling or malformed ENTRY
//! (a plugin, bypass, route, key or send naming no row; a pedal or pad off a
//! track; a fourth bay; a chain past `MAX_CHAIN`) is dropped and listed in
//! [`DeskRead::dropped`], and everything else reads. Only a desk that is not
//! a desk at all ([`Structural`]: too many rows, two rows with one id, the
//! wrong shape) is refused, and then the refusal carries the raw JSON so the
//! caller can keep it beside the file rather than lose it. What the caller
//! does with a refusal (a side file, `extra["desk_refused"]`, a refused open)
//! is the persistence lane's; this module never touches the disk.
//!
//! **Nothing is allocated by a value.** A key's number (`t4000000000`,
//! `c65535`, a hardware jack) is parsed and compared, never used as a size;
//! the only allocation a file controls is proportional to the file itself,
//! and the row count is bounded by `MAX_ROWS` before any row is read.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::desk_id::{Node, SeatBay, TrackId, MAX_ROWS};
use crate::desk_lift::DeskEra;
use crate::desk_model::{
    DeskModel, Dx7, Frozen, KeysSel, RouteEdge, RouteTo, RowKind, Send, SideKey, Special, TrackRow,
    KEYS_SPAN,
};
use crate::recorder::MidiInputSel;

/// The `format` a format-2 desk says it is.
pub const DESK_FORMAT: u64 = 2;

/// **The one switch.** While false, production writes today's bytes: no
/// format-2 desk, no stamp 23 or 8, no backup, no pending marker, no
/// pstate name. Tests reach the format-2 writers through their own entry
/// points and never flip this. MERGE-ME(W4-integrate (B)): flipped there,
/// in the commit that lands the copy-only shelf.
pub const WRITE_V8: bool = false;

/// [`WRITE_V8`], or a test on this thread that asked for format 2
/// ([`with_format2`]). Every format-2 side effect reads THIS, so a test can
/// drive a whole save road (the stamps, the backups, the pending marker,
/// the drumpad keys) without the const ever being flipped.
pub fn write_v8_on() -> bool {
    WRITE_V8 || format2_forced()
}

/// **Which desk a writer writes.** The production writers ask
/// [`Format::current`]; the `*_as` entry points take one, which is how a
/// test reaches format 2 while production still writes today's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Today's shape: the dense arrays and masks, stamped 22 and 7.
    Legacy,
    /// The `desk` object of this module, stamped 23 and 8.
    V2,
}

impl Format {
    /// What the production writers write: [`WRITE_V8`] decides, and nothing
    /// else.
    pub fn current() -> Self {
        if write_v8_on() {
            Format::V2
        } else {
            Format::Legacy
        }
    }

    /// The `settings_version` a settings file in this format says. The
    /// stamp follows the shape: nothing says 23 before it is format 2.
    pub const fn settings_stamp(self) -> u64 {
        match self {
            Format::Legacy => 22,
            Format::V2 => 23,
        }
    }

    /// The `arrangement_version` a document in this format says, the
    /// floor under a document's own (a higher stamp rides along).
    pub const fn document_stamp(self) -> u64 {
        match self {
            Format::Legacy => 7,
            Format::V2 => 8,
        }
    }
}

#[cfg(not(test))]
const fn format2_forced() -> bool {
    false
}

#[cfg(test)]
thread_local! {
    static FORMAT2_FORCED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn format2_forced() -> bool {
    FORMAT2_FORCED.with(std::cell::Cell::get)
}

/// **`f` run with the format-2 writers on**, on this thread only: what
/// flipping [`WRITE_V8`] will do, pinned before the flip. Restored on the
/// way out, a panic included.
#[cfg(test)]
pub(crate) fn with_format2<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            FORMAT2_FORCED.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(FORMAT2_FORCED.with(|c| c.replace(true)));
    f()
}

/// **A desk read**, and every entry that was dropped on the way: the
/// caller raises one notice naming them.
#[derive(Debug, Clone, PartialEq)]
pub struct DeskRead {
    pub model: DeskModel,
    pub dropped: Vec<Dropped>,
}

/// **One entry the reader dropped**: what kind it was (`plugin`, `bypass`,
/// `route`, `key`, `send`, `row`) and the text that named it, so a notice
/// can say which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub what: &'static str,
    pub key: String,
}

impl std::fmt::Display for Dropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.what, self.key)
    }
}

/// **Why a desk is not a desk.** Each refuses the DESK only, never the
/// file: see the module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Structural {
    /// More rows than `MAX_ROWS`: the count is reported, nothing is read.
    TooManyRows(usize),
    /// Two rows hold this id, and no rule can say which one a reference
    /// meant.
    DuplicateId(u32),
    /// Not the documented shape; the text names the part that was wrong.
    BadShape(&'static str),
}

impl std::fmt::Display for Structural {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Structural::TooManyRows(n) => write!(f, "the desk has {n} rows, past {MAX_ROWS}"),
            Structural::DuplicateId(id) => write!(f, "two rows are t{id}"),
            Structural::BadShape(what) => write!(f, "the desk is not readable: {what}"),
        }
    }
}

/// **A refused desk, and the JSON it was**, kept whole so the caller can
/// put it somewhere instead of losing it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeskRefused {
    pub why: Structural,
    pub raw: Value,
}

/// Whether a `desk` value is format 2. The reader's dispatch, and the only
/// one: a legacy desk is anything else, whatever its lengths say.
pub fn is_format2(desk: &Value) -> bool {
    desk.get("format").and_then(Value::as_u64) == Some(DESK_FORMAT)
}

// ── writer ───────────────────────────────────────────────────────────────

/// A float as JSON. `serde_json` writes a non-finite one as `null`, which
/// this reader refuses as the wrong shape, so the writer never lets one
/// out: the desk it writes must be a desk it reads.
fn real(x: f64) -> Value {
    Value::from(if x.is_finite() { x } else { 0.0 })
}

fn obj<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn sends_json(sends: &[Send]) -> Value {
    Value::Array(
        sends
            .iter()
            .map(|s| {
                obj([
                    ("to", Value::String(s.to.to_string())),
                    ("level", real(s.level)),
                    ("pre", Value::Bool(s.pre)),
                ])
            })
            .collect(),
    )
}

fn row_json(r: &TrackRow) -> Value {
    // Exhaustive, so a field added to the row cannot go unwritten.
    let TrackRow {
        id,
        kind,
        added,
        icon,
        name,
        color,
        gain,
        pan,
        input,
        midi,
        mute,
        solo,
        phase,
        arm,
        theory,
        folder,
        in_folder,
        shut,
        sends,
        dx7,
        keys,
        frozen,
    } = r;
    obj([
        ("id", Value::from(id.0)),
        ("kind", kind.map_or(Value::Null, |k| Value::String(k.name().to_owned()))),
        ("added", Value::Bool(*added)),
        ("icon", Value::from(*icon)),
        ("name", Value::String(name.clone())),
        ("color", Value::from(*color)),
        ("gain", real(*gain)),
        ("pan", real(*pan)),
        ("input", Value::String(input.clone())),
        (
            "midi",
            obj([
                ("devices", Value::Array(midi.devices.iter().cloned().map(Value::String).collect())),
                ("channel", midi.channel.map_or(Value::Null, Value::from)),
            ]),
        ),
        ("mute", Value::Bool(*mute)),
        ("solo", Value::Bool(*solo)),
        ("phase", Value::Bool(*phase)),
        ("arm", Value::Bool(*arm)),
        ("theory", Value::Bool(*theory)),
        ("folder", Value::Bool(*folder)),
        ("in_folder", Value::Bool(*in_folder)),
        ("shut", Value::Bool(*shut)),
        ("sends", sends_json(sends)),
        ("dx7", obj([("cart", Value::String(dx7.cart.clone())), ("patch", Value::from(dx7.patch))])),
        (
            "keys",
            obj([
                ("patch", Value::from(keys.patch)),
                ("params", Value::Array(keys.params.iter().map(|p| real(*p)).collect())),
            ]),
        ),
        (
            "frozen",
            frozen.as_ref().map_or(Value::Null, |f| {
                obj([("path", Value::String(f.path.clone())), ("len_s", real(f.len_s))])
            }),
        ),
    ])
}

fn special_json(s: &Special) -> Value {
    let Special { name, color, sends, mute, solo, phase } = s;
    obj([
        ("name", Value::String(name.clone())),
        ("color", Value::from(*color)),
        ("sends", sends_json(sends)),
        ("mute", Value::Bool(*mute)),
        ("solo", Value::Bool(*solo)),
        ("phase", Value::Bool(*phase)),
    ])
}

fn route_json(e: &RouteEdge) -> Value {
    let mut m = Map::new();
    m.insert("from".into(), Value::String(e.from.to_string()));
    match e.to {
        RouteTo::Node(n) => {
            m.insert("to".into(), Value::String(n.to_string()));
        }
        RouteTo::Hardware { first, stereo } => {
            m.insert("to".into(), Value::String(HARDWARE.into()));
            m.insert("first".into(), Value::from(first));
            m.insert("stereo".into(), Value::Bool(stereo));
        }
    }
    m.insert("gain".into(), real(f64::from(e.gain)));
    Value::Object(m)
}

/// A route's hardware end. No node is spelt `hw`, so it cannot be mistaken
/// for one.
const HARDWARE: &str = "hw";

/// **The `desk` object for `model`**: rows in lane order (vacant rows
/// included), the specials, plugins and bypass sorted by bay identity,
/// routes and keys in the order they were made. `legacy_states` appears
/// only while the model carries one. Deterministic: one model, one text.
pub fn write(model: &DeskModel) -> Value {
    let DeskModel { next_id, rows, specials, plugins, bypass, routes, keys, legacy_states } = model;
    let mut m = Map::new();
    m.insert("format".into(), Value::from(DESK_FORMAT));
    m.insert("next_id".into(), Value::from(*next_id));
    if let Some(era) = legacy_states {
        m.insert("legacy_states".into(), Value::String(era.name().to_owned()));
    }
    m.insert("rows".into(), Value::Array(rows.iter().map(row_json).collect()));
    m.insert(
        "specials".into(),
        Value::Object(
            specials
                .iter()
                .map(|(node, s)| (node.to_string(), special_json(s)))
                .collect(),
        ),
    );
    m.insert(
        "plugins".into(),
        Value::Object(plugins.iter().map(|(sb, p)| (sb.to_string(), Value::String(p.clone()))).collect()),
    );
    m.insert(
        "bypass".into(),
        Value::Array(bypass.iter().map(|sb| Value::String(sb.to_string())).collect()),
    );
    m.insert("routes".into(), Value::Array(routes.iter().map(route_json).collect()));
    m.insert(
        "keys".into(),
        Value::Array(
            keys.iter()
                .map(|k| {
                    obj([
                        ("seat", Value::String(k.seat.to_string())),
                        ("from", Value::String(k.from.to_string())),
                    ])
                })
                .collect(),
        ),
    );
    Value::Object(m)
}

// ── reader ───────────────────────────────────────────────────────────────

type Shape<T> = Result<T, Structural>;

fn bad<T>(what: &'static str) -> Shape<T> {
    Err(Structural::BadShape(what))
}

/// A key's value, with `null` read as absent: every field has a default,
/// and a writer that spelt "nothing" as `null` meant the default.
fn field<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    m.get(key).filter(|v| !v.is_null())
}

fn flag(m: &Map<String, Value>, key: &str, what: &'static str) -> Shape<bool> {
    match field(m, key) {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => bad(what),
    }
}

fn float(m: &Map<String, Value>, key: &str, default: f64, what: &'static str) -> Shape<f64> {
    match field(m, key) {
        None => Ok(default),
        Some(v) => v.as_f64().map_or_else(|| bad(what), Ok),
    }
}

fn text(m: &Map<String, Value>, key: &str, what: &'static str) -> Shape<String> {
    match field(m, key) {
        None => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => bad(what),
    }
}

fn count(m: &Map<String, Value>, key: &str, what: &'static str) -> Shape<usize> {
    match field(m, key) {
        None => Ok(0),
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).map_or_else(|| bad(what), Ok),
    }
}

fn object<'a>(v: &'a Value, what: &'static str) -> Shape<&'a Map<String, Value>> {
    v.as_object().map_or_else(|| bad(what), Ok)
}

/// A collection that may be absent: `None` reads as empty, anything but
/// the right container is the wrong shape.
fn array<'a>(m: &'a Map<String, Value>, key: &str, what: &'static str) -> Shape<&'a [Value]> {
    match field(m, key) {
        None => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        Some(_) => bad(what),
    }
}

/// The compact text of a malformed entry, for its dropped-entry notice.
fn raw_text(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// One send, or the notice for it. A malformed send is a LOCAL error: it
/// costs that send.
fn read_send(v: &Value, from: &str) -> Result<Send, Dropped> {
    let lost = || Dropped { what: "send", key: format!("{from}>{}", raw_text(v)) };
    let m = v.as_object().ok_or_else(lost)?;
    let to = field(m, "to").and_then(Value::as_str).and_then(|s| s.parse::<Node>().ok()).ok_or_else(lost)?;
    let level = match field(m, "level") {
        None => 0.0,
        Some(l) => l.as_f64().ok_or_else(lost)?,
    };
    let pre = match field(m, "pre") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(lost()),
    };
    Ok(Send { to, level, pre })
}

/// A sender's sends: malformed ones dropped, a second send to the same
/// place silently folded into the first (two knobs on one path is a
/// hand-edit's mistake, the same forgiveness `Routes::add` gives).
fn read_sends(m: &Map<String, Value>, from: &str, lost: &mut Vec<Dropped>) -> Shape<Vec<Send>> {
    let mut out: Vec<Send> = Vec::new();
    for v in array(m, "sends", "a `sends` is not a list")? {
        match read_send(v, from) {
            Ok(s) if !out.iter().any(|o| o.to == s.to && o.pre == s.pre) => out.push(s),
            Ok(_) => {}
            Err(d) => lost.push(d),
        }
    }
    Ok(out)
}

fn read_row(v: &Value, lost: &mut Vec<Dropped>) -> Shape<TrackRow> {
    let m = object(v, "a row is not an object")?;
    let id = field(m, "id")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .map_or_else(|| bad("a row has no u32 `id`"), Ok)?;
    let mut row = TrackRow::vacant(TrackId(id));
    row.kind = match field(m, "kind") {
        None => None,
        Some(Value::String(s)) => Some(RowKind::from_name(s).map_or_else(|| bad("a row's `kind` is unknown"), Ok)?),
        Some(_) => return bad("a row's `kind` is not a name"),
    };
    row.added = flag(m, "added", "a row's `added`")?;
    row.icon = match field(m, "icon") {
        None => 0,
        Some(v) => v.as_u64().and_then(|n| u8::try_from(n).ok()).map_or_else(|| bad("a row's `icon`"), Ok)?,
    };
    row.name = text(m, "name", "a row's `name`")?;
    row.color = match field(m, "color") {
        None => 0,
        Some(v) => v.as_i64().map_or_else(|| bad("a row's `color`"), Ok)?,
    };
    row.gain = float(m, "gain", 1.0, "a row's `gain`")?;
    row.pan = float(m, "pan", 0.0, "a row's `pan`")?;
    row.input = text(m, "input", "a row's `input`")?;
    if let Some(v) = field(m, "midi") {
        let mm = object(v, "a row's `midi`")?;
        let devices = array(mm, "devices", "a row's `midi.devices`")?
            .iter()
            .map(|d| d.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .map_or_else(|| bad("a row's `midi.devices`"), Ok)?;
        let channel = match field(mm, "channel") {
            None => None,
            Some(c) => Some(
                c.as_u64()
                    .filter(|c| *c < 16)
                    .map_or_else(|| bad("a row's `midi.channel`"), |c| Ok(c as u8))?,
            ),
        };
        row.midi = MidiInputSel { devices, channel };
    }
    row.mute = flag(m, "mute", "a row's `mute`")?;
    row.solo = flag(m, "solo", "a row's `solo`")?;
    row.phase = flag(m, "phase", "a row's `phase`")?;
    row.arm = flag(m, "arm", "a row's `arm`")?;
    row.theory = flag(m, "theory", "a row's `theory`")?;
    row.folder = flag(m, "folder", "a row's `folder`")?;
    row.in_folder = flag(m, "in_folder", "a row's `in_folder`")?;
    row.shut = flag(m, "shut", "a row's `shut`")?;
    row.sends = read_sends(m, &TrackId(id).to_string(), lost)?;
    if let Some(v) = field(m, "dx7") {
        let dm = object(v, "a row's `dx7`")?;
        row.dx7 = Dx7 { cart: text(dm, "cart", "a row's `dx7.cart`")?, patch: count(dm, "patch", "a row's `dx7.patch`")? };
    }
    if let Some(v) = field(m, "keys") {
        let km = object(v, "a row's `keys`")?;
        let mut keys = KeysSel { patch: count(km, "patch", "a row's `keys.patch`")?, ..KeysSel::default() };
        // A short list keeps the shipped knobs past its end and a long one
        // is cut at the span: the knobs are indexed, so a ragged list can
        // only mean "these first".
        for (dst, p) in keys.params.iter_mut().zip(array(km, "params", "a row's `keys.params`")?) {
            *dst = p.as_f64().map_or_else(|| bad("a row's `keys.params`"), Ok)?;
        }
        debug_assert_eq!(keys.params.len(), KEYS_SPAN);
        row.keys = keys;
    }
    if let Some(v) = field(m, "frozen") {
        let fm = object(v, "a row's `frozen`")?;
        row.frozen = Some(Frozen {
            path: text(fm, "path", "a row's `frozen.path`")?,
            len_s: float(fm, "len_s", 0.0, "a row's `frozen.len_s`")?,
        });
    }
    Ok(row)
}

fn read_special(v: Option<&Value>, node: Node, lost: &mut Vec<Dropped>) -> Shape<Special> {
    let Some(v) = v else {
        return Ok(Special::default());
    };
    let m = object(v, "a special is not an object")?;
    Ok(Special {
        name: text(m, "name", "a special's `name`")?,
        color: match field(m, "color") {
            None => 0,
            Some(v) => v.as_i64().map_or_else(|| bad("a special's `color`"), Ok)?,
        },
        sends: read_sends(m, &node.to_string(), lost)?,
        mute: flag(m, "mute", "a special's `mute`")?,
        solo: flag(m, "solo", "a special's `solo`")?,
        phase: flag(m, "phase", "a special's `phase`")?,
    })
}

fn read_route(v: &Value) -> Option<RouteEdge> {
    let m = v.as_object()?;
    let from = field(m, "from")?.as_str()?.parse::<Node>().ok()?;
    let to = match field(m, "to")?.as_str()? {
        HARDWARE => RouteTo::Hardware {
            first: usize::try_from(field(m, "first")?.as_u64()?).ok()?,
            stereo: match field(m, "stereo") {
                None => true,
                Some(s) => s.as_bool()?,
            },
        },
        n => RouteTo::Node(n.parse::<Node>().ok()?),
    };
    // Clamped as `Routes::from_json` clamps: a route is the strip's own
    // fader, 0..=1.
    let gain = match field(m, "gain") {
        None => 1.0,
        Some(g) => g.as_f64()?.clamp(0.0, 1.0) as f32,
    };
    Some(RouteEdge { from, to, gain })
}

fn read_key(v: &Value) -> Option<SideKey> {
    let m = v.as_object()?;
    Some(SideKey {
        seat: field(m, "seat")?.as_str()?.parse::<SeatBay>().ok()?,
        from: field(m, "from")?.as_str()?.parse::<Node>().ok()?,
    })
}

/// **Read a format-2 `desk` object.** Dropped entries come back listed;
/// a structural problem refuses the desk and hands back its JSON.
pub fn read(desk: &Value) -> Result<DeskRead, DeskRefused> {
    read_inner(desk).map_err(|why| DeskRefused { why, raw: desk.clone() })
}

fn read_inner(desk: &Value) -> Shape<DeskRead> {
    let m = object(desk, "the desk is not an object")?;
    if !is_format2(desk) {
        return bad("the desk does not say `format: 2`");
    }
    let rows_json = match field(m, "rows") {
        Some(Value::Array(a)) => a,
        _ => return bad("the desk has no `rows` list"),
    };
    // Before a single row is read: the count is the one number in the file
    // that would otherwise size an allocation.
    if rows_json.len() > MAX_ROWS {
        return Err(Structural::TooManyRows(rows_json.len()));
    }
    let mut lost = Vec::new();
    let mut rows = Vec::with_capacity(rows_json.len());
    let mut ids = BTreeSet::new();
    for v in rows_json {
        let row = read_row(v, &mut lost)?;
        if !ids.insert(row.id) {
            return Err(Structural::DuplicateId(row.id.0));
        }
        rows.push(row);
    }
    // Repaired in silence: a counter behind the ids it should be above is
    // a hand-edit's slip, and raising it loses nothing.
    let next_id = match field(m, "next_id") {
        None => 0,
        Some(v) => v.as_u64().map_or_else(|| bad("the desk's `next_id`"), Ok)?,
    }
    .max(ids.last().map_or(0, |id| u64::from(id.0) + 1));
    // An unknown era is refused rather than forgotten: the marker is what
    // finds a plugin's legacy state, and dropping it would open every such
    // plugin at its defaults.
    let legacy_states = match field(m, "legacy_states") {
        None => None,
        Some(Value::String(s)) => Some(DeskEra::from_name(s).map_or_else(|| bad("the desk's `legacy_states`"), Ok)?),
        Some(_) => return bad("the desk's `legacy_states`"),
    };

    let mut specials = crate::desk_model::Specials::default();
    if let Some(v) = field(m, "specials") {
        let sm = object(v, "the desk's `specials`")?;
        for node in Node::SPECIALS {
            let s = read_special(field(sm, &node.to_string()), node, &mut lost)?;
            *specials.get_mut(node).expect("a special") = s;
        }
    }

    let mut plugins = BTreeMap::new();
    if let Some(v) = field(m, "plugins") {
        for (k, p) in object(v, "the desk's `plugins`")? {
            match (k.parse::<SeatBay>(), p.as_str().filter(|p| !p.is_empty())) {
                (Ok(sb), Some(p)) => {
                    plugins.insert(sb, p.to_owned());
                }
                _ => lost.push(Dropped { what: "plugin", key: k.clone() }),
            }
        }
    }
    let mut bypass = BTreeSet::new();
    for v in array(m, "bypass", "the desk's `bypass`")? {
        match v.as_str().map(str::parse::<SeatBay>) {
            Some(Ok(sb)) => {
                bypass.insert(sb);
            }
            _ => lost.push(Dropped { what: "bypass", key: v.as_str().map_or_else(|| raw_text(v), str::to_owned) }),
        }
    }
    let mut routes: Vec<RouteEdge> = Vec::new();
    for v in array(m, "routes", "the desk's `routes`")? {
        match read_route(v) {
            // Every route is read (no cap), and the same path twice keeps
            // its first, as `Routes::from_json` always has.
            Some(e) if !routes.iter().any(|r| r.from == e.from && r.to == e.to) => routes.push(e),
            Some(_) => {}
            None => lost.push(Dropped { what: "route", key: raw_text(v) }),
        }
    }
    let mut keys: Vec<SideKey> = Vec::new();
    for v in array(m, "keys", "the desk's `keys`")? {
        match read_key(v) {
            // One key per bay, the first, as `KeyEdges::from_json` keeps it.
            Some(k) if !keys.iter().any(|o| o.seat == k.seat) => keys.push(k),
            Some(_) => {}
            None => lost.push(Dropped { what: "key", key: raw_text(v) }),
        }
    }

    let mut model = DeskModel { next_id, rows, specials, plugins, bypass, routes, keys, legacy_states };
    lost.extend(model.prune());
    Ok(DeskRead { model, dropped: lost })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrangement::Arrangement;
    use crate::desk_id::{Rack, MAX_CHAIN};
    use crate::desk_legacy::identity_ids;
    use crate::desk_lift::era_of_desk;
    use crate::desk_model::{picture, V22Desk};
    use crate::era_corpus::{DOCUMENTS, SETTINGS};
    use crate::settings::Settings;

    fn text_of(m: &DeskModel) -> String {
        serde_json::to_string(&write(m)).expect("serialisable")
    }

    /// A fixture's desk, through the legacy chain and `from_v22`.
    fn settings_model(text: &str) -> (V22Desk, DeskRead) {
        let v = V22Desk::from_settings(&Settings::from_json(text));
        let read = DeskModel::from_v22(&v, &identity_ids(32));
        (v, read)
    }

    /// A document's desk, through the legacy chain and `from_v22`, marked
    /// with the era its desk was written in, as the persistence lane will
    /// mark it. Fields the document does not say come from a vacant desk.
    fn document_model(text: &str) -> (V22Desk, DeskRead) {
        let a = Arrangement::from_json(text);
        let snap = a.desk.as_ref().expect("every era fixture has a desk");
        let v = V22Desk::from_snap(snap, &V22Desk::vacant());
        let raw: Value = serde_json::from_str(text).expect("json");
        let mut read = DeskModel::from_v22(&v, &identity_ids(32));
        read.model.legacy_states = Some(era_of_desk(&raw["desk"]));
        (v, read)
    }

    fn every_fixture() -> Vec<(&'static str, V22Desk, DeskRead)> {
        let mut out = Vec::new();
        for (name, text) in SETTINGS {
            let (v, r) = settings_model(text);
            out.push((name, v, r));
        }
        for (name, text) in DOCUMENTS {
            let (v, r) = document_model(text);
            out.push((name, v, r));
        }
        out
    }

    /// **Every era fixture is a fixed point of the format**: written, read
    /// back and written again it is the same bytes, and it reads back to
    /// the same model with nothing dropped.
    #[test]
    fn every_era_fixture_writes_reads_and_rewrites_to_the_same_bytes() {
        for (name, _, read) in every_fixture() {
            assert!(read.dropped.is_empty(), "{name}: the legacy desk lost {:?}", read.dropped);
            let first = text_of(&read.model);
            let back = read_str(&first).unwrap_or_else(|e| panic!("{name}: refused {:?}", e.why));
            assert!(back.dropped.is_empty(), "{name}: {:?}", back.dropped);
            assert_eq!(back.model, read.model, "{name} read back differently");
            assert_eq!(text_of(&back.model), first, "{name} rewrote differently");
        }
    }

    /// **The dense input comes back**: `from_v22 -> write -> read -> to_v22`
    /// returns each fixture's dense desk exactly, ids included.
    #[test]
    fn every_era_fixture_returns_its_dense_desk_through_format_2() {
        for (name, v, read) in every_fixture() {
            let back = read_str(&text_of(&read.model)).expect("readable");
            let (dense, ids) = back.model.to_v22().expect("32 rows");
            assert_eq!(ids, identity_ids(32), "{name}");
            assert_eq!(dense, v, "{name} came back as a different dense desk");
        }
    }

    fn read_str(text: &str) -> Result<DeskRead, DeskRefused> {
        read(&serde_json::from_str(text).expect("json"))
    }

    // ── goldens ──────────────────────────────────────────────────────────

    /// `TANGENT_DESK_DUMP=1 cargo test -p tangent-ui --lib
    /// desk_format::tests::print_pictures -- --ignored --nocapture` prints
    /// every fixture's picture, for reading when a fixture is added.
    #[test]
    #[ignore = "prints the pictures the goldens pin"]
    fn print_pictures() {
        for (name, _, read) in every_fixture() {
            println!("=== {name}\n{}", picture(&read.model));
        }
        for (name, read) in v24_lifted() {
            println!("=== lifted {name}\n{}", picture(&read.model));
        }
    }

    fn v24_lifted() -> Vec<(&'static str, DeskRead)> {
        crate::desk_lift::with_v24_lift(|| {
            let (_, s) = SETTINGS.iter().find(|(n, _)| *n == "settings-v20-27.json").expect("fixture");
            let (_, d) = DOCUMENTS.iter().find(|(n, _)| *n == "doc-v6-27.json").expect("fixture");
            vec![("settings-v20-27.json", settings_model(s).1), ("doc-v6-27.json", document_model(d).1)]
        })
    }

    /// **Every era fixture, run through the legacy chain and
    /// `from_v22(identity_ids)`, is this literal desk.** The picture lists
    /// every field of every row that differs from a vacant row, every
    /// special, plugin, bypass, route and key; a fixture opening one row or
    /// one seat off shows up as the wrong number on the wrong line.
    #[test]
    fn every_era_fixture_is_its_literal_desk() {
        let seen: Vec<(&str, String)> =
            every_fixture().into_iter().map(|(n, _, r)| (n, picture(&r.model))).collect();
        assert_eq!(seen.len(), PICTURES.len(), "the tables are out of step");
        for ((name, got), (want_name, want)) in seen.iter().zip(PICTURES) {
            assert_eq!(*name, want_name, "the tables are out of step");
            assert_eq!(got, want, "{name} opened as a different desk");
        }
    }

    /// The 24-channel window under the lift W4 switches on: its buses and
    /// master on the specials, its pads on their rows.
    #[test]
    fn the_24_channel_window_is_its_literal_desk_under_the_lift() {
        for ((name, read), (want_name, want)) in v24_lifted().iter().zip(V24_PICTURES) {
            assert_eq!(*name, want_name);
            assert_eq!(picture(&read.model), want, "{name} lifted to a different desk");
        }
    }


    // ── shape ────────────────────────────────────────────────────────────

    /// **A small desk with buses is read as itself, never by a length.** An
    /// 8-row desk with three bus rows has the sends count of the 8-channel
    /// era's legacy fingerprint in no field at all, and a legacy reader
    /// handed it would lift it; this reader dispatches on `format` alone
    /// and gives back eight rows.
    #[test]
    fn an_eight_row_desk_with_three_buses_is_never_classified_by_length() {
        let mut m = DeskModel::empty();
        for n in 0..8u32 {
            let mut r = TrackRow::vacant(TrackId(n * 3));
            r.kind = Some(if n % 3 == 1 { RowKind::Bus } else { RowKind::Midi });
            r.sends.push(Send { to: Node::Fx, level: 0.5, pre: false });
            m.append(r).expect("append");
        }
        m.specials.drums.sends.push(Send { to: Node::Fx, level: 0.25, pre: false });
        let desk = write(&m);
        assert!(is_format2(&desk));
        assert_eq!(m.counted(&[]), 5);
        let back = read(&desk).expect("readable");
        assert_eq!(back.model, m);
        assert_eq!(back.model.rows.len(), 8);
        assert_eq!(back.model.legacy_states, None);
        assert_eq!(back.model.classes(&[]).iter().filter(|c| **c == crate::desk_model::RowClass::Bus).count(), 3);
        // A legacy-length `sends` beside the format stamp changes nothing.
        let mut planted = desk.clone();
        planted["sends"] = Value::Array(vec![Value::from(0.0); 11]);
        assert_eq!(read(&planted).expect("readable").model, m);
        // And without the stamp it is not this reader's.
        let mut unstamped = desk;
        unstamped.as_object_mut().unwrap().shift_remove("format");
        assert!(!is_format2(&unstamped));
        assert!(matches!(read(&unstamped), Err(DeskRefused { why: Structural::BadShape(_), .. })));
    }

    /// **The documented shape, written exactly** (design Appendix D), and
    /// a model with every field away from its default round-trips whole.
    #[test]
    fn the_writer_spells_appendix_d() {
        let mut m = DeskModel::empty();
        m.legacy_states = Some(DeskEra::V16);
        let mut r = TrackRow::vacant(TrackId(0));
        r.kind = Some(RowKind::Midi);
        r.added = true;
        r.icon = 3;
        r.name = "Piano".into();
        r.gain = 0.8;
        r.input = "midi:all".into();
        r.arm = true;
        r.theory = true;
        r.sends.push(Send { to: Node::Fx, level: 1.0, pre: false });
        m.append(r).expect("append");
        m.next_id = 41;
        m.plugins.insert(SeatBay::new(Node::Track(TrackId(0)), Rack::Chain(0), 0), "@tangent-dx7".into());
        m.plugins.insert(SeatBay::new(Node::Master, Rack::Chain(1), 2), "/Library/X.vst3".into());
        m.bypass.insert(SeatBay::new(Node::Track(TrackId(0)), Rack::Chain(1), 0));
        m.routes.push(RouteEdge {
            from: Node::Track(TrackId(0)),
            to: RouteTo::Hardware { first: 4, stereo: true },
            gain: 1.0,
        });
        m.keys.push(SideKey {
            seat: SeatBay::new(Node::Track(TrackId(0)), Rack::Chain(0), 1),
            from: Node::Track(TrackId(0)),
        });
        let got = text_of(&m);
        let params = KeysSel::default().params.iter().map(|p| serde_json::to_string(p).unwrap()).collect::<Vec<_>>().join(",");
        let special = r#"{"name":"","color":0,"sends":[],"mute":false,"solo":false,"phase":false}"#;
        let want = format!(
            concat!(
                r#"{{"format":2,"next_id":41,"legacy_states":"v16","rows":[{{"id":0,"kind":"midi","added":true,"#,
                r#""icon":3,"name":"Piano","color":0,"gain":0.8,"pan":0.0,"input":"midi:all","#,
                r#""midi":{{"devices":[],"channel":null}},"mute":false,"solo":false,"phase":false,"arm":true,"#,
                r#""theory":true,"folder":false,"in_folder":false,"shut":false,"#,
                r#""sends":[{{"to":"fx","level":1.0,"pre":false}}],"dx7":{{"cart":"","patch":0}},"#,
                r#""keys":{{"patch":0,"params":[{params}]}},"frozen":null}}],"#,
                r#""specials":{{"drums":{special},"click":{special},"fx":{special},"master":{special}}},"#,
                r#""plugins":{{"t0-c0-b0":"@tangent-dx7","master-c1-b2":"/Library/X.vst3"}},"#,
                r#""bypass":["t0-c1-b0"],"#,
                r#""routes":[{{"from":"t0","to":"hw","first":4,"stereo":true,"gain":1.0}}],"#,
                r#""keys":[{{"seat":"t0-c0-b1","from":"t0"}}]}}"#
            ),
            params = params,
            special = special
        );
        assert_eq!(got, want);
        assert_eq!(read_str(&got).expect("readable").model, m);
    }

    /// Every field away from its default, specials too, and back.
    #[test]
    fn a_desk_using_every_field_round_trips() {
        let mut m = DeskModel::empty();
        for n in [5u32, 2, 900] {
            let mut r = TrackRow::vacant(TrackId(n));
            r.kind = Some(RowKind::Audio);
            r.icon = 250;
            r.name = "a \"quoted\" name\u{e9}".into();
            r.color = -7;
            r.gain = 1.0 / 3.0;
            r.pan = -1.0;
            r.midi = MidiInputSel { devices: vec!["A".into(), "B".into()], channel: Some(15) };
            r.mute = true;
            r.phase = true;
            r.in_folder = true;
            r.shut = true;
            r.dx7 = Dx7 { cart: "/c.syx".into(), patch: 31 };
            r.keys.patch = 7;
            r.keys.params[63] = 0.999;
            r.frozen = Some(Frozen { path: "media/f.wav".into(), len_s: 12.5 });
            // Pushed, not appended: three members under no folder is a run
            // an edit refuses to make, and exactly the stale flags an older
            // file can hold, which the format must carry byte for byte.
            m.rows.push(r);
        }
        // Sent after every row is in: an edit refuses to leave a send to a
        // row that is not there yet.
        for r in &mut m.rows {
            r.sends.push(Send { to: Node::Track(TrackId(2)), level: 0.1, pre: true });
        }
        m.specials.master = Special { name: "M".into(), color: 3, sends: vec![], mute: true, solo: true, phase: true };
        m.specials.fx.sends.push(Send { to: Node::Track(TrackId(900)), level: 0.7, pre: false });
        m.routes.push(RouteEdge { from: Node::Track(TrackId(900)), to: RouteTo::Node(Node::Fx), gain: 0.5 });
        m.next_id = 5000;
        let back = read(&write(&m)).expect("readable");
        assert!(back.dropped.is_empty());
        assert_eq!(back.model, m);
    }

    /// **The writer never writes a desk its reader refuses.** `serde_json`
    /// spells a non-finite float `null`, which the reader takes as the
    /// wrong shape; a NaN that reached the model (a bad plugin's gain, a
    /// division by zero) must cost that number, not the whole desk.
    #[test]
    fn a_non_finite_number_is_written_as_one_the_reader_takes() {
        let mut m = DeskModel::empty();
        let mut r = TrackRow::vacant(TrackId(0));
        r.gain = f64::NAN;
        r.pan = f64::INFINITY;
        r.sends.push(Send { to: Node::Fx, level: f64::NEG_INFINITY, pre: false });
        r.keys.params[0] = f64::NAN;
        m.append(r).expect("append");
        let back = read(&write(&m)).expect("a desk this writer wrote is a desk this reader reads");
        let row = &back.model.rows[0];
        assert_eq!((row.gain, row.pan, row.sends[0].level, row.keys.params[0]), (0.0, 0.0, 0.0, 0.0));
    }

    // ── error policy ─────────────────────────────────────────────────────

    /// One clean row, as JSON.
    fn row(id: u32) -> Value {
        serde_json::json!({ "id": id })
    }

    fn desk(rows: Vec<Value>) -> Value {
        serde_json::json!({ "format": 2, "next_id": 0, "rows": rows })
    }

    /// **One dangling plugin, bypass, route, key and send are exactly five
    /// dropped entries**, listed, and everything else reads.
    #[test]
    fn five_dangling_entries_are_five_dropped_and_listed() {
        let mut d = desk(vec![row(0), row(1)]);
        d["rows"][1]["sends"] = serde_json::json!([{ "to": "t9", "level": 0.5 }, { "to": "fx", "level": 0.25 }]);
        d["plugins"] = serde_json::json!({ "t0-c0-b0": "/kept.vst3", "t7-c0-b1": "/gone.vst3" });
        d["bypass"] = serde_json::json!(["t0-c0-b0", "t8-c1-b2"]);
        d["routes"] = serde_json::json!([
            { "from": "t0", "to": "t1", "gain": 1.0 },
            { "from": "t6", "to": "t1", "gain": 1.0 },
        ]);
        d["keys"] = serde_json::json!([{ "seat": "t1-c0-b0", "from": "t0" }, { "seat": "t1-c0-b1", "from": "t5" }]);
        let got = read(&d).expect("a dangling entry never refuses the desk");
        let listed: Vec<String> = got.dropped.iter().map(ToString::to_string).collect();
        assert_eq!(listed, ["plugin t7-c0-b1", "bypass t8-c1-b2", "route t6>t1", "key t1-c0-b1<t5", "send t1>t9"]);
        let m = got.model;
        assert_eq!((m.plugins.len(), m.bypass.len(), m.routes.len(), m.keys.len()), (1, 1, 1, 1));
        assert_eq!(m.rows[1].sends, vec![Send { to: Node::Fx, level: 0.25, pre: false }]);
        assert!(m.check().is_empty());
    }

    /// **Local errors cost their entry too**: a pedal on a special, a pad
    /// on the master, a fourth bay, a chain at `MAX_CHAIN`, an unspellable
    /// key, a plugin with no path, a route or key or send that is not one.
    #[test]
    fn malformed_entries_are_dropped_not_fatal() {
        let mut d = desk(vec![row(0)]);
        let far = format!("t0-c{MAX_CHAIN}-b0");
        d["plugins"] = serde_json::json!({
            "drums-p1-b0": "/a", "master-n3-b0": "/b", "t0-c0-b3": "/c",
            "T0-c0-b0": "/e", "t00-c0-b0": "/f", "t0-c1-b0": "", "t0-c0-b2": 7, "t0-c0-b0": "/kept",
        });
        d["plugins"][far.as_str()] = Value::from("/d");
        d["bypass"] = serde_json::json!([3, "t0-c0-b9"]);
        d["routes"] = serde_json::json!([{ "from": "t0" }, "t0>t0", { "from": "t0", "to": "hw" }]);
        d["keys"] = serde_json::json!([{ "seat": "t0-c0", "from": "t0" }]);
        d["rows"][0]["sends"] = serde_json::json!([{ "to": "nowhere" }, { "to": "fx", "level": "loud" }]);
        let got = read(&d).expect("readable");
        let mut what: Vec<&str> = got.dropped.iter().map(|x| x.what).collect();
        what.sort_unstable();
        assert_eq!(
            what,
            [
                "bypass", "bypass", "key", "plugin", "plugin", "plugin", "plugin", "plugin", "plugin", "plugin",
                "plugin", "route", "route", "route", "send", "send"
            ]
        );
        assert!(got.dropped.iter().any(|x| x.key == far), "the chain past MAX_CHAIN was not listed by name");
        assert_eq!(got.model.plugins.len(), 1);
    }

    /// **A `next_id` behind the ids is repaired**, silently; one ahead of
    /// them is kept.
    #[test]
    fn a_next_id_behind_the_ids_is_repaired() {
        let mut d = desk(vec![row(3), row(40)]);
        let got = read(&d).expect("readable");
        assert!(got.dropped.is_empty());
        assert_eq!(got.model.next_id, 41);
        d["next_id"] = Value::from(500);
        assert_eq!(read(&d).expect("readable").model.next_id, 500);
        d.as_object_mut().unwrap().shift_remove("next_id");
        assert_eq!(read(&d).expect("readable").model.next_id, 41);
    }

    /// **Structural problems refuse the desk and hand its JSON back**:
    /// 65,537 rows, two rows with one id, and every malformed shape.
    #[test]
    fn structural_problems_refuse_the_desk_with_its_json() {
        let refused = |d: &Value| -> Structural {
            let e = read(d).expect_err("refused");
            assert_eq!(&e.raw, d, "the refusal did not carry the desk");
            e.why
        };
        let many = desk((0..=MAX_ROWS as u32).map(row).collect());
        assert_eq!(refused(&many), Structural::TooManyRows(MAX_ROWS + 1));
        let exactly = desk((0..MAX_ROWS as u32).map(row).collect());
        assert_eq!(read(&exactly).expect("MAX_ROWS rows read").model.rows.len(), MAX_ROWS);
        assert_eq!(refused(&desk(vec![row(4), row(5), row(4)])), Structural::DuplicateId(4));
        let bad: Vec<Value> = vec![
            serde_json::json!([]),
            serde_json::json!("desk"),
            serde_json::json!({ "format": 2 }),
            serde_json::json!({ "format": 2, "rows": {} }),
            serde_json::json!({ "format": 3, "rows": [] }),
            serde_json::json!({ "format": "2", "rows": [] }),
            desk(vec![serde_json::json!({ "name": "no id" })]),
            desk(vec![serde_json::json!({ "id": -1 })]),
            desk(vec![serde_json::json!({ "id": 4_294_967_296u64 })]),
            desk(vec![serde_json::json!(7)]),
            desk(vec![serde_json::json!({ "id": 0, "kind": "guitar" })]),
            desk(vec![serde_json::json!({ "id": 0, "mute": "yes" })]),
            desk(vec![serde_json::json!({ "id": 0, "icon": 256 })]),
            desk(vec![serde_json::json!({ "id": 0, "gain": "loud" })]),
            desk(vec![serde_json::json!({ "id": 0, "midi": { "channel": 16 } })]),
            desk(vec![serde_json::json!({ "id": 0, "keys": { "params": ["x"] } })]),
            desk(vec![serde_json::json!({ "id": 0, "frozen": "yes" })]),
            desk(vec![serde_json::json!({ "id": 0, "sends": {} })]),
            serde_json::json!({ "format": 2, "rows": [], "plugins": [] }),
            serde_json::json!({ "format": 2, "rows": [], "bypass": {} }),
            serde_json::json!({ "format": 2, "rows": [], "routes": 1 }),
            serde_json::json!({ "format": 2, "rows": [], "keys": "k" }),
            serde_json::json!({ "format": 2, "rows": [], "specials": [] }),
            serde_json::json!({ "format": 2, "rows": [], "specials": { "fx": 1 } }),
            serde_json::json!({ "format": 2, "rows": [], "next_id": -3 }),
            serde_json::json!({ "format": 2, "rows": [], "legacy_states": "v40" }),
        ];
        for d in &bad {
            assert!(matches!(refused(d), Structural::BadShape(_)), "{d} was not refused as a bad shape");
        }
    }

    /// **Huge numbers allocate nothing**: ids, chains, jacks and the
    /// counter at the top of their types read as numbers, never as sizes.
    #[test]
    fn huge_numeric_keys_allocate_nothing() {
        let mut d = desk(vec![row(u32::MAX), row(0)]);
        d["next_id"] = Value::from(u64::MAX);
        d["plugins"] = serde_json::json!({
            "t4294967295-c0-b0": "/top.vst3",
            "t4294967294-c0-b0": "/ghost.vst3",
            "t0-c65535-b2": "/far.vst3",
            "t0-n127-b2": "/pad.vst3",
        });
        d["routes"] = serde_json::json!([
            { "from": "t4294967295", "to": "hw", "first": u64::MAX, "stereo": false },
            { "from": "t0", "to": "hw", "first": 9_007_199_254_740_993u64 },
        ]);
        d["rows"][1]["dx7"] = serde_json::json!({ "patch": u64::MAX });
        let got = read(&d).expect("readable");
        let listed: Vec<String> = got.dropped.iter().map(ToString::to_string).collect();
        assert_eq!(listed, ["plugin t0-c65535-b2", "plugin t4294967294-c0-b0"]);
        let m = got.model;
        assert_eq!((m.rows.len(), m.plugins.len(), m.routes.len()), (2, 2, 2));
        assert_eq!(m.next_id, u64::MAX);
        assert_eq!(m.rows[1].keys.params.len(), KEYS_SPAN);
        assert_eq!(read(&write(&m)).expect("rewritten").model, m);
        let mut m = m;
        assert_eq!(m.mint(), None, "the u32 space is spent");
    }

    // ── the pinned pictures ──────────────────────────────────────────────

    /// Each era fixture through the legacy chain and `from_v22`, pictured
    /// (`desk_model::picture`). Pinned as written: the names and colours say
    /// which legacy strip each row or special came from (`s<i>`, `100 + i`),
    /// the bays which plugin.
    const PICTURES: [(&str, &str); 17] = [
        ("settings-v12.json", r#"rows 32 next_id 32 legacy -
t0 midi arm name="s0" color=100 input="midi:all" send fx 0.01
t1 theory name="s1" color=101 send fx 0.02
t2 mute name="s2" color=102 send fx 0.03
t3 audio name="s3" color=103 input="0" send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 solo name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
click mute name="s9" send fx 0.1
fx solo name="s10" color=110 send fx 0.11
master name="s11"
plugins t0-c0-b0=@tangent-dx7 t1-c0-b0=/x/s1.vst3 t2-c0-b0=/x/s2.vst3 t3-c0-b0=/x/s3.vst3 t4-c0-b0=/x/s4.vst3 t5-c0-b0=/x/s5.vst3 t6-c0-b0=/x/s6.vst3 t7-c0-b0=/x/s7.vst3 click-c0-b0=/x/s9.vst3 fx-c0-b0=/x/s10.vst3 master-c0-b0=/x/s11.vst3 master-c0-b2=/x/s11b2.vst3
bypass 
routes 
keys "#),
        ("settings-v17-pads16.json", r#"rows 32 next_id 32 legacy -
t0 midi icon=1 name="s0" color=100 send fx 0.01
t1 midi theory name="s1" color=101 send fx 0.02
t2 name="s2" color=102 send fx 0.03
t3 solo name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 midi name="s7" color=107 send fx 0.08
drums name="s8" color=108 send fx 0.09
click mute name="s9" color=109 send fx 0.1
fx name="s10" color=110 send fx 0.11
master name="s11" color=111
plugins t0-c0-b0=@tangent-drums t0-n36-b0=/x/pad0.vst3 t0-n38-b1=/x/pad2.vst3 t0-n51-b2=/x/pad15.vst3 t1-c0-b0=/x/s1.vst3 t2-c0-b0=/x/s2.vst3 t3-c0-b0=/x/s3.vst3 t4-c0-b0=/x/s4.vst3 t5-c0-b0=/x/s5.vst3 t6-c0-b0=/x/s6.vst3 t7-c0-b0=/x/s7.vst3 drums-c0-b0=/x/s8.vst3 click-c0-b0=/x/s9.vst3 fx-c0-b0=/x/s10.vst3 master-c0-b0=/x/s11.vst3
bypass 
routes 
keys "#),
        ("settings-v18.json", r#"rows 32 next_id 32 legacy -
t0 midi name="s0" color=100 send fx 0.01
t1 midi theory name="s1" color=101 send fx 0.02
t2 mute name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 midi icon=1 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
drums mute name="s8" color=108 send fx 0.09
click name="s9" color=109 send fx 0.1
fx solo name="s10" color=110 send fx 0.11
master name="s11" color=111
plugins t0-c0-b0=/x/s0.vst3 t1-c0-b0=/x/s1.vst3 t2-c0-b0=/x/s2.vst3 t3-c0-b0=/x/s3.vst3 t4-c0-b0=@tangent-drums t4-n0-b0=/x/pad0.vst3 t4-n40-b1=/x/pad40.vst3 t4-n127-b2=/x/pad127.vst3 t5-c0-b0=/x/s5.vst3 t6-c0-b0=/x/s6.vst3 t7-c0-b0=/x/s7.vst3 drums-c0-b0=/x/s8.vst3 click-c0-b0=/x/s9.vst3 fx-c0-b0=/x/s10.vst3 master-c0-b0=/x/s11.vst3
bypass 
routes 
keys "#),
        ("settings-v19.json", r#"rows 32 next_id 32 legacy -
t0 midi icon=1 name="s0" color=100 send fx 0.01
t1 midi arm theory name="s1" color=101 send fx 0.02
t2 audio mute name="s2" color=102 send fx 0.03
t3 midi name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 solo name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
drums phase name="s16" color=116 send fx 0.17
click mute name="s17" send fx 0.18
fx solo name="s18" color=118 send fx 0.19
master phase name="s19"
plugins t0-c0-b0=@tangent-drums t0-n0-b0=/x/pad0.vst3 t0-n40-b1=/x/pad40.vst3 t0-n127-b2=/x/pad127.vst3 t1-c0-b0=@tangent-dx7 t2-c0-b0=/x/s2.vst3 t3-c0-b0=/x/s3.vst3 t3-n5-b0=/x/c3pad5.vst3 t4-c0-b0=/x/s4.vst3 t5-c0-b0=/x/s5.vst3 t6-c0-b0=/x/s6.vst3 t7-c0-b0=/x/s7.vst3 t8-c0-b0=/x/s8.vst3 t9-c0-b0=/x/s9.vst3 t10-c0-b0=/x/s10.vst3 t11-c0-b0=/x/s11.vst3 t12-c0-b0=/x/s12.vst3 t13-c0-b0=/x/s13.vst3 t14-c0-b0=/x/s14.vst3 t15-c0-b0=/x/s15.vst3 drums-c0-b0=/x/s16.vst3 click-c0-b0=/x/s17.vst3 fx-c0-b0=/x/s18.vst3 master-c0-b0=/x/s19.vst3 master-c0-b2=/x/s19b2.vst3 master-c1-b1=/x/master-extra.vst3
bypass 
routes 
keys "#),
        ("settings-v20-27.json", r#"rows 32 next_id 32 legacy -
t0 midi icon=1 name="s0" color=100 send fx 0.01
t1 midi arm theory name="s1" color=101 send fx 0.02
t2 mute name="s2" color=102 send fx 0.03
t3 phase name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 solo name="s21" color=121 send fx 0.22
t22 audio name="s22" color=122 send fx 0.23
t23 midi name="s23" color=123 send fx 0.24
t24 phase name="s24" color=124 send fx 0.25
t25 mute name="s25" color=125 send fx 0.26
t26 solo name="s26" color=126 send fx 0.27
t27 phase name="s27" color=127
plugins t0-c0-b0=@tangent-drums t0-n32-b1=/x/pad40.vst3 t0-n119-b2=/x/pad127.vst3 t0-n120-b1=/x/master-extra.vst3 t1-c0-b0=@tangent-dx7 t2-c0-b0=/x/s2.vst3 t2-n125-b0=/x/c3pad5.vst3 t3-c0-b0=/x/s3.vst3 t4-c0-b0=/x/s4.vst3 t5-c0-b0=/x/s5.vst3 t6-c0-b0=/x/s6.vst3 t7-c0-b0=/x/s7.vst3 t8-c0-b0=/x/s8.vst3 t9-c0-b0=/x/s9.vst3 t10-c0-b0=/x/s10.vst3 t11-c0-b0=/x/s11.vst3 t12-c0-b0=/x/s12.vst3 t13-c0-b0=/x/s13.vst3 t14-c0-b0=/x/s14.vst3 t15-c0-b0=/x/s15.vst3 t16-c0-b0=/x/s16.vst3 t17-c0-b0=/x/s17.vst3 t18-c0-b0=/x/s18.vst3 t19-c0-b0=/x/s19.vst3 t20-c0-b0=/x/s20.vst3 t21-c0-b0=/x/s21.vst3 t22-c0-b0=/x/s22.vst3 t23-c0-b0=/x/s23.vst3 t23-n119-b2=/x/c23pad127.vst3 t24-c0-b0=/x/s24.vst3 t25-c0-b0=/x/s25.vst3 t26-c0-b0=/x/s26.vst3 t27-c0-b0=/x/s27.vst3 t27-c0-b2=/x/s27b2.vst3 t28-c0-b0=/x/pad0.vst3
bypass 
routes 
keys "#),
        ("settings-v20.json", r#"rows 32 next_id 32 legacy -
t0 midi icon=1 name="s0" color=100 send fx 0.01
t1 midi arm theory name="s1" color=101 send fx 0.02
t2 mute name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 name="s21" color=121 send fx 0.22
t22 name="s22" color=122 send fx 0.23
t23 name="s23" color=123 send fx 0.24
t24 name="s24" color=124 send fx 0.25
t25 name="s25" color=125 send fx 0.26
t26 name="s26" color=126 send fx 0.27
t27 name="s27" color=127 send fx 0.28
t28 name="s28" color=128 send fx 0.29
t29 name="s29" color=129 send fx 0.3
t30 name="s30" color=130 send fx 0.31
t31 audio name="s31" color=131 send fx 0.32
drums phase name="s32" color=132 send fx 0.33
click mute name="s33" color=133 send fx 0.34
fx solo name="s34" color=134 send fx 0.35
master phase name="s35" color=135
plugins t0-c0-b0=@tangent-drums t0-n40-b1=/x/pad40.vst3 t1-c0-b0=@tangent-dx7 t2-c0-b0=/x/s2.vst3 t3-c0-b0=/x/s3.vst3 t3-p6-b1=/x/c3pedal6.vst3 t4-c0-b0=/x/s4.vst3 t5-c0-b0=/x/s5.vst3 t5-c1-b2=/x/folder5-extra.vst3 t6-c0-b0=/x/s6.vst3 t7-c0-b0=/x/s7.vst3 t8-c0-b0=/x/s8.vst3 t9-c0-b0=/x/s9.vst3 t10-c0-b0=/x/s10.vst3 t11-c0-b0=/x/s11.vst3 t12-c0-b0=/x/s12.vst3 t13-c0-b0=/x/s13.vst3 t14-c0-b0=/x/s14.vst3 t15-c0-b0=/x/s15.vst3 t16-c0-b0=/x/s16.vst3 t17-c0-b0=/x/s17.vst3 t18-c0-b0=/x/s18.vst3 t19-c0-b0=/x/s19.vst3 t20-c0-b0=/x/s20.vst3 t21-c0-b0=/x/s21.vst3 t22-c0-b0=/x/s22.vst3 t23-c0-b0=/x/s23.vst3 t24-c0-b0=/x/s24.vst3 t25-c0-b0=/x/s25.vst3 t26-c0-b0=/x/s26.vst3 t27-c0-b0=/x/s27.vst3 t28-c0-b0=/x/s28.vst3 t29-c0-b0=/x/s29.vst3 t30-c0-b0=/x/s30.vst3 t31-c0-b0=/x/s31.vst3 t31-n127-b0=/x/c31pad127.vst3 drums-c0-b0=/x/s32.vst3 click-c0-b0=/x/s33.vst3 fx-c0-b0=/x/s34.vst3 master-c0-b0=/x/s35.vst3 master-c1-b1=/x/master-extra.vst3
bypass 
routes 
keys "#),
        ("settings-v21.json", r#"rows 32 next_id 32 legacy -
t0 midi icon=1 name="s0" color=100 send fx 0.01
t1 midi arm theory name="s1" color=101 send fx 0.02
t2 name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 name="s21" color=121 send fx 0.22
t22 name="s22" color=122 send fx 0.23
t23 name="s23" color=123 send fx 0.24
t24 name="s24" color=124 send fx 0.25
t25 name="s25" color=125 send fx 0.26
t26 name="s26" color=126 send fx 0.27
t27 name="s27" color=127 send fx 0.28
t28 name="s28" color=128 send fx 0.29
t29 name="s29" color=129 send fx 0.3
t30 name="s30" color=130 send fx 0.31
t31 name="s31" color=131 send fx 0.32
drums name="s32" color=132 send fx 0.33
click mute name="s33" color=133 send fx 0.34
fx name="s34" color=134 send fx 0.35
master phase name="s35" color=135
plugins t0-c0-b0=@tangent-drums t0-n40-b1=/x/pad40.vst3 t1-c0-b0=@tangent-dx7 t5-c1-b2=/x/folder5-extra.vst3 drums-c0-b0=/x/drumbus.vst3 master-c0-b2=/x/s35b2.vst3
bypass 
routes 
keys "#),
        ("settings-v22.json", r#"rows 32 next_id 32 legacy -
t0 midi icon=1 name="s0" color=100 input="midi:all" send fx 0.01
t1 midi arm theory name="s1" color=101 input="midi:all" send fx 0.02
t2 midi mute name="s2" color=102 input="midi:Keystation 88" midi=["Keystation 88"]/Some(3) send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 name="s21" color=121 send fx 0.22
t22 name="s22" color=122 send fx 0.23
t23 name="s23" color=123 send fx 0.24
t24 name="s24" color=124 send fx 0.25
t25 name="s25" color=125 send fx 0.26
t26 name="s26" color=126 send fx 0.27
t27 name="s27" color=127 send fx 0.28
t28 name="s28" color=128 send fx 0.29
t29 name="s29" color=129 send fx 0.3
t30 name="s30" color=130 send fx 0.31
t31 name="s31" color=131 send fx 0.32
drums solo name="s32" color=132 send fx 0.33
click phase name="s33" color=133 send fx 0.34
fx name="s34" color=134 send fx 0.35
master name="s35" color=135
plugins t0-c0-b0=@tangent-drums t1-c0-b0=@tangent-dx7 t5-c1-b2=/x/folder5-extra.vst3 drums-c0-b0=/x/drumbus.vst3 master-c0-b2=/x/s35b2.vst3
bypass 
routes 
keys "#),
        ("daw-3x.json", r#"rows 32 next_id 32 legacy -
t0 midi name="Piano" color=3 gain=0.8 input="midi:all" send fx 1
t1 midi theory name="Lead" color=7 gain=0.6 input="midi:all" send fx 0.4
t2 audio mute name="Mic" color=16 gain=0.9
t3 audio name="Amp" color=21 gain=0.7 send fx 0.2
t4 send fx 1
t5 send fx 1
t6 send fx 1
t7 midi icon=1 input="midi:all"
fx send fx 0.5
master name="Sum"
plugins t0-c0-b0=/x/Grand 8.vst3 t0-c0-b1=/x/Pro-R 2.vst3 t1-c0-b0=/x/Diva.vst3 t1-c0-b1=/x/Pro-C 3.vst3 t7-c0-b0=@tangent-drums master-c0-b0=/x/Pro-L 2.vst3
bypass 
routes 
keys "#),
        ("daw-4x.json", r#"rows 32 next_id 32 legacy -
t0 midi arm name="s0" color=100 send fx 0.01
t1 audio solo theory name="s1" color=101 input="0" send fx 0.02
t2 name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 midi icon=1 name="s7" color=107 input="midi:all" send fx 0.08
drums mute name="s8" color=108 send fx 0.09
click name="s9" color=109 send fx 0.1
fx name="s10" color=110 send fx 0.11
master name="s11" color=111
plugins t0-c0-b0=@tangent-dx7 t7-c0-b0=@tangent-drums t7-n39-b0=/x/pad3.vst3 drums-c0-b1=/x/drumbus.vst3 master-c0-b0=/x/Pro-L 2.vst3
bypass 
routes 
keys "#),
        ("daw-5x.json", r#"rows 32 next_id 32 legacy -
t0 midi icon=1 name="s0" color=100 send fx 0.01
t1 midi arm theory name="s1" color=101 send fx 0.02
t2 midi name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 solo name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
drums phase name="s16" color=116 send fx 0.17
click mute name="s17" color=117 send fx 0.18
fx name="s18" color=118 send fx 0.19
master name="s19" color=119
plugins t0-c0-b0=@tangent-drums t0-n36-b1=/x/kick.vst3 t1-c0-b0=@tangent-dx7 t2-n2-b2=/x/c2pad2.vst3 drums-c0-b0=/x/drumbus.vst3 master-c0-b0=/x/Pro-L 2.vst3
bypass 
routes 
keys "#),
        ("doc-v4-11.json", r#"rows 32 next_id 32 legacy v8-pads16
t0 midi added mute name="s0" color=100 input="midi:all" send fx 0.01
t1 midi added arm name="s1" color=101 send fx 0.02
t2 added name="s2" color=102 send fx 0.03
t3 added name="s3" color=103 send fx 0.04
t4 added name="s4" color=104 send fx 0.05
t5 added name="s5" color=105 send fx 0.06
t6 added name="s6" color=106 send fx 0.07
t7 added name="s7" color=107 send fx 0.08
drums name="s8" color=108 send fx 0.09
click mute name="s9" color=109 send fx 0.1
fx solo name="s10" color=110 send fx 0.11
master name="s11" color=111
plugins t0-c0-b0=@tangent-drums t0-n38-b0=/x/pad2.vst3 t0-n51-b1=/x/pad15.vst3 t1-c0-b0=@tangent-dx7 drums-c0-b0=/x/drumbus.vst3 click-c0-b1=/x/click.vst3 master-c0-b2=/x/master.vst3
bypass 
routes 
keys "#),
        ("doc-v5-18.json", r#"rows 32 next_id 32 legacy v16-no-drum-bus
t0 midi added name="s0" color=100 input="midi:all" send fx 0.01
t1 midi added arm name="s1" color=101 send fx 0.02
t2 added name="s2" color=102 send fx 0.03
t3 added solo name="s3" color=103 send fx 0.04
t4 added name="s4" color=104 send fx 0.05
t5 added name="s5" color=105 send fx 0.06
t6 added name="s6" color=106 send fx 0.07
t7 added name="s7" color=107 send fx 0.08
t8 added name="s8" color=108 send fx 0.09
t9 added name="s9" color=109 send fx 0.1
t10 added name="s10" color=110 send fx 0.11
t11 added name="s11" color=111 send fx 0.12
t12 added name="s12" color=112 send fx 0.13
t13 added name="s13" color=113 send fx 0.14
t14 added name="s14" color=114 send fx 0.15
t15 added name="s15" color=115 send fx 0.16
click mute name="s16" color=116 send fx 0.17
fx solo name="s17" color=117 send fx 0.18
master name="s18" color=118
plugins t0-c0-b0=@tangent-drums t1-c0-b0=@tangent-dx7 t15-c0-b0=/x/c15.vst3 click-c0-b0=/x/click.vst3 fx-c0-b1=/x/fx.vst3 master-c0-b2=/x/master.vst3
bypass 
routes 
keys "#),
        ("doc-v5-19.json", r#"rows 32 next_id 32 legacy v16
t0 midi added name="s0" color=100 input="midi:all" send fx 0.01
t1 midi added arm name="s1" color=101 send fx 0.02
t2 added solo name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
drums solo name="s16" color=116 send fx 0.17
click mute name="s17" color=117 send fx 0.18
fx name="s18" color=118 send fx 0.19
master name="s19" color=119
plugins t0-c0-b0=@tangent-drums t0-n40-b1=/x/pad40.vst3 t1-c0-b0=@tangent-dx7 t3-n5-b2=/x/c3pad5.vst3 drums-c0-b0=/x/drumbus.vst3 click-c0-b0=/x/click.vst3 master-c0-b2=/x/master.vst3 master-c1-b0=/x/master-extra.vst3
bypass 
routes 
keys "#),
        ("doc-v6-27.json", r#"rows 32 next_id 32 legacy v32
t0 midi added name="s0" color=100 input="midi:all" send fx 0.01
t1 midi added arm name="s1" color=101 send fx 0.02
t2 name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 name="s21" color=121 send fx 0.22
t22 name="s22" color=122 send fx 0.23
t23 added mute name="s23" color=123 send fx 0.24
t24 name="s24" color=124 send fx 0.25
t25 mute name="s25" color=125 send fx 0.26
t26 solo name="s26" color=126 send fx 0.27
t27 name="s27" color=127
plugins t0-c0-b0=@tangent-drums t0-n119-b0=/x/pad127.vst3 t0-n120-b1=/x/master-extra.vst3 t1-c0-b0=@tangent-dx7 t2-n125-b2=/x/c3pad5.vst3 t23-c0-b0=/x/c23.vst3 t24-c0-b0=/x/drumbus.vst3 t26-c0-b1=/x/fx.vst3 t27-c0-b2=/x/master.vst3
bypass 
routes 
keys "#),
        ("doc-v6-35.json", r#"rows 32 next_id 32 legacy v32
t0 midi added name="s0" color=100 input="midi:all" send fx 0.01
t1 midi added name="s1" color=101 send fx 0.02
t2 name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 name="s21" color=121 send fx 0.22
t22 name="s22" color=122 send fx 0.23
t23 name="s23" color=123 send fx 0.24
t24 name="s24" color=124 send fx 0.25
t25 name="s25" color=125 send fx 0.26
t26 name="s26" color=126 send fx 0.27
t27 name="s27" color=127 send fx 0.28
t28 name="s28" color=128 send fx 0.29
t29 name="s29" color=129 send fx 0.3
t30 name="s30" color=130 send fx 0.31
t31 added solo arm name="s31" color=131 send fx 0.32
drums name="s32" color=132 send fx 0.33
click mute name="s33" color=133 send fx 0.34
fx name="s34" color=134 send fx 0.35
master name="s35" color=135
plugins t0-c0-b0=@tangent-drums t0-n40-b1=/x/pad40.vst3 t1-c0-b0=@tangent-dx7 t31-c0-b0=/x/c31.vst3 drums-c0-b0=/x/drumbus.vst3 master-c0-b2=/x/master.vst3
bypass 
routes 
keys "#),
        ("doc-v7.json", r#"rows 32 next_id 32 legacy v32
t0 midi added name="s0" color=100 input="midi:all" send fx 0.01
t1 midi added arm name="s1" color=101 send fx 0.02
t2 mute name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 name="s21" color=121 send fx 0.22
t22 name="s22" color=122 send fx 0.23
t23 name="s23" color=123 send fx 0.24
t24 name="s24" color=124 send fx 0.25
t25 name="s25" color=125 send fx 0.26
t26 name="s26" color=126 send fx 0.27
t27 name="s27" color=127 send fx 0.28
t28 name="s28" color=128 send fx 0.29
t29 name="s29" color=129 send fx 0.3
t30 name="s30" color=130 send fx 0.31
t31 name="s31" color=131 send fx 0.32
drums phase name="s32" color=132 send fx 0.33
click name="s33" color=133 send fx 0.34
fx name="s34" color=134 send fx 0.35
master phase name="s35" color=135
plugins t0-c0-b0=@tangent-drums t0-n40-b1=/x/pad40.vst3 t1-c0-b0=@tangent-dx7 t5-c1-b2=/x/folder5-extra.vst3 drums-c0-b0=/x/drumbus.vst3 master-c0-b2=/x/master.vst3
bypass 
routes 
keys "#),
    ];

    /// The two 24-channel-window fixtures under the V24 lift.
    const V24_PICTURES: [(&str, &str); 2] = [
        ("settings-v20-27.json", r#"rows 32 next_id 32 legacy -
t0 midi icon=1 name="s0" color=100 send fx 0.01
t1 midi arm theory name="s1" color=101 send fx 0.02
t2 mute name="s2" color=102 send fx 0.03
t3 phase name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 solo name="s21" color=121 send fx 0.22
t22 audio name="s22" color=122 send fx 0.23
t23 midi name="s23" color=123 send fx 0.24
drums phase name="s24" color=124 send fx 0.25
click mute name="s25" color=125 send fx 0.26
fx solo name="s26" color=126 send fx 0.27
master phase name="s27" color=127
plugins t0-c0-b0=@tangent-drums t0-n0-b0=/x/pad0.vst3 t0-n40-b1=/x/pad40.vst3 t0-n127-b2=/x/pad127.vst3 t1-c0-b0=@tangent-dx7 t2-c0-b0=/x/s2.vst3 t3-c0-b0=/x/s3.vst3 t3-n5-b0=/x/c3pad5.vst3 t4-c0-b0=/x/s4.vst3 t5-c0-b0=/x/s5.vst3 t6-c0-b0=/x/s6.vst3 t7-c0-b0=/x/s7.vst3 t8-c0-b0=/x/s8.vst3 t9-c0-b0=/x/s9.vst3 t10-c0-b0=/x/s10.vst3 t11-c0-b0=/x/s11.vst3 t12-c0-b0=/x/s12.vst3 t13-c0-b0=/x/s13.vst3 t14-c0-b0=/x/s14.vst3 t15-c0-b0=/x/s15.vst3 t16-c0-b0=/x/s16.vst3 t17-c0-b0=/x/s17.vst3 t18-c0-b0=/x/s18.vst3 t19-c0-b0=/x/s19.vst3 t20-c0-b0=/x/s20.vst3 t21-c0-b0=/x/s21.vst3 t22-c0-b0=/x/s22.vst3 t23-c0-b0=/x/s23.vst3 t23-n127-b2=/x/c23pad127.vst3 drums-c0-b0=/x/s24.vst3 click-c0-b0=/x/s25.vst3 fx-c0-b0=/x/s26.vst3 master-c0-b0=/x/s27.vst3 master-c0-b2=/x/s27b2.vst3 master-c1-b1=/x/master-extra.vst3
bypass 
routes 
keys "#),
        ("doc-v6-27.json", r#"rows 32 next_id 32 legacy v24
t0 midi added name="s0" color=100 input="midi:all" send fx 0.01
t1 midi added arm name="s1" color=101 send fx 0.02
t2 name="s2" color=102 send fx 0.03
t3 name="s3" color=103 send fx 0.04
t4 name="s4" color=104 send fx 0.05
t5 name="s5" color=105 send fx 0.06
t6 name="s6" color=106 send fx 0.07
t7 name="s7" color=107 send fx 0.08
t8 name="s8" color=108 send fx 0.09
t9 name="s9" color=109 send fx 0.1
t10 name="s10" color=110 send fx 0.11
t11 name="s11" color=111 send fx 0.12
t12 name="s12" color=112 send fx 0.13
t13 name="s13" color=113 send fx 0.14
t14 name="s14" color=114 send fx 0.15
t15 name="s15" color=115 send fx 0.16
t16 name="s16" color=116 send fx 0.17
t17 name="s17" color=117 send fx 0.18
t18 name="s18" color=118 send fx 0.19
t19 name="s19" color=119 send fx 0.2
t20 name="s20" color=120 send fx 0.21
t21 name="s21" color=121 send fx 0.22
t22 name="s22" color=122 send fx 0.23
t23 added mute name="s23" color=123 send fx 0.24
drums name="s24" color=124 send fx 0.25
click mute name="s25" color=125 send fx 0.26
fx solo name="s26" color=126 send fx 0.27
master name="s27" color=127
plugins t0-c0-b0=@tangent-drums t0-n127-b0=/x/pad127.vst3 t1-c0-b0=@tangent-dx7 t3-n5-b2=/x/c3pad5.vst3 t23-c0-b0=/x/c23.vst3 drums-c0-b0=/x/drumbus.vst3 fx-c0-b1=/x/fx.vst3 master-c0-b2=/x/master.vst3 master-c1-b1=/x/master-extra.vst3
bypass 
routes 
keys "#),
    ];
}
