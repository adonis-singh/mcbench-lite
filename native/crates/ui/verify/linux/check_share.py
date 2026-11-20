"""The Share panel (share agent) through AT-SPI, real keys and a real
pointer, in a private session, on chat-gate (`--open /chat/chat-gate`).

    check_share.py <theme>               by keyboard: Share opens the panel with
                                         focus on Publish; Return publishes; the
                                         three steps in order; the link, Copy
                                         link, Post on X (the log's opener line);
                                         Escape closes it, focus back on Share.
                                         By pointer: Share opens it, a press
                                         outside closes it; Remove asks first,
                                         then the offer again
    check_share.py fails-<theme>         --share fails-publishing: the push's
                                         failure as an alert, Try again
    check_share.py not-joined-<theme>    --share not-joined: the real core's
                                         sentence as the failed publish, Try again

The theme (`dark` or `light`, from the run's --preferences) names the
pictures, `share-<state>-<theme>-<backend>.png` in the run's folder. The
privacy proof is session.sh's, before and after."""

import sys
import time

import common
from common import Atspi

mode = sys.argv[1] if len(sys.argv) > 1 else "dark"
theme = mode.split("-")[-1]
play = mode[: -len(theme) - 1] if "-" in mode else ""
report = common.Report("share-" + mode)
app = common.app()
time.sleep(2.0)
common.focus_window()

OFFER = "Publishes this build's 3D view to a public page on mcbench-builds/gallery, to post on X."
NOT_JOINED = "Sharing has not joined the native app yet"
PUSH_FAILED = "Could not get a copy of mcbench-builds/gallery from GitHub"


def names():
    return [o.get_name() or "" for o in common.find(app)]


def has(name, prefix=False):
    return any((n.startswith(name) if prefix else n == name) for n in names())


def one(name):
    found = [o for o in common.find(app) if (o.get_name() or "") == name and o.get_role_name() != "label"]
    if not found:
        found = common.find(app, name=name)
    if not found:
        raise AssertionError(f"no {name!r}")
    return found[-1]


def focused():
    """The focused element's name."""
    return common.focused(app).get_name() or ""


def by_key(name, settle=0.6):
    Atspi.Component.grab_focus(one(name))
    time.sleep(0.5)
    common.key("Return", settle=settle)


def shot(state):
    common.screenshot(f"share-{state}-{theme}-{common.BACKEND}.png")


def centre(obj):
    r = Atspi.Component.get_extents(obj, Atspi.CoordType.WINDOW)
    return r.x + r.width // 2, r.y + r.height // 2


def click(obj):
    x, y = centre(obj)
    common.pointer("mousemove", str(x), str(y))
    time.sleep(0.2)
    common.pointer("click", "1")
    time.sleep(0.8)


def open_by_key():
    Atspi.Component.grab_focus(one("Share"))
    time.sleep(0.3)
    common.key("Return", settle=1.0)


common.wait_for(lambda: has("Share"), 30, "the Share button")

if play == "":
    open_by_key()
    common.wait_for(lambda: has("Share this build"), 5, "the panel")
    common.wait_for(lambda: has(OFFER), 5, "the offer")
    report.check("Return on Share opens the panel with focus on Publish", focused() == "Publish", focused())
    shot("offer")
    common.key("Return", settle=0.2)
    common.wait_for(lambda: has("Uploading…"), 8, "Uploading…")
    shot("publishing")
    common.wait_for(lambda: has("Shared page link"), 12, "the link")
    seen = [a.get("phase") for a in common.actions() if a.get("action") == "share state"]
    report.check("the three steps in order, then shared", seen[-4:] == ["exporting", "publishing", "waiting", "shared"], repr(seen))
    common.wait_for(lambda: has("Shared page link"), 8, "the link")
    link = one("Shared page link")
    url = Atspi.Text.get_text(link, 0, -1) if link.get_text_iface() else ""
    report.check("the link holds the page's address", "mcbench-builds.github.io/gallery/b/" in (url or "") or has("Copy link"), repr(url))
    shot("shared")
    by_key("Copy link", settle=0.2)
    copied = has("Copied")
    clip = common.clipboard()
    report.check("Copy link reads Copied", copied, "")
    report.check("the clipboard holds the address", "mcbench-builds.github.io/gallery/b/" in clip, repr(clip))
    by_key("Post on X", settle=1.0)
    opened = [a for a in common.actions() if a.get("action") == "link activated" and "x.com/intent/post?url=" in str(a.get("url", ""))]
    report.check("Post on X opens the X post", bool(opened), repr(opened[-1:] if opened else None))
    common.key("Escape", settle=0.8)
    report.check("Escape closes the panel", not has("Share this build"))
    report.check("focus back on Share", focused() == "Share", focused())
    # By pointer.
    click(one("Share"))
    report.check("a press on Share opens it", has("Share this build"))
    common.pointer("mousemove", "640", "700")
    common.pointer("click", "1")
    time.sleep(0.8)
    report.check("a press outside closes it", not has("Share this build"))
    click(one("Share"))
    click(one("Remove"))
    report.check("Remove asks first", has("Remove the page"))
    shot("remove-confirm")
    click(one("Remove the page"))
    common.wait_for(lambda: has(OFFER), 5, "the offer again")
    report.check("removed: the offer again", has("Publish"))
elif play == "fails":
    open_by_key()
    common.wait_for(lambda: has(OFFER), 5, "the offer")
    common.key("Return", settle=0.2)
    common.wait_for(lambda: has(PUSH_FAILED, prefix=True), 10, "the failure")
    report.check("the push's failure is said, with Try again", has("Try again"))
    shot("failed")
    by_key("Try again", settle=0.3)
    report.check("Try again publishes anew", has("Preparing the build…") or has("Uploading…"))
elif play == "not-joined":
    open_by_key()
    common.wait_for(lambda: has(OFFER), 5, "the offer")
    shot("not-joined-offer")
    common.key("Return", settle=0.8)
    report.check("the core's sentence as a failed publish", has(NOT_JOINED))
    report.check("with Try again", has("Try again"))
    shot("not-joined")
else:
    raise SystemExit(f"check_share.py: unknown mode {mode}")

common.quit_key(settle=1.5)
report.finish()
