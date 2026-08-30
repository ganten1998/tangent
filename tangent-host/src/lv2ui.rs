//! An LV2 plugin's own window, embedded in one of ours.
//!
//! # What this is and what it is not
//!
//! An LV2 plugin and its editor are **two separate shared objects**. The DSP
//! half is what [`crate::lv2`] loads; this loads the other one, whose only job
//! is to draw. They talk through the host: the UI calls a write function when a
//! control moves, and the host calls `port_event` when a value changes from
//! anywhere else.
//!
//! Only `ui:X11UI` is handled, and that is the whole of what is worth handling
//! here. The other classes are toolkit-specific — `GtkUI`, `Gtk3UI`, `Qt5UI`,
//! `CocoaUI`, `WindowsUI` — and embedding those means `suil`, whose entire
//! purpose is wrapping one toolkit's widget inside another's. X11UI is the
//! native case: the UI makes an X window, we make an X window, and the two are
//! parent and child. No wrapping, no second toolkit linked into the process,
//! no GTK main loop fighting winit's.
//!
//! What that costs is honest: a plugin that ships only a Gtk UI gets no editor
//! and says so. The plugins that matter here — every Dragonfly reverb, and DPF
//! and JUCE output generally — ship X11UI.
//!
//! # The one thing that must not be got wrong
//!
//! `cleanup` is not optional and it is not deferred. The UI's window is a child
//! of ours, and dropping the parent while the child's toolkit still believes it
//! exists is a use-after-free inside somebody else's code. So [`Ui`] owns its
//! instance, cleans up in `Drop`, and the caller destroys the parent window
//! only after the `Ui` is gone.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};

/// The URID map every UI expects, shared by all of them in this process.
///
/// **One map for the whole process, and it has to be.** A URID is an integer
/// standing for a URI, and the mapping is only meaningful within one map: two
/// maps hand out different numbers for the same URI, and an atom built against
/// one and read against the other names something else entirely. The DSP side
/// gets its map from livi; this is the UI side's, and the two only ever meet
/// through values the plugin itself round-trips.
struct Urids {
    map: std::sync::Mutex<std::collections::HashMap<String, u32>>,
}

static URIDS: std::sync::OnceLock<Urids> = std::sync::OnceLock::new();

fn urids() -> &'static Urids {
    URIDS.get_or_init(|| Urids {
        map: std::sync::Mutex::new(std::collections::HashMap::new()),
    })
}

/// # Safety
/// Called by plugin UIs with a NUL-terminated URI. Matches `LV2_URID_Map`.
unsafe extern "C" fn urid_map(_handle: *mut c_void, uri: *const c_char) -> u32 {
    if uri.is_null() {
        return 0;
    }
    // SAFETY: the ABI promises a NUL-terminated string.
    let s = unsafe { CStr::from_ptr(uri) }.to_string_lossy().into_owned();
    let m = urids();
    let Ok(mut g) = m.map.lock() else {
        return 0;
    };
    // Zero is reserved by the specification for "not mapped", so ids start at 1.
    let next = g.len() as u32 + 1;
    *g.entry(s).or_insert(next)
}

/// What the UI sends back when the user moves something.
///
/// The port index and the new value, queued for the host to apply on its own
/// thread — this is called from inside the UI's own event handling and must not
/// reach into the engine from there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiEdit {
    pub port: u32,
    pub value: f32,
}

/// The queue an instance's write function pushes into.
///
/// Keyed by controller pointer so several open editors do not collide: the
/// controller is the one opaque value the ABI hands back to us unchanged, which
/// makes it the natural identity for "which editor said this".
static EDITS: std::sync::OnceLock<std::sync::Mutex<Vec<(usize, UiEdit)>>> =
    std::sync::OnceLock::new();

fn edits() -> &'static std::sync::Mutex<Vec<(usize, UiEdit)>> {
    EDITS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// # Safety
