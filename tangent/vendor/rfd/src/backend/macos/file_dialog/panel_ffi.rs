use core::str;
use std::path::Path;
use std::path::PathBuf;

use block2::Block;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponse, NSOpenPanel, NSSavePanel, NSWindow, NSWindowLevel};
use objc2_foundation::{NSArray, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer, NSURL};
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
    /// in the same program. What differs is that a save panel restores the
    /// app's remembered location — `NSOSPLastRootDirectory`, a bookmark in
    /// the app's own preference domain — over the one it was handed, and for
    /// a user who has never saved anywhere that location is HOME.
    ///
    /// That is why the owner reported this as a per-build wildcard that
    /// "never has been consistently applied": whether it looks broken depends
    /// on what the panel last remembered, so it comes and goes with no code
    /// change at all.
    ///
    /// The remedy is to say it again once the restore has happened. Also
    /// measured: re-asserting on the next run-loop turn is too early and
    /// still moves; 30 ms holds, and so does everything later. Three shots
    /// inside 200 ms covers a slower machine and is far quicker than anyone
    /// can navigate a panel, so it can never take a folder away from a hand
    /// that chose it.
    ///
    /// The timers go into the COMMON modes: a modal panel spins the run loop
    /// in `NSModalPanelRunLoopMode`, and a timer left in the default mode
    /// alone would not fire until the panel had already closed.
    fn hold_directory(&self, url: Retained<NSURL>) {
        for after in [0.03_f64, 0.08, 0.18] {
            let panel = self.panel.clone();
            let url = url.clone();
            let block = block2::RcBlock::new(move |_t: core::ptr::NonNull<NSTimer>| unsafe {
                panel.setDirectoryURL(Some(&url));
            });
            let timer =
                unsafe { NSTimer::timerWithTimeInterval_repeats_block(after, false, &block) };
            unsafe {
                NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
            }
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

        let built = Self::new(panel, opt.parent.as_ref());
        // TANGENT: and it has to be said again after the panel is up. The
        // SAVE panel only — `NSOpenPanel` holds what it is given, so this
        // would be three timers doing nothing there. See `hold_directory`.
        if let Some(path) = &opt.starting_directory {
            if let Some(path) = path.to_str() {
                let url = unsafe {
                    NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), true)
                };
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
