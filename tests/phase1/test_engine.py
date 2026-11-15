"""Phase-one black-box tests. Use AGENTKIT_TEST_BIN for a compiled Rust CLI.

These tests use only scratch state and explicit mock/fake providers. They do not
establish real account, native GUI, Windows, Linux, or Persian IME compatibility.
"""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
TERMINAL = {'completed', 'failed', 'cancelled', 'interrupted'}


def binary_path():
    explicit = os.environ.get('AGENTKIT_TEST_BIN') or os.environ.get('AGENTKIT_ENGINE_BIN')
    if explicit:
        return Path(explicit).resolve()
    target = Path(os.environ.get('CARGO_TARGET_DIR', str(ROOT / 'target')))
    return target / 'debug' / ('agentkit.exe' if os.name == 'nt' else 'agentkit')


class EngineTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = binary_path()
        if not cls.binary.is_file():
            raise unittest.SkipTest('Rust CLI not built; set AGENTKIT_TEST_BIN to the compiled agentkit executable')

    def setUp(self):
        # A short path matters for Unix socket address limits on macOS.
        self.scratch = Path(tempfile.mkdtemp(prefix='ak-p1-', dir='/tmp' if os.name != 'nt' else None)).resolve()
        self.addCleanup(shutil.rmtree, self.scratch, True)
        self.project = self.scratch / 'project with spaces'
        self.project.mkdir()
        home = self.scratch / 'home'
        home.mkdir()
        self.data = self.scratch / 'state'
        self.log = self.scratch / 'provider.jsonl'
        self.heartbeat = self.scratch / 'heartbeat'
        self.env = os.environ.copy()
        self.env.update(HOME=str(home), USERPROFILE=str(home), CODEX_HOME=str(home / '.codex'), CLAUDE_CONFIG_DIR=str(home / '.claude'), AGENTKIT_DATA_DIR=str(self.data), AGENTKIT_ENGINE_BIN=str(self.binary), AGENTKIT_FAKE_LOG=str(self.log), AGENTKIT_FAKE_HEARTBEAT=str(self.heartbeat))
        # Copy fixtures into the scratch area with an absolute Python shebang.
        fake = self.scratch / 'provider-fixture'
        content = (Path(__file__).parent / 'fake_provider.py').read_text(encoding='utf-8')
        fake.write_text('#!' + sys.executable + '\n' + content.split('\n', 1)[1], encoding='utf-8')
        fake.chmod(0o700)
        self.env.update(AGENTKIT_CODEX_BIN=str(fake), AGENTKIT_CLAUDE_BIN=str(fake))
        self.daemon = None
        self.addCleanup(self.cleanup_daemon)
        self.start_daemon()
        for provider in ('codex', 'claude'):
            self.cli_json('rpc', 'studio/connection/set', '--params', json.dumps({'provider': provider, 'enabled': True}))

    def start_daemon(self):
        self.daemon = subprocess.Popen([str(self.binary), 'daemon'], env=self.env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if self.daemon.poll() is not None:
                self.fail('Test daemon exited: ' + self.daemon.stderr.read().decode('utf-8', errors='replace'))
            if (self.data / 'engine.token').is_file():
                result = self.cli('handshake', check=False)
                if result.returncode == 0:
                    return
            time.sleep(0.05)
        self.fail('Test daemon did not become ready')

    def cleanup_daemon(self):
        if self.daemon is None:
            return
        if self.daemon.poll() is None:
            try:
                runs = self.cli_json('runs')
                for run in runs:
                    if run['status'] not in TERMINAL:
                        self.cli('stop', run['id'], check=False)
                deadline = time.monotonic() + 4
                while time.monotonic() < deadline:
                    stopped = self.cli('engine-shutdown', check=False)
                    if stopped.returncode == 0:
                        break
                    time.sleep(0.05)
                self.daemon.wait(timeout=3)
            except (OSError, subprocess.TimeoutExpired, AssertionError):
                self.daemon.terminate()  # Only a daemon this test started.
                try:
                    self.daemon.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    self.daemon.kill()
                    self.daemon.wait(timeout=3)
        if self.daemon.stderr:
            self.daemon.stderr.close()

    def cli(self, *args, check=True):
        result = subprocess.run([str(self.binary), '--json', *map(str, args)], env=self.env, text=True, encoding='utf-8', capture_output=True, timeout=15)
        if check:
            self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def cli_json(self, *args):
        return json.loads(self.cli(*args).stdout)

    def start(self, prompt='نمونه\u200cی فارسی + Rust 👋', provider='mock', *extra):
        return self.cli_json('run', '--provider', provider, '--project', self.project, '--prompt', prompt, *extra)

    def wait_status(self, run_id, statuses=TERMINAL, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            run = self.cli_json('status', run_id)
            if run['status'] in statuses:
                return run
            time.sleep(0.05)
        self.fail('Run did not reach {}: {}'.format(statuses, run))

    def all_events(self, run_id):
        output, cursor = [], 0
        while True:
            page = self.cli_json('events', run_id, '--after', cursor)
            events = page['events']
            self.assertLessEqual(len(events), 50)
            self.assertLess(len(json.dumps(page).encode()), 2 * 1024 * 1024)
            if not events:
                self.assertEqual(page['next_seq'], cursor)
                return output
            self.assertTrue(all(event['seq'] > cursor for event in events))
            seqs = [event['seq'] for event in events]
            self.assertEqual(seqs, sorted(set(seqs)))
            cursor = page['next_seq']
            self.assertEqual(cursor, seqs[-1])
            output.extend(events)

    @unittest.skipUnless(os.name == 'posix', 'Direct Unix-socket GUI-client proof is POSIX only')
    def test_cli_and_gui_equivalent_client_share_runs_and_authenticate(self):
        run = self.start()
        self.assertEqual(self.wait_status(run['id'])['status'], 'completed')
        params = {'_auth': (self.data / 'engine.token').read_text(), 'limit': 10}
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(3)
            stream.connect(str(self.data / 'engine.sock'))
            stream.sendall((json.dumps({'id': 'gui-client', 'method': 'list', 'params': params}) + '\n').encode())
            reply = json.loads(stream.makefile('rb').readline())
        self.assertEqual(reply['id'], 'gui-client')
        self.assertEqual(reply['result'][0]['id'], run['id'])
        self.assertEqual(reply['result'][0]['prompt'], run['prompt'])
        self.assertEqual(len(self.cli_json('runs')), 1)
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(3)
            stream.connect(str(self.data / 'engine.sock'))
            stream.sendall(b'{"id":"bad","method":"list","params":{}}\n')
            denied = json.loads(stream.makefile('rb').readline())
        self.assertIn('authentication', denied['error']['message'].lower())

    def test_mock_approval_deny_stop_and_failure(self):
        run = self.start('[approval]')
        self.wait_status(run['id'], {'awaiting_approval'})
        self.assertNotEqual(self.cli('approve', run['id'], 'wrong', '--allow', check=False).returncode, 0)
        self.cli('approve', run['id'], 'mock-approval', '--deny')
        self.assertEqual(self.wait_status(run['id'])['status'], 'cancelled')
        self.assertNotEqual(self.cli('approve', run['id'], 'mock-approval', '--allow', check=False).returncode, 0)
        held = self.start('[hold]')
        self.wait_status(held['id'], {'running'})
        self.cli('stop', held['id'])
        self.assertEqual(self.wait_status(held['id'])['status'], 'cancelled')
        failed = self.start('[fail]')
        self.assertEqual(self.wait_status(failed['id'])['status'], 'failed')

    def test_restart_recovers_without_reexecuting(self):
        run = self.start('[hold]')
        self.wait_status(run['id'], {'running'})
        old = self.all_events(run['id'])
        # A crash is ungraceful. SIGTERM is handled and cleanly cancels runs.
        self.daemon.kill()
        self.daemon.wait(timeout=5)
        self.daemon.stderr.close()
        self.start_daemon()
        recovered = self.cli_json('status', run['id'])
        self.assertEqual(recovered['status'], 'interrupted')
        events = self.all_events(run['id'])
        self.assertEqual(events[:len(old)], old)
        self.assertEqual(events[-1]['payload']['status'], 'interrupted')
        self.assertEqual(len(self.cli_json('runs')), 1)

    def test_large_history_is_paginated_and_cursor_replay_is_exact(self):
        run = self.start('[history]')
        self.assertEqual(self.wait_status(run['id'], timeout=40)['status'], 'completed')
        events = self.all_events(run['id'])
        self.assertEqual(sum(event['kind'] == 'text' for event in events), 10000)
        cursor = events[49]['seq']
        replay = self.cli_json('events', run['id'], '--after', cursor)['events']
        self.assertEqual(replay, events[50:100])
        self.assertTrue(any('نمونه' in event['payload'].get('text', '') for event in events))

    def test_unsupported_attachments_are_rejected_before_creating_a_run(self):
        path = self.project / 'unsupported.bmp'
        path.write_bytes(b'not an image')
        result = self.cli('run', '--provider', 'mock', '--project', self.project, '--prompt', 'test', '--image', path, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('MIME', result.stderr)
        self.assertEqual(self.cli_json('runs'), [])
        result = self.cli('run', '--provider', 'mock', '--project', self.project, '--prompt', 'test', '--file', self.project / 'missing', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.cli_json('runs'), [])

    def reset_fixture_scenario(self, scenario):
        self.cleanup_daemon()
        self.env['AGENTKIT_FAKE_SCENARIO'] = scenario
        self.start_daemon()

    @unittest.skipUnless(os.name == 'posix', 'Executable script fixtures are currently POSIX only')
    def test_codex_protocol_attachments_identity_approval_and_resume(self):
        self.reset_fixture_scenario('approval')
        image = self.project / 'screen.png'
        image.write_bytes(b'\x89PNG\r\n\x1a\nfixture')
        document = self.project / 'note فارسی.txt'
        document.write_text('نیم\u200cفاصله', encoding='utf-8')
        run = self.start('Review fixture', 'codex', '--image', image, '--file', document, '--role', 'backend')
        self.wait_status(run['id'], {'awaiting_approval'})
        self.cli('approve', run['id'], '99', '--allow')
        self.assertEqual(self.wait_status(run['id'])['status'], 'completed')
        events = self.all_events(run['id'])
        self.assertTrue(any(event['agent_id'] == 'child-fixture' and event['parent_agent_id'] == 'thread-fixture' for event in events))
        self.assertTrue(any(event['payload'].get('text') == 'سلام Rust 👋' for event in events))
        messages = [json.loads(line) for line in self.log.read_text().splitlines()]
        turn = next(message for message in messages if message.get('method') == 'turn/start')
        self.assertIn({'type': 'localImage', 'path': str(image)}, turn['params']['input'])
        text = turn['params']['input'][0]['text']
        self.assertIn('backend', text)
        self.assertIn(str(document), text)
        self.assertIn({'id': 99, 'result': {'decision': 'accept'}}, messages)
        resumed = self.cli_json('resume', run['id'], '--prompt', 'Continue fixture')
        self.wait_status(resumed['id'], {'awaiting_approval'})
        self.cli('approve', resumed['id'], '99', '--deny')
        self.wait_status(resumed['id'])
        messages = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertTrue(any(message.get('method') == 'thread/resume' and message['params']['threadId'] == 'thread-fixture' for message in messages))

    @unittest.skipUnless(os.name == 'posix', 'Executable script fixtures are currently POSIX only')
    def test_claude_structured_input_preserves_identity_and_denials(self):
        image = self.project / 'screen.png'
        image.write_bytes(b'png fixture')
        run = self.start('بررسی متن', 'claude', '--image', image, '--role', 'backend')
        self.assertEqual(self.wait_status(run['id'])['status'], 'completed')
        handshake = self.cli_json('handshake')
        cap = next(cap for cap in handshake['capabilities'] if cap['provider'] == 'claude')
        self.assertFalse(cap['approvals'])
        events = self.all_events(run['id'])
        self.assertTrue(any(event['kind'] == 'permission_denied' for event in events))
        self.assertTrue(any(event['agent_id'] == 'claude-child' and event['parent_agent_id'] == 'claude-session' for event in events))
        self.assertTrue(any(event['agent_id'] == 'claude-orphan' and event['parent_agent_id'] is None for event in events))
        self.assertFalse(any(event['parent_agent_id'] in {'spawn-tool', 'unobserved-tool'} for event in events))
        messages = [json.loads(line) for line in self.log.read_text().splitlines()]
        argv = messages[0]['argv']
        self.assertEqual(argv[argv.index('--permission-prompts') + 1], 'none')
        self.assertEqual(argv[argv.index('--agent') + 1], 'backend')
        self.assertFalse(any('skip-permissions' in arg for arg in argv))
        content = messages[1]['message']['content']
        self.assertTrue(any(item['type'] == 'image' and item['source']['data'] for item in content))

    @unittest.skipUnless(os.name == 'posix', 'Executable script fixtures are currently POSIX only')
    def test_large_approval_remains_actionable_after_persistence_and_replay(self):
        self.reset_fixture_scenario('large-approval')
        run = self.start('Oversized approval fixture', 'codex')
        self.wait_status(run['id'], {'awaiting_approval'})
        before = self.all_events(run['id'])
        approval = next(event for event in before if event['kind'] == 'approval')
        self.assertEqual(approval['payload']['request_id'], '99')
        self.assertTrue(approval['payload'].get('truncated'))
        self.cli('approve', run['id'], approval['payload']['request_id'], '--allow')
        self.assertEqual(self.wait_status(run['id'])['status'], 'completed')
        after = self.all_events(run['id'])
        resolved = next(event for event in after if event['kind'] == 'approval_resolved')
        self.assertEqual(resolved['payload']['request_id'], approval['payload']['request_id'])
        self.assertGreater(resolved['seq'], approval['seq'])
        self.assertTrue(resolved['payload']['decision'])
        # A reconnecting client sees both the durable request and its resolution.
        replay = self.cli_json('events', run['id'], '--after', approval['seq'] - 1)['events']
        self.assertTrue(any(event['kind'] == 'approval' for event in replay))
        self.assertTrue(any(event['kind'] == 'approval_resolved' for event in replay))

    @unittest.skipUnless(os.name == 'posix', 'POSIX owned-process-tree cleanup proof')
    def test_stopping_native_provider_stops_owned_descendant(self):
        self.reset_fixture_scenario('hold')
        run = self.start('Hold subprocess fixture', 'codex')
        deadline = time.monotonic() + 5
        while not self.heartbeat.exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertTrue(self.heartbeat.exists(), 'Descendant never started')
        self.cli('stop', run['id'])
        self.assertEqual(self.wait_status(run['id'])['status'], 'cancelled')
        first = self.heartbeat.read_text()
        time.sleep(0.25)
        self.assertEqual(self.heartbeat.read_text(), first, 'Provider descendant continues work after stop')

    @unittest.skipUnless(os.name == 'posix', 'Executable script fixtures are currently POSIX only')
    def test_provider_crash_is_a_durable_failure(self):
        self.reset_fixture_scenario('crash')
        run = self.start('Crash fixture', 'codex')
        self.assertEqual(self.wait_status(run['id'])['status'], 'failed')
        events = self.all_events(run['id'])
        self.assertIn('exited before turn completion', events[-1]['payload']['error'])
        self.assertIn('fixture failed', events[-1]['payload']['error'])


if __name__ == '__main__':
    unittest.main()
