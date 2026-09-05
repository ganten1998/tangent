#import <AppKit/AppKit.h>
// Mirrors Tangent's vendored rfd: delegate repair 30 ms after a move (max 2, 3 s grace),
// guarded shots at 30/80/180/320 ms; then photographs the panel, optionally re-asserts late,
// photographs again, cancels, and reads back where the panel says it closed.
@interface Del : NSObject <NSOpenSavePanelDelegate>
@property (strong) NSURL *wanted; @property int repairs; @property (strong) NSDate *opened;
@end
@implementation Del
- (void)panel:(id)sender didChangeToDirectoryURL:(NSURL *)url {
    double ms = -[self.opened timeIntervalSinceNow] * 1000;
    printf("%6.0f ms  delegate: didChangeToDirectoryURL %s\n", ms, url.path.UTF8String); fflush(stdout);
    if ([url.path isEqualToString:self.wanted.path]) return;
    if (self.repairs >= 2 || ms > 3000) return;
    self.repairs++;
    NSSavePanel *p = sender; NSURL *w = self.wanted; int n = self.repairs;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
        printf("%6.0f ms  repair %d: setDirectoryURL %s\n", -[self.opened timeIntervalSinceNow] * 1000, n, w.path.UTF8String); fflush(stdout);
        [p setDirectoryURL:w];
    });
}
@end
static void shot(NSString *file, NSWindow *w) {
    NSRect f = w.frame; CGFloat H = [NSScreen mainScreen].frame.size.height;
    NSString *cmd = [NSString stringWithFormat:@"screencapture -x -T0 -R %.0f,%.0f,%.0f,%.0f '%@'",
        f.origin.x, H - (f.origin.y + f.size.height), f.size.width, f.size.height + 60, file];
    system(cmd.UTF8String);
    printf("        shot %s\n", file.UTF8String); fflush(stdout);
}
int main(int argc, char **argv) {
    @autoreleasepool {
        NSString *target = argc > 1 ? @(argv[1]) : [NSHomeDirectory() stringByAppendingPathComponent:@"Documents/Tangent Media/Projects"];
        NSString *tag = argc > 2 ? @(argv[2]) : @"run";
        BOOL late = argc > 3;
        NSApplication *app = [NSApplication sharedApplication];
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(80, 80, 1400, 900)
                                                  styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
        [w setTitle:@"panel probe"]; [w makeKeyAndOrderFront:nil]; [app activateIgnoringOtherApps:YES];
        if (getenv("PROBE_EXPANDED")) {
            [[NSUserDefaults standardUserDefaults] setBool:YES forKey:@"NSNavPanelExpandedStateForSaveMode"];
            [[NSUserDefaults standardUserDefaults] setObject:@"{1100, 700}" forKey:@"NSNavPanelExpandedSizeForSaveMode"];
        } else {
            [[NSUserDefaults standardUserDefaults] setBool:NO forKey:@"NSNavPanelExpandedStateForSaveMode"];
        }
        [[NSUserDefaults standardUserDefaults] synchronize];
        NSSavePanel *p = [NSSavePanel savePanel];
        Del *d = [Del new]; d.wanted = [NSURL fileURLWithPath:target isDirectory:YES]; d.repairs = 0; d.opened = [NSDate date];
        p.delegate = d;
        if (getenv("PROBE_LEGACY")) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            [p setDirectory:target];
#pragma clang diagnostic pop
            printf("legacy setDirectory: %s\n", target.UTF8String);
        } else {
            [p setDirectoryURL:d.wanted];
        }
        [p setNameFieldStringValue:@"probe"]; [p setMessage:@"panel probe - closes itself"];
        int shots[] = {30, 80, 180, 320};
        for (int i = 0; i < 4; i++) { int ms = shots[i];
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, ms * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                NSString *at = p.directoryURL.path; BOOL held = [at isEqualToString:target];
                printf("%6.0f ms  shot(%d): property %s %s\n", -[d.opened timeIntervalSinceNow] * 1000, ms, at.UTF8String, held ? "held" : "-> RE-ASSERT"); fflush(stdout);
                if (!held) [p setDirectoryURL:d.wanted];
            }); }
        int reads[] = {100, 200, 400, 800, 1200, 1500};
        for (int i = 0; i < 6; i++) { int ms = reads[i];
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, ms * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                printf("%6.0f ms  property %s\n", -[d.opened timeIntervalSinceNow] * 1000, p.directoryURL.path.UTF8String); fflush(stdout);
            }); }
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 1600 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{ shot([NSString stringWithFormat:@"%@-early.png", tag], w); });
        if (late) {
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 2100 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                printf("%6.0f ms  LATE re-assert %s\n", -[d.opened timeIntervalSinceNow] * 1000, target.UTF8String); fflush(stdout);
                [p setDirectoryURL:d.wanted]; });
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 2600 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                printf("%6.0f ms  property %s\n", -[d.opened timeIntervalSinceNow] * 1000, p.directoryURL.path.UTF8String); fflush(stdout); });
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 3100 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{ shot([NSString stringWithFormat:@"%@-late.png", tag], w); });
        }
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 3600 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
            printf("%6.0f ms  cancel; property %s\n", -[d.opened timeIntervalSinceNow] * 1000, p.directoryURL.path.UTF8String); fflush(stdout);
            [p cancel:nil]; });
        [p beginSheetModalForWindow:w completionHandler:^(NSModalResponse r) {}];
        [p runModal];
        [[NSUserDefaults standardUserDefaults] synchronize];
        NSData *bm = [[NSUserDefaults standardUserDefaults] dataForKey:@"NSOSPLastRootDirectory"];
        BOOL stale = NO; NSError *e = nil;
        NSURL *u = bm ? [NSURL URLByResolvingBookmarkData:bm options:NSURLBookmarkResolutionWithoutUI relativeToURL:nil bookmarkDataIsStale:&stale error:&e] : nil;
        printf("after close: NSOSPLastRootDirectory = %s\n", u ? u.path.UTF8String : "(none)"); fflush(stdout);
    }
    return 0;
}
