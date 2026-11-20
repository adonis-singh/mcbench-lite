#!/usr/bin/env python3
"""A stand-in for `claude` (Claude Code 2.1) and `codex` (Codex CLI 0.169) in
mcbench-providers' tests and in the Node/Rust differential.

It is started as `<dir>/bin/<name>` and reads `<dir>/bin/script.json`:
`{"mode": "claude" | "codex", "spawns": [<spawn>, ...]}`, one entry per
start in order (the count of earlier starts is kept in `<dir>/bin/spawns`).
A spawn is `{"turns": [[step, ...], ...], "mcp": true, "on_term": null}`:
Claude reads one stream-json line per turn from stdin and runs the next
turn's steps; Codex reads stdin to its end, then runs `turns[0]`. With
`mcp`, it starts the bridge the way the real CLI does (Claude: the
`--mcp-config` command; Codex: `-c mcp_servers.mcbench.*` overrides, else
CODEX_HOME's config.toml) and sends `initialize`, `notifications/
initialized` and `tools/list` first. `on_term`: an exit code to leave with on
SIGTERM (default: die of the signal). `undumpable`: it makes itself not
dumpable first (prctl), as the kernel makes the real Codex, installed
execute-only, at exec (a script cannot be: its interpreter reads it).

Everything it saw goes to `<dir>/bin/record.jsonl`, one JSON object per line:
the start (argv, cwd, environment, the files of CODEX_HOME and of
Claude's working folder), each stdin turn, and every MCP line it sent and
received, as text.

Steps: {"emit": event} prints one JSON line; {"raw": text} prints text;
{"call": {name, arguments, meta?}, "as": key} sends tools/call and waits;
{"send": {name, arguments, meta?}, "as": key} sends it without waiting;
{"await": key} waits for a sent call; {"cancel": key} sends
notifications/cancelled for it; {"reply": key, "prefix": text, "codex":
bool} prints the result's text as an assistant reply; {"stderr": text};
{"sleep": ms}; {"hang": true} waits for SIGTERM; {"exit": code};
{"kill": signal} kills itself; {"rollout": {thread, events}} appends a
rollout file in CODEX_HOME; {"mcp_raw": text} writes a raw line to the
bridge and records the answer; {"mounts": true} prints, as Codex's reply,
every mount point it sees and CODEX_HOME's names; {"refresh": text}
rewrites CODEX_HOME/auth.json in place with text, as Codex's token refresh
writes it; {"command": [argv...]} runs a command the model asked for as
Codex runs one (its own child, stdin /dev/null, stdout and stderr captured,
no other descriptor) and prints, as Codex's reply, its stdout, its stderr
and `exit <code>`.
"""
import json
import os
import signal
import subprocess
import sys
import threading
import time

BIN = os.path.dirname(os.path.abspath(sys.argv[0]))
SCRIPT = json.load(open(os.path.join(BIN, 'script.json')))
ARGS = sys.argv[1:]

if ARGS == ['--version']:
    # As Codex starts: apply_patch linked, under CODEX_HOME/tmp, to its own
    # executable (codex-cli.js codexInstall reads it), except, as on Linux,
    # for a CODEX_HOME under the temporary folder.
    import tempfile
    home = os.environ.get('CODEX_HOME')
    if home:
        temporary = os.path.realpath(os.environ.get('TMPDIR') or tempfile.gettempdir())
        if not (os.path.realpath(home) + '/').startswith(temporary + '/'):
            os.makedirs(os.path.join(home, 'tmp', 'arg0'), exist_ok=True)
            link = tempfile.mkdtemp(prefix='codex-arg0', dir=os.path.join(home, 'tmp', 'arg0'))
            os.symlink(os.path.realpath(sys.argv[0]), os.path.join(link, 'apply_patch'))
    sys.stdout.write('codex-cli 0.169.0\n')
    sys.exit(0)

# Inside Codex's box the install (and this folder) is read-only: the
# record goes to CODEX_HOME's fake-record/ there.
RECORDS = BIN if os.access(BIN, os.W_OK) else os.path.join(os.environ['CODEX_HOME'], 'fake-record')
os.makedirs(RECORDS, exist_ok=True)
RECORD = os.path.join(RECORDS, 'record.jsonl')
SPAWNS = os.path.join(RECORDS, 'spawns')
lock = threading.Lock()


