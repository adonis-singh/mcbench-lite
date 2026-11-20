#!/usr/bin/env python3
"""A stand-in for a Codex install's `bin/codex` (Codex 0.167.1), as
test/fixtures/fake-codex.js plays it for the shims tests: every start links
apply_patch, in a folder under CODEX_HOME/tmp, to its own executable (the
program it was started as), except, as Codex does on Linux, when CODEX_HOME
is under the temporary folder; `--version` prints the version, `debug
models` a one-model catalogue, and `exec` the executable that ran."""
import json
import os
import sys
import tempfile

home = os.environ['CODEX_HOME']
args = ' '.join(sys.argv[1:])
executable = os.path.realpath(sys.argv[0])
os.makedirs(home, exist_ok=True)
temporary = os.path.realpath(os.environ.get('TMPDIR') or tempfile.gettempdir())
if (os.path.realpath(home) + '/').startswith(temporary + '/'):
    sys.stderr.write('WARNING: proceeding, even though we could not create PATH aliases: Refusing to create helper binaries under temporary dir "%s"\n' % temporary)
else:
    os.makedirs(os.path.join(home, 'tmp', 'arg0'), exist_ok=True)
    arg0 = tempfile.mkdtemp(prefix='codex-arg0', dir=os.path.join(home, 'tmp', 'arg0'))
    os.symlink(executable, os.path.join(arg0, 'apply_patch'))
if args == '--version':
    sys.stdout.write('codex-cli 0.167.1\n')
elif args == 'debug models':
    sys.stdout.write(json.dumps({'models': [{'slug': 'fake-model', 'display_name': 'Fake Model', 'visibility': 'list', 'priority': 1}]}))
elif sys.argv[1:2] == ['exec']:
    sys.stdout.write(json.dumps({'type': 'fake.ran', 'executable': executable}) + '\n')
else:
    sys.stderr.write('fake codex: unexpected arguments %s\n' % args)
    sys.exit(2)
