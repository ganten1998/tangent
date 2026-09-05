use core::str;
use std::path::Path;
use std::path::PathBuf;

use block2::Block;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSModalResponse, NSOpenPanel, NSOpenSavePanelDelegate, NSSavePanel, NSWindow, NSWindowLevel,
};
use objc2_foundation::{NSArray, NSString, NSURL};
use raw_window_handle::RawWindowHandle;

use super::super::{
    modal_future::{AsModal, InnerModal},
    utils::{FocusManager, PolicyManager},
};
use crate::backend::macos::utils::window_from_raw_window_handle;
use crate::FileDialog;

extern "C" {
    pub fn CGShieldingWindowLevel() -> i32;
}

pub struct Panel {
    // Either `NSSavePanel` or the subclass `NSOpenPanel`
    pub(crate) panel: Retained<NSSavePanel>,
    parent: Option<Retained<NSWindow>>,
    _focus_manager: FocusManager,
    _policy_manager: PolicyManager,
    /// TANGENT: the save panel's delegate, which is a WEAK property on the
    /// panel and so has to be owned here for as long as the panel is. See
    /// [`Steer`].
    _steer: Option<Retained<Steer>>,
}

impl AsModal for Panel {
    fn inner_modal(&self) -> &(impl InnerModal + 'static) {
        &*self.panel
    }
}

impl InnerModal for NSSavePanel {
    fn begin_modal(&self, window: &NSWindow, handler: &Block<dyn Fn(NSModalResponse)>) {
        unsafe { self.beginSheetModalForWindow_completionHandler(window, handler) }
    }

    fn run_modal(&self) -> NSModalResponse {
        unsafe { self.runModal() }
    }
}

/// TANGENT: an AppKit pair carried to the MAIN QUEUE and unwrapped there.
///
/// `Retained` is deliberately not `Send`, and rightly: these are main-thread
/// objects. The only thing this crosses is a `DispatchQueue::main().after`
/// block, which runs ON the main thread — the thread both objects were made
/// on and belong to. Nothing here is touched anywhere else. See
/// `Panel::hold_directory`.
struct OnMain(Retained<NSSavePanel>, Retained<NSURL>);

// SAFETY: only ever unwrapped inside a block dispatched to the main queue.
// Both halves: the queue's own bound asks for a closure that is `Send`, and
// the closure carrying this by value needs the wrapper to be `Sync` too.
unsafe impl Send for OnMain {}
// SAFETY: as above — never observed from any thread but the main one.
unsafe impl Sync for OnMain {}

/// TANGENT: whether a panel's directory is the one it was given.
///
/// By PATH, because `NSURL` equality is by string and AppKit hands back the
/// same folder with and without a trailing slash.
fn same_dir(now: Option<&NSURL>, wanted: &NSURL) -> bool {
    let path = |u: &NSURL| u.path().map(|p| p.to_string());
    match (now.and_then(path), path(wanted)) {
        (Some(a), Some(b)) => a.trim_end_matches('/') == b.trim_end_matches('/'),
        _ => false,
    }
}

/// TANGENT: what the save panel's delegate remembers.
struct SteerIvars {
    /// The directory the panel was built with.
    wanted: Retained<NSURL>,
    /// How many times the panel has been put back; see [`Steer::MAX`].
    steers: std::cell::Cell<u8>,
    /// When the panel was built; see [`Steer::GRACE`].
    opened: std::time::Instant,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - The class does not implement `Drop`.
    #[unsafe(super(NSObject))]
    // Main-thread-only because `NSOpenSavePanelDelegate` requires it, and
    // every call here is the panel's, on the thread it runs on.
    #[thread_kind = MainThreadOnly]
    // Deliberately NOT `#[name = "..."]`: objc2 derives a runtime name from
    // the module path and the crate version, and registering a fixed name
    // twice in one process is a hard failure. Tangent ships as an app AND a
    // VST3, and a host can load the plug-in into a process that already
    // holds this crate.
    #[ivars = SteerIvars]
    struct Steer;

