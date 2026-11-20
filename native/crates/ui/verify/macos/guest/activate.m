// libguestactivate.dylib: for the GUEST VM only. The spikes are built never
// to activate (so they cannot take the owner's focus on the host); inside the
// guest the real-window checks need them to be an ordinary active app with a
// key window. Loaded with DYLD_INSERT_LIBRARIES it turns every
// -setActivationPolicy: into Regular, every -activateIgnoringOtherApps: into
// YES, and once launching has finished activates the app and makes its first
// window key. It refuses to load outside a virtual machine.
#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#include <sys/sysctl.h>

__attribute__((constructor)) static void guest_activate(void) {
    int v = 0;
    size_t len = sizeof v;
    if (sysctlbyname("kern.hv_vmm_present", &v, &len, NULL, 0) != 0 || v != 1) {
        fprintf(stderr, "guestactivate: not inside a virtual machine\n");
        _exit(75);
    }
    unsetenv("DYLD_INSERT_LIBRARIES");
    Method pol = class_getInstanceMethod([NSApplication class], @selector(setActivationPolicy:));
    __block IMP pol_orig = method_getImplementation(pol);
    method_setImplementation(pol, imp_implementationWithBlock(^BOOL(id self, NSInteger p) {
        return ((BOOL(*)(id, SEL, NSInteger))pol_orig)(self, @selector(setActivationPolicy:),
                                                       NSApplicationActivationPolicyRegular);
    }));
    Method act = class_getInstanceMethod([NSApplication class], @selector(activateIgnoringOtherApps:));
    __block IMP act_orig = method_getImplementation(act);
    method_setImplementation(act, imp_implementationWithBlock(^(id self, BOOL f) {
        ((void (*)(id, SEL, BOOL))act_orig)(self, @selector(activateIgnoringOtherApps:), YES);
    }));
    __block IMP mk_orig = method_getImplementation(class_getInstanceMethod([NSWindow class], @selector(makeKeyAndOrderFront:)));
    [[NSNotificationCenter defaultCenter] addObserverForName:NSApplicationDidFinishLaunchingNotification
                                                      object:nil
                                                       queue:nil
                                                  usingBlock:^(NSNotification *n) {
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 700 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
            // The originals, called directly: a spike may have replaced these
            // methods itself after this library loaded (ui-iced does).
            ((BOOL(*)(id, SEL, NSInteger))pol_orig)(NSApp, @selector(setActivationPolicy:),
                                                    NSApplicationActivationPolicyRegular);
            ((void (*)(id, SEL, BOOL))act_orig)(NSApp, @selector(activateIgnoringOtherApps:), YES);
            for (NSWindow *w in NSApp.windows)
                if (w.isVisible && w.canBecomeKeyWindow) {
                    ((void (*)(id, SEL, id))mk_orig)(w, @selector(makeKeyAndOrderFront:), nil);
                    break;
                }
        });
    }];
}
