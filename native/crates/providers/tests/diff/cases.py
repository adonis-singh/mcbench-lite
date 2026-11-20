#!/usr/bin/env python3
"""Writes the differential matrix of the CLI harnesses (JSON on stdout).

Each case is one chat (or one-shot completion) driven turn by turn through
today's Node module (node-driver.js) and the Rust provider (tests/cli.rs,
MCBENCH_DIFF_HARNESS), each running tests/fakes/fake_cli.py with the case's
script. A turn is the agent's loop: completions until one answers without a
tool call, each tool call answered from `answers` as the turn loop stores
it (a tool message, and `see_build`'s screenshots after it when given).

Case: {name, cli, script, records?, missing_binary?, turns: [turn]}.
Turn: {conversation: {id, transcript?} | null, messages, model, effort, chat,
tools: [tool file names], answers: [{content, screenshots?, delay?}],
cancel: null | {on: started | tool_call | content}}.
"""
import json
import sys

PNG = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=='
JPG = 'data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAP////8='
SYSTEM = "You build in Minecraft.\n\nKeep it tidy: \"quotes\", back\\slashes and a tab\there."
LARGE = SYSTEM + "\n\n## Resolution\nThis chat's resolution is Large: build bigger."
OTHER = "You build carefully."
TITLE = 'Name this chat in 2 to 6 words. Return only the title, with no quotes or punctuation.'
CHAT_TOOLS = ['build', 'see_build', 'bash.offline', 'view_image', 'prompt_library']
BENCH_TOOLS = ['build', 'see_build', 'bash.offline']
BUILTINS = ['TaskCreate', 'TaskUpdate', 'TaskList', 'TaskGet']


def tool_names(files):
    return [f.split('.')[0] for f in files]


def sys_msg(text=SYSTEM):
    return {'role': 'system', 'content': text}


def user(text):
    return {'role': 'user', 'content': text}


def entry(id, role, **extra):
    out = {'id': id, 'role': role}
    out.update(extra)
    return out


def turn(conversation, messages, tools=CHAT_TOOLS, model='claude-opus-5-5', effort=None, chat=True, answers=None, cancel=None):
    return {'conversation': conversation, 'messages': messages, 'model': model, 'effort': effort, 'chat': chat,
            'tools': tools, 'answers': answers or [], 'cancel': cancel}


# ---- Claude Code -----------------------------------------------------------

def init(tools, builtins=True, extra=(), status='connected', session='sess-1'):
    allowed = ['mcp__mcbench__' + n for n in tool_names(tools)] + (BUILTINS if builtins else []) + list(extra)
    return {'emit': {'type': 'system', 'subtype': 'init', 'cwd': '/ignored', 'tools': allowed,
                     'mcp_servers': [{'name': 'mcbench', 'status': status}], 'session_id': session, 'model': 'claude-opus-5-5'}}


def stream(event):
    return {'emit': {'type': 'stream_event', 'event': event}}


def usage(i, w, r, o=10):
    return {'input_tokens': i, 'cache_creation_input_tokens': w, 'cache_read_input_tokens': r, 'output_tokens': o}


def assistant(content, u):
    return {'emit': {'type': 'assistant', 'message': {'content': content, 'usage': u}}}


def result(session='sess-1', thinking=1, extra=None):
    event = {'type': 'result', 'subtype': 'success', 'is_error': False, 'session_id': session, 'result': 'Built',
             'total_cost_usd': 0.01,
             'usage': {'input_tokens': 6, 'cache_creation_input_tokens': 7381, 'cache_read_input_tokens': 14411,
                       'output_tokens': 108, 'output_tokens_details': {'thinking_tokens': thinking}, 'iterations': []}}
    if extra:
        event.update(extra)
    return {'emit': event}


def tool_steps(tool_id, name='build', args=None, key='r1', index=1):
    args = args if args is not None else {'summary': 's', 'program': 'mc.set(0, 0, 0, "stone")'}
    text = json.dumps(args, separators=(',', ':'))
    cut = len(text) // 2
    return [
        stream({'type': 'content_block_start', 'index': index, 'content_block': {'type': 'tool_use', 'id': tool_id, 'name': 'mcp__mcbench__' + name, 'input': {}}}),
        stream({'type': 'content_block_delta', 'index': index, 'delta': {'type': 'input_json_delta', 'partial_json': text[:cut]}}),
        stream({'type': 'content_block_delta', 'index': index, 'delta': {'type': 'input_json_delta', 'partial_json': text[cut:]}}),
        stream({'type': 'content_block_stop', 'index': index}),
        assistant([{'type': 'tool_use', 'id': tool_id, 'name': 'mcp__mcbench__' + name, 'input': args}], usage(4, 7264, 7147, 60)),
        {'call': {'name': name, 'arguments': args, 'meta': {'claudecode/toolUseId': tool_id, 'progressToken': 7}}, 'as': key},
        {'emit': {'type': 'user', 'message': {'role': 'user', 'content': [{'type': 'tool_result', 'tool_use_id': tool_id, 'content': 'ok'}]}}},
    ]