    unsafe impl NSObjectProtocol for Steer {}

    unsafe impl NSOpenSavePanelDelegate for Steer {
        #[unsafe(method(panel:didChangeToDirectoryURL:))]
        fn did_change_to_directory(&self, sender: &AnyObject, url: Option<&NSURL>) {
            self.steer(sender, url);
        }
    }
);

/// **TANGENT: the SAVE panel's delegate, which puts the panel back on the
/// directory it was given when the panel walks off it by itself.**
///
/// The third attempt at one fault, and the first that is driven by the
/// event rather than by a clock. The fault, measured on the owner's machine
/// on 2026-09-05 with a standalone AppKit program that opens exactly the
/// panel this crate opens (`beginSheetModalForWindow:` then `runModal`) and
/// reads `directoryURL` back:
///
/// ```text
///   ~/Documents                          HELD
///   ~/Desktop                            HELD
///   ~/Music                              HELD
///   /private/tmp/probedir                HELD
///   ~/Documents/Tangent Media/Projects   *** MOVED *** to /Users/<user>
/// ```
///
/// A folder UNDER `~/Documents` is refused and the panel falls back to home
/// — with `NSOSPLastRootDirectory` pointing at home, pointing at the wanted
/// folder, and DELETED, all three. So it is not the remembered location,
/// which is what the previous note believed; it is the panel refusing that
/// directory, silently, and nothing this side of AppKit can say why.
///
/// What the same program did establish:
///
/// * the delegate HEARS it — `panel:didChangeToDirectoryURL:` fires once,
///   with the home directory, before any hand has touched the panel;
/// * `setDirectoryURL:` said again AFTER that call holds, and holds for as
///   long as the panel was watched (three seconds);
/// * and the main queue is BLOCKED during the panel's own setup — every
///   block queued for 10 through 320 ms ran together at 325 ms — which is
///   the timed shots of the previous attempt firing in a bunch at a moment
///   that happened to fall after the move in the probe and, this morning
///   in the app, fell before it: the app logged the right folder at
///   13:28:03Z, the panel opened at home, and the app's own preference
///   domain recorded `/Users/<user>` as where it closed.
///
/// So the panel is asked to say when it moves, and when the move is not to
/// the wanted directory it is put back — from a main-queue block rather
/// than inside the callback, and 30 ms on, the margin the first measurement
/// found. [`Self::MAX`] and [`Self::GRACE`] are what keep this from ever
/// fighting a hand: two repairs at most, and none after three seconds,
/// when whatever is changing the directory is the user. The timed shots
/// stay as a guarded fallback — see [`Panel::hold_directory`] — and do
/// nothing once the delegate has done its work.
impl Steer {
    /// Repairs per panel. One is the measured need; the second is for a
    /// panel that refuses the first re-assert too.
    const MAX: u8 = 2;
    /// No repairs after this: a change this late is a hand navigating.
    const GRACE: std::time::Duration = std::time::Duration::from_secs(3);
    /// After the callback returns, and past the margin measured to hold.
    const SETTLE_MS: u64 = 30;

    fn new(mtm: MainThreadMarker, wanted: Retained<NSURL>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SteerIvars {
            wanted,
            steers: std::cell::Cell::new(0),
            opened: std::time::Instant::now(),
        });
        // SAFETY: `init` on a freshly allocated NSObject subclass whose ivars
        // have been set, which is exactly the initialiser contract.
        unsafe { msg_send![super(this), init] }
    }

    fn steer(&self, sender: &AnyObject, now: Option<&NSURL>) {
        let iv = self.ivars();
        if same_dir(now, &iv.wanted) {
            return;
        }
        if iv.steers.get() >= Self::MAX || iv.opened.elapsed() > Self::GRACE {
            return;
        }
        let Some(panel) = sender.downcast_ref::<NSSavePanel>() else {
            return;
        };
        iv.steers.set(iv.steers.get() + 1);
        let carry = OnMain(panel.retain(), iv.wanted.clone());
        let when = dispatch2::DispatchTime::try_from(std::time::Duration::from_millis(
            Self::SETTLE_MS,
        ));
        let Ok(when) = when else { return };
        let _ = dispatch2::DispatchQueue::main().after(when, move || {
            // The whole wrapper crosses; see `hold_directory` for why it is
            // named once as itself.
            let carry = carry;
            carry.0.setDirectoryURL(Some(&carry.1));
        });
    }
}

