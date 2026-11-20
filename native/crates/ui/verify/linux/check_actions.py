"""Actions and events: every control pressed through AT-SPI's Action
interface (Send, Stop, New chat, a chat row, the work fold, a tool-call row,
Copy code, a link, an image), each asserted in the dev binary's action log,
while an AT-SPI event listener records which events arrive for typing,
sending, a streaming turn and its end, focus moves and a row expanding
(events.json)."""

import json
import time

import common
from gi.repository import GLib

Atspi = common.Atspi
report = common.Report("actions")
events = []
started = time.time()

KINDS = [
    "focus:",
    "object:state-changed",
    "object:text-changed",
    "object:children-changed",
    "object:announcement",
    "object:property-change:accessible-name",
    "object:text-caret-moved",
    "object:text-selection-changed",
]


def on_event(event):
    try:
        source = event.source
        who = f"{source.get_role_name()} {source.get_name()!r}" if source else "?"
    except Exception:  # an object gone by the time the event is read
        who = "gone"
    events.append(
        {
            "t": round(time.time() - started, 3),
            "type": event.type,
            "detail1": event.detail1,
            "detail2": event.detail2,
            "source": who,
            "any": str(event.any_data)[:80] if event.any_data is not None else None,
        }
    )


listener = Atspi.EventListener.new(on_event)
for kind in KINDS:
    listener.register(kind)


def pump(seconds):
    """Lets the listener receive events for `seconds`."""
    context = GLib.MainContext.default()
    end = time.time() + seconds
    while time.time() < end:
        while context.pending():
            context.iteration(False)
        time.sleep(0.02)


def mark(label):
    events.append({"t": round(time.time() - started, 3), "type": "-- " + label})


def logged(action, **fields):
    return any(a["action"] == action and all(a.get(k) == v for k, v in fields.items()) for a in common.actions())


def press(obj):
    Atspi.Action.do_action(obj, 0)
    pump(0.6)


common.app()
pump(2.0)
common.focus_window()
pump(0.3)

mark("typing")
common.type_text("Add a third lamp")
pump(0.5)
app = common.app()
send = common.one(app, "button", "Send message")
report.check("Send is enabled with text", send.get_state_set().contains(Atspi.StateType.ENABLED))

mark("send")
press(send)
report.check("Send pressed through AT-SPI", logged("sent", text="Add a third lamp"))
pump(0.8)
app = common.app()
stop = common.find(app, "button", "Stop generating")
report.check("Stop is present while the turn streams", len(stop) == 1)
transcript = common.one(app, "landmark", "Conversation")
report.check("the transcript is busy while streaming", transcript.get_state_set().contains(Atspi.StateType.BUSY))

mark("stop")
press(stop[0])
report.check("Stop pressed through AT-SPI", logged("stop"))
pump(1.5)
app = common.app()
report.check("Stop is gone after the turn", not common.find(app, "button", "Stop generating"))
report.check("Send is back", len(common.find(app, "button", "Send message")) == 1)
transcript = common.one(app, "landmark", "Conversation")
report.check("the transcript is not busy after the turn", not transcript.get_state_set().contains(Atspi.StateType.BUSY))
status = common.one(app, "status bar")
report.check("the polite status holds the answer", len(status.get_name()) > 0, status.get_name()[:60])

mark("new chat")
press(common.one(app, "button", "New chat"))
report.check("New chat pressed through AT-SPI", logged("new chat"))

mark("chat row")
press(common.row(common.app(), "Castle gate"))
report.check("a chat row pressed through AT-SPI", logged("opened chat", chat="chat-gate"))
press(common.row(common.app(), "Redstone lamp with a lever"))
report.check("the first chat opened again", logged("opened chat", chat="chat-lamp"))
pump(0.5)

mark("fold")
# The first rows: focus a message through AT-SPI (its Focus action), then Home.
common.focus_window()
articles = common.find(common.app(), "article")
Atspi.Component.grab_focus(articles[-1])
pump(0.4)
report.check("a message takes focus through AT-SPI", articles[-1].get_state_set().contains(Atspi.StateType.FOCUSED))
common.key("Home", settle=0.5)
common.key("Down", settle=0.5)
fold = common.one(common.app(), "button", pred=lambda o: o.get_name().startswith("Worked"))
report.check("the fold is collapsed", fold.get_state_set().contains(Atspi.StateType.COLLAPSED))
press(fold)
fold = common.one(common.app(), "button", pred=lambda o: o.get_name().startswith("Worked"))
report.check("the fold expanded through AT-SPI", fold.get_state_set().contains(Atspi.StateType.EXPANDED))