def record(entry):
    with lock:
        with open(RECORD, 'a') as f:
            f.write(json.dumps(entry, ensure_ascii=False, sort_keys=True) + '\n')


def emit(event):
    sys.stdout.write(json.dumps(event, ensure_ascii=False, separators=(',', ':')) + '\n')
    sys.stdout.flush()


def tree(root):
    """Names, link targets and small files' text under `root`."""
    out = {}
    if not root or not os.path.isdir(root):
        return out
    for folder, dirs, files in os.walk(root):
        dirs.sort()
        dirs[:] = [d for d in dirs if d != 'fake-record']
        for name in sorted(dirs + [f for f in files]):
            if name == 'fake-record':
                continue
            path = os.path.join(folder, name)
            key = os.path.relpath(path, root)
            if os.path.islink(path):
                out[key] = {'link': os.readlink(path)}
            elif os.path.isdir(path):
                out[key] = {'dir': True}
            elif not os.path.isfile(path):
                out[key] = {'special': True}
            else:
                data = open(path, 'rb').read()
                try:
                    out[key] = {'text': data.decode('utf-8')} if len(data) < 65536 else {'bytes': len(data)}
                except UnicodeDecodeError:
                    out[key] = {'bytes': len(data), 'head': list(data[:16])}
    return out


count = int(open(SPAWNS).read()) if os.path.exists(SPAWNS) else 0
open(SPAWNS, 'w').write(str(count + 1))
spawns = SCRIPT['spawns']
spawn = spawns[min(count, len(spawns) - 1)]
mode = SCRIPT['mode']
if spawn.get('undumpable'):
    import ctypes
    if ctypes.CDLL(None, use_errno=True).prctl(4, 0, 0, 0, 0) != 0:  # PR_SET_DUMPABLE
        raise SystemExit('fake: prctl(PR_SET_DUMPABLE, 0): errno %d' % ctypes.get_errno())
on_term = spawn.get('on_term')
if on_term is not None:
    signal.signal(signal.SIGTERM, lambda *_: os._exit(on_term))

record({'start': {
    'spawn': count,
    'argv': ARGS,
    'cwd': os.getcwd(),
    'env': dict(os.environ),
    'codex_home': tree(os.environ.get('CODEX_HOME')) if mode == 'codex' else None,
    'cwd_files': tree(os.getcwd()),
}})


def flag(name):
    return ARGS[ARGS.index(name) + 1] if name in ARGS else None


def bridge_command():
    if mode == 'claude':
        server = json.loads(flag('--mcp-config'))['mcpServers']['mcbench']
        return server['command'], server['args']
    overrides = {}
    config = os.path.join(os.environ.get('CODEX_HOME', ''), 'config.toml')
    if os.path.exists(config):
        for line in open(config).read().split('\n'):
            if ' = ' in line:
                key, value = line.split(' = ', 1)
                overrides[key] = value
    for i, arg in enumerate(ARGS):
        if i > 0 and ARGS[i - 1] == '-c' and '=' in arg:
            key, value = arg.split('=', 1)
            overrides[key] = value
    if 'mcp_servers.mcbench.command' not in overrides:
        return None
    return json.loads(overrides['mcp_servers.mcbench.command']), json.loads(overrides['mcp_servers.mcbench.args'])


