"""Offline proof of the live harness: no native model runtime or real credentials."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from contextlib import redirect_stdout
import io
from types import SimpleNamespace

SCRIPT = Path(__file__).resolve().parents[2] / 'scripts/live-smoke.py'
spec = importlib.util.spec_from_file_location('live_smoke', SCRIPT)
module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
FAKE = r'''
import json,os,pathlib,sys,time
root=pathlib.Path(os.environ['AGENTKIT_DATA_DIR']);root.mkdir(exist_ok=True)
if '--version' in sys.argv: print('offline-fixture 1');print('native stderr detail test-provider-secret',file=sys.stderr);sys.exit()
if sys.argv[1]=='daemon':
 (root/'engine.token').write_text('test-ipc-secret')
 print('diagnostic Bearer test-provider-secret',file=sys.stderr,flush=True)
 while not (root/'shutdown').exists():time.sleep(.02)
 sys.exit()
method=sys.argv[3];params=json.loads(sys.argv[5]);mode=pathlib.Path(__file__).name
if method=='studio/settings':print(json.dumps(params.get('settings',{'claude_auth_mode':'configured'})));sys.exit()
if method=='studio/models':print(json.dumps({'account_type':'chatgpt','models':[],'verified_execution':False}));sys.exit()
if method=='start':
 import subprocess
 subprocess.run([os.environ['AGENTKIT_'+params['provider'].upper()+'_BIN'],'--version'],stdout=subprocess.DEVNULL)
 (root/'input.json').write_text(json.dumps(params));print(json.dumps({'id':'fake-run'}));sys.exit()
if method=='shutdown':(root/'shutdown').touch();print('{}');sys.exit()
if method=='stop':(root/'stopped').touch();print('{}');sys.exit()
status='cancelled' if (root/'stopped').exists() else 'running' if 'timeout' in mode else 'failed' if 'failure' in mode else 'completed'
if method=='get':print(json.dumps({'id':'fake-run','status':status}));sys.exit()
if method=='subscribe':
 if 'collection' in mode and params.get('after_seq',0)>0: print('fixture collection failure',file=sys.stderr);sys.exit(1)
 data=json.loads((root/'input.json').read_text());text=(pathlib.Path(data['workspace'])/'fixture.txt').read_text()
 if 'failure' in mode:text='Model unavailable; token=test-provider-secret https://example.test/?api_key=test-query-secret'
 events=[{'seq':1,'kind':'message','payload':{'text':text,'truncated':'truncated' in mode}}, {'seq':2,'kind':'status','payload':{'status':status,'error':text if 'failure' in mode else None,'authorization':'Bearer test-provider-secret'}}]
 print(json.dumps({'events':[e for e in events if e['seq']>params.get('after_seq',0)],'next_seq':2}));sys.exit()
raise RuntimeError(method)
'''

class LiveSmokeHarnessTests(unittest.TestCase):
    def run_fake(self, mode='success', provider='claude', setup=True, native=False):
        scratch=tempfile.TemporaryDirectory(prefix='smoke-harness-',dir='/tmp');self.addCleanup(scratch.cleanup)
        root=Path(scratch.name);home=root/'home';home.mkdir();(home/'.claude').mkdir();(home/'.codex').mkdir()
        config=home/'.claude/settings.json';config.write_text(json.dumps({'model':'sonnet','env':{'ANTHROPIC_BASE_URL':'http://127.0.0.1:9','ANTHROPIC_AUTH_TOKEN':'test-provider-secret'}}))
        if setup:(home/'.codex/auth.json').write_text(json.dumps({'tokens':{'access_token':'test-provider-secret'}}))
        fake=root/('engine-'+mode);fake.write_text('#!'+sys.executable+'\n'+FAKE);fake.chmod(0o700)
        report=root/'report.json';env={k:os.environ[k] for k in ('PATH','TMPDIR','LANG') if k in os.environ};env.update(HOME=str(home),CODEX_HOME=str(home/'.codex'),CLAUDE_CONFIG_DIR=str(home/'.claude'))
        before=config.read_bytes()
        args=[str(SCRIPT),'--provider',provider,'--binary',str(fake),'--runtime',str(fake),'--report',str(report),'--authorize-live','--timeout-seconds','1']
        if native:
            args += ['--claude-auth-mode','native']
            with patch.dict(os.environ,env,clear=True),patch.object(sys,'argv',args),patch.object(module,'native_claude_token',return_value=('test-provider-secret',9999999999999)),redirect_stdout(io.StringIO()):
                proc=SimpleNamespace(returncode=module.main(),stderr='')
        else:
            proc=subprocess.run([sys.executable,'-I',*args],env=env,capture_output=True,text=True,timeout=15)
        self.assertEqual(config.read_bytes(),before)
        data=json.loads(report.read_text());evidence=report.with_suffix('.evidence')
        retained='\n'.join(p.read_text() for p in [report,*evidence.iterdir()])
        self.assertNotIn('test-provider-secret',retained)
        self.assertNotIn('test-query-secret',retained)
        self.assertNotIn('test-ipc-secret',retained)
        return proc,data,evidence

    def test_success_retains_exact_response_and_configured_model(self):
        proc,r,e=self.run_fake();self.assertEqual(proc.returncode,0,proc.stderr)
        self.assertTrue(r['acceptance_passed']);self.assertEqual(r['selected_model'],'sonnet')
        self.assertEqual(r['inference_submissions'],1);self.assertTrue(r['temporary_credentials_and_state_removed'])
        self.assertIn(r['expected']['fixture_code'],(e/'response.txt').read_text());self.assertTrue((e/'events.jsonl').read_text())
        self.assertIn('[REDACTED]',(e/'daemon.stderr.log').read_text())

    def test_codex_auth_copy_and_metadata_are_isolated(self):
        proc,r,e=self.run_fake(provider='codex');self.assertEqual(proc.returncode,0,proc.stderr)
        self.assertTrue(r['original_settings_unchanged']);self.assertEqual(r['metadata_preflight']['account_type'],'chatgpt')

    def test_failure_retains_error_without_credentials(self):
        proc,r,e=self.run_fake('failure');self.assertEqual(proc.returncode,1)
        self.assertFalse(r['acceptance_passed']);self.assertTrue(r['errors']);self.assertIn('Model unavailable',r['response'])
        self.assertIn('native stderr detail [REDACTED]',(e/'provider.stderr.log').read_text())

    def test_collection_failure_cannot_certify_matching_response(self):
        proc,r,e=self.run_fake('collection');self.assertEqual(proc.returncode,1)
        self.assertTrue(r['fixture_code_verified']);self.assertFalse(r['acceptance_passed']);self.assertIn('collection_error',r)
        self.assertTrue((e/'events.jsonl').read_text())

    def test_truncated_evidence_cannot_certify_matching_response(self):
        proc,r,e=self.run_fake('truncated');self.assertEqual(proc.returncode,1)
        self.assertTrue(r['fixture_code_verified']);self.assertFalse(r['acceptance_passed']);self.assertTrue(r['truncated_events'])

    def test_timeout_retains_partial_events_and_stops(self):
        proc,r,e=self.run_fake('timeout');self.assertEqual(proc.returncode,1)
        self.assertTrue(r['timed_out']);self.assertEqual(r['status'],'cancelled');self.assertEqual(r['inference_submissions'],1)
        self.assertTrue((e/'events.jsonl').read_text());self.assertTrue(r['temporary_credentials_and_state_removed'])

    def test_setup_failure_still_has_report_without_submitting(self):
        proc,r,e=self.run_fake(provider='codex',setup=False)
        self.assertEqual(proc.returncode,1);self.assertIn('harness_error',r);self.assertEqual(r['inference_submissions'],0)

    def test_native_mode_preserves_evidence_with_private_oauth_handoff(self):
        proc,r,e=self.run_fake(native=True);self.assertEqual(proc.returncode,0)
        self.assertEqual(r['claude_auth_mode'],'native');self.assertEqual(r['route_host'],'api.anthropic.com')
        self.assertEqual(r['selected_model'],'sonnet');self.assertEqual(r['inference_submissions'],1)

    def test_native_token_requires_existing_unexpired_access_only(self):
        payload=json.dumps({'claudeAiOauth':{'accessToken':'private-access','refreshToken':'never-export-this','expiresAt':500000}})
        token,expiry=module.native_claude_token(lambda:payload,1000)
        self.assertEqual(token,'private-access');self.assertEqual(expiry,500000)
        for oauth in ({'accessToken':'','expiresAt':500000},{'accessToken':'private-access','expiresAt':0}):
            with self.assertRaises(RuntimeError):module.native_claude_token(lambda:json.dumps({'claudeAiOauth':oauth}),1000)

    def test_exact_persian_line_is_required_not_fragment(self):
        result=module.summarize([{'kind':'message','payload':{'text':'nonce نیم‌فاصله'}}],'nonce','متن آزمایشی با نیم‌فاصله و English')
        self.assertTrue(result['fixture_code_verified']);self.assertFalse(result['rtl_verified'])

    def test_redaction_retains_usage_and_removes_nested_auth_and_query(self):
        actual=module.redact({'input_tokens':12,'authorization':'anything','x':'https://host/?token=secret-value&ok=1','nested':{'access_token':'other'}},[])
        self.assertEqual(actual['input_tokens'],12);self.assertEqual(actual['authorization'],'[REDACTED]')
        self.assertNotIn('secret-value',str(actual));self.assertNotIn('other',str(actual))
