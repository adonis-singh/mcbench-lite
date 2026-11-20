// guestctl: drives the GUI session of the rig's macOS guest VM from inside it.
// Real input events through the HID event tap (so input methods, key windows
// and the pointer behave as with a person), accessibility reads and actions,
// input sources, display modes, the window list. It refuses to run anywhere
// but inside a virtual machine, so it can never post events on the host.
//
// usage: guestctl <command> [args]   (see usage() below)
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <Carbon/Carbon.h>
#import <CoreGraphics/CoreGraphics.h>
#include <sys/sysctl.h>

static void die(NSString *msg) {
    fprintf(stderr, "guestctl: %s\n", msg.UTF8String);
    exit(1);
}

static void usage(void) {
    fprintf(stderr,
            "guestctl key <keycode> [cmd,shift,alt,ctrl]   press and release one key\n"
            "guestctl type <ascii>                         type with key codes (goes through the IME)\n"
            "guestctl move|click|dclick|tclick|rclick <x> <y>\n"
            "guestctl drag <x1> <y1> <x2> <y2>\n"
            "guestctl scroll <x> <y> <lines>\n"
            "guestctl ax <pid> [depth]                     accessibility tree as JSON lines\n"
            "guestctl focused <pid>                        focused element\n"
            "guestctl win <pid> list|move x y|resize w h|minimize|unminimize|fullscreen|unfullscreen|close|raise\n"
            "guestctl menu <pid> <title> [<title>...]      press a menu bar item by titles\n"
            "guestctl press <pid> <role> <title>           AXPress the first element with that role and title\n"
            "guestctl inputs                               list keyboard input sources\n"
            "guestctl input-enable|input-select <id>\n"
            "guestctl modes                                list display modes\n"
            "guestctl setmode <w> <h> <hidpi 0|1>           sets the mode and holds it until killed\n"
            "guestctl windows                              on-screen windows: owner, name, layer, bounds\n"
            "guestctl front                                frontmost app\n"
            "guestctl activate <pid>                       ask the app to become active\n"
            "  <el> below is `app`, `focused`, a child-index path from the app such as `0.2.1`,\n"
            "  or `find:<AXRole>:<title or description>` (first match, depth first)\n"
            "guestctl axall <pid> [depth]                  every attribute of every element, with its path\n"
            "guestctl node <pid> <el>                      every attribute of one element\n"
            "guestctl text <pid> <el>                      the parameterised text attributes, line by line\n"
            "guestctl actions <pid> <el>                   action names and their descriptions\n"
            "guestctl act <pid> <el> <AXAction>            AXUIElementPerformAction, prints the AXError\n"
            "guestctl set <pid> <el> <attr> <json>          AXUIElementSetAttributeValue (bool, number, string, [loc,len])\n"
            "guestctl observe <pid> <seconds>              AXObserver on the app: one JSON line per notification\n"
            "guestctl pb                                   the general pasteboard's types and string\n"
            "guestctl pbclear                              empties the general pasteboard\n"
            "guestctl trust                                AXIsProcessTrusted and this process's identity\n");
    exit(2);
}

static void require_vm(void) {
    int v = 0;
    size_t len = sizeof v;
    if (sysctlbyname("kern.hv_vmm_present", &v, &len, NULL, 0) != 0 || v != 1)
        die(@"not inside a virtual machine: refusing to post events or read this machine's windows");
}

static void print_json(id obj) {
    NSData *d = [NSJSONSerialization dataWithJSONObject:obj options:NSJSONWritingSortedKeys error:nil];
    fwrite(d.bytes, 1, d.length, stdout);
    fputc('\n', stdout);
}

static void post(CGEventRef e) {
    CGEventPost(kCGHIDEventTap, e);
    CFRelease(e);
}

static CGEventFlags parse_mods(const char *s) {
    CGEventFlags f = 0;
    if (!s) return f;
    if (strstr(s, "cmd")) f |= kCGEventFlagMaskCommand;
    if (strstr(s, "shift")) f |= kCGEventFlagMaskShift;
    if (strstr(s, "alt")) f |= kCGEventFlagMaskAlternate;
    if (strstr(s, "ctrl")) f |= kCGEventFlagMaskControl;
    // fn: the flag a Mac keyboard sets on function and arrow keys (no key of its own here).
    if (strstr(s, "fn")) f |= kCGEventFlagMaskSecondaryFn;
    return f;
}

// Modifier keys are pressed and released as keys of their own around the
// key, as a keyboard does: toolkits that track modifiers from flagsChanged
// events (winit does) ignore flags set only on the key event.
static void key(CGKeyCode code, CGEventFlags flags) {
    struct { CGEventFlags f; CGKeyCode k; } mods[] = {{kCGEventFlagMaskCommand, kVK_Command},
        {kCGEventFlagMaskShift, kVK_Shift}, {kCGEventFlagMaskAlternate, kVK_Option}, {kCGEventFlagMaskControl, kVK_Control}};
    CGEventFlags held = 0;
    for (int i = 0; i < 4; i++)
        if (flags & mods[i].f) {
            held |= mods[i].f;
            CGEventRef m = CGEventCreateKeyboardEvent(NULL, mods[i].k, true);
            CGEventSetFlags(m, held);
            post(m);
            usleep(15000);
        }
    CGEventRef d = CGEventCreateKeyboardEvent(NULL, code, true);
    CGEventSetFlags(d, flags);
    post(d);
    usleep(15000);
    CGEventRef u = CGEventCreateKeyboardEvent(NULL, code, false);
    CGEventSetFlags(u, flags);
    post(u);
    usleep(15000);
    for (int i = 3; i >= 0; i--)
        if (flags & mods[i].f) {
            held &= ~mods[i].f;
            CGEventRef m = CGEventCreateKeyboardEvent(NULL, mods[i].k, false);
            CGEventSetFlags(m, held);
            post(m);
            usleep(15000);
        }
    usleep(25000);
}

// ANSI key codes for the characters `type` supports (US layout positions).
static int ansi_code(char c, int *shift) {
    static const char *lower = "asdfhgzxcv\0bqweryt123465=97-80]ou[ip\0lj'k;\\,/nm.";
    *shift = 0;
    if (c >= 'A' && c <= 'Z') {
        *shift = 1;
        c = (char)(c - 'A' + 'a');
    }
    if (c == ' ') return kVK_Space;
    if (c == '\n') return kVK_Return;
    for (int i = 0; i < 48; i++)
        if (lower[i] == c && c) return i;
    return -1;
}

