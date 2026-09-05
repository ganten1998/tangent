#import <AppKit/AppKit.h>
@interface Del : NSObject <NSOpenSavePanelDelegate> @property (strong) NSDate *opened; @end
@implementation Del
- (void)panel:(id)sender didChangeToDirectoryURL:(NSURL *)url {
    printf("%6.0f ms  delegate: didChangeToDirectoryURL %s\n", -[self.opened timeIntervalSinceNow]*1000, url.path.UTF8String); fflush(stdout); }
@end
int main(int argc, char **argv) { @autoreleasepool {
    NSString *target = @(argv[1]); NSString *tag = @(argv[2]);
    NSApplication *app = [NSApplication sharedApplication]; [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(80, 80, 1400, 900) styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
    [w setTitle:@"open probe"]; [w makeKeyAndOrderFront:nil]; [app activateIgnoringOtherApps:YES];
    NSOpenPanel *p = [NSOpenPanel openPanel];
    p.canChooseDirectories = YES; p.canChooseFiles = NO; p.canCreateDirectories = YES; p.prompt = @"Save"; p.message = @"open-panel probe - closes itself";
    Del *d = [Del new]; d.opened = [NSDate date]; p.delegate = d;
    // an accessory view with a name field, the shape a save-by-open-panel would take
    NSView *acc = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 420, 40)];
    NSTextField *label = [NSTextField labelWithString:@"Save As:"]; label.frame = NSMakeRect(10, 10, 70, 22);
    NSTextField *field = [[NSTextField alloc] initWithFrame:NSMakeRect(85, 8, 320, 24)]; field.stringValue = @"probe";
    [acc addSubview:label]; [acc addSubview:field]; p.accessoryView = acc; p.accessoryViewDisclosed = YES;
    [p setDirectoryURL:[NSURL fileURLWithPath:target isDirectory:YES]];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 1600 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
        NSRect f = w.frame; CGFloat H = [NSScreen mainScreen].frame.size.height;
        system([NSString stringWithFormat:@"screencapture -x -T0 -R %.0f,%.0f,%.0f,%.0f '%@-open.png'", f.origin.x, H-(f.origin.y+f.size.height), f.size.width, f.size.height+60, tag].UTF8String);
        printf("%6.0f ms  property %s (shot)\n", -[d.opened timeIntervalSinceNow]*1000, p.directoryURL.path.UTF8String); fflush(stdout); });
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 2400 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{ [p cancel:nil]; });
    [p beginSheetModalForWindow:w completionHandler:^(NSModalResponse r) {}];
    [p runModal];
} return 0; }
