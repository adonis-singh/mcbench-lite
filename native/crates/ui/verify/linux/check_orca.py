"""A screen reader: Orca (built from its source into a folder given by
ORCA_PREFIX, with its Python dependencies in ORCA_PYTHONPATH) runs on the
private session's AT-SPI bus with its debug log on, while real keys walk
the chat: the Tab walk, the chat list, the transcript message by message,
into a message's fold and link, Orca's structural navigation (H for
headings, K for links, L for lists, T for tables) and its "where am I".
Orca has no speech output here (no speech-dispatcher); its debug log
records each utterance it would speak, and orca-speech.txt is those lines
in order (the transcript of what Orca says). X11 only: Orca reads keys
through the X server."""

import os
import re
import subprocess
import time

import common

report = common.Report("orca")
prefix = os.environ["ORCA_PREFIX"]
python_path = os.environ["ORCA_PYTHONPATH"]
site = [os.path.join(prefix, "lib", d, "site-packages") for d in os.listdir(os.path.join(prefix, "lib")) if d.startswith("python")]
env = dict(os.environ)
env["PYTHONPATH"] = ":".join([python_path, *site])
env["GSETTINGS_SCHEMA_DIR"] = os.path.join(prefix, "share", "glib-2.0", "schemas")
env["XDG_CONFIG_HOME"] = common.out("orca-config")
env["XDG_DATA_HOME"] = common.out("orca-data")
debug = common.out("orca-debug.txt")

common.app()
time.sleep(2.0)
orca = subprocess.Popen(
    [os.path.join(prefix, "bin", "orca"), "--replace", "--debug-file", debug],
    env=env,
    stdout=open(common.out("orca.log"), "w"),
    stderr=subprocess.STDOUT,
)
time.sleep(6.0)
report.check("Orca runs", orca.poll() is None, f"exit {orca.poll()}")
common.focus_window()
time.sleep(1.0)


def say(keys, settle=0.8):
    for key in keys:
        common.key(key, settle=settle)


steps = [
    ("the Tab walk", ["Tab", "Tab", "Tab", "Tab"]),
    ("the chat list", ["shift+Tab", "shift+Tab", "Down", "Up"]),
    ("the transcript, message by message", ["Tab", "Home", "Down", "Down", "Up"]),
    # The answer, block by block (Right, Left), then back to the work row.
    ("block by block", ["Down", "Right", "Right", "Right", "Left", "Up"]),
    ("into the work fold", ["Tab", "Return", "Tab", "Tab"]),
    ("headings", ["Escape", "h", "h", "shift+h"]),
    ("links, lists, tables", ["k", "l", "t"]),
    ("where am I", ["KP_Enter"]),
]
for label, keys in steps:
    with open(debug, "a") as f:
        f.write(f"\n==== MCBENCH STEP: {label}\n")
    say(keys)

time.sleep(1.0)
orca.terminate()
try:
    orca.wait(timeout=10)
except subprocess.TimeoutExpired:
    orca.kill()

log = open(debug, errors="replace").read() if os.path.exists(debug) else ""
spoken = []
for line in log.splitlines():
    if line.startswith("==== MCBENCH STEP"):
        spoken.append(line)
        continue
    match = re.search(r"SPEECH OUTPUT: '(.*?)'(\{|$)", line)
    if match:
        spoken.append("  " + match.group(1))
with open(common.out("orca-speech.txt"), "w") as f:
    f.write("\n".join(spoken) + "\n")
utterances = [s for s in spoken if s.startswith("  ")]
report.check("Orca's log records utterances", len(utterances) > 10, f"{len(utterances)} utterances")
text = "\n".join(utterances)
# A focused message is read with its text, once.
for expected in ["New chat", "Redstone lamp with a lever", "Message", "claude-opus-5-5", "Worked", "You, Build a redstone lamp that turns on with a lever",
                 # Right and Left read each block on its own.
                 "heading level 1", "A lamp on a lever", "heading level 2", "What was placed",
                 "paragraph", "The lamp is built and wired"]:
    report.check(f"Orca says {expected!r}", expected in text)
# Each block is read once: its kind, then its text, not the text twice.
blocks = [u.strip() for u in utterances if re.match(r"\s*(paragraph|heading level \d|You|claude-opus-5-5), ", u)]


def body(utterance):
    """The block's text after its kind."""
    return re.sub(r"^(paragraph|heading level \d|You|claude-opus-5-5), ", "", utterance)


twice = [b for b in blocks if b.count(body(b)[:12]) > 1]
report.check("each message and block is read once", bool(blocks) and not twice, f"{len(blocks)} blocks; read twice: {twice[:2]}")
common.quit_key(settle=1.5)
report.finish()