static void mouse(CGEventType t, CGPoint p, CGMouseButton b, int clicks) {
    CGEventRef e = CGEventCreateMouseEvent(NULL, t, p, b);
    CGEventSetIntegerValueField(e, kCGMouseEventClickState, clicks);
    post(e);
}

static void click(CGPoint p, int n, BOOL right) {
    CGEventType dn = right ? kCGEventRightMouseDown : kCGEventLeftMouseDown;
    CGEventType up = right ? kCGEventRightMouseUp : kCGEventLeftMouseUp;
    CGMouseButton b = right ? kCGMouseButtonRight : kCGMouseButtonLeft;
    mouse(kCGEventMouseMoved, p, b, 0);
    usleep(30000);
    for (int i = 1; i <= n; i++) {
        mouse(dn, p, b, i);
        usleep(20000);
        mouse(up, p, b, i);
        usleep(40000);
    }
}

static id ax(AXUIElementRef e, CFStringRef a) {
    CFTypeRef v = NULL;
    if (AXUIElementCopyAttributeValue(e, a, &v) != kAXErrorSuccess) return nil;
    return CFBridgingRelease(v);
}

static id plain(id v) {
    if (!v) return [NSNull null];
    if ([v isKindOfClass:NSString.class] || [v isKindOfClass:NSNumber.class]) return v;
    if (CFGetTypeID((__bridge CFTypeRef)v) == AXValueGetTypeID()) {
        AXValueRef a = (__bridge AXValueRef)v;
        CGPoint p;
        CGSize s;
        CFRange r;
        switch (AXValueGetType(a)) {
        case kAXValueCGPointType: AXValueGetValue(a, kAXValueCGPointType, &p); return @[ @(p.x), @(p.y) ];
        case kAXValueCGSizeType: AXValueGetValue(a, kAXValueCGSizeType, &s); return @[ @(s.width), @(s.height) ];
        case kAXValueCFRangeType: AXValueGetValue(a, kAXValueCFRangeType, &r); return @[ @(r.location), @(r.length) ];
        default: return @"<axvalue>";
        }
    }
    if ([v isKindOfClass:NSAttributedString.class]) return [(NSAttributedString *)v string];
    if ([v isKindOfClass:NSArray.class]) return [NSString stringWithFormat:@"<%lu items>", (unsigned long)[v count]];
    return [v description];
}

static NSDictionary *describe(AXUIElementRef e, int depth) {
    NSMutableDictionary *d = [NSMutableDictionary dictionary];
    d[@"depth"] = @(depth);
    CFStringRef attrs[] = {kAXRoleAttribute, kAXSubroleAttribute, kAXRoleDescriptionAttribute, kAXTitleAttribute,
                           kAXDescriptionAttribute, kAXValueAttribute, kAXPlaceholderValueAttribute,
                           kAXFocusedAttribute, kAXEnabledAttribute, kAXPositionAttribute, kAXSizeAttribute,
                           kAXSelectedTextRangeAttribute, kAXNumberOfCharactersAttribute, kAXIdentifierAttribute,
                           kAXHelpAttribute, kAXSelectedAttribute, kAXExpandedAttribute, kAXSelectedTextAttribute,
                           kAXInsertionPointLineNumberAttribute, kAXURLAttribute, kAXValueDescriptionAttribute,
                           kAXMinValueAttribute, kAXMaxValueAttribute, kAXDisclosureLevelAttribute,
                           CFSTR("AXRequired"), CFSTR("AXDOMIdentifier")};
    for (unsigned i = 0; i < sizeof attrs / sizeof *attrs; i++) {
        id v = ax(e, attrs[i]);
        if (!v) continue;
        id p = plain(v);
        if ([p isKindOfClass:NSString.class] && [p length] > 300)
            p = [[p substringToIndex:300] stringByAppendingString:@"…"];
        d[(__bridge NSString *)attrs[i]] = p;
    }
    CFArrayRef actions = NULL;
    if (AXUIElementCopyActionNames(e, &actions) == kAXErrorSuccess && actions) {
        NSArray *a = CFBridgingRelease(actions);
        if (a.count) d[@"actions"] = a;
    }
    return d;
}

static void dump(AXUIElementRef e, int depth, int max, NSString *path) {
    NSMutableDictionary *d = [describe(e, depth) mutableCopy];
    d[@"path"] = path;
    print_json(d);
    if (depth >= max) return;
    NSArray *kids = ax(e, kAXChildrenAttribute);
    for (NSUInteger i = 0; i < kids.count; i++)
        dump((__bridge AXUIElementRef)kids[i], depth + 1, max,
             path.length ? [NSString stringWithFormat:@"%@.%lu", path, (unsigned long)i]
                         : [NSString stringWithFormat:@"%lu", (unsigned long)i]);
}

// A short name for an element held in an attribute (parent, window, ...),
// so a full dump never recurses through references.
static NSString *el_summary(AXUIElementRef e) {
    NSString *r = ax(e, kAXRoleAttribute) ?: @"?";
    NSString *t = ax(e, kAXTitleAttribute);
    if (!t.length) t = ax(e, kAXDescriptionAttribute);
    return t.length ? [NSString stringWithFormat:@"<%@ %@>", r, t] : [NSString stringWithFormat:@"<%@>", r];
}

static id plain_full(id v) {
    if (!v) return [NSNull null];
    if (CFGetTypeID((__bridge CFTypeRef)v) == AXUIElementGetTypeID()) return el_summary((__bridge AXUIElementRef)v);
    if ([v isKindOfClass:NSArray.class]) {
        NSMutableArray *o = [NSMutableArray array];
        for (id x in (NSArray *)v) [o addObject:plain_full(x)];
        return o;
    }
    if ([v isKindOfClass:NSURL.class]) return [(NSURL *)v absoluteString];
    if (CFGetTypeID((__bridge CFTypeRef)v) == AXValueGetTypeID()) {
        AXValueRef a = (__bridge AXValueRef)v;
        if (AXValueGetType(a) == kAXValueCGRectType) {
            CGRect r;
            AXValueGetValue(a, kAXValueCGRectType, &r);
            return @[ @(r.origin.x), @(r.origin.y), @(r.size.width), @(r.size.height) ];
        }
    }
    return plain(v);
}

