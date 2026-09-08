//! Finding VST3 bundles on disk and reading what they claim to be.
//!
//! This is the first half of hosting: locate the modules, open them, and ask
//! the factory what classes it exports. Instantiating a component and pulling
//! audio comes next; nothing here creates a processor.
//!
//! # The bundle shape, which is not obvious
//!
//! A `.vst3` is a directory bundle on macOS and often on Windows/Linux — and a PLAIN FILE for many Windows/Linux builds (reaplugs); both forms are scanned and loaded since VST 3.6.10
//! — Steinberg made the Windows and Linux layouts match macOS's. The binary
//! lives at:
//!
//! ```text
//! macOS    Foo.vst3/Contents/MacOS/Foo
//! Windows  Foo.vst3/Contents/x86_64-win/Foo.vst3
//! Linux    Foo.vst3/Contents/x86_64-linux/Foo.so
//! ```
//!
//! and the executable inside has **no extension on macOS**, which is why
//! globbing for a file rather than reading `Contents` finds nothing.
//!
//! # The macOS entry points, which are the actual trap
//!
//! On Windows and Linux you `dlopen` and call `GetPluginFactory`. On macOS that
//! is **not sufficient**: the bundle must first be initialised with
//! `bundleEntry(CFBundleRef)`, and plugins are entitled to do real work there —
//! the modelled piano and the Arturia plugins both locate their sample and preset
//! directories relative to the bundle. Skipping it gets you a factory pointer
//! that segfaults later rather than an error now.
//!
//! Symmetrically, `bundleExit()` must be called before unloading, and **not
//! calling it is safer than calling it at the wrong time**: a plugin that has
//! spawned threads in `bundleEntry` will use-after-free if the library is
//! unmapped from under them. `Module` therefore does the pairing itself and
//! deliberately leaks on the failure path.

use std::collections::HashSet;
use std::ffi::{c_void, CString, OsStr};
use std::path::{Path, PathBuf};

use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo2, PFactoryInfo,
};
use vst3::ComPtr;

/// Where a VST3 bundle's loadable binary lives inside it, per platform.
#[cfg(target_os = "macos")]
const CONTENTS_SUBDIR: &str = "MacOS";
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const CONTENTS_SUBDIR: &str = "x86_64-win";
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
const CONTENTS_SUBDIR: &str = "arm64-win";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const CONTENTS_SUBDIR: &str = "x86_64-linux";
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const CONTENTS_SUBDIR: &str = "aarch64-linux";
// **FreeBSD says `amd64-linux`, and that is not a typo.**
//
// This arm was first written from the spec's `<cpu>-<os>` shape, which gives
// `x86_64-freebsd`, and that guess was WRONG IN BOTH HALVES. The authority is
// `audio/vst3sdk` in ports — the SDK a FreeBSD plugin is actually built with —
// and `module_linux.cpp:149-172` is unambiguous:
//
//     // use the Machine Hardware Name (from uname cmd-line) as prefix
//     auto machine = getCurrentMachineName ();     // uname().machine
//     modulePath /= *machine + "-linux";
//
// The `-linux` suffix is hardcoded, so the OS half is `linux` on any Unix the
// SDK builds on, FreeBSD included. And the CPU half is `uname -m`, which on
// FreeBSD/amd64 is **`amd64`** where Linux/x86_64 reports `x86_64` — so the two
// platforms disagree about the prefix as well.
//
// Net: `Contents/amd64-linux/Name.so`, and neither of the two names originally
// written here would ever have matched. Verified against the installed SDK
// rather than reasoned about, which is the only reason it is right.
#[cfg(all(target_os = "freebsd", target_arch = "x86_64"))]
const CONTENTS_SUBDIR: &str = "amd64-linux";
#[cfg(all(target_os = "freebsd", target_arch = "aarch64"))]
const CONTENTS_SUBDIR: &str = "arm64-linux";

/// Other directories a FreeBSD-native bundle plausibly lands in, tried in order
/// after [`CONTENTS_SUBDIR`].
///
/// The SDK settles what its own builder produces; it does not settle what every
/// OTHER build system produces. JUCE and DPF reach FreeBSD by treating it as a
/// Linux variant and derive the architecture string from their own tables,
/// which mostly stop at `x86_64-linux` and do not consult `uname`. A bundle
/// holding a perfectly good FreeBSD ELF under `x86_64-linux` is therefore a
/// realistic artifact, and looking only at the SDK's name would report "no
/// plugin" about one that is right there and would load.
///
/// The `-freebsd` spellings are kept last because they are what a
/// spec-reading implementation would emit, and this file was one of those.
///
/// Safe in the direction that matters: a genuine *Linux* `.so` cannot be
/// `dlopen`ed by the FreeBSD runtime linker — the ELF OSABI note and the
/// syscall vector both refuse it — so none of these can smuggle in a foreign
/// binary. They can only find a native plugin that was filed elsewhere.
#[cfg(all(target_os = "freebsd", target_arch = "x86_64"))]
const CONTENTS_SUBDIR_ALTS: &[&str] = &["x86_64-linux", "amd64-freebsd", "x86_64-freebsd"];
#[cfg(all(target_os = "freebsd", target_arch = "aarch64"))]
const CONTENTS_SUBDIR_ALTS: &[&str] = &["aarch64-linux", "arm64-freebsd", "aarch64-freebsd"];

/// The standard search paths, in the order the SDK specifies.
///
/// User-local first, so a user's own build shadows a system one — which is what
/// every host does and what anyone debugging a plugin expects.
///
/// `VST3_PATH` comes first of all when it is set. It is the SDK's own documented
/// override, every serious host honours it, and it is the one thing somebody
/// with plugins in an unusual place will already have tried.
pub fn search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(env) = std::env::var_os("VST3_PATH") {
        out.extend(std::env::split_paths(&env));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(&home).join("Library/Audio/Plug-Ins/VST3"));
        }
        out.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
        out.push(PathBuf::from("/Network/Library/Audio/Plug-Ins/VST3"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            out.push(PathBuf::from(local).join("Programs/Common/VST3"));
        }
        if let Some(pf) = std::env::var_os("CommonProgramFiles") {
            out.push(PathBuf::from(pf).join("VST3"));
        }
        // The 64-bit view from a 32-bit-shaped environment, the x86 shelf,
        // and the literal spelling for an environment with no variables at
        // all — plus the folder beside the exe, the portable convention.
        if let Some(pf) = std::env::var_os("CommonProgramW6432") {
            out.push(PathBuf::from(pf).join("VST3"));
        }
        if let Some(pf) = std::env::var_os("CommonProgramFiles(x86)") {
            out.push(PathBuf::from(pf).join("VST3"));
        }
        out.push(PathBuf::from("C:\\Program Files\\Common Files\\VST3"));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                out.push(dir.join("VST3"));
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(&home).join(".vst3"));
        }
        out.push(PathBuf::from("/usr/lib/vst3"));
        out.push(PathBuf::from("/usr/local/lib/vst3"));
    }
    // FreeBSD beside Linux rather than folded into it, because the system path
    // genuinely differs: ports and packages install under `/usr/local`, and
    // `/usr/lib` is base-system territory where a VST3 will never be. Listing
    // `/usr/lib/vst3` here would be a path that cannot exist on this OS.
    // `~/.vst3` is shared, being an SDK convention rather than a Linux one.
    //
    // Without this arm FreeBSD contributed nothing but `VST3_PATH`, so the
    // plugin picker was empty unless the user had already set an env var they
    // had no reason to know about.
    #[cfg(target_os = "freebsd")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(&home).join(".vst3"));
        }
        out.push(PathBuf::from("/usr/local/lib/vst3"));
    }
    out
}

