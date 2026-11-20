// Shared by the host probes. Everything here reads state that needs no
// permission: display list and modes, window owner names/pids/layers/bounds
// (titles would need Screen Recording and are never read), the frontmost
// application, the pointer position.
#pragma once
#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#include <signal.h>
#include <unistd.h>

// The rig's virtual displays carry this vendor number ('mc').
#define RIG_VENDOR 0x6d63

static inline void rig_quiet_exit(int sig) { _exit(128 + sig); }
static inline void rig_install_quiet_handlers(void) {
    int sigs[] = {SIGABRT, SIGSEGV, SIGBUS, SIGILL, SIGTRAP, SIGFPE, SIGTERM, SIGINT};
    for (unsigned i = 0; i < sizeof sigs / sizeof *sigs; i++) signal(sigs[i], rig_quiet_exit);
}

static inline NSArray *rig_rect(CGRect r) { return @[ @(r.origin.x), @(r.origin.y), @(r.size.width), @(r.size.height) ]; }

static inline NSArray<NSDictionary *> *rig_displays(void) {
    uint32_t n = 0;
    CGDirectDisplayID ids[32];
    CGGetOnlineDisplayList(32, ids, &n);
    NSMutableArray *out = [NSMutableArray array];
    for (uint32_t i = 0; i < n; i++) {
        CGDirectDisplayID d = ids[i];
        CGDisplayModeRef m = CGDisplayCopyDisplayMode(d);
        [out addObject:@{
            @"id" : @(d),
            @"bounds" : rig_rect(CGDisplayBounds(d)),
            @"main" : @(CGDisplayIsMain(d) != 0),
            @"builtin" : @(CGDisplayIsBuiltin(d) != 0),
            @"active" : @(CGDisplayIsActive(d) != 0),
            @"asleep" : @(CGDisplayIsAsleep(d) != 0),
            @"mirrorOf" : @(CGDisplayMirrorsDisplay(d)),
            @"inMirrorSet" : @(CGDisplayIsInMirrorSet(d) != 0),
            @"vendor" : @(CGDisplayVendorNumber(d)),
            @"model" : @(CGDisplayModelNumber(d)),
            @"serial" : @(CGDisplaySerialNumber(d)),
            @"mode" : m ? @[ @(CGDisplayModeGetWidth(m)), @(CGDisplayModeGetHeight(m)),
                             @(CGDisplayModeGetPixelWidth(m)), @(CGDisplayModeGetPixelHeight(m)),
                             @(CGDisplayModeGetRefreshRate(m)), @(CGDisplayModeGetIOFlags(m)) ]
                        : @[],
        }];
        if (m) CGDisplayModeRelease(m);
    }
    return out;
}

static inline NSArray<NSDictionary *> *rig_windows(CGWindowListOption opt) {
    NSArray *list = CFBridgingRelease(CGWindowListCopyWindowInfo(opt, kCGNullWindowID));
    NSMutableArray *out = [NSMutableArray array];
    for (NSDictionary *w in list) {
        NSDictionary *b = w[(id)kCGWindowBounds];
        [out addObject:@{
            @"n" : w[(id)kCGWindowNumber],
            @"owner" : w[(id)kCGWindowOwnerName] ?: @"",
            @"pid" : w[(id)kCGWindowOwnerPID],
            @"layer" : w[(id)kCGWindowLayer],
            @"onscreen" : w[(id)kCGWindowIsOnscreen] ?: @NO,
            @"alpha" : w[(id)kCGWindowAlpha] ?: @1,
            @"b" : @[ b[@"X"], b[@"Y"], b[@"Width"], b[@"Height"] ],
        }];
    }
    return out;
}

static inline NSDictionary *rig_frontmost(void) {
    NSRunningApplication *f = NSWorkspace.sharedWorkspace.frontmostApplication;
    return @{@"name" : f.localizedName ?: @"", @"pid" : @(f ? f.processIdentifier : -1),
             @"bundle" : f.bundleIdentifier ?: @""};
}

static inline CGPoint rig_pointer(void) {
    CGEventRef e = CGEventCreate(NULL);
    CGPoint p = CGEventGetLocation(e);
    CFRelease(e);
    return p;
}

static inline NSString *rig_policy(NSApplicationActivationPolicy p) {
    switch (p) {
    case NSApplicationActivationPolicyRegular: return @"regular";
    case NSApplicationActivationPolicyAccessory: return @"accessory";
    case NSApplicationActivationPolicyProhibited: return @"prohibited";
    }
    return @"unknown";
}

static inline void rig_print_json(id obj) {
    NSError *err = nil;
    NSData *d = [NSJSONSerialization dataWithJSONObject:obj options:NSJSONWritingSortedKeys error:&err];
    if (!d) {
        fprintf(stderr, "json: %s\n", err.description.UTF8String);
        _exit(70);
    }
    fwrite(d.bytes, 1, d.length, stdout);
    fputc('\n', stdout);
    fflush(stdout);
}