// Every attribute the element reports, its value, whether it is settable,
// its parameterised attribute names and its actions.
static NSDictionary *describe_all(AXUIElementRef e) {
    NSMutableDictionary *d = [NSMutableDictionary dictionary];
    CFArrayRef names = NULL;
    AXError err = AXUIElementCopyAttributeNames(e, &names);
    if (err != kAXErrorSuccess) {
        d[@"error"] = @(err);
        return d;
    }
    NSMutableArray *settable = [NSMutableArray array];
    for (NSString *n in (__bridge_transfer NSArray *)names) {
        if ([n isEqual:@"AXChildren"] || [n isEqual:@"AXChildrenInNavigationOrder"] || [n isEqual:@"AXVisibleChildren"]) {
            NSArray *k = ax(e, (__bridge CFStringRef)n);
            d[n] = [NSString stringWithFormat:@"<%lu items>", (unsigned long)k.count];
        } else {
            CFTypeRef v = NULL;
            AXError ae = AXUIElementCopyAttributeValue(e, (__bridge CFStringRef)n, &v);
            d[n] = ae == kAXErrorSuccess ? plain_full(CFBridgingRelease(v)) : [NSString stringWithFormat:@"<error %d>", ae];
        }
        Boolean s = false;
        if (AXUIElementIsAttributeSettable(e, (__bridge CFStringRef)n, &s) == kAXErrorSuccess && s) [settable addObject:n];
    }
    if (settable.count) d[@"_settable"] = settable;
    CFArrayRef params = NULL;
    if (AXUIElementCopyParameterizedAttributeNames(e, &params) == kAXErrorSuccess && params) {
        NSArray *p = CFBridgingRelease(params);
        if (p.count) d[@"_params"] = p;
    }
    CFArrayRef actions = NULL;
    if (AXUIElementCopyActionNames(e, &actions) == kAXErrorSuccess && actions) {
        NSArray *a = CFBridgingRelease(actions);
        if (a.count) d[@"_actions"] = a;
    }
    return d;
}

static void dump_all(AXUIElementRef e, int depth, int max, NSString *path) {
    NSMutableDictionary *d = [describe_all(e) mutableCopy];
    d[@"_depth"] = @(depth);
    d[@"_path"] = path;
    print_json(d);
    if (depth >= max) return;
    NSArray *kids = ax(e, kAXChildrenAttribute);
    for (NSUInteger i = 0; i < kids.count; i++)
        dump_all((__bridge AXUIElementRef)kids[i], depth + 1, max,
                 path.length ? [NSString stringWithFormat:@"%@.%lu", path, (unsigned long)i]
                             : [NSString stringWithFormat:@"%lu", (unsigned long)i]);
}

// The child-index path of an element below the application element.
static NSString *path_of(AXUIElementRef e) {
    NSMutableArray *parts = [NSMutableArray array];
    AXUIElementRef cur = (AXUIElementRef)CFRetain(e);
    for (int guard = 0; guard < 80; guard++) {
        id p = ax(cur, kAXParentAttribute);
        if (!p) break;
        AXUIElementRef parent = (__bridge AXUIElementRef)p;
        NSArray *kids = ax(parent, kAXChildrenAttribute);
        NSUInteger idx = NSNotFound;
        for (NSUInteger i = 0; i < kids.count; i++)
            if (CFEqual((__bridge CFTypeRef)kids[i], cur)) idx = i;
        [parts insertObject:idx == NSNotFound ? @"?" : @(idx).stringValue atIndex:0];
        CFRelease(cur);
        cur = (AXUIElementRef)CFRetain(parent);
    }
    CFRelease(cur);
    return [parts componentsJoinedByString:@"."];
}

static AXUIElementRef find(AXUIElementRef e, NSString *role, NSString *title, int depth) {
    if (depth > 40) return NULL;
    NSString *r = ax(e, kAXRoleAttribute);
    NSString *t = ax(e, kAXTitleAttribute) ?: ax(e, kAXDescriptionAttribute);
    if ([r isEqual:role] && [t isEqual:title]) return (AXUIElementRef)CFRetain(e);
    for (id k in (NSArray *)ax(e, kAXChildrenAttribute)) {
        AXUIElementRef f = find((__bridge AXUIElementRef)k, role, title, depth + 1);
        if (f) return f;
    }
    return NULL;
}

static AXUIElementRef app_el(const char *pid) { return AXUIElementCreateApplication(atoi(pid)); }

static AXUIElementRef first_window(AXUIElementRef app) {
    NSArray *ws = ax(app, kAXWindowsAttribute);
    if (!ws.count) die(@"application has no windows");
    return (AXUIElementRef)CFRetain((__bridge CFTypeRef)ws[0]);
}

static void set_value(AXUIElementRef e, CFStringRef a, CFTypeRef v) {
    AXError err = AXUIElementSetAttributeValue(e, a, v);
    if (err != kAXErrorSuccess) die([NSString stringWithFormat:@"set %@ failed: %d", a, err]);
}

static AXUIElementRef find_role_label(AXUIElementRef e, NSString *role, NSString *label, int depth) {
    if (depth > 60) return NULL;
    NSString *r = ax(e, kAXRoleAttribute);
    if ([r isEqual:role]) {
        for (id a in @[ (__bridge NSString *)kAXTitleAttribute, (__bridge NSString *)kAXDescriptionAttribute,
                        (__bridge NSString *)kAXValueAttribute ]) {
            id v = ax(e, (__bridge CFStringRef)a);
            if ([v isKindOfClass:NSString.class] && [v isEqual:label]) return (AXUIElementRef)CFRetain(e);
        }
    }
    for (id k in (NSArray *)ax(e, kAXChildrenAttribute)) {
        AXUIElementRef f = find_role_label((__bridge AXUIElementRef)k, role, label, depth + 1);
        if (f) return f;
    }
    return NULL;
}

// <el>: app | focused | 0.2.1 | find:<AXRole>:<label>
static AXUIElementRef resolve(const char *pid, const char *spec) {
    AXUIElementRef app = app_el(pid);
    NSString *s = @(spec);
    if ([s isEqual:@"app"]) return app;
    if ([s isEqual:@"focused"]) {
        id f = ax(app, kAXFocusedUIElementAttribute);
        if (!f) die(@"no focused element");
        return (AXUIElementRef)CFBridgingRetain(f);
    }
    if ([s hasPrefix:@"find:"]) {
        NSArray *p = [[s substringFromIndex:5] componentsSeparatedByString:@":"];
        if (p.count < 2) die(@"find:<AXRole>:<label>");
        NSString *label = [[p subarrayWithRange:NSMakeRange(1, p.count - 1)] componentsJoinedByString:@":"];
        AXUIElementRef f = find_role_label(app, p[0], label, 0);
        if (!f) die([NSString stringWithFormat:@"no %@ labelled '%@'", p[0], label]);
        return f;
    }
    AXUIElementRef cur = app;
    for (NSString *part in [s componentsSeparatedByString:@"."]) {
        NSArray *kids = ax(cur, kAXChildrenAttribute);
        NSInteger i = part.integerValue;
        if (i < 0 || (NSUInteger)i >= kids.count)
            die([NSString stringWithFormat:@"path %@: index %ld out of %lu children", s, (long)i, (unsigned long)kids.count]);
        cur = (AXUIElementRef)CFBridgingRetain(kids[(NSUInteger)i]);
    }
    return cur;
}