mark("tool row")
tool = common.one(common.app(), "button", "Built: a lamp with a lever")
report.check("the tool row is collapsed", tool.get_state_set().contains(Atspi.StateType.COLLAPSED))
press(tool)
report.check("the tool row toggled through AT-SPI", logged("toggled", key="call-build", open=True))
tool = common.one(common.app(), "button", "Built: a lamp with a lever")
report.check("the tool row is expanded", tool.get_state_set().contains(Atspi.StateType.EXPANDED))
details = common.find(common.app(), pred=lambda o: o.get_name() == "Built: a lamp with a lever, code")
report.check("its details are labelled by the row", len(details) == 1, details[0].get_role_name() if details else "")

mark("copy code")
common.key("Down", settle=0.6)
copy = common.find(common.app(), "button", "Copy code")
report.check("Copy code present", len(copy) >= 1)
if copy:
    press(copy[0])
    report.check(
        "Copy code pressed through AT-SPI",
        any(a["action"] == "copied" and a["text"].startswith("fn build(") for a in common.actions()),
    )

mark("link")
link = common.find(common.app(), "link", "the wiki page on lamps")
report.check("the link is present with its address", len(link) == 1 and "Hyperlink" in link[0].get_interfaces())
if link:
    uri = link[0].get_hyperlink().get_uri(0) if hasattr(link[0], "get_hyperlink") else None
    report.check("the link's URI", uri == "https://minecraft.wiki/w/Redstone_Lamp", str(uri))
    press(link[0])
    report.check("the link activated through AT-SPI", logged("link activated", url="https://minecraft.wiki/w/Redstone_Lamp"))

mark("image")
common.key("End", settle=0.4)
common.key("Up", settle=0.4)
common.key("Up", settle=0.6)
image = common.find(common.app(), "image")
report.check("the image is present with its alt text", len(image) == 1 and image[0].get_name().startswith("The two lamps"), image[0].get_name() if image else "")
if image:
    press(image[0])
    report.check("the image activated through AT-SPI", any(a["action"] == "image activated" for a in common.actions()))
    # chat agent: the image opens in the lightbox, which Escape closes.
    common.key("Escape", settle=0.5)
    report.check("Escape closes the image's lightbox", logged("closed", layer="lightbox"))

mark("error")
FAILURE = "The provider refused the request: rate limited (a fixture error)"
press(common.row(common.app(), "Wheat farm"))
composer = common.one(common.app(), "entry", "Message")
Atspi.Component.grab_focus(composer)
pump(0.4)
common.type_text("hi")
common.key("Return", settle=1.0)
pump(0.8)
app = common.app()
alert = common.one(app, "notification")
report.check("the error is in the assertive region", alert.get_name() == FAILURE, alert.get_name())
report.check(
    "the error is announced",
    any(e["type"] == "object:announcement" and e["source"].startswith("notification") and FAILURE in (e["any"] or "") for e in events),
)
error = common.one(app, "article", pred=lambda o: o.get_name().startswith("Error, "))
paragraph = common.one(error, "paragraph")
report.check("the error item's text", common.atspi_tree.describe(paragraph).get("text") == FAILURE)
dismiss = common.one(app, "button", "Dismiss error")
press(dismiss)
report.check("Dismiss error pressed through AT-SPI", logged("dismissed error", text=FAILURE))
report.check(
    "the error item is gone",
    not common.find(common.app(), "article", pred=lambda o: o.get_name().startswith("Error, ")),
)

pump(0.5)
with open(common.out("events.json"), "w") as f:
    json.dump(events, f, indent=1, ensure_ascii=False)
counts = {}
for e in events:
    if not e["type"].startswith("--"):
        counts[e["type"]] = counts.get(e["type"], 0) + 1
with open(common.out("event-counts.json"), "w") as f:
    json.dump(counts, f, indent=1)
# AccessKit announces focus as object:state-changed:focused, as current
# toolkits do; the legacy focus: event is recorded when it comes.
for kind in ["object:state-changed:focused", "object:text-changed:insert", "object:children-changed:add", "object:state-changed:busy", "object:state-changed:expanded"]:
    report.check(f"events of {kind}", any(e["type"].startswith(kind) for e in events), str(counts.get(kind, "")))
report.check("an announcement or a name change of the live region", any(
    e["type"] == "object:announcement" or (e["type"].startswith("object:property-change:accessible-name") and e["source"].startswith("status bar"))
    for e in events
))
common.quit_key(settle=1.5)
report.finish()
