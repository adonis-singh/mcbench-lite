// displaylog: JSON lines on stdout for <seconds> seconds: every display
// reconfiguration event (display, flags, bounds, mode), and, sampled every
// 20 ms, every change of the frontmost application, the pointer position, and
// the set of on-screen windows that intersect a real (non-rig) display, as
// window number|owner|layer|bounds. No event tap, no capture, no accessibility.
// usage: displaylog <seconds>
#import "common.h"

static double g_t0;
static double now_s(void) { return [NSDate date].timeIntervalSince1970; }

static void emit(NSDictionary *d) {
    NSMutableDictionary *m = [d mutableCopy];
    m[@"t"] = @(round((now_s() - g_t0) * 1000) / 1000);
    rig_print_json(m);
}

static NSArray *flag_names(CGDisplayChangeSummaryFlags f) {
    struct { CGDisplayChangeSummaryFlags f; const char *n; } t[] = {
        {kCGDisplayBeginConfigurationFlag, "begin"}, {kCGDisplayMovedFlag, "moved"},
        {kCGDisplaySetMainFlag, "setMain"}, {kCGDisplaySetModeFlag, "setMode"}, {kCGDisplayAddFlag, "add"},
        {kCGDisplayRemoveFlag, "remove"}, {kCGDisplayEnabledFlag, "enabled"}, {kCGDisplayDisabledFlag, "disabled"},
        {kCGDisplayMirrorFlag, "mirror"}, {kCGDisplayUnMirrorFlag, "unmirror"},
        {kCGDisplayDesktopShapeChangedFlag, "desktopShapeChanged"},
    };
    NSMutableArray *a = [NSMutableArray array];
    for (unsigned i = 0; i < sizeof t / sizeof *t; i++)
        if (f & t[i].f) [a addObject:@(t[i].n)];
    return a;
}

static void reconfig(CGDirectDisplayID d, CGDisplayChangeSummaryFlags flags, void *ctx) {
    (void)ctx;
    NSMutableDictionary *e = [@{
        @"ev" : @"reconfig", @"display" : @(d), @"flags" : flag_names(flags),
        @"builtin" : @(CGDisplayIsBuiltin(d) != 0), @"main" : @(CGDisplayIsMain(d) != 0),
        @"vendor" : @(CGDisplayVendorNumber(d)), @"bounds" : rig_rect(CGDisplayBounds(d))
    } mutableCopy];
    CGDisplayModeRef m = CGDisplayCopyDisplayMode(d);
    if (m) {
        e[@"mode"] = @[ @(CGDisplayModeGetWidth(m)), @(CGDisplayModeGetHeight(m)), @(CGDisplayModeGetPixelWidth(m)),
                        @(CGDisplayModeGetPixelHeight(m)), @(CGDisplayModeGetRefreshRate(m)) ];
        CGDisplayModeRelease(m);
    }
    emit(e);
}

static NSSet *window_sig(void) {
    NSMutableArray *rects = [NSMutableArray array];
    for (NSDictionary *d in rig_displays())
        if ([d[@"vendor"] unsignedIntValue] != RIG_VENDOR) [rects addObject:d[@"bounds"]];
    NSMutableSet *sig = [NSMutableSet set];
    for (NSDictionary *w in rig_windows(kCGWindowListOptionOnScreenOnly)) {
        NSArray *b = w[@"b"];
        CGRect r = CGRectMake([b[0] doubleValue], [b[1] doubleValue], [b[2] doubleValue], [b[3] doubleValue]);
        BOOL on_real = NO;
        for (NSArray *d in rects)
            if (CGRectIntersectsRect(r, CGRectMake([d[0] doubleValue], [d[1] doubleValue], [d[2] doubleValue],
                                                   [d[3] doubleValue])))
                on_real = YES;
        if (!on_real) continue;
        [sig addObject:[NSString stringWithFormat:@"%@|%@|%@|%.0f,%.0f,%.0f,%.0f", w[@"n"], w[@"owner"], w[@"layer"],
                                                  r.origin.x, r.origin.y, r.size.width, r.size.height]];
    }
    return sig;
}

int main(int argc, char **argv) {
    rig_install_quiet_handlers();
    signal(SIGTERM, SIG_DFL);
    if (argc != 2) {
        fprintf(stderr, "usage: displaylog <seconds>\n");
        return 2;
    }
    double seconds = atof(argv[1]);
    @autoreleasepool {
        g_t0 = now_s();
        // Reconfiguration callbacks reach a process only once AppKit has set
        // up its window server connection. Prohibited: no Dock icon, no menu
        // bar, never activates; this process makes no window.
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyProhibited];
        NSArray *initial = rig_displays();
        CGError err = CGDisplayRegisterReconfigurationCallback(reconfig, NULL);
        if (err != kCGErrorSuccess) {
            fprintf(stderr, "CGDisplayRegisterReconfigurationCallback: %d\n", err);
            return 1;
        }
        __block NSString *last_front = @"";
        __block CGPoint last_ptr = CGPointMake(-1e9, -1e9);
        __block NSSet *last_sig = nil;
        __block NSArray *last_disp = initial;
        emit(@{@"ev" : @"start", @"seconds" : @(seconds), @"displays" : initial});
        NSTimer *t = [NSTimer timerWithTimeInterval:0.02 repeats:YES block:^(NSTimer *timer) {
            NSDictionary *f = rig_frontmost();
            NSString *front = [NSString stringWithFormat:@"%@|%@", f[@"name"], f[@"pid"]];
            if (![front isEqual:last_front]) {
                emit(@{@"ev" : @"frontmost", @"app" : front});
                last_front = front;
            }
            CGPoint p = rig_pointer();
            if (!CGPointEqualToPoint(p, last_ptr)) {
                emit(@{@"ev" : @"pointer", @"x" : @(p.x), @"y" : @(p.y)});
                last_ptr = p;
            }
            NSArray *disp = rig_displays();
            if (![disp isEqual:last_disp]) {
                emit(@{@"ev" : @"displays-changed", @"displays" : disp});
                last_disp = disp;
            }
            NSSet *sig = window_sig();
            if (!last_sig) {
                emit(@{@"ev" : @"windows-initial", @"count" : @(sig.count),
                       @"list" : [sig.allObjects sortedArrayUsingSelector:@selector(compare:)]});
            } else if (![sig isEqual:last_sig]) {
                NSMutableSet *gone = [last_sig mutableCopy];
                [gone minusSet:sig];
                NSMutableSet *added = [sig mutableCopy];
                [added minusSet:last_sig];
                emit(@{@"ev" : @"windows-changed",
                       @"gone" : [gone.allObjects sortedArrayUsingSelector:@selector(compare:)],
                       @"new" : [added.allObjects sortedArrayUsingSelector:@selector(compare:)]});
            }
            last_sig = sig;
        }];
        [NSRunLoop.mainRunLoop addTimer:t forMode:NSRunLoopCommonModes];
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(seconds * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
            emit(@{@"ev" : @"end", @"displays" : rig_displays()});
            exit(0);
        });
        [NSApp run];
    }
    return 0;
}