/// How deep to walk under a search path.
///
/// **Not one level, which is what this used to do.** The SDK allows vendor and
/// category folders under the VST3 directory and the big installers all use
/// them — Steinberg, Native Instruments, Waves, Applied Acoustics, Kilohearts.
/// On the machine this was found on, 112 of 160 installed plugins were at the
/// top level and the other 48 — HALion Sonic, Groove Agent, Lounge Lizard —
/// simply did not exist as far as this app was concerned, with nothing on
/// screen to suggest they were being skipped.
///
/// Three is deep enough for `VST3/Vendor/Category/Thing.vst3` and shallow
/// enough that it cannot turn into a walk of somebody's home directory if they
/// add one as a custom folder.
const MAX_DEPTH: usize = 3;

/// A cap on how many directories one scan will open.
///
/// The depth limit bounds the SHAPE of the walk and this bounds its SIZE, which
/// are different failures: a custom folder pointed at a network volume or a
/// build tree can be three levels deep and still contain tens of thousands of
/// directories, and a picker that takes a minute to open is a hung app.
const MAX_DIRS: usize = 4_000;

/// Every `.vst3` bundle under the standard paths, plus any the user added.
///
/// Recursive, bounded, symlink-safe and deduplicated — see [`MAX_DEPTH`],
/// [`MAX_DIRS`], and the `seen` set, which holds canonical paths so that a
/// folder reachable two ways (a symlink into the system directory is the common
/// one) is walked once and listed once.
/// The vendor a bundle NAMES, without loading a byte of its code.
///
/// VST3 3.7+ ships `moduleinfo.json` (in `Contents/Resources/` per the spec,
/// `Contents/` in the wild) whose "Factory Info" object carries "Vendor".
/// Read with a tolerant scan rather than a JSON crate: the answer feeds a
/// GROUPING in the picker, and a file this scanner cannot read simply lands
/// the plugin in the unlabelled group at the bottom — the same place a
/// pre-3.7 plugin with no moduleinfo at all lands.
pub fn vendor_of(bundle: &Path) -> Option<String> {
    vendor_from_moduleinfo(bundle)
        .or_else(|| vendor_from_parent_dir(bundle))
        .or_else(|| vendor_from_plist(bundle))
}

fn vendor_from_moduleinfo(bundle: &Path) -> Option<String> {
    let text = ["Contents/Resources/moduleinfo.json", "Contents/moduleinfo.json"]
        .iter()
        .find_map(|rel| std::fs::read_to_string(bundle.join(rel)).ok())?;
    let factory = text.find("\"Factory Info\"")?;
    let after = &text[factory..];
    let vkey = after.find("\"Vendor\"")?;
    let rest = &after[vkey + "\"Vendor\"".len()..];
    let open = rest.find('"')? + 1;
    let close = rest[open..].find('"')?;
    let vendor = rest[open..open + close].trim();
    (!vendor.is_empty()).then(|| vendor.to_owned())
}

/// The SDK's own layout convention: `VST3/Vendor/Thing.vst3` (with an
/// optional category level handled by the walk). A bundle whose parent is
/// not the VST3 root itself, while its grandparent IS, sits in a folder
/// somebody named after the maker — "Applied Acoustics Systems" on the
/// owner's disk. Measured there: 48 of 161 bundles answer this way.
fn vendor_from_parent_dir(bundle: &Path) -> Option<String> {
    let parent = bundle.parent()?;
    let name = parent.file_name()?.to_str()?;
    let grandparent = parent.parent()?.file_name()?.to_str()?;
    (!name.eq_ignore_ascii_case("vst3") && grandparent.eq_ignore_ascii_case("vst3"))
        .then(|| name.to_owned())
}

/// The macOS bundle identifier's second segment — `com.arturia.acidv` says
/// Arturia. XML plists only: a binary plist would need a real parser, and
/// this feeds a GROUPING, not a fact anybody acts on. Measured on the
/// owner's disk this catches the hundred bundles the first two miss.
fn vendor_from_plist(bundle: &Path) -> Option<String> {
    let text = std::fs::read_to_string(bundle.join("Contents/Info.plist")).ok()?;
    let key = text.find("<key>CFBundleIdentifier</key>")?;
    let after = &text[key..];
    let open = after.find("<string>")? + "<string>".len();
    let close = after[open..].find("</string>")?;
    let ident = &after[open..open + close];
    let mut parts = ident.split('.');
    let tld = parts.next()?;
    if !matches!(tld, "com" | "org" | "net" | "de" | "uk" | "se" | "jp" | "co" | "io") {
        return None;
    }
    let raw = parts.next()?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut c = raw.chars();
    let first = c.next()?;
    Some(first.to_uppercase().collect::<String>() + c.as_str())
}

pub fn discover_in(extra: &[PathBuf]) -> Vec<PathBuf> {
    discover_reported(extra).found
}

/// What one scan actually did — the picker prints this when it found
/// nothing, because "no plugins" and "the scan never looked" were
/// pixel-identical and the Windows tester could not tell which they had.
#[derive(Debug, Default, Clone)]
pub struct ScanReport {
    /// Every root tried: the path and what became of it — a count, or the
    /// reason it was skipped.
    pub roots: Vec<(PathBuf, Result<usize, String>)>,
    pub found: Vec<PathBuf>,
    /// VST2-shaped `.dll` files seen along the way. Not plugins this host
    /// can load — but the one honest explanation for a folder somebody
    /// SWEARS has plugins in it.
    pub vst2_seen: usize,
    /// Copies of Tangent's own RETIRED VST3 plugin (retired 2026-08-24; the
    /// installers stopped shipping it, but nothing ever cleaned up what the
    /// old ones planted). Never listed as loadable — a DAW hosting its own
    /// stale plugin build is a bug farm — but remembered here so the scan
    /// note can tell the user the leftover exists and how it goes away.
    pub retired_seen: Vec<PathBuf>,
}

