#import <AppKit/AppKit.h>
@interface Del : NSObject <NSOpenSavePanelDelegate> @end
@implementation Del
- (void)panel:(id)sender didChangeToDirectoryURL:(NSURL *)url {
    printf("  delegate didChangeToDirectoryURL %s\n", url.path.UTF8String); fflush(stdout);
}
@end
int main(int argc, char **argv) {
    @autoreleasepool {
        NSString *target = [NSHomeDirectory() stringByAppendingPathComponent:@"Documents/Tangent Media/Projects"];
        NSApplication *app = [NSApplication sharedApplication];
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(300, 300, 520, 320)
                                                  styleMask:NSWindowStyleMaskTitled
                                                    backing:NSBackingStoreBuffered defer:NO];
        [w makeKeyAndOrderFront:nil];
        [app activateIgnoringOtherApps:YES];
        NSSavePanel *p = [NSSavePanel savePanel];
        Del *d = [Del new];
        p.delegate = d;
        [p setDirectoryURL:[NSURL fileURLWithPath:target isDirectory:YES]];
        [p setNameFieldStringValue:@"probe"];
        [p setMessage:@"panel timeline probe - closes itself"];
        NSDate *t0 = [NSDate date];
        __block int reasserted = 0;
        for (int ms = 10; ms <= 2600; ms += (ms < 200 ? 10 : (ms < 800 ? 50 : 200))) {
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, ms * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                NSString *now = p.directoryURL.path;
                printf("%5.0f ms  %s\n", -[t0 timeIntervalSinceNow] * 1000.0, now.UTF8String);
                fflush(stdout);
                // one guarded re-assert at ~1000 ms, exactly as the app would do it
                if (ms >= 1000 && reasserted == 0 && ![now isEqualToString:target]) {
                    reasserted = 1;
                    printf("  re-assert (panel sat at %s)\n", now.UTF8String);
                    [p setDirectoryURL:[NSURL fileURLWithPath:target isDirectory:YES]];
                }
            });
        }
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 2900 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{ [p cancel:nil]; });
        [p beginSheetModalForWindow:w completionHandler:^(NSModalResponse r) {}];
        [p runModal];
    }
    return 0;
}