impl Panel {
    fn as_open_panel(&self) -> Option<&NSOpenPanel> {
        self.panel.downcast_ref::<NSOpenPanel>()
    }

    pub fn new(panel: Retained<NSSavePanel>, parent: Option<&RawWindowHandle>) -> Self {
        let _policy_manager = PolicyManager::new(MainThreadMarker::from(&*panel));

        let _focus_manager = FocusManager::new(MainThreadMarker::from(&*panel));

        panel.setLevel(unsafe { CGShieldingWindowLevel() } as NSWindowLevel);
        Self {
            panel,
            parent: parent.map(window_from_raw_window_handle),
            _focus_manager,
            _policy_manager,
            _steer: None,
        }
    }

    /// **TANGENT: put the start directory back, once the panel is up.**
    ///
    /// `NSSavePanel` accepts `setDirectoryURL:` and then MOVES, and this was
    /// measured rather than guessed. A small AppKit program that reads the
    /// panel's own `directoryURL` back — before it is shown and again after —
    /// says, on the owner's machine:
    ///
    /// ```text
    ///     SAVE  before show /Users/x/Documents/Tangent Media/Projects
    ///           AFTER  show /Users/x                        *** MOVED ***
    ///     OPEN  before show /Users/x/Documents/Tangent Media/Projects
    ///           AFTER  show /Users/x/Documents/Tangent Media/Projects  HELD
    /// ```
    ///
    /// So it is not the directory, not the file name, not this crate's
    /// shielding window level, and not the ORDER any of them are set in — all
    /// four were varied and all four moved. `NSOpenPanel` holds the same URL
    /// in the same program.
    ///
    /// **What moves it was misread here once, and the correction is on
    /// [`Steer`].** This note used to say the panel restores the app's
    /// remembered location — `NSOSPLastRootDirectory` — over the one it was
    /// handed. Measured on 2026-09-05: with that key deleted, and with it
    /// pointing at the wanted folder, the panel moved just the same. It is
    /// the panel refusing a folder under `~/Documents`, and the reason is
    /// not visible from this side of AppKit.
    ///
    /// What did hold: saying it again AFTER the move. Measured first as "the
    /// next run-loop turn is too early and 30 ms holds", which was the truth
    /// of a probe whose main queue was blocked by the panel's own setup until
    /// the move had happened; in the app the same shots fired before it and
    /// changed nothing. So the delegate is what listens for the move now,
    /// and these shots are its fallback, GUARDED so that a shot landing on a
    /// panel that already stands where it was told does nothing — otherwise
    /// four shots would be four chances to take a folder away from a hand.
    fn hold_directory(&self, url: Retained<NSURL>) {
        // **On the MAIN QUEUE, not on a run-loop timer.**
        //
        // The first attempt scheduled `NSTimer`s in `NSRunLoopCommonModes`.
        // That holds in a standalone AppKit program — measured, three shots,
        // all three fired — and did NOT hold inside Tangent, where the save
        // panel runs as a sheet nested inside winit's own event loop: the
        // app handed the panel the right folder, logged that it existed, and
        // the panel opened at home anyway. Which run-loop modes are common
        // under a nested loop is not something to reason about from here.
        //
        // The main QUEUE has no such question. It is drained while a modal
        // panel is up regardless of mode, and it is what the original
        // measurement used — the one that established 30 ms is late enough
        // and the next run-loop turn is not.
        //
        // `Retained` is not `Send`, so the pair crosses in a wrapper. It is
        // sound for the reason the wrapper's own note gives: the block runs
        // ON the main thread, which is where these objects live and where
        // they were made, and nothing touches them anywhere else.
        for after in [30_u64, 80, 180, 320] {
            let carry = OnMain(self.panel.clone(), url.clone());
            let when = dispatch2::DispatchTime::try_from(
                std::time::Duration::from_millis(after),
            );
            let Ok(when) = when else { continue };
            let _ = dispatch2::DispatchQueue::main().after(when, move || {
                // **The WHOLE wrapper crosses, not its two fields.** Rust
                // 2021 captures disjoint fields, so destructuring `carry`
                // straight away made the closure hold two `Retained`s and
                // the `Send` on the wrapper applied to nothing. Naming it
                // once as itself is what puts the wrapper in the capture.
                let carry = carry;
                // **Guarded, since the delegate arrived.** A shot that lands
                // while the panel already stands where it was told — the
                // delegate's doing, or the panel never having moved — must
                // say nothing, or four shots become four chances to take a
                // folder away from a hand. See `Steer`.
                let held = carry
                    .0
                    .directoryURL()
                    .is_some_and(|d| same_dir(Some(&d), &carry.1));
                if !held {
                    carry.0.setDirectoryURL(Some(&carry.1));
                }
            });
        }
    }

