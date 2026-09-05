#import <AppKit/AppKit.h>
static NSData *bm(NSString *path) {
    NSURL *u = [NSURL fileURLWithPath:path isDirectory:YES];
    NSError *e = nil;
    NSData *d = [u bookmarkDataWithOptions:0 includingResourceValuesForKeys:nil relativeToURL:nil error:&e];
    if (!d) NSLog(@"bookmark failed for %@: %@", path, e);
    return d;
}
int main(int argc, char **argv) {
    @autoreleasepool {
        NSString *mode = argc > 1 ? @(argv[1]) : @"home";
        NSString *target = argc > 2 ? @(argv[2])
            : [NSHomeDirectory() stringByAppendingPathComponent:@"Documents/Tangent Media/Projects"];
        NSApplication *app = [NSApplication sharedApplication];
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        NSUserDefaults *d = [NSUserDefaults standardUserDefaults];
        if ([mode isEqualToString:@"home"]) [d setObject:bm(NSHomeDirectory()) forKey:@"NSOSPLastRootDirectory"];
        else if ([mode isEqualToString:@"steer"]) [d setObject:bm(target) forKey:@"NSOSPLastRootDirectory"];
        else [d removeObjectForKey:@"NSOSPLastRootDirectory"];
        [d synchronize];
        NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(300, 300, 520, 320)
                                                  styleMask:NSWindowStyleMaskTitled
                                                    backing:NSBackingStoreBuffered defer:NO];
        [w setTitle:@"panel probe"];
        [w makeKeyAndOrderFront:nil];
        [app activateIgnoringOtherApps:YES];
        NSSavePanel *p = [NSSavePanel savePanel];
        [p setDirectoryURL:[NSURL fileURLWithPath:target isDirectory:YES]];
        [p setNameFieldStringValue:@"probe"];
        [p setMessage:@"panel probe - closes itself"];
        printf("%s: before show %s\n", mode.UTF8String, p.directoryURL.path.UTF8String);
        BOOL reassert = argc > 3;
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 1500 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
            NSString *now = p.directoryURL.path;
            printf("%s: AFTER show  %s  %s\n", mode.UTF8String, now.UTF8String,
                   [now isEqualToString:target] ? "HELD" : "*** MOVED ***");
            fflush(stdout);
            if (!reassert) { [p cancel:nil]; return; }
            [p setDirectoryURL:[NSURL fileURLWithPath:target isDirectory:YES]];
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 1500 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                NSString *later = p.directoryURL.path;
                printf("%s: REASSERTED  %s  %s\n", mode.UTF8String, later.UTF8String,
                       [later isEqualToString:target] ? "HELD" : "*** MOVED ***");
                fflush(stdout);
                [p cancel:nil];
            });
        });
        [p beginSheetModalForWindow:w completionHandler:^(NSModalResponse r) {}];
        [p runModal];
    }
    return 0;
}