static id param(AXUIElementRef e, CFStringRef attr, CFTypeRef arg, AXError *err) {
    CFTypeRef v = NULL;
    *err = AXUIElementCopyParameterizedAttributeValue(e, attr, arg, &v);
    return *err == kAXErrorSuccess ? CFBridgingRelease(v) : nil;
}

static AXValueRef range_value(CFIndex loc, CFIndex len) {
    CFRange r = CFRangeMake(loc, len);
    return AXValueCreate(kAXValueCFRangeType, &r);
}

// The text an element exposes, through the parameterised attributes a
// screen reader uses: the whole string, each line's range and string, the
// bounds of each line, the line of each word start, and the style runs.
static void text_report(AXUIElementRef e) {
    NSMutableDictionary *d = [NSMutableDictionary dictionary];
    d[@"role"] = ax(e, kAXRoleAttribute) ?: @"";
    NSNumber *n = ax(e, kAXNumberOfCharactersAttribute);
    d[@"AXNumberOfCharacters"] = n ?: [NSNull null];
    d[@"AXSelectedTextRange"] = plain(ax(e, kAXSelectedTextRangeAttribute));
    d[@"AXVisibleCharacterRange"] = plain(ax(e, kAXVisibleCharacterRangeAttribute));
    d[@"AXInsertionPointLineNumber"] = plain(ax(e, kAXInsertionPointLineNumberAttribute));
    CFArrayRef pn = NULL;
    AXUIElementCopyParameterizedAttributeNames(e, &pn);
    d[@"params"] = pn ? CFBridgingRelease(pn) : @[];
    print_json(d);
    if (!n) return;
    CFIndex len = n.integerValue;
    AXError err;
    AXValueRef all = range_value(0, len);
    id whole = param(e, kAXStringForRangeParameterizedAttribute, all, &err);
    print_json(@{@"AXStringForRange" : whole ?: [NSString stringWithFormat:@"<error %d>", err]});
    id attributed = param(e, kAXAttributedStringForRangeParameterizedAttribute, all, &err);
    if (attributed) {
        NSMutableArray *runs = [NSMutableArray array];
        NSAttributedString *as = attributed;
        [as enumerateAttributesInRange:NSMakeRange(0, as.length)
                               options:0
                            usingBlock:^(NSDictionary *attrs, NSRange r, BOOL *stop) {
            NSMutableDictionary *o = [NSMutableDictionary dictionary];
            for (NSString *k in attrs) o[k] = plain_full(attrs[k]);
            [runs addObject:@{@"range" : @[ @(r.location), @(r.length) ], @"attrs" : o}];
        }];
        print_json(@{@"AXAttributedStringForRange" : runs});
    } else
        print_json(@{@"AXAttributedStringForRange" : [NSString stringWithFormat:@"<error %d>", err]});
    CFRelease(all);
    // Lines: walk AXLineForIndex over every index and AXRangeForLine for each line seen.
    NSMutableArray *lines = [NSMutableArray array];
    long last = -1;
    for (CFIndex i = 0; i <= len; i++) {
        id l = param(e, kAXLineForIndexParameterizedAttribute, (__bridge CFTypeRef) @(i), &err);
        if (!l) {
            [lines addObject:@{@"index" : @(i), @"AXLineForIndex" : [NSString stringWithFormat:@"<error %d>", err]}];
            break;
        }
        long line = [l longValue];
        if (line == last) continue;
        last = line;
        id r = param(e, kAXRangeForLineParameterizedAttribute, (__bridge CFTypeRef) @(line), &err);
        NSMutableDictionary *o = [@{@"line" : @(line), @"firstIndex" : @(i)} mutableCopy];
        o[@"AXRangeForLine"] = r ? plain(r) : [NSString stringWithFormat:@"<error %d>", err];
        if (r) {
            id s = param(e, kAXStringForRangeParameterizedAttribute, (__bridge CFTypeRef)r, &err);
            o[@"string"] = s ?: [NSString stringWithFormat:@"<error %d>", err];
            id b = param(e, kAXBoundsForRangeParameterizedAttribute, (__bridge CFTypeRef)r, &err);
            o[@"AXBoundsForRange"] = b ? plain_full(b) : [NSString stringWithFormat:@"<error %d>", err];
        }
        [lines addObject:o];
        if (lines.count > 400) break;
    }
    print_json(@{@"lines" : lines});
    // The first character's bounds, AXRangeForPosition back from its centre,
    // AXRangeForIndex and AXStyleRangeForIndex at 0.
    if (len > 0) {
        AXValueRef one = range_value(0, 1);
        id b = param(e, kAXBoundsForRangeParameterizedAttribute, one, &err);
        NSMutableDictionary *o = [NSMutableDictionary dictionary];
        o[@"AXBoundsForRange(0,1)"] = b ? plain_full(b) : [NSString stringWithFormat:@"<error %d>", err];
        if (b) {
            CGRect r;
            AXValueGetValue((__bridge AXValueRef)b, kAXValueCGRectType, &r);
            CGPoint c = CGPointMake(CGRectGetMidX(r), CGRectGetMidY(r));
            AXValueRef pv = AXValueCreate(kAXValueCGPointType, &c);
            id rp = param(e, kAXRangeForPositionParameterizedAttribute, pv, &err);
            o[@"AXRangeForPosition(centre)"] = rp ? plain(rp) : [NSString stringWithFormat:@"<error %d>", err];
            CFRelease(pv);
        }
        id ri = param(e, kAXRangeForIndexParameterizedAttribute, (__bridge CFTypeRef) @0, &err);
        o[@"AXRangeForIndex(0)"] = ri ? plain(ri) : [NSString stringWithFormat:@"<error %d>", err];
        id sr = param(e, kAXStyleRangeForIndexParameterizedAttribute, (__bridge CFTypeRef) @0, &err);
        o[@"AXStyleRangeForIndex(0)"] = sr ? plain(sr) : [NSString stringWithFormat:@"<error %d>", err];
        print_json(o);
        CFRelease(one);
    }
}

static NSString *ts(void) { return [NSString stringWithFormat:@"%.3f", [NSDate date].timeIntervalSince1970]; }