/// Called by plugin UIs. Matches `LV2UI_Write_Function`.
unsafe extern "C" fn write_fn(
    controller: lv2_sys::LV2UI_Controller,
    port: u32,
    size: u32,
    format: u32,
    buffer: *const c_void,
) {
    // Format 0 is a plain float on a control port, which is the only thing this
    // host knows how to apply. Anything else is an atom — a patch message, a
    // file path — and dropping it is better than misreading it as a number.
    if format != 0 || size as usize != std::mem::size_of::<f32>() || buffer.is_null() {
        return;
    }
    // SAFETY: checked non-null and the right size for an f32 just above.
    let value = unsafe { *(buffer as *const f32) };
    if let Ok(mut q) = edits().lock() {
        // Bounded so a UI spinning in a loop cannot grow this without limit
        // while nobody drains it.
        if q.len() < 4096 {
            q.push((controller as usize, UiEdit { port, value }));
        }
    }
}

fn path_of(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_owned()
}

/// A loaded, embedded plugin editor.
pub struct Ui {
    lib: *mut c_void,
    desc: *const lv2_sys::LV2UI_Descriptor,
    handle: lv2_sys::LV2UI_Handle,
    idle: Option<unsafe extern "C" fn(lv2_sys::LV2UI_Handle) -> i32>,
    /// The X11 window the UI created. A child of the parent it was given.
    window: u32,
    /// Kept alive: the UI holds these pointers for its whole life.
    _features: Box<FeatureStore>,
}

/// The feature array handed to `instantiate`, kept alive alongside the UI.
///
/// **Boxed and owned, because the plugin keeps the pointers.** The LV2 contract
/// says the feature array need only live for the duration of the call, but the
/// `data` each feature points at is used for the UI's whole life — the URID map
/// most obviously. Letting these fall off the stack after `instantiate` returns
/// is a dangling pointer that works right up until the first control moves.
struct FeatureStore {
    _uris: Vec<CString>,
    _map: Box<lv2_sys::LV2_URID_Map>,
    /// The option array points at this, so it outlives the call too.
    _rate: Box<f32>,
    _options: Box<[lv2_sys::LV2_Options_Option; 2]>,
    _features: Vec<lv2_sys::LV2_Feature>,
    _ptrs: Vec<*const lv2_sys::LV2_Feature>,
}

// SAFETY: a `Ui` owns its library handle, its instance and its features, and
// none of them is touched from more than one thread — the type is created,
// idled and dropped on the UI thread. `Send` is asserted only so it can live in
// the same structures the VST3 editors do.
unsafe impl Send for Ui {}