def claude_turn(tools=CHAT_TOOLS, builtins=True, session='sess-1', tool_id='toolu_01', reply_prefix='Built: ', calls=True):
    steps = [init(tools, builtins, session=session), stream({'type': 'message_start', 'message': {}}),
             stream({'type': 'content_block_start', 'index': 0, 'content_block': {'type': 'thinking', 'thinking': ''}}),
             stream({'type': 'content_block_delta', 'index': 0, 'delta': {'type': 'thinking_delta', 'thinking': 'Plan the walls.'}})]
    if calls:
        steps += tool_steps(tool_id)
        steps += [stream({'type': 'message_start', 'message': {}}), {'reply': 'r1', 'prefix': reply_prefix}]
    else:
        steps += [stream({'type': 'content_block_delta', 'index': 1, 'delta': {'type': 'text_delta', 'text': 'Here it is.'}})]
    steps += [assistant([{'type': 'text', 'text': 'done'}], usage(2, 117, 7264, 48)), result(session)]
    return steps


def claude_script(*spawns):
    return {'mode': 'claude', 'spawns': [{'mcp': True, 'turns': list(s)} for s in spawns]}


OK = [{'content': '{"ok":true}'}]
HUT = [entry('u1', 'user')]
HUT_MSG = [sys_msg(), user('Build a hut.')]


def second(transcript_extra=(), last='u2', text='Make it taller.'):
    return HUT + [entry('a1', 'assistant'), entry('t1', 'tool'), entry('a2', 'assistant')] + list(transcript_extra) + [entry(last, 'user')]


SECOND_MSG = HUT_MSG + [
    {'role': 'assistant', 'content': '', 'tool_calls': [{'id': 'toolu_h1', 'type': 'function', 'function': {'name': 'build', 'arguments': '{"summary":"s"}'}}]},
    {'role': 'tool', 'tool_call_id': 'toolu_h1', 'content': '{"ok":true}'},
    {'role': 'assistant', 'content': 'Built: {"ok":true}'},
    user('Make it taller.'),
]