pub fn discover_reported(extra: &[PathBuf]) -> ScanReport {
    // The user's own folders first, so a plugin they pointed at deliberately
    // shadows a copy of the same thing in a system directory.
    let mut roots: Vec<PathBuf> = extra.to_vec();
    roots.extend(search_paths());

    let mut report = ScanReport::default();
    let mut out = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut opened = 0_usize;
    let is_vst3 = |p: &Path| {
        p.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("vst3"))
    };
    let is_dll = |p: &Path| {
        p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll"))
    };
    for root in roots {
        let mut found_here = 0usize;
        let mut fate: Option<String> = None;
        let mut queue: Vec<(PathBuf, usize)> = vec![(root.clone(), 0)];
        while let Some((dir, depth)) = queue.pop() {
            if opened >= MAX_DIRS {
                fate = Some("scan budget spent".to_owned());
                break;
            }
            // Canonicalised, so a symlink loop terminates and a directory
            // reachable by two names is not scanned twice.
            let real = match dir.canonicalize() {
                Ok(r) => r,
                Err(e) => {
                    if depth == 0 {
                        fate = Some(format!("not found ({e})"));
                    }
                    continue;
                }
            };
            if !seen.insert(real.clone()) {
                if depth == 0 {
                    fate = Some("already covered by an earlier root".to_owned());
                }
                continue;
            }
            let entries = match std::fs::read_dir(&real) {
                Ok(e) => e,
                Err(e) => {
                    if depth == 0 {
                        fate = Some(format!("unreadable ({e})"));
                    }
                    continue;
                }
            };
            opened += 1;
            for entry in entries.flatten() {
                // The ORIGINAL spelling, not the canonical one: Windows'
                // canonicalise answers verbatim `\\?\` paths, which load
                // fine but read like line noise everywhere they are shown
                // or saved.
                let path = dir.join(entry.file_name());
                if is_vst3(&path) {
                    // Tangent's own retired plugin is cleanup, not a plugin:
                    // see `ScanReport::retired_seen`.
                    if path
                        .file_name()
                        .is_some_and(|n| n.eq_ignore_ascii_case("Tangent.vst3"))
                    {
                        report.retired_seen.push(path);
                        continue;
                    }
                    // A bundle is a directory and is NOT descended into.
                    out.push(path);
                    found_here += 1;
                    continue;
                }
                if is_dll(&path) {
                    report.vst2_seen += 1;
                }
                // `is_dir` follows symlinks, which is what we want.
                if depth + 1 <= MAX_DEPTH && path.is_dir() {
                    queue.push((path, depth + 1));
                }
            }
        }
        report
            .roots
            .push((root, fate.map_or(Ok(found_here), Err)));
    }

    // By canonical path, so the same bundle reached through a symlinked folder
    // is one entry. Sorted by the path shown, so the picker is stable.
    let mut unique: HashSet<PathBuf> = HashSet::new();
    out.retain(|p| unique.insert(p.canonicalize().unwrap_or_else(|_| p.clone())));
    out.sort();
    report.found = out;
    report
}

/// Every `.vst3` bundle under the standard paths.
pub fn discover() -> Vec<PathBuf> {
    discover_in(&[])
}

/// The binary inside a bundle, or the bundle itself if it is already a plain
/// shared library (which some Linux builds still are).
pub fn binary_in_bundle(bundle: &Path) -> Option<PathBuf> {
    if bundle.is_file() {
        return Some(bundle.to_path_buf());
    }
    let dir = bundle.join("Contents").join(CONTENTS_SUBDIR);
    // See `CONTENTS_SUBDIR_ALTS`: the SDK settles what its own builder emits,
    // and other build systems on this platform emit other things. The
    // difference between finding them and not is the difference between "your
    // plugins work" and "the picker is permanently empty". Tried in order and
    // only when the SDK's own name is genuinely absent, so a correctly-built
    // bundle is never second-guessed.
    #[cfg(target_os = "freebsd")]
    let dir = if dir.is_dir() {
        dir
    } else {
        CONTENTS_SUBDIR_ALTS
            .iter()
            .map(|alt| bundle.join("Contents").join(alt))
            .find(|p| p.is_dir())
            .unwrap_or(dir)
    };
    let entries = std::fs::read_dir(dir).ok()?;
    // Windows bundles ship resource DLLs, `.pdb`s and `moduleinfo.json`
    // BESIDE the plugin, and `read_dir` order picked whichever came first.
    // Ordered preference instead: the bundle's own name, then any `.vst3`,
    // then any shared library; junk never wins.
    let files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && !p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                && !p.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("pdb")
                        || e.eq_ignore_ascii_case("json")
                        || e.eq_ignore_ascii_case("txt")
                })
        })
        .collect();
    let own = bundle.file_name();
    files
        .iter()
        .find(|p| p.file_name() == own)
        .or_else(|| {
            files.iter().find(|p| {
                p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vst3"))
            })
        })
        .or_else(|| {
            files.iter().find(|p| {
                p.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("dll") || e.eq_ignore_ascii_case("so")
                })
            })
        })
        .or_else(|| files.first())
        .cloned()
}

/// What a plugin class advertises about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassInfo {
    pub name: String,
    /// `"Audio Module Class"` for a processor, `"Component Controller Class"`
    /// for its editor half. An instrument is the former.
    ///
    /// **It does not say what KIND of processor.** A synth and a reverb are
    /// both "Audio Module Class"; see [`sub_categories`](Self::sub_categories).
    pub category: String,
    /// The `|`-separated list a plugin uses to say what it is —
    /// `"Instrument|Synth"`, `"Fx|Reverb"`, `"Fx|Instrument"` for the rare
    /// thing that is both.
    ///
    /// **Empty when the factory is too old to be asked.** `subCategories` is a
    /// `PClassInfo2` field, reachable only through `IPluginFactory2`, and a
    /// factory that does not implement it tells us nothing — which must read as
    /// "unknown" rather than as "not an instrument". See [`kind`](Self::kind).
    pub sub_categories: String,
    pub cid: [u8; 16],
}

/// What a plugin is for, as far as it will say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// It makes sound from notes. This is what a slot can play.
    Instrument,
    /// It changes sound it is given. A slot feeds it nothing, so it would sit
    /// there silent — which is exactly the complaint this enum exists to answer.
    Effect,
    /// It did not say, and nothing may be assumed. Treated as an instrument,
    /// because that is what every build before this one did with everything.
    Unknown,
}

impl ClassInfo {
    /// Whether this is the audio-processing half, as opposed to a controller or
    /// something else the factory happens to export.
    pub fn is_audio_module(&self) -> bool {
        self.category == "Audio Module Class"
    }

    /// What the plugin says it is for.
    ///
    /// **Instrument wins a tie.** A handful of plugins declare `Fx|Instrument`
    /// — samplers with an audio input, mostly — and those genuinely do play
    /// notes, so refusing them would be a regression dressed as a fix.
    pub fn kind(&self) -> Kind {
        let has = |needle: &str| {
            self.sub_categories
                .split('|')
                .any(|part| part.trim().eq_ignore_ascii_case(needle))
        };
        if has("Instrument") {
            Kind::Instrument
        } else if has("Fx") {
            Kind::Effect
        } else {
            Kind::Unknown
        }
    }
}

/// A loaded VST3 module: the dynamic library plus its factory.
///
/// Deliberately not `Clone` and deliberately not `Send`. The SDK requires that
/// the factory be used from the thread that initialised the bundle, and a
/// second `Module` for the same path would call `bundleEntry` twice.
///
/// **Which is why nothing outside this file should call [`Module::open`].**
/// Use [`module_for`], which keeps one module per bundle path per thread. Two
/// bays holding the same plugin is the ordinary case — the owner's report was
/// two drum lanes — and it used to open the bundle twice.
pub struct Module {
    path: PathBuf,
    /// Kept alive for as long as the factory is. Dropping this unmaps the
    /// library, and every COM pointer into it becomes a dangling function
    /// table.
    _lib: Library,
    factory: ComPtr<IPluginFactory>,
    vendor: String,
    url: String,
}

