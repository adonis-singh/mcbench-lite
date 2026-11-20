#!/usr/bin/env python3
"""The cases of mcbench-agent's harness differential (JSON on stdout): one
chat per CLI, six turns through the chat route, the fake CLI's script built
with mcbench-providers' differential helpers (tests/diff/cases.py, loaded
without its own output).

Case: {cli, provider, model, script, turns: [{text, tools: [entry]}]}. A
tool entry is what the replay tool body answers, in call order; `stop`
stops the turn from inside that call (deterministically on both sides).
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
HELPERS = os.path.join(HERE, '..', '..', '..', 'providers', 'tests', 'diff', 'cases.py')
source = open(HELPERS).read().rsplit('json.dump(cases', 1)[0]
ns = {'__name__': 'cases'}
exec(compile(source, HELPERS, 'exec'), ns)

TOOLS = ['test_build.updates', 'build', 'see_build', 'bash.network', 'view_image', 'prompt_library']
BUILD = {'name': 'build', 'result': {'ok': True, 'worldRevision': 1, 'changed': False, 'diff': {'added': 0, 'removed': 0, 'changed': 0}, 'totalBlocks': 0, 'bounds': None, 'logs': []}}
BASH = {'name': 'bash', 'result': {'ok': True, 'text': '$ ls\nfile\n'}}
STOP = {'name': 'bash', 'stop': True, 'result': {'ok': False, 'cancelled': True, 'error': 'killed'}}

claude_turn, tool_steps, init, stream, assistant, usage, result = (ns[k] for k in ['claude_turn', 'tool_steps', 'init', 'stream', 'assistant', 'usage', 'result'])


def claude_batch(session, first, second):
    # Two calls in one CLI turn: each is a completion of the loop's.
    steps = [init(TOOLS, session=session), stream({'type': 'message_start', 'message': {}})]
    steps += tool_steps(first, key='r1')
    steps += tool_steps(second, name='bash', args={'command': 'ls'}, key='r2', index=2)
    steps += [{'reply': 'r2', 'prefix': 'Listed: '}, assistant([{'type': 'text', 'text': 'done'}], usage(2, 117, 7264, 48)), result(session)]
    return steps


def claude_stopped(session, tool_id):
    return [init(TOOLS, session=session), stream({'type': 'message_start', 'message': {}})] + tool_steps(tool_id, name='bash', args={'command': 'sleep 100'}) + [{'hang': True}]


claude = {
    'cli': 'claude', 'provider': 'claude-code', 'model': 'claude-opus-5-5',
    'script': {'mode': 'claude', 'spawns': [
        {'mcp': True, 'turns': [claude_turn(tools=TOOLS, tool_id='toolu_01'), claude_batch('sess-1', 'toolu_02', 'toolu_03'), claude_stopped('sess-1', 'toolu_04')]},
        {'mcp': True, 'turns': [claude_turn(tools=TOOLS, tool_id='toolu_05'), claude_turn(tools=TOOLS, tool_id='toolu_06'), claude_turn(tools=TOOLS, calls=False)]},
    ]},
    'turns': [
        {'text': 'Build a hut.', 'tools': [BUILD]},
        {'text': 'Add a door and look.', 'tools': [BUILD, BASH]},
        {'text': 'Wait a while.', 'tools': [STOP]},
        {'text': 'Carry on.', 'tools': [BUILD]},
        {'text': 'Make it taller.', 'tools': [BUILD]},
        {'text': 'Thanks.', 'tools': []},
    ],
}

codex_turn, codex_call, ev, item, completed, token_count = (ns[k] for k in ['codex_turn', 'codex_call', 'ev', 'item', 'completed', 'token_count'])


def codex_batch(thread):
    steps = [ev({'type': 'thread.started', 'thread_id': thread}), ev({'type': 'turn.started'})]
    steps += codex_call(key='r1') + codex_call(name='bash', args={'command': 'ls'}, key='r2')
    steps += [{'reply': 'r2', 'prefix': 'Listed: ', 'codex': True}, item('agent_message', text='Done.'),
              {'rollout': {'thread': thread, 'events': [token_count(5000, 0), token_count(None, 0)]}}, completed()]
    return steps


def codex_stopped(thread):
    return [ev({'type': 'thread.started', 'thread_id': thread}), ev({'type': 'turn.started'})] + codex_call(name='bash', args={'command': 'sleep 100'}, key='r1')[:3] + [{'hang': True}]


codex = {
    'cli': 'codex', 'provider': 'codex-cli', 'model': 'gpt-6-astra',
    'script': {'mode': 'codex', 'spawns': [
        {'mcp': True, 'turns': [codex_turn('th-1')], 'on_term': None},
        {'mcp': True, 'turns': [codex_batch('th-1')], 'on_term': None},
        {'mcp': True, 'turns': [codex_stopped('th-1')], 'on_term': None},
        {'mcp': True, 'turns': [codex_turn('th-1', append=True)], 'on_term': None},
        {'mcp': True, 'turns': [codex_turn('th-1', append=True)], 'on_term': None},
        {'mcp': True, 'turns': [codex_turn('th-1', calls=False, append=True)], 'on_term': None},
    ]},
    'turns': claude['turns'],
}

json.dump([claude, codex], sys.stdout, indent=1, ensure_ascii=False)
