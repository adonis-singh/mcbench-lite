// hoststate: one JSON snapshot of what the person at this Mac can see:
// displays, every window (owner, pid, layer, bounds, on-screen), frontmost
// application, menu bar owner, pointer, and for each --pid the activation
// policy and active flag of that process. No window, no activation, no events.
// usage: hoststate [--pid <pid>]...
#import "common.h"

int main(int argc, char **argv) {
    rig_install_quiet_handlers();
    @autoreleasepool {
        NSMutableArray *apps = [NSMutableArray array];
        for (int i = 1; i < argc; i += 2) {
            if (strcmp(argv[i], "--pid") != 0 || i + 1 >= argc) {
                fprintf(stderr, "usage: hoststate [--pid <pid>]...\n");
                return 2;
            }
            pid_t p = atoi(argv[i + 1]);
            NSRunningApplication *a = [NSRunningApplication runningApplicationWithProcessIdentifier:p];
            if (a)
                [apps addObject:@{@"pid" : @(p), @"policy" : rig_policy(a.activationPolicy), @"active" : @(a.active),
                                  @"hidden" : @(a.hidden), @"name" : a.localizedName ?: @""}];
            else
                [apps addObject:@{@"pid" : @(p), @"policy" : @"not-an-app"}];
        }
        CGPoint p = rig_pointer();
        rig_print_json(@{
            @"t" : @([NSDate date].timeIntervalSince1970),
            @"frontmost" : rig_frontmost(),
            @"menuBarOwner" : NSWorkspace.sharedWorkspace.menuBarOwningApplication.localizedName ?: @"",
            @"pointer" : @[ @(p.x), @(p.y) ],
            @"displays" : rig_displays(),
            @"windows" : rig_windows(kCGWindowListOptionAll),
            @"apps" : apps,
        });
    }
    return 0;
}