thread_local! {
    /// Every bundle this thread has opened, by canonicalised path.
    ///
    /// **One `bundleEntry` per bundle, which is what the SDK requires.** A
    /// plugin is entitled to do real work in `bundleEntry` — the module header
    /// names the modelled piano and Arturia, which locate their samples and presets
    /// there — and drum samplers commonly start a shared streaming engine in
    /// it. Calling it a second time re-runs that initialiser underneath the
    /// instance that is already using it, and the SDK's own `ModuleHandle`
    /// refcounts per path for exactly this reason. Measured before this
    /// existed: two `Module::open` calls on one bundle, two `bundleEntry`
    /// calls.
    ///
    /// **THREAD-LOCAL, and that is load-bearing rather than convenient.** The
    /// SDK requires the factory be used from the thread that initialised the
    /// bundle. A process-wide cache would happily hand a module opened on the
    /// UI thread to some other thread and the rule would be broken silently;
    /// keyed per thread, a second thread gets its own module and the rule
    /// cannot be broken by sharing. Every caller is on the UI thread today —
    /// the audio thread only ever HOLDS an `Arc<Module>` inside a `PluginBox`,
    /// it never calls into the factory — so in practice there is one map.
    ///
    /// **`Arc` and not `Rc`, even though the map is thread-local.** The handle
    /// itself crosses to the audio thread inside a `PluginBox`, which carries
    /// an `unsafe impl Send` that would let an `Rc` across just as happily —
    /// and an `Rc`'s refcount is not atomic, so a decrement there racing a
    /// clone here corrupts it silently. One atomic per plugin load is not a
    /// price worth arguing about; the audio thread never touches the count in
    /// a callback, only when a box is handed over or retired.
    ///
    /// **Never emptied.** The entry keeps a strong reference for the life of
    /// the thread, so the refcount cannot reach zero on the audio thread and
    /// drop a `ComPtr` there. That matches what `Library`'s `Drop` already
    /// decided: modules stay loaded for the process, because unmapping one
    /// from under a thread the plugin spawned is a use-after-free. A failed
    /// open is NOT cached — a bundle that was not there at startup should load
    /// once it is.
    static MODULES: std::cell::RefCell<std::collections::HashMap<PathBuf, std::sync::Arc<Module>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The module for `bundle`, opening it only if this thread has not already.
///
/// This is the only way a caller outside this file should obtain a [`Module`].
/// See `MODULES` for why it is per thread and why it never releases.
pub fn module_for(bundle: &Path) -> Result<std::sync::Arc<Module>, String> {
    // Canonicalised so that two spellings of one bundle — a symlink, a
    // relative path, a `/private` prefix macOS adds to `/tmp` — are one entry
    // and not two `bundleEntry` calls wearing different names. A path that
    // cannot be canonicalised is used as given: it will fail to open in a
    // moment anyway, and with a better message than this function could give.
    let key = std::fs::canonicalize(bundle).unwrap_or_else(|_| bundle.to_path_buf());
    if let Some(hit) = MODULES.with(|m| m.borrow().get(&key).cloned()) {
        return Ok(hit);
    }
    let module = std::sync::Arc::new(Module::open(bundle)?);
    // `insert` rather than the entry API: `Module::open` is re-entrant in the
    // sense that it runs somebody else's code, and holding a `RefCell` borrow
    // across it would panic if that code came back through here.
    MODULES.with(|m| m.borrow_mut().insert(key, std::sync::Arc::clone(&module)));
    Ok(module)
}

/// Hand-written rather than derived: `ComPtr` and the raw handles inside
/// `Library` are not `Debug`, and printing a function-table address would be
/// noise. What is useful in a log is which bundle, from whom, and what it
/// exports.
impl std::fmt::Debug for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Module")
            .field("path", &self.path)
            .field("vendor", &self.vendor)
            .field("classes", &self.classes().len())
            .finish()
    }
}

impl Module {
    /// Open a bundle and get its factory.
    ///
    /// # Safety of the whole operation
    ///
    /// This loads and executes arbitrary third-party code in this process. A
    /// crash inside a plugin is a crash of Tangent, and takes the user's take
    /// with it. That is a known and accepted cost for now — see
    /// `docs/RECORDER-PLAN.md` §8 on moving hosting out of process — but it is
    /// the reason nothing here should ever run while a take is recording.
    /// **`pub(crate)`, so that nothing outside can bypass [`module_for`].**
    /// This is the function that calls `bundleEntry`; calling it twice for one
    /// bundle is the bug `module_for` exists to prevent, and a visibility rule
    /// cannot pass vacuously the way a test can.
    pub(crate) fn open(bundle: &Path) -> Result<Self, String> {
        let binary = binary_in_bundle(bundle)
            .ok_or_else(|| format!("no loadable binary inside {}", bundle.display()))?;

        let lib = Library::open(&binary, bundle)?;

        // SAFETY: the symbol is the VST3 ABI's single required export. A module
        // without it is not a VST3, which is what the error says.
        let get_factory: unsafe extern "system" fn() -> *mut IPluginFactory =
            unsafe { lib.symbol(b"GetPluginFactory\0") }
                .ok_or_else(|| format!("{} exports no GetPluginFactory", binary.display()))?;

        // SAFETY: calling the plugin's own factory entry point, after
        // bundleEntry has succeeded.
        let raw = unsafe { get_factory() };
        if raw.is_null() {
            return Err(format!("{} returned a null factory", binary.display()));
        }
        // SAFETY: the factory is returned with a reference already added, which
        // `from_raw` takes ownership of rather than adding a second.
        let factory = unsafe { ComPtr::from_raw(raw) }
            .ok_or_else(|| format!("{} returned a null factory", binary.display()))?;

        let mut info = PFactoryInfo {
            vendor: [0; 64],
            url: [0; 256],
            email: [0; 128],
            flags: 0,
        };
        // SAFETY: `info` is a valid, fully-initialised out-parameter.
        unsafe {
            factory.getFactoryInfo(&mut info);
        }

        Ok(Self {
            path: bundle.to_path_buf(),
            _lib: lib,
            factory,
            vendor: c_array_to_string(&info.vendor),
            url: c_array_to_string(&info.url),
        })
    }

