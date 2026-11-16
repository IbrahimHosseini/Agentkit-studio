"""Development-tool acceptance tests: real isolated daemon/PTY, no model services."""
import base64
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == 'posix', 'Unix socket and PTY acceptance harness')
class DevToolsTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='studio-tools-', dir='/tmp')).resolve()
        self.addCleanup(shutil.rmtree, self.root, True)
        home = self.root / 'home'; home.mkdir()
        self.project = self.root / 'project'; self.project.mkdir()
        self.data = self.root / 'state'
        self.binary = Path(os.environ.get('AGENTKIT_TEST_BIN', ROOT / 'target/debug/agentkit')).resolve()
        self.assertTrue(self.binary.is_file(), 'Build agentkit first')
        self.env = {k: os.environ[k] for k in ('PATH', 'TMPDIR', 'LANG') if k in os.environ}
        self.env.update(HOME=str(home), USERPROFILE=str(home), CODEX_HOME=str(home/'.codex'),
                        CLAUDE_CONFIG_DIR=str(home/'.claude'), AGENTKIT_DATA_DIR=str(self.data),
                        AGENTKIT_ENGINE_BIN=str(self.binary), SHELL='/bin/sh')
        self.daemon = subprocess.Popen([str(self.binary), 'daemon'], env=self.env,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        self.addCleanup(self.cleanup)
        deadline = time.monotonic() + 10
        while not (self.data/'engine.token').exists() or not (self.data/'engine.sock').exists():
            if self.daemon.poll() is not None: self.fail(self.daemon.stderr.read().decode())
            if time.monotonic() > deadline: self.fail('Daemon startup timed out')
            time.sleep(.03)
        self.pid = self.rpc('studio/project/create', {'path': str(self.project)})['id']
        self.cid = self.conversation()

    def cleanup(self):
        if self.daemon.poll() is None:
            try: self.rpc('shutdown')
            except (OSError, AssertionError): self.daemon.terminate()
            try: self.daemon.wait(timeout=8)
            except subprocess.TimeoutExpired: self.daemon.kill(); self.daemon.wait()
        self.daemon.stderr.close()

    def rpc(self, method, params=None, error=False):
        p = dict(params or {}, _auth=(self.data/'engine.token').read_text())
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(10); stream.connect(str(self.data/'engine.sock'))
            stream.sendall((json.dumps({'id':'tools-test', 'method':method, 'params':p})+'\n').encode())
            with stream.makefile('rb') as response: result = json.loads(response.readline())
        if error: return result
        self.assertNotIn('error', result, result.get('error'))
        return result['result']

    def conversation(self):
        return self.rpc('studio/conversation/create', {'project_id': self.pid})['id']

    def tool(self, target, action, params=None, cid=None, error=False):
        return self.rpc(f'devtools/{target}/{action}', dict(params or {}, conversation_id=cid or self.cid), error)

    def read_until(self, tid, pattern, after=0):
        text = ''; deadline = time.monotonic()+5
        while time.monotonic() < deadline:
            result = self.tool('terminal', 'read', {'id':tid, 'after':after})
            after = result['cursor']; text += base64.b64decode(result['data']).decode('utf-8', errors='replace')
            if re.search(pattern, text): return text, after
            time.sleep(.03)
        self.fail(f'Terminal did not produce {pattern!r}: {text!r}')

    def mcp(self, requests, scope=None):
        env = self.env.copy()
        if scope is not None: env['AGENTKIT_DEVTOOLS_CONVERSATION'] = scope
        payload = ''.join(json.dumps(dict(jsonrpc='2.0', id=i, **request))+'\n' for i, request in enumerate(requests, 1))
        completed = subprocess.run([str(self.binary), 'mcp-server'], env=env, input=payload,
                                   text=True, capture_output=True, timeout=15)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        return [json.loads(line) for line in completed.stdout.splitlines()]

    def test_public_client_accepts_null_chat_defaults_on_fresh_profile(self):
        self.assertIsNone(self.rpc('studio/chat-defaults'))
        completed = subprocess.run([str(self.binary), '--json', 'rpc', 'studio/chat-defaults'],
                                   env=self.env, text=True, capture_output=True, timeout=15)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIsNone(json.loads(completed.stdout))

    def test_real_pty_cwd_utf8_resize_cursor_and_conversation_isolation(self):
        tid = self.tool('terminal', 'create', {'cols':80, 'rows':24})['id']
        self.tool('terminal', 'write', {'id':tid, 'data':"stty -echo; printf 'MARK:%s\\n' \"$PWD\"; printf '\\330\\263\\331\\204\\330\\247\\331\\205\\n'\n"})
        output, cursor = self.read_until(tid, 'سلام')
        self.assertIn('MARK:'+str(self.project), output)
        resized = self.tool('terminal', 'resize', {'id':tid, 'cols':91, 'rows':27})
        self.assertEqual((resized['cols'],resized['rows']), (91,27))
        self.tool('terminal', 'write', {'id':tid, 'data':'stty size\n'})
        output, cursor = self.read_until(tid, r'27 91', cursor)
        self.assertNotIn('MARK:', output, 'Read cursor must not replay prior output')
        self.tool('terminal','write',{'id':tid,'data':'sleep 10\n'})
        time.sleep(.1)
        self.tool('terminal','write',{'id':tid,'data':'\x03'})
        time.sleep(.05)
        self.tool('terminal','write',{'id':tid,'data':"printf 'AFTER_INTERRUPT\\n'\n"})
        _, cursor = self.read_until(tid, 'AFTER_INTERRUPT', cursor)
        self.assertIn('error', self.tool('terminal','read',{'id':tid,'after':cursor+1000000},error=True))
        other = self.conversation()
        self.assertEqual(self.tool('terminal','list',cid=other)['terminals'], [])
        self.assertIn('error', self.tool('terminal','write',{'id':tid,'data':'echo WRONG\n'},cid=other,error=True))
        self.tool('terminal','close',{'id':tid})
        self.assertEqual(self.tool('terminal','list')['terminals'], [])
        self.assertIn('error',self.tool('terminal','read',{'id':tid},error=True))

    def test_grants_default_deny_revoke_and_scoped_mcp_cannot_self_grant(self):
        call = lambda target,action,arguments=None,**extra: {'method':'tools/call','params':{'name':'devtools','arguments':dict(target=target,action=action,arguments=arguments or {},**extra)}}
        grants = self.tool('access','get')
        self.assertTrue(all(grants[k] is False for k in ('terminal','browser','ios','android')))
        self.assertIn('error',self.tool('terminal','create',{'actor':'agent'},error=True))
        replies = self.mcp([{'method':'tools/list'},call('terminal','create',{'actor':'user'}),call('access','grant',{'target':'terminal'})],self.cid)
        self.assertEqual([t['name'] for t in replies[0]['result']['tools']],['devtools'])
        self.assertTrue(replies[1]['result']['isError'])
        self.assertTrue(replies[2]['result']['isError'])
        self.assertFalse(self.tool('access','get')['terminal'])
        self.tool('access','grant',{'target':'terminal'})
        other = self.conversation()
        replies = self.mcp([call('terminal','create'),call('terminal','list',conversation_id=other)],self.cid)
        self.assertFalse(replies[0]['result']['isError'])
        self.assertTrue(replies[1]['result']['isError'])
        terminal = json.loads(replies[0]['result']['content'][0]['text'])
        self.assertEqual(len(self.tool('terminal','list')['terminals']),1)
        activity = self.tool('activity','get')
        self.assertEqual(activity['target'],'terminal'); self.assertTrue(activity['id'])
        self.tool('terminal','read',{'id':terminal['id'],'actor':'agent'})
        self.assertEqual(self.tool('activity','get')['id'],activity['id'])
        self.tool('access','revoke',{'target':'terminal'})
        reply = self.mcp([call('terminal','read',{'id':terminal['id'],'actor':'user'})],self.cid)[0]
        self.assertTrue(reply['result']['isError'])
        self.assertEqual(len(self.tool('terminal','list')['terminals']),1, 'User retains access after agent revocation')
        self.assertTrue(self.mcp([call('terminal','list',conversation_id=self.cid)])[0]['result']['isError'], 'External bridge stays opt-in')

    def test_projectless_and_reassigned_project_do_not_inherit_access_or_terminals(self):
        standalone=self.rpc('studio/conversation/create',{})['id']
        self.assertIn('error',self.tool('terminal','create',cid=standalone,error=True))
        self.tool('access','grant',{'target':'terminal'})
        tid=self.tool('terminal','create')['id']
        elsewhere=self.root/'elsewhere';elsewhere.mkdir()
        project=self.rpc('studio/project/create',{'path':str(elsewhere)})
        self.rpc('studio/conversation/project',{'conversation_id':self.cid,'project_id':project['id']})
        self.assertFalse(self.tool('access','get')['terminal'])
        self.assertEqual(self.tool('terminal','list')['terminals'],[])
        self.assertIn('error',self.tool('terminal','read',{'id':tid},error=True))

    def test_deleting_conversation_closes_its_shell(self):
        tid=self.tool('terminal','create')['id']
        self.tool('terminal','write',{'id':tid,'data':"stty -echo; printf 'SHELLPID:%s\\n' $$\n"})
        output,_=self.read_until(tid,r'SHELLPID:\d+')
        shell_pid=int(re.search(r'SHELLPID:(\d+)',output).group(1))
        self.rpc('studio/conversation/delete',{'conversation_id':self.cid})
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            try: os.kill(shell_pid,0)
            except ProcessLookupError: break
            time.sleep(.03)
        else: self.fail('Deleted conversation left its terminal shell running')

    def test_detaching_project_closes_shell_and_does_not_restore_grant_on_reattach(self):
        self.tool('access','grant',{'target':'terminal'})
        tid=self.tool('terminal','create')['id']
        self.tool('terminal','write',{'id':tid,'data':"stty -echo; printf 'SHELLPID:%s\\n' $$\n"})
        output,_=self.read_until(tid,r'SHELLPID:\d+')
        shell_pid=int(re.search(r'SHELLPID:(\d+)',output).group(1))
        self.rpc('studio/conversation/project',{'conversation_id':self.cid,'project_id':None})
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            try: os.kill(shell_pid,0)
            except ProcessLookupError: break
            time.sleep(.03)
        else: self.fail('Detached project left its terminal shell running')
        self.rpc('studio/conversation/project',{'conversation_id':self.cid,'project_id':self.pid})
        self.assertFalse(self.tool('access','get')['terminal'])
        self.assertEqual(self.tool('terminal','list')['terminals'],[])


if __name__ == '__main__': unittest.main()