def claude_cases():
    cases = []
    conv = lambda id, transcript: {'id': id, 'transcript': transcript}
    cases.append({'name': 'claude-first-turn-chat-high', 'cli': 'claude', 'script': claude_script([claude_turn()]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG, effort='high', answers=OK)]})
    title = [{'role': 'system', 'content': TITLE}, user('Name this chat from its first message, quoted below. Do not answer or carry out the message.\n\n<message>\nBuild a hut.\n</message>')]
    cases.append({'name': 'claude-one-shot-title', 'cli': 'claude',
                  'script': claude_script([claude_turn(tools=[], builtins=False, calls=False)]),
                  'turns': [turn(None, title, tools=[], effort='low', chat=False)]})
    cases.append({'name': 'claude-warm-follow-ups', 'cli': 'claude',
                  'script': claude_script([claude_turn(), claude_turn(tool_id='toolu_02')], [claude_turn(tool_id='toolu_03')]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG, answers=OK),
                            turn(conv('c1', second()), SECOND_MSG, answers=[{'content': 'taller'}]),
                            turn(conv('c1', second([entry('u2b', 'user'), entry('a3', 'assistant')], last='u3')),
                                 SECOND_MSG + [{'role': 'assistant', 'content': 'Taller'}, user('Add a door.')], model='claude-sonnet-5-5', answers=OK)]})
    history = HUT_MSG + [
        {'role': 'assistant', 'content': 'Starting.', 'tool_calls': [{'id': 'call-1', 'type': 'function', 'function': {'name': 'build', 'arguments': '{"summary":"hut"}'}},
                                                                     {'id': 'call-2', 'type': 'function', 'function': {'name': 'see_build', 'arguments': ''}}]},
        {'role': 'tool', 'tool_call_id': 'call-1', 'content': '{"ok":true}'},
        {'role': 'tool', 'tool_call_id': 'call-2', 'content': 'shot'},
        {'role': 'user', 'seeBuild': True, 'content': [{'type': 'text', 'text': '[see_build] Screenshots of the current build:'}, {'type': 'image_url', 'image_url': {'url': PNG}}]},
        {'role': 'assistant', 'content': 'Done.'},
        {'role': 'user', 'content': [{'type': 'text', 'text': 'Make the roof dark oak.'}, {'type': 'image_url', 'image_url': {'url': JPG}}, {'type': 'text', 'text': ''}]},
    ]
    revert_transcript = HUT + [entry('a1', 'assistant'), entry('t1', 'tool'), entry('t2', 'tool'), entry('s1', 'user', seeBuild=True), entry('a2', 'assistant'), entry('u9', 'user')]
    cases.append({'name': 'claude-revert-renders-history', 'cli': 'claude', 'script': claude_script([claude_turn()]),
                  'records': [{'conv': 'c1', 'record': {'sessionId': 'sess-old', 'cwd': '$CWD', 'instructions': '$INSTRUCTIONS', 'resolution': '$RESOLUTION', 'lastSeen': 'gone', 'tasks': []}}],
                  'turns': [turn(conv('c1', revert_transcript), history, answers=OK)]})
    stopped_transcript = HUT + [entry('a1', 'assistant'), entry('s1', 'user', seeBuild=True), entry('t1', 'tool'), entry('a2', 'assistant', stopped=True), entry('u2', 'user')]
    cases.append({'name': 'claude-stopped-at-tool-then-resumed', 'cli': 'claude',
                  'script': claude_script([claude_turn()], [claude_turn(tool_id='toolu_02')]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG, cancel={'on': 'tool_call'}),
                            turn(conv('c1', stopped_transcript), HUT_MSG + [user('Carry on.')], answers=OK)]})
    reply_transcript = HUT + [entry('a1', 'assistant', stopped=True, reply='First the floor, then the'), entry('u2', 'user')]
    cases.append({'name': 'claude-stopped-reply-note', 'cli': 'claude', 'script': claude_script([claude_turn()]),
                  'records': [{'conv': 'c1', 'record': {'sessionId': 'sess-1', 'cwd': '$CWD', 'instructions': '$INSTRUCTIONS', 'resolution': '$RESOLUTION', 'lastSeen': 'u1', 'tasks': []}}],
                  'turns': [turn(conv('c1', reply_transcript), HUT_MSG + [{'role': 'assistant', 'content': '(stopped by user)'}, user('continue')], answers=OK)]})
    large = [sys_msg(LARGE)] + SECOND_MSG[1:]
    t3 = second([entry('u2b', 'user'), entry('a3', 'assistant')], last='u3')
    t4 = second([entry('u2b', 'user'), entry('a3', 'assistant'), entry('u3b', 'user'), entry('a4', 'assistant')], last='u4')
    cases.append({'name': 'claude-resolution-change', 'cli': 'claude',
                  'script': claude_script([claude_turn(), claude_turn(tool_id='toolu_02'), claude_turn(tool_id='toolu_03')], [claude_turn(tool_id='toolu_04', session='sess-2')]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG, answers=OK),
                            turn(conv('c1', second()), large, answers=OK),
                            turn(conv('c1', t3), [sys_msg(LARGE), user('Add a door.')], answers=OK),
                            turn(conv('c1', t4), [sys_msg(OTHER)] + SECOND_MSG[1:-1] + [user('Add a roof.')], answers=OK)]})
    cases.append({'name': 'claude-built-in-tool-in-init', 'cli': 'claude',
                  'script': {'mode': 'claude', 'spawns': [{'mcp': True, 'turns': [[init(CHAT_TOOLS, extra=['Bash', 'Read']), {'hang': True}]]}]},
                  'turns': [turn(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'claude-mcp-server-failed', 'cli': 'claude',
                  'script': {'mode': 'claude', 'spawns': [{'mcp': True, 'turns': [[init(CHAT_TOOLS, status='failed'), {'hang': True}]]}]},
                  'turns': [turn(conv('c1', HUT), HUT_MSG)]})
    error_result = [init(CHAT_TOOLS), stream({'type': 'message_start', 'message': {}}),
                    {'emit': {'type': 'result', 'subtype': 'error_max_turns', 'is_error': True, 'session_id': 'sess-1', 'result': 'Reached the turn limit'}}]
    failed_transcript = HUT + [entry('e1', 'error'), entry('u2', 'user')]
    cases.append({'name': 'claude-error-result-then-resumed', 'cli': 'claude',
                  'script': claude_script(error_result, [claude_turn(tool_id='toolu_02')]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG), turn(conv('c1', failed_transcript), HUT_MSG + [user('Try again.')], answers=OK)]})
    no_usage = claude_turn()[:-1] + [{'emit': {'type': 'result', 'subtype': 'success', 'is_error': False, 'session_id': 'sess-1', 'result': 'Built'}}]
    cases.append({'name': 'claude-result-without-usage', 'cli': 'claude',
                  'script': claude_script(no_usage, [claude_turn(tool_id='toolu_02')]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG, answers=OK), turn(conv('c1', failed_transcript), HUT_MSG + [user('Try again.')], answers=OK)]})
    crash = [init(CHAT_TOOLS), stream({'type': 'message_start', 'message': {}}),
             stream({'type': 'content_block_delta', 'index': 0, 'delta': {'type': 'text_delta', 'text': 'Halfway'}}),
             {'stderr': 'segfault in the fake\n'}, {'exit': 3}]
    cases.append({'name': 'claude-crash-mid-stream', 'cli': 'claude',
                  'script': claude_script(crash, [claude_turn(tool_id='toolu_02')]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG), turn(conv('c1', failed_transcript), HUT_MSG + [user('Try again.')], answers=OK)]})

    def task_created(id, subject, call):
        return {'emit': {'type': 'user', 'message': {'content': [{'type': 'tool_result', 'tool_use_id': call}]}, 'tool_use_result': {'task': {'id': id, 'subject': subject}}}}

    def task_update(call, update, success=True):
        return [assistant([{'type': 'tool_use', 'id': call, 'name': 'TaskUpdate', 'input': update}], usage(1, 0, 0, 1)),
                {'emit': {'type': 'user', 'message': {'content': [{'type': 'tool_result', 'tool_use_id': call}]}, 'tool_use_result': {'success': success, 'taskId': update['taskId']}}}]

    events = [init(CHAT_TOOLS),
              {'emit': {'type': 'system', 'subtype': 'status', 'status': 'compacting'}},
              {'emit': {'type': 'system', 'subtype': 'api_retry', 'attempt': 2, 'max_retries': 10, 'error_status': 529}},
              {'emit': {'type': 'system', 'subtype': 'api_retry', 'attempt': 3, 'max_retries': 10, 'error_status': None}},
              {'emit': {'type': 'system', 'subtype': 'status', 'status': 'requesting'}},
              {'emit': {'type': 'system', 'subtype': 'compact_boundary'}},
              {'emit': {'type': 'system', 'subtype': 'model_fallback', 'level': 'warning', 'content': 'Switched to Claude Sonnet 5.5'}},
              {'emit': {'type': 'system', 'subtype': 'informational', 'content': 'A note'}},
              {'emit': {'type': 'system', 'subtype': 'informational', 'content': ''}},
              {'emit': {'type': 'system', 'subtype': 'notification', 'color': 'error', 'text': 'Credit low'}},
              {'emit': {'type': 'system', 'subtype': 'notification', 'text': 'Heads up'}},
              {'emit': {'type': 'rate_limit_event', 'rate_limit_info': {'status': 'allowed_warning', 'rateLimitType': 'five_hour', 'resetsAt': 1791118200, 'utilization': 0.9}}},
              task_created('1', 'Lay the floor', 'c1'), task_created('2', 'Raise the walls', 'c2'), task_created('3', 'Roof', 'c3')]
    events += task_update('u1', {'taskId': '1', 'status': 'completed'})
    events += task_update('u2', {'taskId': '2', 'status': 'in_progress', 'activeForm': 'Raising the walls'})
    events += task_update('u3', {'taskId': '9', 'status': 'completed'})
    events += task_update('u4', {'taskId': '3', 'subject': 'Pitched roof'})
    events += task_update('u5', {'taskId': '1', 'status': 'pending'}, success=False)
    events += [stream({'type': 'content_block_delta', 'index': 1, 'delta': {'type': 'text_delta', 'text': 'Planned.'}}),
               assistant([{'type': 'text', 'text': 'Planned.'}], usage(3, 120, 7026)), result()]
    later = [init(CHAT_TOOLS)] + task_update('u6', {'taskId': '2', 'status': 'deleted'}) + [task_created('4', 'Door', 'c4')] + task_update('u7', {'taskId': '4', 'status': 'in_progress'}) + [
        stream({'type': 'content_block_delta', 'index': 1, 'delta': {'type': 'text_delta', 'text': 'More.'}}), assistant([{'type': 'text', 'text': 'More.'}], {'input_tokens': 2}), result()]
    cases.append({'name': 'claude-events-notices-tasks', 'cli': 'claude', 'script': claude_script(events, later),
                  'turns': [turn(conv('c1', HUT), HUT_MSG), turn(conv('c1', HUT + [entry('a1', 'assistant'), entry('u2', 'user')]), HUT_MSG + [{'role': 'assistant', 'content': 'Planned.'}, user('Go on.')])]})
    image_request = [sys_msg(), {'role': 'user', 'content': [{'type': 'text', 'text': 'Build this.'}, {'type': 'image_url', 'image_url': {'url': PNG}}, {'type': 'image_url', 'image_url': {'url': JPG}}]}]
    cases.append({'name': 'claude-images-in-and-out', 'cli': 'claude', 'script': claude_script([claude_turn()]),
                  'turns': [turn(conv('c1', HUT), image_request, answers=[{'content': '{"ok":true}', 'screenshots': [{'type': 'text', 'text': '[see_build] Screenshots:'}, {'type': 'image_url', 'image_url': {'url': PNG}}]}])]})
    efforts = ['low', 'medium', 'high', 'xhigh', 'max']
    spawns = [[claude_turn(tool_id='toolu_e%d' % i)] for i in range(5)]
    eturns = []
    transcript = HUT
    for i, effort in enumerate(efforts):
        if i:
            transcript = transcript + [entry('a%d' % i, 'assistant'), entry('u%d' % (i + 1), 'user')]
        eturns.append(turn(conv('c1', transcript), HUT_MSG if i == 0 else HUT_MSG + [{'role': 'assistant', 'content': 'ok'}, user('Again at %s.' % effort)], effort=effort, answers=OK))
    cases.append({'name': 'claude-each-effort', 'cli': 'claude', 'script': claude_script(*spawns), 'turns': eturns})
    two = [init(CHAT_TOOLS), stream({'type': 'message_start', 'message': {}})] + tool_steps('toolu_a', key='ra') + [
        stream({'type': 'message_start', 'message': {}}),
        stream({'type': 'content_block_start', 'index': 1, 'content_block': {'type': 'tool_use', 'id': 'toolu_b', 'name': 'mcp__mcbench__see_build', 'input': {}}}),
        stream({'type': 'content_block_start', 'index': 2, 'content_block': {'type': 'tool_use', 'id': 'toolu_c', 'name': 'mcp__mcbench__bash', 'input': {}}}),
        stream({'type': 'content_block_delta', 'index': 2, 'delta': {'type': 'input_json_delta', 'partial_json': '{"command":"ls"}'}}),
        stream({'type': 'content_block_delta', 'index': 1, 'delta': {'type': 'input_json_delta', 'partial_json': '{"size":"small","n":2.50}'}}),
        assistant([{'type': 'tool_use', 'id': 'toolu_b', 'name': 'mcp__mcbench__see_build', 'input': {'size': 'small', 'n': 2.5}},
                   {'type': 'tool_use', 'id': 'toolu_c', 'name': 'mcp__mcbench__bash', 'input': {'command': 'ls'}}], usage(1, 2, 3)),
        {'send': {'name': 'see_build', 'arguments': {'size': 'small', 'n': 2.50, '10': 'x', 'big': 12345678901234567890}, 'meta': {'claudecode/toolUseId': 'toolu_b'}}, 'as': 'rb'},
        {'send': {'name': 'bash', 'arguments': {'command': 'ls'}, 'meta': {'claudecode/toolUseId': 'toolu_c'}}, 'as': 'rc'},
        {'await': 'rb'}, {'await': 'rc'},
        {'reply': 'rb', 'prefix': 'Saw: '}, {'reply': 'rc', 'prefix': ' Listed: '},
        assistant([{'type': 'text', 'text': 'x'}], usage(5, 6, 7)), result()]
    cases.append({'name': 'claude-sequential-and-parallel-calls', 'cli': 'claude', 'script': claude_script(two),
                  'turns': [turn(conv('c1', HUT), HUT_MSG, answers=[{'content': 'built'}, {'content': 'seen', 'screenshots': [{'type': 'image_url', 'image_url': {'url': PNG}}]}, {'content': 'a\nb'}])]})
    cases.append({'name': 'claude-benchmark-attempt', 'cli': 'claude', 'script': claude_script([claude_turn(tools=BENCH_TOOLS, builtins=False)]),
                  'turns': [turn(conv('attempt-1', [entry('p1', 'user')]), [sys_msg(), user('Build a clock.')], tools=BENCH_TOOLS, chat=False, effort='max', answers=OK)]})
    stale = [init(CHAT_TOOLS), assistant([{'type': 'tool_use', 'id': 'toolu_stale', 'name': 'mcp__mcbench__build', 'input': {}}], usage(1, 1, 1)),
             stream({'type': 'content_block_delta', 'index': 1, 'delta': {'type': 'text_delta', 'text': 'Never mind.'}}), assistant([{'type': 'text', 'text': 'x'}], usage(1, 1, 1)), result()]
    cases.append({'name': 'claude-stale-tool-use-block', 'cli': 'claude', 'script': claude_script([stale, claude_turn(tool_id='toolu_02')]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG), turn(conv('c1', second()), SECOND_MSG, answers=OK)]})
    cases.append({'name': 'claude-stopped-when-started', 'cli': 'claude',
                  'script': {'mode': 'claude', 'spawns': [{'mcp': True, 'turns': [[init(CHAT_TOOLS), {'hang': True}]]}, {'mcp': True, 'turns': [claude_turn(tool_id='toolu_02')]}]},
                  'turns': [turn(conv('c1', HUT), HUT_MSG, cancel={'on': 'started'}),
                            turn(conv('c1', HUT + [entry('a1', 'assistant', stopped=True), entry('u2', 'user')]), HUT_MSG + [user('Carry on.')], answers=OK)]})
    cancelled = [init(CHAT_TOOLS)] + tool_steps('toolu_a', key='ra') + [
        assistant([{'type': 'tool_use', 'id': 'toolu_b', 'name': 'mcp__mcbench__build', 'input': {}}, {'type': 'tool_use', 'id': 'toolu_c', 'name': 'mcp__mcbench__build', 'input': {}}], usage(1, 1, 1)),
        {'send': {'name': 'build', 'arguments': {'summary': 'b'}, 'meta': {'claudecode/toolUseId': 'toolu_b'}}, 'as': 'rb'},
        {'send': {'name': 'build', 'arguments': {'summary': 'c'}, 'meta': {'claudecode/toolUseId': 'toolu_c'}}, 'as': 'rc'},
        {'cancel': 'rc'}, {'await': 'rb'}, {'await': 'rc'},
        {'reply': 'rc', 'prefix': 'C: '}, assistant([{'type': 'text', 'text': 'x'}], usage(1, 1, 1)), result()]
    cases.append({'name': 'claude-mcp-cancelled-call', 'cli': 'claude', 'script': claude_script(cancelled),
                  'turns': [turn(conv('c1', HUT), HUT_MSG, answers=[{'content': 'a'}, {'content': 'b', 'delay': 400}])]})
    cases.append({'name': 'claude-missing-binary', 'cli': 'claude', 'missing_binary': True, 'script': claude_script([]),
                  'turns': [turn(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'claude-one-shot-image-title', 'cli': 'claude',
                  'script': claude_script([claude_turn(tools=[], builtins=False, calls=False)]),
                  'turns': [turn(None, [{'role': 'system', 'content': TITLE}, {'role': 'user', 'content': [{'type': 'text', 'text': 'Name this Minecraft build from the attached image.'}, {'type': 'image_url', 'image_url': {'url': PNG}}]}], tools=[], chat=False, effort='low')]})
    cases.append({'name': 'claude-garbage-stdout-line', 'cli': 'claude',
                  'script': {'mode': 'claude', 'spawns': [{'mcp': True, 'turns': [[init(CHAT_TOOLS), {'raw': 'not json at all'}, {'hang': True}]]}]},
                  'turns': [turn(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'claude-old-folder-record', 'cli': 'claude', 'script': claude_script([claude_turn()]),
                  'records': [{'conv': 'c1', 'record': {'sessionId': 'sess-old', 'lastSeen': 'u1', 'tasks': []}}],
                  'turns': [turn(conv('c1', second()), SECOND_MSG, answers=OK)]})
    cases.append({'name': 'claude-chat-without-transcript', 'cli': 'claude', 'script': claude_script([claude_turn()]),
                  'turns': [turn({'id': 'c1'}, HUT_MSG, answers=OK)]})
    cases.append({'name': 'claude-continue-after-tool-results', 'cli': 'claude', 'script': claude_script([claude_turn()]),
                  'turns': [turn(conv('c1', HUT + [entry('a1', 'assistant'), entry('t1', 'tool'), entry('s1', 'user', seeBuild=True)]),
                                 HUT_MSG + [{'role': 'assistant', 'content': '', 'tool_calls': [{'id': 'x1', 'type': 'function', 'function': {'name': 'see_build', 'arguments': '{}'}}]},
                                            {'role': 'tool', 'tool_call_id': 'x1', 'content': 'shot'},
                                            {'role': 'user', 'seeBuild': True, 'content': [{'type': 'image_url', 'image_url': {'url': PNG}}]}], answers=OK)]})
    return cases


# ---- Codex CLI -------------------------------------------------------------

def token_count(i, cached):
    return {'type': 'event_msg', 'timestamp': '2026-11-11T12:00:01.000Z', 'payload': {'type': 'token_count', 'rate_limits': {}, 'info': None if i is None else {
        'total_token_usage': {'input_tokens': 999999, 'cached_input_tokens': 0, 'output_tokens': 1, 'reasoning_output_tokens': 0, 'total_tokens': 1000000},
        'last_token_usage': {'input_tokens': i, 'cached_input_tokens': cached, 'cache_write_input_tokens': 0, 'output_tokens': 283, 'reasoning_output_tokens': 65, 'total_tokens': i + 283},
        'last_usage_includes_reasoning': True, 'model_context_window': 258400}}}


def ev(event):
    return {'emit': event}


def item(kind, event='item.completed', **fields):
    body = {'id': 'item_' + kind, 'type': kind}
    body.update(fields)
    return ev({'type': event, 'item': body})


def completed(i=1200, cached=1024):
    return ev({'type': 'turn.completed', 'usage': {'input_tokens': i, 'cached_input_tokens': cached, 'output_tokens': 66, 'reasoning_output_tokens': 12}})


def codex_call(name='build', args=None, key='r1', meta=None):
    args = args if args is not None else {'summary': 's', 'program': 'mc.set(1,2,3,"oak_planks")'}
    spec = {'name': name, 'arguments': args}
    if meta:
        spec['meta'] = meta
    base = {'id': 'item_' + key, 'type': 'mcp_tool_call', 'server': 'mcbench', 'tool': name, 'arguments': args}
    # Codex's stdout and its MCP calls are two channels with no handshake
    # (Claude Code's tool_use block is one): an item printed just before a
    # call can reach the relay after it, in Node as in Rust. The pause keeps
    # the matrix about the code, not about that race.
    return [ev({'type': 'item.started', 'item': dict(base, status='in_progress')}), {'sleep': 50}, {'call': spec, 'as': key},
            ev({'type': 'item.completed', 'item': dict(base, status='completed')})]


def codex_turn(thread='01a1f500-0000-7000-8000-000000000001', calls=True, append=False, requests=((40000, 0), (98982, 81408))):
    steps = [ev({'type': 'thread.started', 'thread_id': thread}), ev({'type': 'turn.started'}),
             item('reasoning', text='**Planning** the hut')]
    if calls:
        steps += codex_call() + [{'reply': 'r1', 'prefix': 'Built: ', 'codex': True}]
    steps += [item('agent_message', text='Done.'),
              {'rollout': {'thread': thread, 'events': ([] if append else [{'type': 'session_meta', 'payload': {'id': thread}}, token_count(None, 0)]) + [token_count(i, c) for i, c in requests] + [token_count(None, 0)]}},
              completed()]
    return steps


def codex_script(*spawns, on_term=None):
    return {'mode': 'codex', 'spawns': [{'mcp': True, 'turns': [list(s)], 'on_term': on_term} for s in spawns]}


def codex_cases():
    cases = []
    conv = lambda id, transcript: {'id': id, 'transcript': transcript}
    T = lambda *a, **k: turn(*a, model='gpt-6-astra', **k)
    cases.append({'name': 'codex-first-turn-chat-low', 'cli': 'codex', 'script': codex_script(codex_turn()),
                  'turns': [T(conv('c1', HUT), HUT_MSG, effort='low', answers=OK)]})
    cases.append({'name': 'codex-resumed-thread', 'cli': 'codex', 'script': codex_script(codex_turn(), codex_turn(append=True, requests=((120500, 99000),))),
                  'turns': [T(conv('c1', HUT), HUT_MSG, answers=OK), T(conv('c1', second()), SECOND_MSG, answers=OK)]})
    title = [{'role': 'system', 'content': TITLE}, user('Name this chat from its first message, quoted below. Do not answer or carry out the message.\n\n<message>\nBuild a hut.\n</message>')]
    cases.append({'name': 'codex-one-shot-title', 'cli': 'codex', 'script': codex_script(codex_turn(calls=False)),
                  'turns': [T(None, title, tools=[], chat=False, effort='low')]})
    history = HUT_MSG + [
        {'role': 'assistant', 'content': 'Starting.', 'tool_calls': [{'id': 'p:call-1', 'type': 'function', 'function': {'name': 'build', 'arguments': '{"summary":"hut"}'}}]},
        {'role': 'tool', 'tool_call_id': 'p:call-1', 'content': '{"ok":true}'},
        {'role': 'user', 'seeBuild': True, 'content': [{'type': 'text', 'text': '[see_build] Screenshots:'}, {'type': 'image_url', 'image_url': {'url': PNG}}]},
        {'role': 'assistant', 'content': 'Done.'},
        {'role': 'user', 'content': [{'type': 'text', 'text': 'Make the roof dark oak.'}, {'type': 'image_url', 'image_url': {'url': JPG}}]},
    ]
    revert = HUT + [entry('a1', 'assistant'), entry('t1', 'tool'), entry('s1', 'user', seeBuild=True), entry('a2', 'assistant'), entry('u9', 'user')]
    cases.append({'name': 'codex-revert-renders-history-and-images', 'cli': 'codex', 'script': codex_script(codex_turn(thread='01a1f500-0000-7000-8000-00000000000c')),
                  'records': [{'conv': 'c1', 'harness': 'codex-cli', 'record': {'threadId': '01a1f500-0000-7000-8000-00000000000d', 'home': 'chat', 'instructions': '$INSTRUCTIONS', 'resolution': '$RESOLUTION', 'lastSeen': 'gone'}}],
                  'turns': [T(conv('c1', revert), history, answers=OK)]})
    large = [sys_msg(LARGE)] + SECOND_MSG[1:]
    cases.append({'name': 'codex-resolution-change', 'cli': 'codex', 'script': codex_script(codex_turn(), codex_turn(append=True)),
                  'turns': [T(conv('c1', HUT), HUT_MSG, answers=OK), T(conv('c1', second()), large, answers=OK)]})
    cases.append({'name': 'codex-other-instructions-new-thread', 'cli': 'codex', 'script': codex_script(codex_turn(), codex_turn(thread='01a1f500-0000-7000-8000-000000000002')),
                  'turns': [T(conv('c1', HUT), HUT_MSG, answers=OK), T(conv('c1', second()), [sys_msg(OTHER)] + SECOND_MSG[1:], answers=OK)]})
    cases.append({'name': 'codex-shell-command-ends-run', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), item('command_execution', event='item.started', command='ls'), {'hang': True}]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'codex-foreign-mcp-server-ends-run', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), item('mcp_tool_call', event='item.started', server='github', tool='x'), {'hang': True}], on_term=0),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    failed = HUT + [entry('e1', 'error'), entry('u2', 'user')]
    cases.append({'name': 'codex-turn-failed-then-resumed', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), ev({'type': 'turn.failed', 'error': {'message': 'usage limit reached'}})], codex_turn(append=True)),
                  'turns': [T(conv('c1', HUT), HUT_MSG), T(conv('c1', failed), HUT_MSG + [user('Try again.')], answers=OK)]})
    cases.append({'name': 'codex-error-event', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), ev({'type': 'error', 'message': 'stream disconnected'})]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'codex-error-event-without-message', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), ev({'type': 'error', 'code': 7, 'ratio': 0.5})]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'codex-exit-code', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), {'stderr': 'Error: config is broken\n'}, {'exit': 2}]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'codex-killed-mid-stream', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), item('agent_message', text='Half'), {'kill': 'SIGKILL'}]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'codex-stopped-when-started', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), {'hang': True}], codex_turn(append=True), on_term=0),
                  'turns': [T(conv('c1', HUT), HUT_MSG, cancel={'on': 'started'}),
                            T(conv('c1', HUT + [entry('a1', 'assistant', stopped=True), entry('u2', 'user')]), HUT_MSG + [user('Carry on.')], answers=OK)]})
    plan = [ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}),
            item('todo_list', event='item.started', items=[{'text': 'Floor', 'completed': False}, {'text': 'Walls', 'completed': False}]),
            item('reasoning', text='first'), item('reasoning', text='second'),
            item('todo_list', event='item.updated', items=[{'text': 'Floor', 'completed': True}, {'text': 'Walls', 'completed': False}]),
            item('error', message='Falling back to a smaller model'), item('error', message=''),
            item('agent_message', text='One.'), item('agent_message', text='Two.'),
            item('todo_list', items=[{'text': 'Floor', 'completed': True}, {'text': 'Walls', 'completed': True}]),
            {'rollout': {'thread': '01a1f500-0000-7000-8000-000000000001', 'events': [token_count(5000, 0)]}}, completed()]
    cases.append({'name': 'codex-plan-notices-messages', 'cli': 'codex', 'script': codex_script(plan),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    image_request = [sys_msg(), {'role': 'user', 'content': [{'type': 'text', 'text': 'Build this.'}, {'type': 'image_url', 'image_url': {'url': PNG}}, {'type': 'image_url', 'image_url': {'url': JPG}}, {'type': 'image_url', 'image_url': {'url': 'data:image/webp;base64,UklGRg=='}}]}]
    cases.append({'name': 'codex-images-in-request', 'cli': 'codex', 'script': codex_script(codex_turn()),
                  'turns': [T(conv('c1', HUT), image_request, answers=OK)]})
    efforts = ['minimal', 'low', 'medium', 'high', 'xhigh']
    eturns = []
    transcript = HUT
    for i, effort in enumerate(efforts):
        if i:
            transcript = transcript + [entry('a%d' % i, 'assistant'), entry('u%d' % (i + 1), 'user')]
        eturns.append(T(conv('c1', transcript), HUT_MSG if i == 0 else HUT_MSG + [{'role': 'assistant', 'content': 'ok'}, user('Again at %s.' % effort)], effort=effort, answers=OK))
    cases.append({'name': 'codex-each-effort', 'cli': 'codex', 'script': codex_script(codex_turn(), *[codex_turn(append=True) for _ in range(4)]), 'turns': eturns})
    cases.append({'name': 'codex-no-rollout-file', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), item('agent_message', text='Done.'), completed()]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'codex-rollout-without-usage', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), item('agent_message', text='Done.'),
                                          {'rollout': {'thread': '01a1f500-0000-7000-8000-000000000001', 'events': [{'type': 'session_meta', 'payload': {'id': '01a1f500-0000-7000-8000-000000000001'}}, token_count(None, 0)]}}, completed()]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    two = [ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'})] + codex_call(key='ra') + codex_call(name='see_build', args={'size': 'small', 'ratio': 1e-7, 'n': 1.0}, key='rb') + [
        {'reply': 'ra', 'prefix': 'A: ', 'codex': True}, {'reply': 'rb', 'prefix': 'B: ', 'codex': True},
        {'rollout': {'thread': '01a1f500-0000-7000-8000-000000000001', 'events': [token_count(7, 1)]}}, completed()]
    cases.append({'name': 'codex-two-calls-with-screenshots', 'cli': 'codex', 'script': codex_script(two),
                  'turns': [T(conv('c1', HUT), HUT_MSG, answers=[{'content': 'built'}, {'content': [{'type': 'text', 'text': 'seen'}], 'screenshots': [{'type': 'text', 'text': 'shots'}, {'type': 'image_url', 'image_url': {'url': PNG}}]}])]})
    cancelled = [ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'})] + codex_call(key='ra') + [
        {'send': {'name': 'build', 'arguments': {'summary': 'b'}}, 'as': 'rb'},
        {'send': {'name': 'build', 'arguments': {'summary': 'c'}}, 'as': 'rc'},
        {'cancel': 'rc'}, {'await': 'rb'}, {'await': 'rc'}, {'reply': 'rc', 'prefix': 'C: ', 'codex': True},
        {'rollout': {'thread': '01a1f500-0000-7000-8000-000000000001', 'events': [token_count(9, 1)]}}, completed()]
    cases.append({'name': 'codex-mcp-cancelled-call', 'cli': 'codex', 'script': codex_script(cancelled),
                  'turns': [T(conv('c1', HUT), HUT_MSG, answers=[{'content': 'a'}, {'content': 'b', 'delay': 400}])]})
    meta = [ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'})] + codex_call(key='ra', meta={'callId': 'call_abc'}) + codex_call(key='rb', meta={'callId': 42}) + [
        {'reply': 'ra', 'codex': True}, {'rollout': {'thread': '01a1f500-0000-7000-8000-000000000001', 'events': [token_count(3, 0)]}}, completed()]
    cases.append({'name': 'codex-call-ids-from-meta', 'cli': 'codex', 'script': codex_script(meta),
                  'turns': [T(conv('c1', HUT), HUT_MSG, answers=[{'content': 'a'}, {'content': 'b'}])]})
    stopped = HUT + [entry('a1', 'assistant'), entry('t1', 'tool'), entry('a2', 'assistant', stopped=True), entry('u2', 'user')]
    cases.append({'name': 'codex-stopped-at-tool-then-resumed', 'cli': 'codex', 'script': codex_script(codex_turn(), codex_turn(append=True), on_term=0),
                  'turns': [T(conv('c1', HUT), HUT_MSG, cancel={'on': 'tool_call'}), T(conv('c1', stopped), HUT_MSG + [user('Carry on.')], answers=OK)]})
    cases.append({'name': 'codex-benchmark-attempt', 'cli': 'codex', 'script': codex_script(codex_turn()),
                  'turns': [T({'id': 'attempt-1'}, [sys_msg(), user('Build a clock.')], tools=BENCH_TOOLS, chat=False, answers=OK)]})
    cases.append({'name': 'codex-missing-binary', 'cli': 'codex', 'missing_binary': True, 'script': codex_script([]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'codex-garbage-stdout-line', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), {'raw': 'not json at all'}, {'hang': True}]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    three = []
    transcript = HUT
    for i in range(3):
        if i:
            transcript = transcript + [entry('a%d' % i, 'assistant'), entry('t%d' % i, 'tool'), entry('u%d' % (i + 1), 'user')]
        three.append(T(conv('c1', transcript), HUT_MSG if i == 0 else SECOND_MSG, answers=OK))
    cases.append({'name': 'codex-three-turns-with-calls', 'cli': 'codex', 'script': codex_script(codex_turn(), codex_turn(append=True), codex_turn(append=True)), 'turns': three})
    cases.append({'name': 'codex-reasoning-only', 'cli': 'codex',
                  'script': codex_script([ev({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}), item('reasoning', text='thinking'), {'rollout': {'thread': '01a1f500-0000-7000-8000-000000000001', 'events': [token_count(1, 0)]}}, completed(0, 0)]),
                  'turns': [T(conv('c1', HUT), HUT_MSG)]})
    cases.append({'name': 'codex-continue-after-tool-results', 'cli': 'codex', 'script': codex_script(codex_turn()),
                  'turns': [T(conv('c1', HUT + [entry('a1', 'assistant'), entry('t1', 'tool')]),
                              HUT_MSG + [{'role': 'assistant', 'content': '', 'tool_calls': [{'id': 'x1', 'type': 'function', 'function': {'name': 'build', 'arguments': '{}'}}]},
                                         {'role': 'tool', 'tool_call_id': 'x1', 'content': 'built'}], answers=OK)]})
    return cases


cases = claude_cases() + codex_cases()
names = [c['name'] for c in cases]
assert len(names) == len(set(names))
json.dump(cases, sys.stdout, indent=1, ensure_ascii=False)
sys.stdout.write('\n')
turns = {}
for case in cases:
    turns[case['cli']] = turns.get(case['cli'], 0) + len(case['turns'])
sys.stderr.write('cases: %d, turns: %s\n' % (len(cases), turns))