    /// The factory, for . Crate-visible rather than public: a
    /// raw COM pointer is not something a caller outside this crate should be
    /// holding, and everything they need is on  and .
    pub(crate) fn factory(&self) -> &ComPtr<IPluginFactory> {
        &self.factory
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Every class the factory exports.
    ///
    /// **Asked twice, and the second question is optional.** `getClassInfo`
    /// answers on every factory ever shipped and does not say whether a class
    /// is a synth or a reverb; `getClassInfo2` says, and exists only on
    /// `IPluginFactory2`. So the cheap one is authoritative for identity and
    /// the richer one is consulted for kind when it is there — a factory that
    /// is too old simply leaves `sub_categories` empty, which reads as
    /// "unknown" and behaves exactly as this app did before.
    pub fn classes(&self) -> Vec<ClassInfo> {
        // SAFETY: the factory is a live COM pointer for the module's lifetime.
        let count = unsafe { self.factory.countClasses() };
        // Queried once for the whole enumeration rather than per class: it is a
        // `QueryInterface` and the answer cannot change between two classes of
        // one factory.
        let richer = self.factory.cast::<IPluginFactory2>();
        let mut out = Vec::with_capacity(count.max(0) as usize);
        for i in 0..count {
            let mut info = PClassInfo {
                cid: [0; 16],
                cardinality: 0,
                category: [0; 32],
                name: [0; 64],
            };
            // SAFETY: `i` is in range and `info` is a valid out-parameter.
            let result = unsafe { self.factory.getClassInfo(i, &mut info) };
            if result != vst3::Steinberg::kResultOk {
                continue;
            }
            let sub_categories = richer
                .as_ref()
                .and_then(|f| {
                    let mut two = PClassInfo2 {
                        cid: [0; 16],
                        cardinality: 0,
                        category: [0; 32],
                        name: [0; 64],
                        classFlags: 0,
                        subCategories: [0; 128],
                        vendor: [0; 64],
                        version: [0; 64],
                        sdkVersion: [0; 64],
                    };
                    // SAFETY: same index, and `two` is a valid out-parameter.
                    // A factory that implements the interface but refuses the
                    // call is treated as one that never had it.
                    let ok = unsafe { f.getClassInfo2(i, &mut two) };
                    (ok == vst3::Steinberg::kResultOk)
                        .then(|| c_array_to_string(&two.subCategories))
                })
                .unwrap_or_default();
            out.push(ClassInfo {
                name: c_array_to_string(&info.name),
                category: c_array_to_string(&info.category),
                sub_categories,
                // A plain reinterpretation of sixteen bytes whose only
                // difference is signedness — and `c_char` rather than `i8`
                // because that signedness is the thing that varies by target.
                cid: unsafe {
                    std::mem::transmute::<[std::os::raw::c_char; 16], [u8; 16]>(info.cid)
                },
            });
        }
        out
    }

    /// The audio-processing classes, which is what an instrument is.
    pub fn audio_modules(&self) -> Vec<ClassInfo> {
        self.classes().into_iter().filter(ClassInfo::is_audio_module).collect()
    }
}

/// Fixed-size C string fields in the SDK's structs are `char8` (i8) arrays that
/// are NOT guaranteed to be NUL-terminated when full.
/// A NUL-terminated C string out of a fixed-size field.
///
/// `&[c_char]` and not `&[i8]`: `char` is signed on x86 and unsigned on
/// aarch64, so an `i8` signature type-checks on half the targets and fails to
/// compile on the other half. The bytes are the same either way.
fn c_array_to_string(raw: &[std::os::raw::c_char]) -> String {
    let bytes: Vec<u8> = raw.iter().map(|c| *c as u8).collect();
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

// ── The platform loader ──────────────────────────────────────────────────────

/// A dynamically loaded module, with the macOS bundle dance done correctly.
struct Library {
    handle: *mut c_void,
    #[cfg(target_os = "macos")]
    bundle: *mut c_void,
    /// Whether `bundleEntry` succeeded and `bundleExit` is therefore owed.
    entered: bool,
}

impl Library {
    fn open(binary: &Path, #[allow(unused)] bundle_path: &Path) -> Result<Self, String> {
        let c_path = CString::new(binary.as_os_str().as_encoded_bytes())
            .map_err(|_| "path contains a NUL".to_string())?;

        // SAFETY: `c_path` is a valid NUL-terminated path. RTLD_LOCAL keeps the
        // plugin's symbols out of the global namespace, which matters when two
        // plugins built against different versions of the same framework are
        // loaded at once — the classic cause of "loading plugin B breaks plugin
        // A" in every host that gets this wrong.
        let handle = load(&c_path, binary)?;

        let mut lib = Self {
            handle,
            #[cfg(target_os = "macos")]
            bundle: std::ptr::null_mut(),
            entered: false,
        };

        #[cfg(target_os = "macos")]
        {
            // bundleEntry takes the CFBundleRef of the .vst3 itself. Plugins use
            // it to find their resources; the modelled piano and Arturia both do.
            let cf_bundle = unsafe { cfbundle_for(bundle_path) }?;
            lib.bundle = cf_bundle;
            // SAFETY: freshly dlopen'd handle; the symbol is optional in the
            // sense that a badly-built bundle may lack it, which is an error.
            let entry: unsafe extern "system" fn(*mut c_void) -> bool =
                unsafe { lib.symbol(b"bundleEntry\0") }
                    .ok_or_else(|| "bundle exports no bundleEntry".to_string())?;
            // SAFETY: handing the plugin its own CFBundleRef, which is what the
            // ABI asks for.
            if !unsafe { entry(cf_bundle) } {
                return Err("bundleEntry returned false".to_string());
            }
            lib.entered = true;
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Windows: InitDll. Linux and FreeBSD: ModuleEntry. All are optional
            // in practice — many plugins omit them — so a missing symbol is not
            // an error, unlike macOS's bundleEntry.
            //
            // FreeBSD deliberately shares the Linux spelling rather than getting
            // an arm of its own: the name is an ELF-platform convention, not a
            // kernel one, and the SDK emits `ModuleEntry` for every ELF target.
            // Note this is the arm whose semantics FreeBSD wants — optional, no
            // error when absent — and NOT macOS's, where a missing `bundleEntry`
            // is a hard failure.
            #[cfg(target_os = "windows")]
            let name: &[u8] = b"InitDll\0";
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            let name: &[u8] = b"ModuleEntry\0";
            // SAFETY: freshly loaded handle.
            if let Some(entry) = unsafe { lib.symbol::<unsafe extern "system" fn() -> bool>(name) }
            {
                // SAFETY: the ABI's initialiser, called once.
                unsafe { entry() };
                lib.entered = true;
            }
        }

        Ok(lib)
    }

    /// # Safety
    /// The caller must name a symbol whose real signature is `T`.
    unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Option<T> {
        debug_assert_eq!(name.last(), Some(&0), "symbol name must be NUL-terminated");
        debug_assert_eq!(
            size_of::<T>(),
            size_of::<*mut c_void>(),
            "T must be pointer-sized"
        );
        let sym = unsafe { find_symbol(self.handle, name.as_ptr().cast()) };
        if sym.is_null() {
            return None;
        }
        Some(unsafe { *(&sym as *const *mut c_void).cast::<T>() })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // DELIBERATELY LEAKED. Calling bundleExit and dlclose is the tidy thing
        // and it is not the safe thing: plugins spawn threads and register
        // atexit handlers in bundleEntry, and unmapping the library from under
        // a thread that is still running is a use-after-free with a stack trace
        // pointing into someone else's code. Every mature host either keeps
        // modules loaded for the process lifetime or unloads them only through
        // a carefully sequenced teardown. A recorder loads a piano once and
        // keeps it; leaking a few hundred KB of mapping is the correct trade.
        //
        // Written as an explicit no-op with this comment rather than by omitting
        // the impl, so that nobody "fixes" the missing cleanup later.
        let _ = self.entered;
    }
}

// ── Loading a shared library, per platform ──────────────────────────────────
//
// Split because `dlopen` does not exist on Windows, and `cargo check` will not
// tell you: a check never links, so this compiled cleanly for the Windows
// target for as long as it existed and failed only at `cargo xwin build` with
// three undefined symbols. That is the argument for building the release
// artifact rather than trusting a check, and it is why `build-cross.sh` has
// both.

/// Open the module. The handle is opaque and only [`find_symbol`] reads it.
#[cfg(not(windows))]
fn load(c_path: &CString, _binary: &Path) -> Result<*mut c_void, String> {
    // Imported here rather than at the top of the file: the Windows loader
    // below has no use for it, and an unconditional import is a warning there.
    use std::ffi::CStr;

    // Minimal dlfcn bindings. `libloading` would do this too, but it is a
    // dependency for three symbols and it does not know about CFBundle.
    const RTLD_NOW: i32 = 2;
    #[cfg(target_os = "macos")]
    const RTLD_LOCAL: i32 = 4;
    #[cfg(not(target_os = "macos"))]
    const RTLD_LOCAL: i32 = 0;

    // `c_char`, not `i8`: these are C's own signatures, and C's `char` is
    // signed on x86 and unsigned on aarch64. Declaring them `i8` makes the
    // extern block itself fail to compile on ARM Linux.
    extern "C" {
        #[link_name = "dlopen"]
        fn libc_dlopen(path: *const std::os::raw::c_char, flags: i32) -> *mut c_void;
        #[link_name = "dlerror"]
        fn libc_dlerror() -> *const std::os::raw::c_char;
    }

    // SAFETY: `c_path` is a NUL-terminated path that outlives the call.
    let handle = unsafe { libc_dlopen(c_path.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
    if handle.is_null() {
        // SAFETY: dlerror is valid immediately after a failed dlopen.
        let err = unsafe {
            let e = libc_dlerror();
            if e.is_null() {
                "unknown error".to_string()
            } else {
                CStr::from_ptr(e).to_string_lossy().into_owned()
            }
        };
        return Err(format!("dlopen failed: {err}"));
    }
    Ok(handle)
}

/// `LoadLibraryW`, and the path has to be UTF-16.
///
/// The `W` form rather than `A`: plugin directories live under the user's
/// profile, and a user whose name is not representable in the system code page
/// cannot load a plugin at all through the ANSI entry point.
#[cfg(windows)]
fn load(_c_path: &CString, binary: &Path) -> Result<*mut c_void, String> {
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn LoadLibraryExW(
            path: *const u16,
            file: *mut c_void,
            flags: u32,
        ) -> *mut c_void;
        fn GetLastError() -> u32;
    }
    /// The plugin's own folder joins the DLL search: bundle-form plugins
    /// ship their dependencies BESIDE the binary, and the default search
    /// path never looks there — ERROR_MOD_NOT_FOUND on a file that
    /// plainly exists.
    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;

    let mut wide: Vec<u16> = binary.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err("path contains a NUL".to_string());
    }
    wide.push(0);
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let handle = unsafe {
        LoadLibraryExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            LOAD_WITH_ALTERED_SEARCH_PATH,
        )
    };
    if handle.is_null() {
        // SAFETY: no FFI call happens between the failure and this read.
        let code = unsafe { GetLastError() };
        // 126 is ERROR_MOD_NOT_FOUND, which on a plugin almost always means a
        // DEPENDENCY is missing rather than the plugin itself — the message
        // says so because "module not found" pointing at a file that plainly
        // exists is the most confusing error in Windows plugin hosting.
        let hint = if code == 126 {
            " (the plugin or one of its dependent DLLs could not be found)"
        } else {
            ""
        };
        return Err(format!("LoadLibrary failed: error {code}{hint}"));
    }
    Ok(handle)
}

/// Look up an exported symbol. `null` when it is absent.
///
/// # Safety
/// `handle` must be a live handle from [`load`] and `name` a NUL-terminated
/// symbol name.
#[cfg(not(windows))]
unsafe fn find_symbol(handle: *mut c_void, name: *const i8) -> *mut c_void {
    extern "C" {
        #[link_name = "dlsym"]
        fn libc_dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
    }
    // SAFETY: the caller's contract is this function's.
    unsafe { libc_dlsym(handle, name) }
}

/// # Safety
/// As above.
#[cfg(windows)]
unsafe fn find_symbol(handle: *mut c_void, name: *const i8) -> *mut c_void {
    extern "system" {
        fn GetProcAddress(module: *mut c_void, name: *const i8) -> *mut c_void;
    }
    // SAFETY: the caller's contract is this function's.
    unsafe { GetProcAddress(handle, name) }
}

#[cfg(target_os = "macos")]
mod cf {
    use std::ffi::c_void;
    #[repr(C)]
    pub struct __CFString(c_void);
    // The framework must be named explicitly. Rust links libSystem but not
    // CoreFoundation, and the failure is a linker error naming `_CFBundleCreate`
    // rather than anything that points at a missing `#[link]`.
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub static kCFAllocatorDefault: *const c_void;
        pub fn CFStringCreateWithBytes(
            alloc: *const c_void,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external: bool,
        ) -> *mut __CFString;
        pub fn CFURLCreateWithFileSystemPath(
            alloc: *const c_void,
            path: *mut __CFString,
            style: isize,
            is_directory: bool,
        ) -> *mut c_void;
        pub fn CFBundleCreate(alloc: *const c_void, url: *mut c_void) -> *mut c_void;
        pub fn CFRelease(cf: *const c_void);
    }
    pub const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    pub const K_CF_URL_POSIX_PATH_STYLE: isize = 0;
}