static void observer_cb(AXObserverRef o, AXUIElementRef e, CFStringRef note, CFDictionaryRef info, void *ctx) {
    NSMutableDictionary *d = [NSMutableDictionary dictionary];
    d[@"t"] = ts();
    d[@"note"] = (__bridge NSString *)note;
    d[@"role"] = ax(e, kAXRoleAttribute) ?: @"";
    NSString *label = ax(e, kAXTitleAttribute);
    if (!label.length) label = ax(e, kAXDescriptionAttribute);
    if (label.length) d[@"label"] = label;
    id v = ax(e, kAXValueAttribute);
    if (v) {
        id p = plain(v);
        if ([p isKindOfClass:NSString.class] && [p length] > 200) p = [[p substringToIndex:200] stringByAppendingString:@"…"];
        d[@"value"] = p;
    }
    id sel = ax(e, kAXSelectedTextRangeAttribute);
    if (sel) d[@"sel"] = plain(sel);
    if (info) {
        NSMutableDictionary *i = [NSMutableDictionary dictionary];
        for (NSString *k in (__bridge NSDictionary *)info) i[k] = plain_full(((__bridge NSDictionary *)info)[k]);
        d[@"info"] = i;
    }
    d[@"path"] = path_of(e);
    print_json(d);
    fflush(stdout);
}

static void observe(const char *pid, double seconds) {
    AXObserverRef o = NULL;
    AXError err = AXObserverCreateWithInfoCallback(atoi(pid), observer_cb, &o);
    if (err != kAXErrorSuccess) die([NSString stringWithFormat:@"AXObserverCreate: %d", err]);
    AXUIElementRef app = app_el(pid);
    NSArray *notes = @[
        (__bridge NSString *)kAXFocusedUIElementChangedNotification, (__bridge NSString *)kAXFocusedWindowChangedNotification,
        (__bridge NSString *)kAXMainWindowChangedNotification, (__bridge NSString *)kAXWindowCreatedNotification,
        (__bridge NSString *)kAXWindowMovedNotification, (__bridge NSString *)kAXWindowResizedNotification,
        (__bridge NSString *)kAXWindowMiniaturizedNotification, (__bridge NSString *)kAXWindowDeminiaturizedNotification,
        (__bridge NSString *)kAXValueChangedNotification, (__bridge NSString *)kAXTitleChangedNotification,
        (__bridge NSString *)kAXSelectedTextChangedNotification, (__bridge NSString *)kAXSelectedChildrenChangedNotification,
        (__bridge NSString *)kAXSelectedRowsChangedNotification, (__bridge NSString *)kAXRowCountChangedNotification,
        (__bridge NSString *)kAXRowExpandedNotification, (__bridge NSString *)kAXRowCollapsedNotification,
        (__bridge NSString *)kAXCreatedNotification, (__bridge NSString *)kAXUIElementDestroyedNotification,
        (__bridge NSString *)kAXLayoutChangedNotification, (__bridge NSString *)kAXAnnouncementRequestedNotification,
        (__bridge NSString *)kAXMenuOpenedNotification, (__bridge NSString *)kAXMenuClosedNotification,
        @"AXExpandedChanged", @"AXLiveRegionChanged", @"AXLiveRegionCreated", @"AXElementBusyChanged",
        @"AXLoadComplete", @"AXInvalidStatusChanged", @"AXSelectedCellsChanged"
    ];
    NSMutableDictionary *reg = [NSMutableDictionary dictionary];
    for (NSString *n in notes) reg[n] = @(AXObserverAddNotification(o, app, (__bridge CFStringRef)n, NULL));
    print_json(@{@"t" : ts(), @"registered" : reg});
    fflush(stdout);
    CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(o), kCFRunLoopDefaultMode);
    CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, false);
    print_json(@{@"t" : ts(), @"end" : @YES});
}

static id parse_json_arg(const char *s) {
    NSData *data = [@(s) dataUsingEncoding:NSUTF8StringEncoding];
    id v = [NSJSONSerialization JSONObjectWithData:data options:NSJSONReadingFragmentsAllowed error:nil];
    if (!v) die(@"set: value must be JSON (true, 3, \"text\", [loc,len])");
    return v;
}

