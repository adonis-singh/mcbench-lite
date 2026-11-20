#!/usr/bin/env python3
"""Compares the Node and Rust sides of the CLI harnesses' differential.

  python3 compare.py <dir> <repo> <rust bridge program>

Reads <dir>/node/<case>/out.json and <dir>/rust/<case>/out.json for every
case of <dir>/cases.json, normalises both by ONE rule and requires them
equal: every turn's deltas and messages or errors, every file under the
data folder's harness/ (session records byte for byte), and every line the
fake recorded (its command line, environment, working folder, files, stdin
and each MCP line sent and received).

The rule, applied to every string at every level (a string that is JSON, or
a `-c key=<JSON>` override, is decoded and normalised inside): the side's
folder (<dir>/node or <dir>/rust) is `<side>`; a temporary folder's six
random characters (`mcb-`, `mcbench-codex-`, `codex-install-`) are `XXXXXX`;
a UUID is `<uuid>`; the eight-character call prefix of a Codex call id is
`<prefix>`; and the bridge's command is `<bridge>`: Node's `python3
<repo>/server/mcp-bridge.py <socket>` and Rust's `<test binary> tool-bridge
<socket>` both become `<bridge> <socket>`. Nothing else is changed: the
environment is compared whole, so a variable one side passes and the other
does not is a difference.
"""
import json
import os
import re
import sys

dir, repo, exe = sys.argv[1:4]
only = sys.argv[4] if len(sys.argv) > 4 else ''
BRIDGE_PY = os.path.join(repo, 'server', 'mcp-bridge.py')
SIDES = [os.path.join(dir, 'node'), os.path.join(dir, 'rust')]
UUID = re.compile(r'[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}')
TEMP = re.compile(r'(mcb-|mcbench-codex-|codex-install-)[A-Za-z0-9]{6}')
PREFIX = re.compile(r'(?<![0-9a-f])[0-9a-f]{8}:(?=[^ ])')


def bridge(value):
    """`command` and `args` of an MCP server entry, either side's."""
    if isinstance(value, dict) and 'command' in value and 'args' in value:
        args = value['args']
        if value['command'] == 'python3' and args[:1] == [BRIDGE_PY]:
            return dict(value, command='<bridge>', args=args[1:])
        if value['command'] == exe and args[:1] == ['tool-bridge']:
            return dict(value, command='<bridge>', args=args[1:])
    return value


# The bridge inside the box: today's `python3 /run/mcbench/mcp-bridge.py`
# and the static agent's `/run/mcbench-relay tool-bridge`, in config.toml.
BOX_BRIDGES = [
    'mcp_servers.mcbench.command = "python3"\nmcp_servers.mcbench.args = ["/run/mcbench/mcp-bridge.py", ',
    'mcp_servers.mcbench.command = "/run/mcbench-relay"\nmcp_servers.mcbench.args = ["tool-bridge", ',
]


def text(s):
    for bridge_line in BOX_BRIDGES:
        s = s.replace(bridge_line, 'mcp_servers.mcbench.command = <bridge>\nmcp_servers.mcbench.args = [')
    for side in SIDES:
        s = s.replace(side, '<side>')
    s = TEMP.sub(lambda m: m.group(1) + 'XXXXXX', s)
    s = UUID.sub('<uuid>', s)
    s = PREFIX.sub('<prefix>:', s)
    return s


def norm(value):
    if isinstance(value, dict):
        return bridge({k: norm(v) for k, v in value.items()})
    if isinstance(value, list):
        out = [norm(v) for v in value]
        # Codex's overrides: the bridge's command and args are two `-c`s.
        for i, item in enumerate(out):
            if isinstance(item, str) and item.startswith('mcp_servers.mcbench.command='):
                command = json.loads(item.split('=', 1)[1])
                args_at = next((j for j, a in enumerate(out) if isinstance(a, str) and a.startswith('mcp_servers.mcbench.args=')), None)
                if args_at is not None:
                    args = json.loads(out[args_at].split('=', 1)[1])
                    fixed = bridge({'command': command, 'args': args})
                    out[i] = 'mcp_servers.mcbench.command=' + json.dumps(fixed['command'])
                    out[args_at] = 'mcp_servers.mcbench.args=' + json.dumps(fixed['args'])
        return out
    if isinstance(value, str):
        stripped = value.strip()
        if stripped[:1] in '{[' and stripped[-1:] in '}]':
            try:
                return {'<json>': norm(json.loads(value))}
            except ValueError:
                pass
        return text(value)
    return value


def toml_bridge(record):
    """The sandbox's config.toml has the bridge as two lines too."""
    return record


cases = json.load(open(os.path.join(dir, 'cases.json')))
rows = []
total = {'claude': [0, 0], 'codex': [0, 0]}
differences = []
for case in cases:
    name = case['name']
    if only not in name:
        continue
    sides = []
    for side in ('node', 'rust'):
        path = os.path.join(dir, side, name, 'out.json')
        sides.append(json.load(open(path)) if os.path.exists(path) else None)
    turns = len(case['turns'])
    total[case['cli']][0] += turns
    if None in sides:
        differences.append((name, 'missing output: ' + ', '.join(s for s, o in zip(('node', 'rust'), sides) if o is None)))
        continue
    node, rust = (norm(s) for s in sides)
    found = []
    for key in ('turns', 'harness', 'fake'):
        a, b = node[key], rust[key]
        if a != b:
            if isinstance(a, list) and isinstance(b, list):
                for i in range(max(len(a), len(b))):
                    x = a[i] if i < len(a) else None
                    y = b[i] if i < len(b) else None
                    if x != y:
                        found.append('%s[%d]:\n  node: %s\n  rust: %s' % (key, i, json.dumps(x)[:1500], json.dumps(y)[:1500]))
            else:
                ka, kb = set(a), set(b)
                for k in sorted(ka | kb):
                    if a.get(k) != b.get(k):
                        found.append('%s[%s]:\n  node: %s\n  rust: %s' % (key, k, json.dumps(a.get(k))[:1500], json.dumps(b.get(k))[:1500]))
    identical_turns = turns if not found else 0
    total[case['cli']][1] += identical_turns
    rows.append((name, turns, 'identical' if not found else 'DIFFERENT'))
    if found:
        differences.append((name, '\n'.join(found)))

for name, turns, state in rows:
    print('%-45s %2d turns  %s' % (name, turns, state))
print()
for cli, (turns, same) in total.items():
    print('%s: %d of %d turns in identical cases' % (cli, same, turns))
for name, text_ in differences:
    print('\n== %s\n%s' % (name, text_))
sys.exit(1 if differences else 0)