    pub fn run_modal(&self) -> NSModalResponse {
        if let Some(parent) = self.parent.clone() {
            let completion = block2::StackBlock::new(|_: isize| {});

            unsafe {
                self.panel
                    .beginSheetModalForWindow_completionHandler(&parent, &completion)
            }
        }

        unsafe { self.panel.runModal() }
    }

    pub fn get_result(&self) -> PathBuf {
        unsafe {
            let url = self.panel.URL().unwrap();
            url.path().unwrap().to_string().into()
        }
    }

    pub fn get_results(&self) -> Vec<PathBuf> {
        unsafe {
            let urls = self.as_open_panel().unwrap().URLs();

            let mut res = Vec::new();
            for url in urls {
                res.push(url.path().unwrap().to_string().into());
            }

            res
        }
    }
}

trait PanelExt {
    fn panel(&self) -> &NSSavePanel;

    fn set_can_create_directories(&self, can: bool) {
        unsafe { self.panel().setCanCreateDirectories(can) }
    }

    fn add_filters(&self, opt: &FileDialog) {
        let mut exts: Vec<String> = Vec::new();

        for filter in opt.filters.iter() {
            exts.append(&mut filter.extensions.to_vec());
        }

        let f_raw: Vec<_> = exts.iter().map(|ext| NSString::from_str(ext)).collect();
        let array = NSArray::from_retained_slice(&f_raw);

        unsafe {
            #[allow(deprecated)]
            self.panel().setAllowedFileTypes(Some(&array));
        }
    }

    // TANGENT: the file name is NOT appended to the starting directory.
    //
    // Upstream builds `<dir>/<suggested name>` and hands it to
    // `setDirectoryURL:` with `isDirectory: true`. That path does not exist —
    // it is where the file is ABOUT to be written — so AppKit discards the
    // URL and the panel opens wherever NSSavePanel last remembered, which for
    // a fresh user is the home directory. It bites only when a file name is
    // set AND the directory exists, which is exactly a Save panel and not an
    // Open one, and it looks intermittent because the remembered directory is
    // sometimes the right one anyway.
    //
    // The append is also redundant: every builder that passes a `file_name`
    // here calls `set_file_name` two lines later, which is
    // `setNameFieldStringValue:` and is the correct way to fill the name
    // field. So the suggested name still arrives; only the corrupted
    // directory URL is gone.
    //
    // Reported by the owner of this app as save panels "reverted AGAIN to
    // home directory", and settled by a log line proving Tangent handed
    // macOS `~/Documents/Tangent Media/Projects` and that the directory
    // existed.
    fn set_path(&self, path: &Path, _file_name: Option<&str>) {
        let path = path.to_owned();

        if let Some(path) = path.to_str() {
            unsafe {
                let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), true);
                self.panel().setDirectoryURL(Some(&url));
            }
        }
    }

    fn set_file_name(&self, file_name: &str) {
        unsafe {
            self.panel()
                .setNameFieldStringValue(&NSString::from_str(file_name))
        }
    }

    fn set_title(&self, title: &str) {
        unsafe { self.panel().setMessage(Some(&NSString::from_str(title))) }
    }
}