int main(int argc, char **argv) {
    @autoreleasepool {
        require_vm();
        if (argc < 2) usage();
        NSString *cmd = @(argv[1]);
        if ([cmd isEqual:@"trust"]) {
            NSDictionary *opt = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt : @NO};
            print_json(@{@"trusted" : @(AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)opt)),
                         @"pid" : @(getpid()), @"ppid" : @(getppid()), @"uid" : @(getuid()),
                         @"exe" : NSProcessInfo.processInfo.arguments[0]});
            return 0;
        }
        if ([cmd isEqual:@"pb"]) {
            NSPasteboard *pb = NSPasteboard.generalPasteboard;
            print_json(@{@"changeCount" : @(pb.changeCount), @"types" : pb.types ?: @[],
                         @"string" : [pb stringForType:NSPasteboardTypeString] ?: [NSNull null]});
            return 0;
        }
        if ([cmd isEqual:@"pbclear"]) {
            [NSPasteboard.generalPasteboard clearContents];
            return 0;
        }
        if (![cmd isEqual:@"modes"] && ![cmd isEqual:@"inputs"] && ![cmd isEqual:@"front"] && !AXIsProcessTrusted())
            die(@"this process is not trusted for accessibility in the guest");
        if ([cmd isEqual:@"axall"] && argc >= 3) {
            dump_all(app_el(argv[2]), 0, argc > 3 ? atoi(argv[3]) : 60, @"");
        } else if ([cmd isEqual:@"node"] && argc == 4) {
            AXUIElementRef e = resolve(argv[2], argv[3]);
            NSMutableDictionary *d = [describe_all(e) mutableCopy];
            d[@"_path"] = path_of(e);
            print_json(d);
        } else if ([cmd isEqual:@"text"] && argc == 4) {
            text_report(resolve(argv[2], argv[3]));
        } else if ([cmd isEqual:@"actions"] && argc == 4) {
            AXUIElementRef e = resolve(argv[2], argv[3]);
            CFArrayRef names = NULL;
            AXError err = AXUIElementCopyActionNames(e, &names);
            if (err != kAXErrorSuccess) die([NSString stringWithFormat:@"AXUIElementCopyActionNames: %d", err]);
            NSMutableArray *o = [NSMutableArray array];
            for (NSString *n in (__bridge_transfer NSArray *)names) {
                CFStringRef desc = NULL;
                AXUIElementCopyActionDescription(e, (__bridge CFStringRef)n, &desc);
                [o addObject:@{@"name" : n, @"description" : desc ? (__bridge_transfer NSString *)desc : @""}];
            }
            print_json(@{@"path" : path_of(e), @"actions" : o});
        } else if ([cmd isEqual:@"act"] && argc == 5) {
            AXUIElementRef e = resolve(argv[2], argv[3]);
            AXError err = AXUIElementPerformAction(e, (__bridge CFStringRef) @(argv[4]));
            print_json(@{@"t" : ts(), @"path" : path_of(e), @"action" : @(argv[4]), @"AXError" : @(err)});
            if (err != kAXErrorSuccess) return 1;
        } else if ([cmd isEqual:@"set"] && argc == 6) {
            AXUIElementRef e = resolve(argv[2], argv[3]);
            id v = parse_json_arg(argv[5]);
            CFTypeRef cv;
            NSString *attr = @(argv[4]);
            if ([v isKindOfClass:NSArray.class] && [v count] == 2 && [attr isEqual:@"AXPosition"]) {
                CGPoint p = CGPointMake([v[0] doubleValue], [v[1] doubleValue]);
                cv = AXValueCreate(kAXValueCGPointType, &p);
            } else if ([v isKindOfClass:NSArray.class] && [v count] == 2 && [attr isEqual:@"AXSize"]) {
                CGSize s = CGSizeMake([v[0] doubleValue], [v[1] doubleValue]);
                cv = AXValueCreate(kAXValueCGSizeType, &s);
            } else if ([v isKindOfClass:NSArray.class] && [v count] == 2)
                cv = range_value([v[0] integerValue], [v[1] integerValue]);
            else
                cv = (__bridge_retained CFTypeRef)v;
            AXError err = AXUIElementSetAttributeValue(e, (__bridge CFStringRef) @(argv[4]), cv);
            print_json(@{@"t" : ts(), @"path" : path_of(e), @"attr" : @(argv[4]), @"AXError" : @(err)});
            if (err != kAXErrorSuccess) return 1;
        } else if ([cmd isEqual:@"attr"] && argc == 5) {
            // attr <pid> <el> <AXName>: one attribute by name, listed or not.
            AXUIElementRef e = resolve(argv[2], argv[3]);
            CFTypeRef v = NULL;
            AXError err = AXUIElementCopyAttributeValue(e, (__bridge CFStringRef) @(argv[4]), &v);
            print_json(@{@"path" : path_of(e), @"attr" : @(argv[4]), @"AXError" : @(err),
                         @"value" : err == kAXErrorSuccess ? plain_full(CFBridgingRelease(v)) : [NSNull null]});
        } else if ([cmd isEqual:@"search"] && argc >= 5) {
            // search <pid> <el> <AXSearchKey> [next|previous] [start-el] [limit]: the
            // parameterised attributes VoiceOver's find and rotor commands use.
            AXUIElementRef e = resolve(argv[2], argv[3]);
            NSMutableDictionary *q = [@{@"AXSearchKey" : @(argv[4]),
                                        @"AXDirection" : (argc > 5 && !strcmp(argv[5], "previous")) ? @"AXDirectionPrevious"
                                                                                                   : @"AXDirectionNext",
                                        @"AXResultsLimit" : @(argc > 7 ? atoi(argv[7]) : -1),
                                        @"AXVisibleOnly" : @NO, @"AXImmediateDescendantsOnly" : @NO} mutableCopy];
            if (argc > 6 && strcmp(argv[6], "-")) q[@"AXStartElement"] = (__bridge_transfer id)resolve(argv[2], argv[6]);
            AXError err;
            id count = param(e, CFSTR("AXUIElementCountForSearchPredicate"), (__bridge CFTypeRef)q, &err);
            AXError cerr = err;
            id found = param(e, CFSTR("AXUIElementsForSearchPredicate"), (__bridge CFTypeRef)q, &err);
            NSMutableArray *out = [NSMutableArray array];
            for (id f in (NSArray *)found) {
                if (CFGetTypeID((__bridge CFTypeRef)f) != AXUIElementGetTypeID()) {
                    [out addObject:@{@"class" : NSStringFromClass([f class]), @"value" : plain_full(f)}];
                    continue;
                }
                NSMutableDictionary *d = [describe((__bridge AXUIElementRef)f, 0) mutableCopy];
                d[@"path"] = path_of((__bridge AXUIElementRef)f);
                [out addObject:d];
            }
            print_json(@{@"count" : count ?: [NSString stringWithFormat:@"<error %d>", cerr],
                         @"elements" : found ? out : [NSString stringWithFormat:@"<error %d>", err]});
        } else if ([cmd isEqual:@"axfocus"] && argc == 3) {
            // Every element below the app whose own AXFocused is true: the
            // focused node as the element reports it, independent of what
            // the application's AXFocusedUIElement answers.
            NSMutableArray *stack = [NSMutableArray arrayWithObject:@[ (__bridge id)app_el(argv[2]), @"" ]];
            NSMutableArray *hits = [NSMutableArray array];
            while (stack.count) {
                NSArray *top = stack.lastObject;
                [stack removeLastObject];
                AXUIElementRef e = (__bridge AXUIElementRef)top[0];
                NSString *p = top[1];
                NSString *role = ax(e, kAXRoleAttribute);
                if (p.length && ![role isEqual:@"AXWindow"] && [ax(e, kAXFocusedAttribute) boolValue]) {
                    NSMutableDictionary *d = [describe(e, 0) mutableCopy];
                    d[@"path"] = p;
                    [hits addObject:d];
                }
                if ([role hasPrefix:@"AXMenu"]) continue;
                NSArray *kids = ax(e, kAXChildrenAttribute);
                for (NSInteger i = (NSInteger)kids.count - 1; i >= 0; i--)
                    [stack addObject:@[ kids[(NSUInteger)i], p.length ? [NSString stringWithFormat:@"%@.%ld", p, (long)i]
                                                                      : [NSString stringWithFormat:@"%ld", (long)i] ]];
            }
            print_json(@{@"t" : ts(), @"focusedNodes" : hits});
        } else if ([cmd isEqual:@"observe"] && argc == 4) {
            observe(argv[2], atof(argv[3]));
        } else if ([cmd isEqual:@"key"] && argc >= 3) {
            key((CGKeyCode)atoi(argv[2]), parse_mods(argc > 3 ? argv[3] : NULL));
        } else if ([cmd isEqual:@"type"] && argc == 3) {
            for (const char *c = argv[2]; *c; c++) {
                int shift;
                int code = ansi_code(*c, &shift);
                if (code < 0) die([NSString stringWithFormat:@"type: unsupported character '%c'", *c]);
                key((CGKeyCode)code, shift ? kCGEventFlagMaskShift : 0);
            }
        } else if (([cmd isEqual:@"move"] || [cmd isEqual:@"click"] || [cmd isEqual:@"dclick"] ||
                    [cmd isEqual:@"tclick"] || [cmd isEqual:@"rclick"]) && argc == 4) {
            CGPoint p = CGPointMake(atof(argv[2]), atof(argv[3]));
            if ([cmd isEqual:@"move"]) mouse(kCGEventMouseMoved, p, kCGMouseButtonLeft, 0);
            else click(p, [cmd isEqual:@"tclick"] ? 3 : [cmd isEqual:@"dclick"] ? 2 : 1, [cmd isEqual:@"rclick"]);
        } else if ([cmd isEqual:@"drag"] && argc == 6) {
            CGPoint a = CGPointMake(atof(argv[2]), atof(argv[3])), b = CGPointMake(atof(argv[4]), atof(argv[5]));
            mouse(kCGEventMouseMoved, a, kCGMouseButtonLeft, 0);
            usleep(50000);
            mouse(kCGEventLeftMouseDown, a, kCGMouseButtonLeft, 1);
            usleep(80000);
            for (int i = 1; i <= 20; i++) {
                CGPoint p = CGPointMake(a.x + (b.x - a.x) * i / 20, a.y + (b.y - a.y) * i / 20);
                mouse(kCGEventLeftMouseDragged, p, kCGMouseButtonLeft, 1);
                usleep(20000);
            }
            usleep(80000);
            mouse(kCGEventLeftMouseUp, b, kCGMouseButtonLeft, 1);
        } else if ([cmd isEqual:@"scroll"] && argc == 5) {
            mouse(kCGEventMouseMoved, CGPointMake(atof(argv[2]), atof(argv[3])), kCGMouseButtonLeft, 0);
            usleep(30000);
            post(CGEventCreateScrollWheelEvent(NULL, kCGScrollEventUnitLine, 1, atoi(argv[4])));
        } else if ([cmd isEqual:@"ax"] && argc >= 3) {
            dump(app_el(argv[2]), 0, argc > 3 ? atoi(argv[3]) : 60, @"");
        } else if ([cmd isEqual:@"focused"] && argc == 3) {
            id f = ax(app_el(argv[2]), kAXFocusedUIElementAttribute);
            if (!f) die(@"no focused element");
            NSMutableDictionary *d = [describe((__bridge AXUIElementRef)f, 0) mutableCopy];
            d[@"path"] = path_of((__bridge AXUIElementRef)f);
            d[@"t"] = ts();
            print_json(d);
        } else if ([cmd isEqual:@"win"] && argc >= 4) {
            AXUIElementRef app = app_el(argv[2]);
            NSString *op = @(argv[3]);
            if ([op isEqual:@"list"]) {
                for (id w in (NSArray *)ax(app, kAXWindowsAttribute)) {
                    NSMutableDictionary *d = [describe((__bridge AXUIElementRef)w, 0) mutableCopy];
                    d[@"minimized"] = plain(ax((__bridge AXUIElementRef)w, kAXMinimizedAttribute));
                    d[@"fullscreen"] = plain(ax((__bridge AXUIElementRef)w, CFSTR("AXFullScreen")));
                    d[@"main"] = plain(ax((__bridge AXUIElementRef)w, kAXMainAttribute));
                    print_json(d);
                }
                return 0;
            }
            AXUIElementRef w = first_window(app);
            if ([op isEqual:@"move"] && argc == 6) {
                CGPoint p = CGPointMake(atof(argv[4]), atof(argv[5]));
                AXValueRef v = AXValueCreate(kAXValueCGPointType, &p);
                set_value(w, kAXPositionAttribute, v);
                CFRelease(v);
            } else if ([op isEqual:@"resize"] && argc == 6) {
                CGSize s = CGSizeMake(atof(argv[4]), atof(argv[5]));
                AXValueRef v = AXValueCreate(kAXValueCGSizeType, &s);
                set_value(w, kAXSizeAttribute, v);
                CFRelease(v);
            } else if ([op isEqual:@"minimize"] || [op isEqual:@"unminimize"]) {
                set_value(w, kAXMinimizedAttribute, [op isEqual:@"minimize"] ? kCFBooleanTrue : kCFBooleanFalse);
            } else if ([op isEqual:@"fullscreen"] || [op isEqual:@"unfullscreen"]) {
                set_value(w, CFSTR("AXFullScreen"), [op isEqual:@"fullscreen"] ? kCFBooleanTrue : kCFBooleanFalse);
            } else if ([op isEqual:@"close"]) {
                id b = ax(w, kAXCloseButtonAttribute);
                if (!b) die(@"window has no close button");
                AXUIElementPerformAction((__bridge AXUIElementRef)b, kAXPressAction);
            } else if ([op isEqual:@"raise"]) {
                AXUIElementPerformAction(w, kAXRaiseAction);
            } else usage();
        } else if ([cmd isEqual:@"menu"] && argc >= 4) {
            AXUIElementRef cur = (AXUIElementRef)CFBridgingRetain(ax(app_el(argv[2]), kAXMenuBarAttribute));
            if (!cur) die(@"no menu bar");
            for (int i = 3; i < argc; i++) {
                AXUIElementRef next = NULL;
                NSArray *kids = ax(cur, kAXChildrenAttribute);
                if (kids.count == 1 && [ax((__bridge AXUIElementRef)kids[0], kAXRoleAttribute) isEqual:@"AXMenu"])
                    kids = ax((__bridge AXUIElementRef)kids[0], kAXChildrenAttribute);
                for (id k in kids)
                    if ([ax((__bridge AXUIElementRef)k, kAXTitleAttribute) isEqual:@(argv[i])])
                        next = (AXUIElementRef)CFRetain((__bridge CFTypeRef)k);
                if (!next) die([NSString stringWithFormat:@"menu item '%s' not found", argv[i]]);
                if (i == argc - 1) {
                    AXError err = AXUIElementPerformAction(next, kAXPressAction);
                    if (err != kAXErrorSuccess) die([NSString stringWithFormat:@"press failed: %d", err]);
                }
                cur = next;
            }
        } else if ([cmd isEqual:@"press"] && argc == 5) {
            AXUIElementRef e = find(app_el(argv[2]), @(argv[3]), @(argv[4]), 0);
            if (!e) die(@"element not found");
            AXError err = AXUIElementPerformAction(e, kAXPressAction);
            if (err != kAXErrorSuccess) die([NSString stringWithFormat:@"press failed: %d", err]);
        } else if ([cmd isEqual:@"inputs"]) {
            NSArray *all = CFBridgingRelease(TISCreateInputSourceList(NULL, true));
            for (id s in all) {
                TISInputSourceRef r = (__bridge TISInputSourceRef)s;
                print_json(@{
                    @"id" : (__bridge NSString *)TISGetInputSourceProperty(r, kTISPropertyInputSourceID),
                    @"category" : (__bridge NSString *)TISGetInputSourceProperty(r, kTISPropertyInputSourceCategory),
                    @"enabled" : @(CFBooleanGetValue(TISGetInputSourceProperty(r, kTISPropertyInputSourceIsEnabled))),
                    @"selected" : @(CFBooleanGetValue(TISGetInputSourceProperty(r, kTISPropertyInputSourceIsSelected))),
                });
            }
        } else if (([cmd isEqual:@"input-enable"] || [cmd isEqual:@"input-select"]) && argc == 3) {
            NSDictionary *q = @{(__bridge NSString *)kTISPropertyInputSourceID : @(argv[2])};
            NSArray *l = CFBridgingRelease(TISCreateInputSourceList((__bridge CFDictionaryRef)q, true));
            if (!l.count) die(@"no such input source");
            TISInputSourceRef r = (__bridge TISInputSourceRef)l[0];
            OSStatus st = [cmd isEqual:@"input-enable"] ? TISEnableInputSource(r) : TISSelectInputSource(r);
            if (st != noErr) die([NSString stringWithFormat:@"TIS call failed: %d", (int)st]);
        } else if ([cmd isEqual:@"modes"]) {
            CGDirectDisplayID d = CGMainDisplayID();
            NSDictionary *opt = @{(__bridge NSString *)kCGDisplayShowDuplicateLowResolutionModes : @YES};
            NSArray *modes = CFBridgingRelease(CGDisplayCopyAllDisplayModes(d, (__bridge CFDictionaryRef)opt));
            CGDisplayModeRef cur = CGDisplayCopyDisplayMode(d);
            for (id m in modes) {
                CGDisplayModeRef r = (__bridge CGDisplayModeRef)m;
                print_json(@{@"w" : @(CGDisplayModeGetWidth(r)), @"h" : @(CGDisplayModeGetHeight(r)),
                             @"pw" : @(CGDisplayModeGetPixelWidth(r)), @"ph" : @(CGDisplayModeGetPixelHeight(r)),
                             @"hz" : @(CGDisplayModeGetRefreshRate(r)),
                             @"current" : @(CGDisplayModeGetIODisplayModeID(r) == CGDisplayModeGetIODisplayModeID(cur))});
            }
            CGDisplayModeRelease(cur);
        } else if ([cmd isEqual:@"setmode"] && argc == 5) {
            CGDirectDisplayID d = CGMainDisplayID();
            size_t w = (size_t)atoi(argv[2]), h = (size_t)atoi(argv[3]);
            BOOL hidpi = atoi(argv[4]) != 0;
            NSDictionary *opt = @{(__bridge NSString *)kCGDisplayShowDuplicateLowResolutionModes : @YES};
            NSArray *modes = CFBridgingRelease(CGDisplayCopyAllDisplayModes(d, (__bridge CFDictionaryRef)opt));
            CGDisplayModeRef pick = NULL;
            for (id m in modes) {
                CGDisplayModeRef r = (__bridge CGDisplayModeRef)m;
                BOOL is_hidpi = CGDisplayModeGetPixelWidth(r) > CGDisplayModeGetWidth(r);
                if (CGDisplayModeGetWidth(r) == w && CGDisplayModeGetHeight(r) == h && is_hidpi == hidpi) pick = r;
            }
            if (!pick) die(@"no such mode");
            // A plain CGDisplaySetDisplayMode is undone when this process exits.
            CGDisplayConfigRef cfg;
            CGBeginDisplayConfiguration(&cfg);
            CGConfigureDisplayWithDisplayMode(cfg, d, pick, NULL);
            CGError err = CGCompleteDisplayConfiguration(cfg, kCGConfigurePermanently);
            if (err != kCGErrorSuccess) die([NSString stringWithFormat:@"display configuration: %d", err]);
            CGDisplayModeRef now = CGDisplayCopyDisplayMode(d);
            if (CGDisplayModeGetWidth(now) != w || CGDisplayModeGetHeight(now) != h ||
                (CGDisplayModeGetPixelWidth(now) > w) != hidpi)
                die([NSString stringWithFormat:@"mode did not change: now %zux%zu (%zux%zu px)", CGDisplayModeGetWidth(now),
                                               CGDisplayModeGetHeight(now), CGDisplayModeGetPixelWidth(now),
                                               CGDisplayModeGetPixelHeight(now)]);
            CGDisplayModeRelease(now);
            // Measured in the Tahoe guest: the window server undoes the
            // change when this process exits, even with
            // kCGConfigurePermanently, so the process stays until killed.
            printf("mode set; holding it until this process is stopped\n");
            fflush(stdout);
            for (;;) pause();
        } else if ([cmd isEqual:@"windows"]) {
            NSArray *l = CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID));
            for (NSDictionary *w in l) {
                NSDictionary *b = w[(id)kCGWindowBounds];
                print_json(@{@"owner" : w[(id)kCGWindowOwnerName] ?: @"", @"name" : w[(id)kCGWindowName] ?: @"",
                             @"pid" : w[(id)kCGWindowOwnerPID], @"layer" : w[(id)kCGWindowLayer],
                             @"b" : @[ b[@"X"], b[@"Y"], b[@"Width"], b[@"Height"] ]});
            }
        } else if ([cmd isEqual:@"activate"] && argc == 3) {
            NSRunningApplication *a = [NSRunningApplication runningApplicationWithProcessIdentifier:atoi(argv[2])];
            if (!a) die(@"no such application");
            if (![a activateWithOptions:NSApplicationActivateAllWindows]) die(@"activateWithOptions refused");
            usleep(300000);
            print_json(@{@"active" : @(a.active), @"policy" : @(a.activationPolicy)});
        } else if ([cmd isEqual:@"front"]) {
            NSRunningApplication *f = NSWorkspace.sharedWorkspace.frontmostApplication;
            print_json(@{@"name" : f.localizedName ?: @"", @"pid" : @(f.processIdentifier),
                         @"bundle" : f.bundleIdentifier ?: @""});
        } else usage();
    }
    return 0;
}