class Mcp:
    def __init__(self, command, args):
        self.proc = subprocess.Popen([command] + args, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.next = 0
        self.replies = {}
        self.cond = threading.Condition()
        threading.Thread(target=self.read, daemon=True, name='mcp-reader').start()

    def read(self):
        for raw in self.proc.stdout:
            line = raw.decode('utf-8').rstrip('\n')
            record({'mcp_in': line})
            message = json.loads(line)
            with self.cond:
                self.replies[json.dumps(message.get('id'))] = message
                self.cond.notify_all()

    def write(self, text):
        record({'mcp_out': text})
        self.proc.stdin.write((text + '\n').encode('utf-8'))
        self.proc.stdin.flush()

    def send(self, method, params):
        self.next += 1
        request = {'jsonrpc': '2.0', 'id': self.next, 'method': method, 'params': params}
        self.write(json.dumps(request, ensure_ascii=False, separators=(',', ':')))
        return self.next

    def notify(self, method, params):
        self.write(json.dumps({'jsonrpc': '2.0', 'method': method, 'params': params}, separators=(',', ':')))

    def wait(self, id):
        key = json.dumps(id)
        with self.cond:
            while key not in self.replies:
                if self.proc.poll() is not None and not self.reader_alive():
                    raise SystemExit('fake: the bridge exited before answering')
                self.cond.wait(0.2)
            return self.replies.pop(key)

    def reader_alive(self):
        return any(t.name == 'mcp-reader' and t.is_alive() for t in threading.enumerate())

    def call(self, method, params):
        return self.wait(self.send(method, params))


mcp = None
if spawn.get('mcp'):
    command = bridge_command()
    if command:
        mcp = Mcp(*command)
        mcp.call('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {}, 'clientInfo': {'name': 'fake-' + mode, 'version': '1'}})
        mcp.notify('notifications/initialized', {})
        mcp.call('tools/list', {})

sent = {}
results = {}


def call_params(spec):
    params = {'name': spec['name'], 'arguments': spec.get('arguments', {})}
    if 'meta' in spec:
        params['_meta'] = spec['meta']
    return params


def run(steps):
    for step in steps:
        if 'emit' in step:
            emit(step['emit'])
        elif 'raw' in step:
            sys.stdout.write(step['raw'] + '\n')
            sys.stdout.flush()
        elif 'call' in step:
            results[step.get('as', 'last')] = mcp.call('tools/call', call_params(step['call']))
        elif 'send' in step:
            sent[step['as']] = mcp.send('tools/call', call_params(step['send']))
        elif 'await' in step:
            results[step['await']] = mcp.wait(sent[step['await']])
        elif 'cancel' in step:
            mcp.notify('notifications/cancelled', {'requestId': sent[step['cancel']], 'reason': 'test'})
        elif 'mcp_raw' in step:
            mcp.write(step['mcp_raw'])
            if step.get('wait'):
                mcp.wait(step['wait'])
        elif 'reply' in step:
            reply = results[step['reply']]
            if 'result' in reply:
                text = '|'.join(c.get('text', '<' + c.get('type', '') + '>') for c in reply['result']['content'])
            else:
                text = 'error: ' + reply['error']['message']
            text = step.get('prefix', '') + text
            if step.get('codex'):
                emit({'type': 'item.completed', 'item': {'id': 'item_reply', 'type': 'agent_message', 'text': text}})
            else:
                emit({'type': 'stream_event', 'event': {'type': 'content_block_delta', 'index': 9, 'delta': {'type': 'text_delta', 'text': text}}})
        elif 'mounts' in step:
            points = [line.split(' ')[4] for line in open('/proc/self/mountinfo').read().splitlines()]
            names = sorted(os.listdir(os.environ['CODEX_HOME']))
            emit({'type': 'item.completed', 'item': {'id': 'item_mounts', 'type': 'agent_message', 'text': '\n'.join(['mount ' + p for p in points] + ['home ' + n for n in names])}})
        elif 'refresh' in step:
            with open(os.path.join(os.environ['CODEX_HOME'], 'auth.json'), 'w') as f:
                f.write(step['refresh'])
        elif 'probe' in step:
            # Inside Codex's box: what the network and the limits allow.
            import socket
            out = []
            for host in step['probe']:
                try:
                    s = socket.create_connection(('127.0.0.1', 3128), 5)
                    s.sendall(('CONNECT %s:443 HTTP/1.1\r\nHost: %s:443\r\n\r\n' % (host, host)).encode())
                    out.append('%s %s' % (host, s.recv(200).decode().splitlines()[0]))
                    s.close()
                except OSError as e:
                    out.append('%s error %s' % (host, e))
            for target in [('1.1.1.1', 443), ('example.com', 443)]:
                try:
                    socket.create_connection(target, 5)
                    out.append('direct %s open' % target[0])
                except OSError:
                    out.append('direct %s refused' % target[0])
            started = 0
            try:
                for _ in range(700):
                    threading.Thread(target=time.sleep, args=(5,), daemon=True).start()
                    started += 1
            except RuntimeError:
                pass
            out.append('threads %d' % started)
            emit({'type': 'item.completed', 'item': {'id': 'item_probe', 'type': 'agent_message', 'text': '\n'.join(out)}})
        elif 'command' in step:
            ran = subprocess.run(step['command'], stdin=subprocess.DEVNULL, capture_output=True)
            text = ran.stdout.decode('utf-8', 'replace') + ran.stderr.decode('utf-8', 'replace') + 'exit %d' % ran.returncode
            emit({'type': 'item.completed', 'item': {'id': 'item_command', 'type': 'agent_message', 'text': text}})
        elif 'stderr' in step:
            sys.stderr.write(step['stderr'])
            sys.stderr.flush()
        elif 'sleep' in step:
            time.sleep(step['sleep'] / 1000)
        elif 'hang' in step:
            while True:
                time.sleep(3600)
        elif 'exit' in step:
            sys.stdout.flush()
            os._exit(step['exit'])
        elif 'kill' in step:
            sys.stdout.flush()
            os.kill(os.getpid(), getattr(signal, step['kill']))
        elif 'rollout' in step:
            spec = step['rollout']
            folder = os.path.join(os.environ['CODEX_HOME'], 'sessions', '2026', '11', '11')
            os.makedirs(folder, exist_ok=True)
            with open(os.path.join(folder, 'rollout-2026-11-11T12-00-00-%s.jsonl' % spec['thread']), 'a') as f:
                for event in spec['events']:
                    f.write(json.dumps(event, separators=(',', ':')) + '\n')
        else:
            raise SystemExit('fake: unknown step %r' % step)


def code_mode_turn():
    """A code_mode_only model's turn as Codex 0.167.1 and 0.169.0 run it
    (test/fixtures/fake-codex.js): with features.code_mode_host off, `exec`
    fails with Codex's error item; otherwise the script calls each tool the
    mcbench server lists, with no arguments. From a config file, the
    deprecated features.multi_agent is warned about in an error item."""
    config = os.path.join(os.environ.get('CODEX_HOME', ''), 'config.toml')
    in_file = {}
    if os.path.exists(config):
        for line in open(config).read().split('\n'):
            if ' = ' in line:
                key, value = line.split(' = ', 1)
                in_file[key] = value
    overrides = dict(in_file)
    for i, arg in enumerate(ARGS):
        if i > 0 and ARGS[i - 1] == '-c' and '=' in arg:
            key, value = arg.split('=', 1)
            overrides[key] = value
    emit({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-00000000000b'})
    if 'features.multi_agent' in in_file:
        emit({'type': 'item.completed', 'item': {'id': 'item_warning', 'type': 'error', 'message': '`[features].multi_agent` is deprecated.'}})
    end = lambda: emit({'type': 'turn.completed', 'usage': {'input_tokens': 1, 'cached_input_tokens': 0, 'output_tokens': 1, 'reasoning_output_tokens': 0}})
    if overrides.get('features.code_mode_host') == 'false':
        emit({'type': 'item.completed', 'item': {'id': 'item_0', 'type': 'error', 'message': 'Code Mode is unavailable because code-mode host is disabled.'}})
        return end()
    if mcp is None:
        return end()
    tools = mcp.call('tools/list', {})['result']['tools']
    for index, tool in enumerate(tools):
        item = {'id': 'item_%d' % (index + 1), 'type': 'mcp_tool_call', 'server': 'mcbench', 'tool': tool['name'], 'arguments': {}}
        emit({'type': 'item.started', 'item': dict(item, status='in_progress')})
        reply = mcp.call('tools/call', {'name': tool['name'], 'arguments': {}})
        emit({'type': 'item.completed', 'item': dict(item, status='completed', result=reply.get('result'))})
    end()


turns = spawn['turns']
if mode == 'codex':
    prompt = sys.stdin.read()
    record({'stdin': prompt})
    if spawn.get('code_mode'):
        code_mode_turn()
    run(turns[0] if turns else [])
else:
    index = 0
    for raw in sys.stdin:
        line = raw.rstrip('\n')
        record({'stdin': line})
        if index < len(turns):
            run(turns[index])
        index += 1
if mcp:
    mcp.proc.stdin.close()
    mcp.proc.wait()
sys.stdout.flush()
os._exit(0)