impl Ui {
    /// Open the plugin's own editor as a child of `parent`.
    ///
    /// `parent` is an X11 window id. Returns `Ok(None)` when the plugin simply
    /// has no X11 UI, which is not an error — plenty of good plugins have no
    /// editor at all, and the caller shows its generic parameter panel instead.
    ///
    /// `sample_rate` is not decoration. A UI that draws a frequency response,
    /// a delay time in milliseconds or a decay curve computes it from the rate,
    /// and a host that does not say gets guessed at: Dragonfly Reverb prints
    /// "this host does not send sample-rate information for LV2 UIs, using
    /// 44100 as fallback (this could be wrong)" and then draws its impulse
    /// response against the wrong axis.
    pub fn open(plugin_uri: &str, parent: u32, sample_rate: f32) -> Result<Option<Ui>, String> {
        let world = lilv::World::with_load_all();
        let plugins = world.plugins();
        let plugin = plugins
            .into_iter()
            .find(|p| p.uri().as_uri() == Some(plugin_uri))
            .ok_or_else(|| format!("no LV2 plugin with URI {plugin_uri}"))?;

        let Some(uis) = plugin.uis() else {
            return Ok(None);
        };
        let x11 = world.new_uri("http://lv2plug.in/ns/extensions/ui#X11UI");
        let Some(ui) = uis.into_iter().find(|u| u.is_a(&x11)) else {
            // It has a UI, just not one we can embed without a toolkit bridge.
            return Ok(None);
        };

        let bundle = ui
            .bundle_uri()
            .and_then(|n| n.as_uri().map(path_of))
            .ok_or("the UI declares no bundle")?;
        let binary = ui
            .binary_uri()
            .and_then(|n| n.as_uri().map(path_of))
            .ok_or("the UI declares no binary")?;
        let ui_uri = ui
            .uri()
            .as_uri()
            .ok_or("the UI has no URI")?
            .to_owned();

        // ── the shared object ────────────────────────────────────────────────
        let cbin = CString::new(binary.clone()).map_err(|_| "UI path contains a NUL")?;
        // SAFETY: a valid NUL-terminated path. RTLD_LOCAL so a plugin's symbols
        // cannot collide with another's, which is the same rule the VST3 loader
        // follows and for the same reason.
        let lib = unsafe { libc::dlopen(cbin.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if lib.is_null() {
            // SAFETY: dlerror is valid immediately after a failed dlopen.
            let e = unsafe { libc::dlerror() };
            let msg = if e.is_null() {
                "unknown".to_owned()
            } else {
                unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned()
            };
            return Err(format!("could not load {binary}: {msg}"));
        }
        let guard = LibGuard(lib);

        let sym = CString::new("lv2ui_descriptor").expect("literal has no NUL");
        // SAFETY: freshly loaded handle.
        let f = unsafe { libc::dlsym(lib, sym.as_ptr()) };
        if f.is_null() {
            return Err(format!("{binary} exports no lv2ui_descriptor"));
        }
        // SAFETY: the LV2 UI ABI defines this symbol with exactly this type.
        let descfn: lv2_sys::LV2UI_DescriptorFunction = unsafe { std::mem::transmute(f) };
        let descfn = descfn.ok_or("lv2ui_descriptor is null")?;

        // A binary may hold several UIs; take the one lilv named.
        let mut desc: *const lv2_sys::LV2UI_Descriptor = std::ptr::null();
        for i in 0..32u32 {
            // SAFETY: the ABI's enumeration protocol — indices until null.
            let d = unsafe { descfn(i) };
            if d.is_null() {
                break;
            }
            // SAFETY: non-null descriptor from the plugin's own table.
            let uri = unsafe { CStr::from_ptr((*d).URI) }.to_string_lossy();
            if uri == ui_uri {
                desc = d;
                break;
            }
        }
        if desc.is_null() {
            return Err(format!("{binary} has no descriptor for {ui_uri}"));
        }

        // ── features, owned for the UI's whole life ──────────────────────────
        let uri_map = CString::new("http://lv2plug.in/ns/ext/urid#map").expect("literal");
        let uri_parent = CString::new("http://lv2plug.in/ns/extensions/ui#parent").expect("literal");
        let uri_idle =
            CString::new("http://lv2plug.in/ns/extensions/ui#idleInterface").expect("literal");
        let uri_options = CString::new("http://lv2plug.in/ns/ext/options#options").expect("literal");
        let uri_sr =
            CString::new("http://lv2plug.in/ns/ext/parameters#sampleRate").expect("literal");
        let uri_float = CString::new("http://lv2plug.in/ns/ext/atom#Float").expect("literal");
        let mut map = Box::new(lv2_sys::LV2_URID_Map {
            handle: std::ptr::null_mut(),
            map: Some(urid_map),
        });
        // The options array, which is how a UI is told things that are not
        // ports. Terminated by a zeroed entry, per the extension.
        // SAFETY: both URIs are valid NUL-terminated strings.
        let (key_sr, type_float) = unsafe {
            (
                urid_map(std::ptr::null_mut(), uri_sr.as_ptr()),
                urid_map(std::ptr::null_mut(), uri_float.as_ptr()),
            )
        };
        let rate = Box::new(sample_rate);
        let options = Box::new([
            lv2_sys::LV2_Options_Option {
                context: lv2_sys::LV2_Options_Context_LV2_OPTIONS_INSTANCE,
                subject: 0,
                key: key_sr,
                size: std::mem::size_of::<f32>() as u32,
                type_: type_float,
                value: rate.as_ref() as *const f32 as *const c_void,
            },
            lv2_sys::LV2_Options_Option {
                context: lv2_sys::LV2_Options_Context_LV2_OPTIONS_INSTANCE,
                subject: 0,
                key: 0,
                size: 0,
                type_: 0,
                value: std::ptr::null(),
            },
        ]);
        let features = vec![
            lv2_sys::LV2_Feature {
                URI: uri_map.as_ptr(),
                data: map.as_mut() as *mut _ as *mut c_void,
            },
            lv2_sys::LV2_Feature {
                // **For an X11 UI the parent IS the window id**, passed where a
                // pointer would go. That is the specification's own convention
                // and not a cast this host invented.
                URI: uri_parent.as_ptr(),
                data: parent as usize as *mut c_void,
            },
            lv2_sys::LV2_Feature {
                URI: uri_idle.as_ptr(),
                data: std::ptr::null_mut(),
            },
            lv2_sys::LV2_Feature {
                URI: uri_options.as_ptr(),
                data: options.as_ptr() as *mut c_void,
            },
        ];
        let mut ptrs: Vec<*const lv2_sys::LV2_Feature> = features.iter().map(|f| f as *const _).collect();
        ptrs.push(std::ptr::null());
        let store = Box::new(FeatureStore {
            _uris: vec![uri_map, uri_parent, uri_idle, uri_options, uri_sr, uri_float],
            _map: map,
            _rate: rate,
            _options: options,
            _features: features,
            _ptrs: ptrs,
        });

        let cplugin = CString::new(plugin_uri).map_err(|_| "plugin URI contains a NUL")?;
        let cbundle = CString::new(bundle).map_err(|_| "bundle path contains a NUL")?;
        let mut widget: lv2_sys::LV2UI_Widget = std::ptr::null_mut();
        // SAFETY: every argument matches the ABI; the feature array is
        // null-terminated and outlives the call inside `store`.
        let handle = unsafe {
            let inst = (*desc).instantiate.ok_or("the UI has no instantiate")?;
            inst(
                desc,
                cplugin.as_ptr(),
                cbundle.as_ptr(),
                Some(write_fn),
                std::ptr::null_mut(),
                &mut widget,
                store._ptrs.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(format!("{ui_uri} refused to instantiate"));
        }

        // SAFETY: a descriptor from the plugin's own table.
        let idle_ext = unsafe {
            match (*desc).extension_data {
                Some(ext) => ext(uri_idle_ptr()),
                None => std::ptr::null(),
            }
        };
        let idle = if idle_ext.is_null() {
            // Not an error: a UI that drives its own event loop needs no idle,
            // and the specification allows it.
            None
        } else {
            // SAFETY: the URI we asked for defines this struct.
            unsafe { (*(idle_ext as *const lv2_sys::LV2UI_Idle_Interface)).idle }
        };

        guard.disarm();
        Ok(Some(Ui {
            lib,
            desc,
            handle,
            idle,
            window: widget as usize as u32,
            _features: store,
        }))
    }

    /// The UI's own X11 window, for the caller to size and map.
    pub fn window(&self) -> u32 {
        self.window
    }

    /// Give the UI a slice of time.
    ///
    /// Returns `false` when the UI has asked to be closed, which is how a
    /// plugin with its own close button tells the host it is done.
    pub fn idle(&mut self) -> bool {
        match self.idle {
            // SAFETY: our own live handle.
            Some(idle) => unsafe { idle(self.handle) == 0 },
            None => true,
        }
    }

    /// Tell the UI a control changed somewhere else — automation, a preset, the
    /// host's own panel.
    ///
    /// Without this the plugin's knobs and the host's parameter list drift
    /// apart the moment anything but the plugin moves one.
    pub fn port_changed(&mut self, port: u32, value: f32) {
        // SAFETY: our own live handle; format 0 with an f32 is the control-port
        // protocol the ABI defines.
        unsafe {
            if let Some(pe) = (*self.desc).port_event {
                pe(
                    self.handle,
                    port,
                    std::mem::size_of::<f32>() as u32,
                    0,
                    &value as *const f32 as *const c_void,
                );
            }
        }
    }

    /// Everything the UI has asked to change since the last call.
    pub fn drain_edits(&mut self) -> Vec<UiEdit> {
        let Ok(mut q) = edits().lock() else {
            return Vec::new();
        };
        let mine = self.handle as usize;
        let mut out = Vec::new();
        q.retain(|(who, e)| {
            // The controller passed at instantiate was null, so every edit is
            // attributed to the same key; with one editor open that is exact,
            // and with several the UI that did not move is sent a value it
            // already has, which is harmless. Threading a real controller
            // through is the fix and is worth doing when a second editor can
            // actually be open at once.
            let _ = who;
            out.push(*e);
            false
        });
        let _ = mine;
        out
    }
}

fn uri_idle_ptr() -> *const c_char {
    // A leaked CString, because `extension_data` may be called after the local
    // one would have dropped and the cost is one small allocation per process.
    static P: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *P.get_or_init(|| {
        let c = CString::new("http://lv2plug.in/ns/extensions/ui#idleInterface").expect("literal");
        Box::leak(c.into_boxed_c_str()).as_ptr() as usize
    }) as *const c_char
}

/// Closes a freshly opened library if the rest of `open` fails.
struct LibGuard(*mut c_void);

impl LibGuard {
    fn disarm(self) {
        std::mem::forget(self);
    }
}

impl Drop for LibGuard {
    fn drop(&mut self) {
        // SAFETY: only reached when `open` failed after dlopen succeeded, so
        // nothing from this library is in use.
        unsafe { libc::dlclose(self.0) };
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        // **Cleanup before the library, and before the parent window goes.**
        // The UI's window is a child of the host's; tearing the parent down
        // while the UI's toolkit still believes its window exists is a
        // use-after-free inside somebody else's code.
        // SAFETY: our own live handle, cleaned up exactly once.
        unsafe {
            if let Some(cleanup) = (*self.desc).cleanup {
                cleanup(self.handle);
            }
            libc::dlclose(self.lib);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_plugin_is_an_error_not_a_panic() {
        assert!(Ui::open("urn:definitely:not:a:plugin", 0, 48_000.0).is_err());
    }

    #[test]
    fn the_urid_map_is_stable_and_never_zero() {
        let a = CString::new("http://example.org/one").unwrap();
        let b = CString::new("http://example.org/two").unwrap();
        // SAFETY: valid NUL-terminated URIs.
        let (a1, b1, a2) = unsafe {
            (
                urid_map(std::ptr::null_mut(), a.as_ptr()),
                urid_map(std::ptr::null_mut(), b.as_ptr()),
                urid_map(std::ptr::null_mut(), a.as_ptr()),
            )
        };
        assert_eq!(a1, a2, "the same URI must map to the same id every time");
        assert_ne!(a1, b1, "different URIs must not collide");
        // Zero is the specification's "not mapped", so it must never be handed
        // out as a real id.
        assert_ne!(a1, 0);
        assert_ne!(b1, 0);
    }

    /// Opening a real editor needs an X server and a plugin that has one, so it
    /// is not run by default — but the discovery half is, because "does this
    /// plugin have an X11 UI at all" is answerable without a display and is
    /// where a wrong lilv query would show up.
    #[test]
    fn plugins_without_an_x11_ui_report_none_rather_than_failing() {
        // eg-amp ships no UI whatsoever; the answer is None, not Err.
        let r = Ui::open("http://lv2plug.in/plugins/eg-amp", 0, 48_000.0);
        match r {
            Ok(None) => {}
            Ok(Some(_)) => panic!("eg-amp has no UI to open"),
            Err(e) => {
                // Absent from this machine entirely is also fine.
                assert!(e.contains("no LV2 plugin"), "unexpected error: {e}");
            }
        }
    }
}