impl PanelExt for Retained<NSSavePanel> {
    fn panel(&self) -> &NSSavePanel {
        &self
    }
}

impl PanelExt for Retained<NSOpenPanel> {
    fn panel(&self) -> &NSSavePanel {
        &self
    }
}

impl Panel {
    pub fn build_pick_file(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if !opt.filters.is_empty() {
            panel.add_filters(opt);
        }

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(file_name) = &opt.file_name {
            panel.set_file_name(file_name);
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        if let Some(can) = opt.can_create_directories {
            panel.set_can_create_directories(can);
        }

        unsafe { panel.setCanChooseDirectories(false) };
        unsafe { panel.setCanChooseFiles(true) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }

    pub fn build_save_file(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSSavePanel::savePanel(mtm) };

        if !opt.filters.is_empty() {
            panel.add_filters(opt);
        }

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(file_name) = &opt.file_name {
            panel.set_file_name(file_name);
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        if let Some(can) = opt.can_create_directories {
            panel.set_can_create_directories(can);
        }

        let mut built = Self::new(panel, opt.parent.as_ref());
        // TANGENT: and it has to be said again after the panel is up. The
        // SAVE panel only — `NSOpenPanel` holds what it is given, so this
        // would be three timers doing nothing there. See `hold_directory`,
        // and `Steer` for the half that listens rather than counts.
        if let Some(path) = &opt.starting_directory {
            if let Some(path) = path.to_str() {
                let url = unsafe {
                    NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), true)
                };
                let steer = Steer::new(mtm, url.clone());
                unsafe { built.panel.setDelegate(Some(ProtocolObject::from_ref(&*steer))) };
                built._steer = Some(steer);
                built.hold_directory(url);
            }
        }
        built
    }

    pub fn build_pick_folder(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        let can = opt.can_create_directories.unwrap_or(true);
        panel.set_can_create_directories(can);

        unsafe { panel.setCanChooseDirectories(true) };
        unsafe { panel.setCanChooseFiles(false) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }

    pub fn build_pick_folders(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        let can = opt.can_create_directories.unwrap_or(true);
        panel.set_can_create_directories(can);

        unsafe { panel.setCanChooseDirectories(true) };
        unsafe { panel.setCanChooseFiles(false) };
        unsafe { panel.setAllowsMultipleSelection(true) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }

    pub fn build_pick_files(opt: &FileDialog, mtm: MainThreadMarker) -> Self {
        let panel = unsafe { NSOpenPanel::openPanel(mtm) };

        if !opt.filters.is_empty() {
            panel.add_filters(opt);
        }

        if let Some(path) = &opt.starting_directory {
            panel.set_path(path, opt.file_name.as_deref());
        }

        if let Some(title) = &opt.title {
            panel.set_title(title);
        }

        if let Some(can) = opt.can_create_directories {
            panel.set_can_create_directories(can);
        }

        unsafe { panel.setCanChooseDirectories(false) };
        unsafe { panel.setCanChooseFiles(true) };
        unsafe { panel.setAllowsMultipleSelection(true) };

        Self::new(Retained::into_super(panel), opt.parent.as_ref())
    }
}
