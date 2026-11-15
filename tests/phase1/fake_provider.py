#!/usr/bin/env python3
"""Offline protocol fixture. Never invokes a target CLI, shell tool or model."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def emit(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)


def record(value):
    with open(os.environ['AGENTKIT_FAKE_LOG'], 'a', encoding='utf-8') as stream:
        stream.write(json.dumps(value, ensure_ascii=False) + '\n')


def child_heartbeat():
    path = Path(os.environ['AGENTKIT_FAKE_HEARTBEAT'])
    while True:
        path.write_text(str(time.time()), encoding='ascii')
        time.sleep(0.05)


def codex():
    record({'argv': sys.argv[1:]})
    scenario = os.environ.get('AGENTKIT_FAKE_SCENARIO', 'normal')
    for line in sys.stdin:
        message = json.loads(line)
        record(message)
        method = message.get('method')
        if method == 'initialize':
            emit({'id': message['id'], 'result': {'userAgent': 'offline-fixture'}})
        elif method == 'account/read':
            emit({'id': message['id'], 'result': {'account': {'type': 'chatgpt'}, 'requiresOpenaiAuth': True}})
        elif method in ('thread/start', 'thread/resume'):
            emit({'id': message['id'], 'result': {'thread': {'id': 'thread-fixture'}}})
        elif method == 'turn/start':
            emit({'id': message['id'], 'result': {'turn': {'id': 'turn-fixture'}}})
            emit({'method': 'turn/started', 'params': {'threadId': 'thread-fixture', 'turn': {'id': 'turn-fixture'}}})
            emit({'method': 'item/started', 'params': {'threadId': 'thread-fixture', 'item': {'type': 'collabToolCall', 'tool': 'spawnAgent', 'senderThreadId': 'thread-fixture', 'newThreadId': 'child-fixture', 'agentStatus': 'running'}}})
            emit({'method': 'item/agentMessage/delta', 'params': {'threadId': 'thread-fixture', 'delta': 'سلام Rust 👋'}})
            if scenario in ('approval', 'large-approval'):
                reason = 'X' * 70000 if scenario == 'large-approval' else 'Offline approval test'
                emit({'id': 99, 'method': 'item/commandExecution/requestApproval', 'params': {'threadId': 'thread-fixture', 'turnId': 'turn-fixture', 'command': 'fixture command', 'reason': reason}})
            elif scenario == 'hold':
                subprocess.Popen([sys.executable, __file__, '--heartbeat'], stdin=subprocess.DEVNULL)
            elif scenario == 'crash':
                print('fixture failed before result', file=sys.stderr, flush=True)
                return 7
            else:
                emit({'method': 'turn/completed', 'params': {'turn': {'status': 'completed'}}})
        elif message.get('id') == 99:
            decision = message.get('result', {}).get('decision')
            emit({'method': 'turn/completed', 'params': {'turn': {'status': 'completed' if decision == 'accept' else 'interrupted'}}})
        elif method == 'turn/interrupt':
            emit({'id': message['id'], 'result': {}})
            emit({'method': 'turn/completed', 'params': {'turn': {'status': 'interrupted'}}})
    return 0


def claude():
    record({'argv': sys.argv[1:]})
    message = json.loads(sys.stdin.readline())
    record(message)
    emit({'type': 'system', 'subtype': 'init', 'session_id': 'claude-session'})
    emit({'type': 'stream_event', 'session_id': 'claude-session', 'parent_tool_use_id': None, 'event': {'delta': {'text': 'پاسخ فارسی'}}})
    # A spawning tool call has its own identity, distinct from the owning agent.
    emit({'type': 'assistant', 'session_id': 'claude-session', 'parent_tool_use_id': None, 'message': {'content': [{'type': 'tool_use', 'name': 'Agent', 'id': 'spawn-tool', 'input': {'subagent_type': 'backend'}}]}})
    emit({'type': 'assistant', 'session_id': 'claude-session', 'agent_id': 'claude-child', 'parent_tool_use_id': 'spawn-tool', 'message': {'content': [{'type': 'tool_use', 'name': 'Read', 'input': {'file_path': 'fixture.txt'}}]}})
    emit({'type': 'assistant', 'session_id': 'claude-session', 'agent_id': 'claude-orphan', 'parent_tool_use_id': 'unobserved-tool', 'message': {'content': [{'type': 'text', 'text': 'Parent mapping is unknown'}]}})
    emit({'type': 'system', 'subtype': 'permission_denied', 'session_id': 'claude-session'})
    emit({'type': 'result', 'session_id': 'claude-session', 'subtype': 'success', 'is_error': False, 'result': 'fixture result', 'permission_denials': [{'tool_name': 'Bash'}]})
    return 0


if __name__ == '__main__':
    if '--heartbeat' in sys.argv:
        child_heartbeat()
    else:
        sys.exit(codex() if 'app-server' in sys.argv else claude())