/// # Safety
/// `path` must be a real directory on disk.
#[cfg(target_os = "macos")]
unsafe fn cfbundle_for(path: &Path) -> Result<*mut c_void, String> {
    use cf::*;
    let bytes = path.as_os_str().as_encoded_bytes();
    // SAFETY: `bytes` is a valid UTF-8 byte range for the lifetime of the call.
    let s = unsafe {
        CFStringCreateWithBytes(
            kCFAllocatorDefault,
            bytes.as_ptr(),
            bytes.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            false,
        )
    };
    if s.is_null() {
        return Err("could not make a CFString from the bundle path".into());
    }
    // SAFETY: `s` is a live CFString; the URL takes its own reference.
    let url = unsafe {
        CFURLCreateWithFileSystemPath(kCFAllocatorDefault, s, K_CF_URL_POSIX_PATH_STYLE, true)
    };
    // SAFETY: we own `s` and are done with it.
    unsafe { CFRelease(s.cast()) };
    if url.is_null() {
        return Err("could not make a CFURL from the bundle path".into());
    }
    // SAFETY: `url` is live.
    let bundle = unsafe { CFBundleCreate(kCFAllocatorDefault, url) };
    // SAFETY: we own `url` and are done with it.
    unsafe { CFRelease(url) };
    if bundle.is_null() {
        return Err("CFBundleCreate failed".into());
    }
    // Intentionally NOT released: the plugin holds it for its lifetime, and the
    // module is never unloaded (see Library::drop).
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_paths_are_the_standard_ones() {
        let paths = search_paths();
        assert!(!paths.is_empty());
        #[cfg(target_os = "macos")]
        assert!(
            paths.iter().any(|p| p.ends_with("Library/Audio/Plug-Ins/VST3")),
            "the SDK's standard macOS location must be searched: {paths:?}"
        );
    }

    #[test]
    fn user_paths_come_before_system_paths() {
        // A user's own build of a plugin must shadow a system install, which is
        // what every host does and what anyone debugging a plugin expects.
        let paths = search_paths();
        if paths.len() >= 2 {
            let first = paths[0].to_string_lossy().to_string();
            assert!(
                first.contains("Users") || first.contains("HOME") || first.starts_with('/'),
                "unexpected first search path: {first}"
            );
        }
    }

    #[test]
    fn a_missing_bundle_is_an_error_not_a_panic() {
        let err = Module::open(Path::new("/nonexistent/Nope.vst3")).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn a_directory_that_is_not_a_bundle_is_an_error_not_a_panic() {
        let err = Module::open(Path::new("/tmp")).unwrap_err();
        assert!(err.contains("no loadable binary"), "{err}");
    }

    #[test]
    fn c_arrays_stop_at_the_nul_and_survive_a_full_field() {
        assert_eq!(c_array_to_string(&[0x41, 0x42, 0, 0x43]), "AB");
        assert_eq!(c_array_to_string(&[0x41; 4]), "AAAA", "unterminated is fine");
        assert_eq!(c_array_to_string(&[]), "");
        assert_eq!(c_array_to_string(&[0x20, 0x41, 0x20, 0]), "A", "trimmed");
    }

    #[test]
    fn an_audio_module_is_distinguished_from_a_controller() {
        let processor = ClassInfo {
            name: "piano".into(),
            category: "Audio Module Class".into(),
            sub_categories: "Instrument|Piano".into(),
            cid: [0; 16],
        };
        let controller = ClassInfo {
            name: "the modelled piano Controller".into(),
            category: "Component Controller Class".into(),
            sub_categories: String::new(),
            cid: [0; 16],
        };
        assert!(processor.is_audio_module());
        assert!(!controller.is_audio_module());
    }

    fn class_of(sub: &str) -> ClassInfo {
        ClassInfo {
            name: "Something".into(),
            category: "Audio Module Class".into(),
            sub_categories: sub.into(),
            cid: [0; 16],
        }
    }

    /// **A synth and a reverb are both "Audio Module Class".**
    ///
    /// Which is why loading Pro-R into an instrument slot produced silence and
    /// no explanation: nothing the app could see said the two were different.
    /// `subCategories` says, and this is the reading of it.
    #[test]
    fn a_plugin_says_whether_it_plays_notes_or_changes_them() {
        use Kind::*;
        for (sub, want) in [
            ("Instrument|Synth", Instrument),
            ("Instrument", Instrument),
            ("Fx|Reverb", Effect),
            ("Fx|Delay|Stereo", Effect),
            // **Instrument wins a tie.** Samplers with an audio input declare
            // both, and they genuinely do play notes: refusing one would be a
            // regression wearing a fix's clothes.
            ("Fx|Instrument", Instrument),
            ("Instrument|Fx", Instrument),
            // Case and spacing are a plugin's own business.
            ("fx|reverb", Effect),
            (" Fx | Reverb ", Effect),
            // **Silence is not a no.** A factory too old to be asked tells us
            // nothing, and nothing must not read as "not an instrument" — that
            // would refuse to load working plugins that have always worked.
            ("", Unknown),
            ("Spatial|Ambisonics", Unknown),
        ] {
            assert_eq!(
                class_of(sub).kind(),
                want,
                "{sub:?} was read as the wrong kind"
            );
        }
    }

    /// **The scan finds what is actually installed, not what is at the top.**
    ///
    /// The SDK allows vendor and category folders under the VST3 directory and
    /// every large installer uses them. A one-level scan found 112 of the 160
    /// plugins on the machine this was written on and said nothing about the
    /// other 48 — they simply were not offered, which reads as "this host does
    /// not support my plugin" rather than as a bug.
    ///
    /// Also pins the three things that make a recursive scan safe: it stops at
    /// `MAX_DEPTH`, it does not walk INTO a bundle, and a folder reachable two
    /// ways is listed once.
    #[test]
    fn a_single_file_vst3_is_found_and_is_its_own_binary() {
        // Windows reaplugs ship PLAIN .vst3 DLLs, no bundle folder. The
        // walk and the loader both take that form.
        let dir = std::env::temp_dir().join(format!("ivory-flat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("reaeq.vst3"), b"not really a dll").unwrap();
        std::fs::write(dir.join("REACOMP.VST3"), b"loud case").unwrap();
        std::fs::write(dir.join("readme.txt"), b"noise").unwrap();
        let report = discover_reported(&[dir.clone()]);
        // Only OUR root's finds: the report also walks this machine's real
        // plugin folders, which is the point of the report and noise here.
        let ours: Vec<_> = report
            .found
            .iter()
            .filter(|f| f.starts_with(&dir))
            .cloned()
            .collect();
        assert_eq!(ours.len(), 2, "flat .vst3 files missed: {ours:?}");
        for f in &ours {
            assert_eq!(
                binary_in_bundle(f).as_deref(),
                Some(f.as_path()),
                "a plain file is its own binary"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bundle's binary is chosen by NAME, never by directory order —
    /// Windows bundles ship .pdb and resource DLLs beside the plugin.
    #[test]
    fn the_bundle_binary_beats_its_neighbours() {
        let dir = std::env::temp_dir().join(format!("ivory-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bundle = dir.join("Foo.vst3");
        let inner = bundle.join("Contents").join(CONTENTS_SUBDIR);
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("aaa-helper.dll"), b"x").unwrap();
        std::fs::write(inner.join("Foo.pdb"), b"x").unwrap();
        std::fs::write(inner.join("moduleinfo.json"), b"x").unwrap();
        std::fs::write(inner.join("Foo.vst3"), b"x").unwrap();
        let got = binary_in_bundle(&bundle).expect("no binary found");
        assert_eq!(
            got.file_name().and_then(|n| n.to_str()),
            Some("Foo.vst3"),
            "picked {got:?} over the bundle's own name"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_scan_finds_nested_bundles_without_walking_the_whole_disk() {
        let root = std::env::temp_dir().join(format!("tangent-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mk = |p: &Path| std::fs::create_dir_all(p).expect("mkdir");

        // One at the top, one under a vendor folder, one under a vendor AND a
        // category folder — the three shapes that exist in the wild.
        mk(&root.join("Top.vst3/Contents"));
        mk(&root.join("Vendor/Nested.vst3/Contents"));
        mk(&root.join("Vendor/Category/Deep.vst3/Contents"));
        // Past the depth limit: present on disk, deliberately not found.
        mk(&root.join("a/b/c/d/TooDeep.vst3/Contents"));
        // A directory that is not a bundle and holds nothing.
        mk(&root.join("Vendor/Presets"));
        // And something inside a bundle that looks exactly like a bundle: the
        // walk must not descend into a `.vst3` at all.
        mk(&root.join("Top.vst3/Contents/Inner.vst3"));

        let found = discover_in(&[root.clone()]);
        let names: Vec<String> = found
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();

        assert!(names.contains(&"Top.vst3".to_owned()), "{names:?}");
        assert!(names.contains(&"Nested.vst3".to_owned()), "{names:?}");
        assert!(names.contains(&"Deep.vst3".to_owned()), "{names:?}");
        assert!(
            !names.contains(&"Inner.vst3".to_owned()),
            "the walk went inside a bundle: {names:?}"
        );
        assert!(
            !names.contains(&"TooDeep.vst3".to_owned()),
            "the walk went past MAX_DEPTH: {names:?}"
        );

        // The same folder twice is not the same plugin twice. This is the case
        // a symlinked plugin directory produces, and a picker with every
        // instrument listed twice is worse than one missing them.
        let twice = discover_in(&[root.clone(), root.clone()]);
        assert_eq!(twice.len(), found.len(), "duplicates: {twice:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A search path that does not exist is the normal case — most machines
    /// have never had most vendors' installers run — and it must not be an
    /// error, a panic, or a reason to stop looking in the others.
    #[test]
    fn a_missing_folder_is_skipped_rather_than_fatal() {
        let nowhere = std::env::temp_dir().join(format!("tangent-scan-does-not-exist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&nowhere);
        let found = discover_in(&[nowhere]);
        // Whatever is really installed on the machine running this, plus
        // nothing from the folder that is not there.
        assert_eq!(found, discover());
    }

    /// **One `bundleEntry` per bundle, however many bays hold the plugin.**
    ///
    /// Two lanes running the same drum plugin is the ordinary case and it used
    /// to open the bundle twice. macOS requires `bundleEntry(CFBundleRef)`
    /// before the factory is usable, plugins are entitled to do real work in
    /// it — this module's header names the modelled piano and Arturia, which find their
    /// resources there, and drum samplers commonly start a shared streaming
    /// engine — and the SDK's own `ModuleHandle` refcounts per path for
    /// exactly this reason. Re-running that initialiser underneath a live
    /// instance is undefined at the plugin's discretion.
    ///
    /// Measured with the stub below before `module_for` existed: two opens,
    /// two `bundleEntry` calls. This asserts one.
    ///
    /// The stub is built here rather than committed because a checked-in
    /// dynamic library is a binary nobody can review. If there is no C
    /// compiler the test says so LOUDLY and returns — a silent skip is a green
    /// tick for nothing, which this repo has been bitten by once already.
    #[cfg(unix)]
    #[test]
    fn a_bundle_is_initialised_once_however_many_bays_hold_it() {
        use std::io::Write;

        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
        if std::process::Command::new(&cc)
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!(
                "SKIPPED a_bundle_is_initialised_once_however_many_bays_hold_it: \
                 no C compiler ({cc}); it builds a stub .vst3 to count bundleEntry"
            );
            return;
        }

        // PID in the path: 47 scratch dirs in this repo were once built from a
        // tag alone, and two concurrent `cargo test` runs deleted each other's
        // trees mid-run. That is what the "flaky" record tests actually were.
        let root = std::env::temp_dir().join(format!("tangent-stub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let inner = root.join("StubCount.vst3").join("Contents").join(CONTENTS_SUBDIR);
        std::fs::create_dir_all(&inner).expect("make the stub bundle");
        let log = root.join("entries.txt");

        let src = root.join("stub.c");
        let mut f = std::fs::File::create(&src).expect("write the stub source");
        f.write_all(STUB_C.as_bytes()).expect("write the stub source");
        drop(f);

        // The binary is named exactly like the bundle, which `binary_in_bundle`
        // prefers first on every platform.
        let out = inner.join("StubCount.vst3");
        let built = std::process::Command::new(&cc)
            .args(["-shared", "-fPIC", "-o"])
            .arg(&out)
            .arg(&src)
            .output()
            .expect("run the compiler");
        assert!(
            built.status.success(),
            "could not build the stub: {}",
            String::from_utf8_lossy(&built.stderr)
        );

        // The stub writes one line per entry point call to $TANGENT_STUB_LOG.
        std::env::set_var("TANGENT_STUB_LOG", &log);
        let bundle = root.join("StubCount.vst3");
        let a = module_for(&bundle).expect("the stub bundle should open");
        let b = module_for(&bundle).expect("the stub bundle should open twice");

        assert!(
            std::sync::Arc::ptr_eq(&a, &b),
            "two bays got two different Modules for one bundle"
        );
        let entries = std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter(|l| *l == "entry")
            .count();
        assert_eq!(
            entries, 1,
            "the bundle was initialised {entries} times for two bays; \
             a plugin is entitled to do real work in bundleEntry"
        );

        // A path spelled differently is the same bundle, not a second one.
        let spelled = root.join(".").join("StubCount.vst3");
        let c = module_for(&spelled).expect("the same bundle by another name");
        assert!(
            std::sync::Arc::ptr_eq(&a, &c),
            "a different spelling of one path opened it again"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The counting stub: a minimal VST3 whose only job is to say how often it
    /// was initialised. `GetPluginFactory` returns a real enough
    /// `IPluginFactory` that `Module::open` succeeds — `getFactoryInfo` is the
    /// only method it calls.
    #[cfg(unix)]
    const STUB_C: &str = r#"
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void note(const char *what) {
    const char *p = getenv("TANGENT_STUB_LOG");
    if (!p) return;
    FILE *f = fopen(p, "a");
    if (!f) return;
    fprintf(f, "%s\n", what);
    fclose(f);
}
typedef struct { char vendor[64]; char url[256]; char email[128]; int32_t flags; } PFactoryInfo;
typedef struct Factory Factory;
typedef struct {
    int32_t  (*queryInterface)(Factory*, const void*, void**);
    uint32_t (*addRef)(Factory*);
    uint32_t (*release)(Factory*);
    int32_t  (*getFactoryInfo)(Factory*, PFactoryInfo*);
    int32_t  (*countClasses)(Factory*);
    int32_t  (*getClassInfo)(Factory*, int32_t, void*);
    int32_t  (*createInstance)(Factory*, const char*, const char*, void**);
} FactoryVtbl;
struct Factory { const FactoryVtbl* vtbl; };
static int32_t  f_query(Factory* s, const void* i, void** o) { (void)s;(void)i; *o = 0; return -1; }
static uint32_t f_addref(Factory* s) { (void)s; return 2; }
static uint32_t f_release(Factory* s) { (void)s; return 1; }
static int32_t  f_info(Factory* s, PFactoryInfo* i) {
    (void)s; memset(i, 0, sizeof(*i));
    strcpy(i->vendor, "Tangent Stub");
    return 0;
}
static int32_t  f_count(Factory* s) { (void)s; return 0; }
static int32_t  f_class(Factory* s, int32_t n, void* i) { (void)s;(void)n;(void)i; return -1; }
static int32_t  f_create(Factory* s, const char* c, const char* i, void** o) { (void)s;(void)c;(void)i; *o=0; return -1; }
static const FactoryVtbl VTBL = { f_query, f_addref, f_release, f_info, f_count, f_class, f_create };
static Factory FACTORY = { &VTBL };
__attribute__((visibility("default"))) bool bundleEntry(void *b) { (void)b; note("entry"); return true; }
__attribute__((visibility("default"))) bool bundleExit(void) { note("exit"); return true; }
__attribute__((visibility("default"))) bool ModuleEntry(void *h) { (void)h; note("entry"); return true; }
__attribute__((visibility("default"))) bool ModuleExit(void) { note("exit"); return true; }
__attribute__((visibility("default"))) void *GetPluginFactory(void) { return &FACTORY; }
"#;
}
