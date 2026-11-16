"""Studio acceptance tests: isolated homes, real daemon, fake native providers."""
import json
import os
from pathlib import Path
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest

ROOT=Path(__file__).resolve().parents[2]
TERMINAL={'completed','failed','cancelled','interrupted'}

@unittest.skipUnless(os.name=='posix','Unix socket acceptance harness')
class StudioTests(unittest.TestCase):
    def setUp(self):
        self.root=Path(tempfile.mkdtemp(prefix='studio-p2-',dir='/tmp')).resolve()
        self.addCleanup(shutil.rmtree,self.root,True)
        self.home=self.root/'home';self.home.mkdir()
        self.project=self.root/'project';self.project.mkdir()
        self.data=self.root/'state'
        self.log=self.root/'fixture.jsonl'
        self.binary=Path(os.environ.get('AGENTKIT_TEST_BIN',ROOT/'target/debug/agentkit')).resolve()
        self.assertTrue(self.binary.is_file(),'Build agentkit first')
        self.env={k:os.environ[k] for k in ('PATH','TMPDIR','LANG') if k in os.environ}
        self.env.update(HOME=str(self.home),USERPROFILE=str(self.home),CODEX_HOME=str(self.home/'.codex'),CLAUDE_CONFIG_DIR=str(self.home/'.claude'),AGENTKIT_DATA_DIR=str(self.data),AGENTKIT_ENGINE_BIN=str(self.binary),STUDIO_FIXTURE_LOG=str(self.log))
        for runtime in ('codex','claude'):
            fake=self.root/(runtime+'-fixture')
            source=(Path(__file__).parent/'provider_fixture.py').read_text()
            fake.write_text('#!'+sys.executable+'\n'+source.split('\n',1)[1]);fake.chmod(0o700)
            self.env['AGENTKIT_'+runtime.upper()+'_BIN']=str(fake)
        codex=self.home/'.codex';(codex/'agents').mkdir(parents=True)
        (codex/'config.toml').write_text('model = "fixture-codex"\nmodel_reasoning_effort = "medium"\n')
        (codex/'agents/researcher.toml').write_text('model = "fixture-codex"\nmodel_reasoning_effort = "medium"\ndeveloper_instructions = "Read project files and report findings."\n')
        claude=self.home/'.claude/agents';claude.mkdir(parents=True)
        (claude/'writer.md').write_text('---\nname: writer\ndescription: Review and write reports\nmodel: sonnet\neffort: high\n---\nReview findings and write reports.\n')
        self.daemon=None
        self.addCleanup(self.cleanup)
        self.launch()
        self.initial_settings=self.rpc('studio/settings')
        for provider in ('codex','claude'):
            self.rpc('studio/connection/set',{'provider':provider,'enabled':True})

    def launch(self):
        self.daemon=subprocess.Popen([str(self.binary),'daemon'],env=self.env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
        deadline=time.monotonic()+10
        while time.monotonic()<deadline:
            if self.daemon.poll() is not None:
                self.fail(self.daemon.stderr.read().decode())
            if (self.data/'engine.sock').exists() and (self.data/'engine.token').exists():
                try:
                    self.rpc('handshake');return
                except (OSError,AssertionError):pass
            time.sleep(.05)
        self.fail('daemon startup timed out')

    def cleanup(self):
        if self.daemon:
            if self.daemon.poll() is None:
                self.daemon.terminate()
                try:self.daemon.wait(timeout=10)
                except subprocess.TimeoutExpired:self.daemon.kill();self.daemon.wait()
            self.daemon.stderr.close()

    def rpc(self,method,params=None,allow_error=False):
        params=dict(params or {});params['_auth']=(self.data/'engine.token').read_text()
        with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as s:
            s.settimeout(15);s.connect(str(self.data/'engine.sock'))
            s.sendall((json.dumps({'id':'test','method':method,'params':params})+'\n').encode())
            reply=json.loads(s.makefile('rb').readline())
        if allow_error:return reply
        self.assertNotIn('error',reply,reply.get('error'))
        return reply['result']

    def test_projectless_chat_attach_move_detach_preserves_history(self):
        c=self.rpc('studio/conversation/create',{})
        cid=c['id'];self.assertIsNone(c['project_id'])
        scratch=Path(c['standalone_workspace'])
        self.assertTrue(scratch.is_dir());self.assertTrue(scratch.is_relative_to(self.data))
        self.assertNotEqual(scratch,self.home);self.assertNotEqual(scratch,self.project)
        attachment=self.root/'note.txt';attachment.write_text('fixture attachment')
        self.rpc('studio/message/send',{'conversation_id':cid,'text':'DIRECT_QUESTION','attachments':[{'id':'note','kind':'file','path':str(attachment),'mime':'text/plain'}]})
        first=self.done(cid);self.assertEqual(first['conversation']['status'],'completed')
        self.assertEqual(first['messages'][0]['attachments'][0]['path'],str(attachment))
        self.assertEqual(first['tasks'][0]['workspace'],str(scratch))
        project=self.rpc('studio/project/create',{'path':str(self.project)})
        moved=self.rpc('studio/conversation/project',{'conversation_id':cid,'project_id':project['id']})
        self.assertEqual(moved['conversation']['project_id'],project['id'])
        self.assertEqual(moved['messages'],first['messages'])
        self.assertFalse(any(c['id']==cid for c in self.rpc('studio/conversations',{'project_id':None})))
        self.send(cid,'DIRECT_QUESTION again');second=self.done(cid)
        self.assertEqual(second['tasks'][-1]['workspace'],str(self.project))
        detached=self.rpc('studio/conversation/project',{'conversation_id':cid,'project_id':None})
        self.assertIsNone(detached['conversation']['project_id'])
        self.assertEqual(len(detached['messages']),len(second['messages']))
        self.assertTrue(any(c['id']==cid for c in self.rpc('studio/conversations',{'project_id':None})))
        self.assertTrue(any(c['id']==cid for c in self.rpc('studio/conversations')))
        self.send(cid,'DIRECT_QUESTION after removal');third=self.done(cid)
        self.assertEqual(third['tasks'][-1]['workspace'],str(scratch))
        self.assertEqual(len({t['run_id'] for t in third['tasks']}),3)

    def test_chat_permission_mode_is_validated_scoped_and_persistent(self):
        first=self.rpc('studio/conversation/create',{'permission_mode':'auto'})
        other=self.rpc('studio/conversation/create',{})
        self.assertEqual(first['permission_mode'],'auto')
        self.assertEqual(other['permission_mode'],'ask')
        for value in ('unknown',None,True,{},7):
            self.assertIn('error',self.rpc('studio/conversation/create',{'permission_mode':value},allow_error=True))
            self.assertIn('error',self.rpc('studio/conversation/permission',{'conversation_id':first['id'],'permission_mode':value},allow_error=True))
        self.assertIn('error',self.rpc('studio/conversation/permission',{'conversation_id':first['id']},allow_error=True))
        changed=self.rpc('studio/conversation/permission',{'conversation_id':first['id'],'permission_mode':'full_access'})
        self.assertEqual(changed['conversation']['permission_mode'],'full_access')
        self.cleanup();self.daemon=None;self.launch()
        self.assertEqual(self.get(first['id'])['conversation']['permission_mode'],'full_access')
        self.assertEqual(self.get(other['id'])['conversation']['permission_mode'],'ask')
        # Emulate a chat written before permission_mode was introduced.
        self.cleanup();self.daemon=None
        with sqlite3.connect(self.data/'history.sqlite3') as db:
            row=db.execute("SELECT data FROM studio_entities WHERE kind='conversation' AND id=?",(other['id'],)).fetchone()
            legacy=json.loads(row[0]);legacy.pop('permission_mode')
            db.execute("UPDATE studio_entities SET data=? WHERE kind='conversation' AND id=?",(json.dumps(legacy),other['id']))
        self.launch()
        self.assertEqual(self.get(other['id'])['conversation']['permission_mode'],'ask')
        self.assertEqual(next(c for c in self.rpc('studio/conversations') if c['id']==other['id'])['permission_mode'],'ask')

    def test_chat_permission_cannot_change_during_execution(self):
        cid=self.rpc('studio/conversation/create',{})['id']
        self.send(cid,'HOLD_TASK permission test')
        self.wait(cid,lambda d:any(t['run_id'] for t in d['tasks']))
        rejected=self.rpc('studio/conversation/permission',{'conversation_id':cid,'permission_mode':'full_access'},allow_error=True)
        self.assertIn('error',rejected)
        self.assertEqual(self.get(cid)['conversation']['permission_mode'],'ask')
        self.rpc('studio/conversation/stop',{'conversation_id':cid});self.done(cid)
        deadline=time.monotonic()+10
        while time.monotonic()<deadline:
            result=self.rpc('studio/conversation/permission',{'conversation_id':cid,'permission_mode':'auto'},allow_error=True)
            if 'result' in result:break
            time.sleep(.05)
        self.assertEqual(result['result']['conversation']['permission_mode'],'auto')

    def test_full_access_does_not_expand_projectless_chat_permissions(self):
        cid=self.rpc('studio/conversation/create',{'permission_mode':'full_access'})['id']
        self.send(cid,'Read the direct conversation fixture');done=self.done(cid)
        self.assertEqual(done['conversation']['status'],'completed')
        self.assertTrue(all(t['read_only'] for t in done['tasks']))
        self.assertTrue(all(t['permission_mode']=='full_access' for t in done['tasks']))
        calls=[json.loads(line) for line in self.log.read_text().splitlines()]
        starts=[v['rpc']['params'] for v in calls if v.get('rpc',{}).get('method')=='thread/start']
        turns=[v['rpc']['params'] for v in calls if v.get('rpc',{}).get('method')=='turn/start']
        self.assertEqual(len(starts),1);self.assertEqual(len(turns),1)
        self.assertEqual(starts[0]['sandbox'],'read-only')
        self.assertEqual(turns[0]['sandboxPolicy']['type'],'readOnly')
        self.assertTrue(all(p['approvalPolicy']=='never' and p['approvalsReviewer']=='user' for p in starts+turns))

    def test_automatic_project_permissions_apply_without_repeated_plan_prompts(self):
        project=self.rpc('studio/project/create',{'path':str(self.project)})
        for mode in ('auto','full_access'):
            cid=self.rpc('studio/conversation/create',{'project_id':project['id'],'permission_mode':mode})['id']
            for request in ('WRITE_TASK','CODEX_WRITE_TASK'):
                before=len(self.log.read_text().splitlines()) if self.log.exists() else 0
                self.send(cid,request);done=self.done(cid)
                self.assertEqual(done['conversation']['status'],'completed')
                self.assertEqual(done['plan']['status'],'approved')
                current=[t for t in done['tasks'] if t['request_id']==done['conversation']['request_id']]
                self.assertTrue(all(t['permission_mode']==mode for t in current))
                calls=[json.loads(line) for line in self.log.read_text().splitlines()[before:]]
                starts=[v['rpc']['params'] for v in calls if v.get('rpc',{}).get('method')=='thread/start']
                readonly=[p for p in starts if p.get('sandbox')=='read-only']
                self.assertTrue(readonly,'planning and summarizing stay read-only')
                self.assertTrue(all(p['approvalPolicy']=='never' and p['approvalsReviewer']=='user' for p in readonly))
                if request=='CODEX_WRITE_TASK':
                    writes=[p for p in starts if p.get('sandbox')!='read-only']
                    self.assertEqual(len(writes),1)
                    self.assertEqual(writes[0]['sandbox'],'workspace-write' if mode=='auto' else 'danger-full-access')
                    self.assertEqual(writes[0]['approvalsReviewer'],'auto_review' if mode=='auto' else 'user')
                    self.assertEqual(writes[0]['approvalPolicy'],'on-request' if mode=='auto' else 'never')
                else:
                    invocations=[v['argv'] for v in calls if '--print' in v.get('argv',[])]
                    self.assertEqual(len(invocations),1)
                    self.assertEqual(invocations[0][invocations[0].index('--permission-mode')+1],'auto' if mode=='auto' else 'bypassPermissions')

    def test_direct_chat_selection_persists_switches_and_has_no_planner(self):
        first={'provider':'codex','model':'fixture-codex','effort':'medium'}
        c=self.rpc('studio/conversation/create',{'selection':first});cid=c['id']
        note=self.root/'note فارسی.txt';note.write_text('historical attachment')
        self.rpc('studio/message/send',{'conversation_id':cid,'text':'Remember DIRECT_HISTORY_NONCE','attachments':[{'id':'note','kind':'file','path':str(note),'mime':'text/plain'}]})
        one=self.done(cid)
        self.assertEqual([t['kind'] for t in one['tasks']],['direct'])
        self.assertEqual(one['messages'][-1]['provider'],'codex')
        second={'provider':'claude','model':'sonnet','effort':'high'}
        self.rpc('studio/conversation/selection',{'conversation_id':cid,'selection':second})
        self.cleanup();self.daemon=None;self.launch()
        self.assertEqual(self.get(cid)['conversation']['chat_selection'],second)
        self.send(cid,'What did I ask before?');two=self.done(cid)
        self.assertEqual([t['kind'] for t in two['tasks']],['direct','direct'])
        self.assertEqual([t['provider'] for t in two['tasks']],['codex','claude'])
        self.assertEqual(len([m for m in two['messages'] if m['role']=='assistant']),2)
        self.assertEqual(two['messages'][-1]['provider'],'claude')
        events=[json.loads(line) for line in self.log.read_text().splitlines()]
        claude_inputs=[event['input'] for event in events if 'input' in event]
        self.assertEqual(len(claude_inputs),1)
        self.assertIn('DIRECT_HISTORY_NONCE',json.dumps(claude_inputs))
        self.assertIn(str(note),json.dumps(claude_inputs,ensure_ascii=False))
        starts=[event['rpc']['params'] for event in events if event.get('rpc',{}).get('method')=='thread/start']
        self.assertEqual(len(starts),1)
        self.assertEqual(starts[0]['sandbox'],'read-only')
        self.assertNotIn('STUDIO_PLANNER_V1',json.dumps(events))
        self.assertNotIn('STUDIO_SUMMARIZER_V1',json.dumps(events))
        project=self.rpc('studio/project/create',{'path':str(self.project)})
        self.rpc('studio/conversation/project',{'conversation_id':cid,'project_id':project['id']})
        self.assertIn('error',self.rpc('studio/message/send',{'conversation_id':cid,'text':'hi','selection':first},allow_error=True))
        self.rpc('studio/conversation/project',{'conversation_id':cid,'project_id':None})
        self.assertEqual(self.get(cid)['conversation']['chat_selection'],second)

    def test_direct_chat_rejects_invalid_selection_and_active_changes(self):
        cid=self.rpc('studio/conversation/create',{})['id']
        for selection in [{'provider':'codex','model':'missing','effort':None},{'provider':'codex','model':'fixture-codex','effort':'impossible'},{'provider':'claude','model':'sonnet','effort':7},{'provider':'codex','model':False,'effort':None},{'provider':'codex','effort':None}]:
            self.assertIn('error',self.rpc('studio/conversation/selection',{'conversation_id':cid,'selection':selection},allow_error=True))
        original=self.get(cid)['conversation']['chat_selection']
        self.send(cid,'HOLD_TASK direct question')
        self.assertIn('error',self.rpc('studio/conversation/selection',{'conversation_id':cid,'selection':{'provider':'claude','model':'sonnet','effort':None}},allow_error=True))
        self.rpc('studio/conversation/stop',{'conversation_id':cid});self.done(cid)
        self.assertEqual(self.get(cid)['conversation']['chat_selection'],original)
        deadline=time.monotonic()+10
        while True:
            change=self.rpc('studio/conversation/selection',{'conversation_id':cid,'selection':original},allow_error=True)
            if 'error' not in change:break
            self.assertLess(time.monotonic(),deadline);time.sleep(.05)
        self.assertEqual(len(self.get(cid)['tasks']),1)

    def test_direct_chat_does_not_require_superagent_connection(self):
        self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        c=self.rpc('studio/conversation/create',{});self.assertEqual(c['chat_selection']['provider'],'claude')
        self.send(c['id'],'hello');self.assertEqual(self.done(c['id'])['conversation']['status'],'completed')
        self.rpc('studio/connection/set',{'provider':'claude','enabled':False})
        empty=self.rpc('studio/conversation/create',{})
        self.assertIsNone(empty['chat_selection'])
        self.assertIsNone(self.rpc('studio/chat-defaults'))
        self.assertIn('Connect a provider',str(self.rpc('studio/message/send',{'conversation_id':empty['id'],'text':'hi'},allow_error=True)))
        self.assertEqual(self.get(empty['id'])['messages'],[])

    def test_direct_chat_slow_provider_start_launches_one_owned_run(self):
        self.cleanup();self.daemon=None
        self.env['STUDIO_FIXTURE_HELP_DELAY']='0.8';self.launch()
        cid=self.rpc('studio/conversation/create',{'selection':{'provider':'claude','model':'sonnet','effort':'high'}})['id']
        self.send(cid,'hello with a slow capability probe')
        result=self.done(cid)
        self.assertEqual(len(result['tasks']),1)
        self.assertEqual(len(result['tasks'][0]['attempts']),1)
        self.assertEqual(len([m for m in result['messages'] if m['role']=='assistant']),1)
        for _ in range(5):
            self.assertEqual(len(self.get(cid)['tasks'][0]['attempts']),1)
            time.sleep(.05)
        entries=[json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertEqual(len([entry for entry in entries if 'input' in entry]),1)
        self.assertEqual(len(self.rpc('list',{'limit':100})),1)

    def test_direct_chat_crash_and_model_unavailable_never_replay_or_fallback(self):
        cid=self.rpc('studio/conversation/create',{})['id']
        workspace=Path(self.get(cid)['conversation']['standalone_workspace'])
        (workspace/'.fixture-scenario').write_text('direct_model_unavailable')
        self.send(cid,'hi');failed=self.done(cid)
        self.assertEqual(failed['conversation']['status'],'failed')
        self.assertEqual(len(failed['tasks']),1)
        self.assertEqual(len(failed['tasks'][0]['attempts']),1)
        self.assertEqual(failed['tasks'][0]['model'],'fixture-codex')
        self.assertIn('no automatic model switch',failed['tasks'][0]['retry_notice'])
        (workspace/'.fixture-scenario').unlink()
        self.send(cid,'HOLD_TASK')
        self.wait(cid,lambda d:d['tasks'][-1]['status']=='running')
        deadline=time.monotonic()+5
        while not any(json.loads(line).get('rpc',{}).get('method')=='turn/start' for line in self.log.read_text().splitlines()):
            self.assertLess(time.monotonic(),deadline);time.sleep(.02)
        self.daemon.kill();self.daemon.wait();self.daemon.stderr.close()
        import signal
        for line in self.log.read_text().splitlines():
            event=json.loads(line)
            if event.get('event')=='started':
                try:os.killpg(event['pid'],signal.SIGKILL)
                except ProcessLookupError:pass
        before=self.log.read_text();self.launch()
        recovered=self.get(cid);self.assertEqual(recovered['conversation']['status'],'interrupted')
        time.sleep(.3);self.assertEqual(self.log.read_text(),before)
        self.send(cid,'hello after restart');continued=self.done(cid)
        self.assertEqual(continued['conversation']['status'],'completed')
        self.assertEqual(len(continued['tasks']),3)
        self.assertEqual(len([m for m in continued['messages'] if m['role']=='assistant']),1)

    def test_project_change_rejects_active_and_pending_plan(self):
        cid=self.conversation();self.send(cid,'HOLD_TASK DIRECT_QUESTION')
        denied=self.rpc('studio/conversation/project',{'conversation_id':cid,'project_id':None},allow_error=True)
        self.assertIn('error',denied)
        self.rpc('studio/conversation/stop',{'conversation_id':cid});self.done(cid)
        other=self.conversation();self.send(other,'WRITE_TASK')
        self.wait(other,lambda d:d['conversation']['status']=='awaiting_approval')
        self.assertIn('error',self.rpc('studio/conversation/project',{'conversation_id':other,'project_id':None},allow_error=True))
        self.rpc('studio/plan/approve',{'conversation_id':other,'decision':False})

    def test_projectless_chat_is_read_only_without_orchestration(self):
        cid=self.rpc('studio/conversation/create',{})['id']
        self.send(cid,'WRITE_TASK');detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'completed')
        self.assertFalse((self.project/'report.txt').exists())
        self.assertFalse((Path(detail['conversation']['standalone_workspace'])/'report.txt').exists())
        self.assertEqual([t['kind'] for t in detail['tasks']],['direct'])
        self.assertTrue(detail['tasks'][0]['read_only'])

    def test_disconnect_blocks_legacy_managed_resume_catalog_and_preserves_settings(self):
        original=(self.home/'.codex/config.toml').read_bytes()
        run=self.rpc('start',{'provider':'codex','workspace':str(self.project),'prompt':'Direct read','read_only':True,'attachments':[]})
        deadline=time.monotonic()+10
        while self.rpc('get',{'run_id':run['id']})['status'] not in TERMINAL:
            self.assertLess(time.monotonic(),deadline);time.sleep(.05)
        self.wait(run['id'],lambda d:all(t.get('replay_complete') for t in d['tasks']))
        cid=self.rpc('studio/conversation/create',{'selection':{'provider':'codex','model':'fixture-codex','effort':'medium'}})['id']
        settings=self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        self.assertFalse(settings['provider_connections']['codex'])
        self.assertIn('disconnected',str(self.rpc('resume',{'run_id':run['id']},allow_error=True)))
        self.assertEqual((self.home/'.codex/config.toml').read_bytes(),original)
        self.assertIn('disconnected',str(self.rpc('studio/message/send',{'conversation_id':cid,'text':'DIRECT_QUESTION'},allow_error=True)))
        self.assertIn('disconnected',str(self.rpc('start',{'provider':'codex','workspace':str(self.project),'prompt':'Hello','read_only':True,'attachments':[]},allow_error=True)))
        self.assertIn('disconnected',str(self.rpc('studio/models',{'provider':'codex'},allow_error=True)))
        self.assertEqual(self.get(cid)['messages'],[])
        agents=self.rpc('studio/agents',{'workspace':str(self.project)})
        self.assertTrue(all(not a['available'] for a in agents if a['provider']=='codex'))
        self.rpc('studio/connection/set',{'provider':'codex','enabled':True})
        self.send(cid,'DIRECT_QUESTION');self.assertEqual(self.done(cid)['conversation']['status'],'completed')
        active=self.conversation();self.send(active,'HOLD_TASK DIRECT_QUESTION')
        self.assertIn('error',self.rpc('studio/connection/set',{'provider':'codex','enabled':False},allow_error=True))
        settings=self.rpc('studio/settings');settings['provider_connections']['codex']=False
        self.assertIn('error',self.rpc('studio/settings',{'settings':settings},allow_error=True))
        self.rpc('studio/conversation/stop',{'conversation_id':active});self.done(active)

    def test_fresh_profile_requires_explicit_studio_connection(self):
        self.assertEqual(self.initial_settings['provider_connections'],{'codex':False,'claude':False})
        self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        self.rpc('studio/connection/set',{'provider':'claude','enabled':False})
        self.assertEqual(self.rpc('studio/model-catalog')['models'],[])

    def test_delete_project_detaches_history_and_keeps_files(self):
        cid=self.conversation();self.send(cid,'DIRECT_QUESTION');before=self.done(cid)
        project_id=before['conversation']['project_id']
        marker=self.project/'keep.txt';marker.write_text('Do not delete project files')
        result=self.rpc('studio/project/delete',{'project_id':project_id})
        self.assertEqual(result['detached_conversations'],1)
        self.assertEqual(marker.read_text(),'Do not delete project files')
        self.assertEqual(self.rpc('studio/projects'),[])
        after=self.get(cid)
        self.assertIsNone(after['conversation']['project_id'])
        self.assertEqual(after['messages'],before['messages'])
        self.assertEqual(after['tasks'],before['tasks'])
        self.send(cid,'DIRECT_QUESTION after project deletion');self.assertEqual(self.done(cid)['conversation']['status'],'completed')

    def test_delete_chat_removes_provider_history_without_resurrection(self):
        cid=self.conversation();self.send(cid,'DIRECT_QUESTION');detail=self.done(cid)
        run_ids=[t['run_id'] for t in detail['tasks']]
        self.wait(cid,lambda d:all(t.get('replay_complete') for t in d['tasks']))
        self.rpc('studio/conversation/delete',{'conversation_id':cid})
        self.assertIn('error',self.rpc('studio/conversation/get',{'conversation_id':cid},allow_error=True))
        for rid in run_ids:
            self.assertIn('error',self.rpc('get',{'run_id':rid},allow_error=True))
        self.daemon.terminate();self.daemon.wait(timeout=5);self.daemon.stderr.close();self.launch()
        self.assertFalse(any(c['id']==cid for c in self.rpc('studio/conversations')))

    def test_delete_active_chat_or_project_rejected(self):
        cid=self.conversation();self.send(cid,'HOLD_TASK DIRECT_QUESTION')
        pid=self.get(cid)['conversation']['project_id']
        for method,params in [('studio/conversation/delete',{'conversation_id':cid}),('studio/project/delete',{'project_id':pid})]:
            self.assertIn('error',self.rpc(method,params,allow_error=True))
        self.rpc('studio/conversation/stop',{'conversation_id':cid});self.done(cid)
        self.assertEqual(len(self.rpc('studio/projects')),1)
        self.assertTrue(self.get(cid)['conversation'])

    def test_conversation_sort_order_and_stable_ties(self):
        ids=[]
        for title in ('Zulu','alpha','Beta'):
            ids.append(self.rpc('studio/conversation/create',{'title':title})['id']);time.sleep(.01)
        ordered=self.rpc('studio/conversations',{'sort':'title','direction':'asc'})
        self.assertEqual([c['title'] for c in ordered],['alpha','Beta','Zulu'])
        self.assertEqual([c['id'] for c in self.rpc('studio/conversations',{'sort':'created_at','direction':'asc'})],ids)
        self.send(ids[0],'DIRECT_QUESTION');self.done(ids[0])
        self.assertEqual(self.rpc('studio/conversations')[0]['id'],ids[0])
        self.assertIn('error',self.rpc('studio/conversations',{'sort':'invalid'},allow_error=True))
        self.assertEqual(self.rpc('studio/conversations',{'sort':'title'}),self.rpc('studio/conversations',{'sort':'title'}))

    def test_catalog_persists_and_hides_disconnected_provider(self):
        loaded=self.rpc('studio/models',{'provider':'codex','workspace':str(self.project)})
        self.assertEqual(loaded['models'][0]['verification'],'advertised')
        catalog=self.rpc('studio/model-catalog',{'workspace':str(self.project)})
        self.assertEqual({m['provider'] for m in catalog['models']},{'codex','claude'})
        self.daemon.terminate();self.daemon.wait(timeout=5);self.daemon.stderr.close();self.launch()
        cached=self.rpc('studio/model-catalog',{'workspace':str(self.project)})
        self.assertEqual(cached,catalog)
        self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        self.assertTrue(all(m['provider']=='claude' for m in self.rpc('studio/model-catalog')['models']))
        self.rpc('studio/connection/set',{'provider':'codex','enabled':True})
        self.assertEqual(self.rpc('studio/model-catalog'),catalog)

    def test_failed_catalog_refresh_retains_last_advertised_models(self):
        self.rpc('studio/models',{'provider':'codex'})
        fixture=Path(self.env['AGENTKIT_CODEX_BIN'])
        fixture.write_text(fixture.read_text().replace("elif method=='model/list':", "elif method=='model/list':\n                emit({'id':msg['id'],'error':{'message':'Catalog fixture failure'}});return"))
        failed=self.rpc('studio/models',{'provider':'codex'})
        self.assertIn('catalog_error',failed)
        cached=self.rpc('studio/model-catalog')
        model=next(m for m in cached['models'] if m['provider']=='codex')
        self.assertEqual(model['verification'],'advertised')
        provider=next(p for p in cached['providers'] if p['provider']=='codex')
        self.assertTrue(provider['stale']);self.assertIn('Catalog fixture failure',provider['catalog_error'])

    def test_same_model_name_keeps_provider_specific_efforts(self):
        (self.home/'.claude/agents/writer.md').write_text('---\nname: writer\nmodel: fixture-codex\neffort: low\n---\nWrite reports')
        catalog=self.rpc('studio/model-catalog')
        same=[m for m in catalog['models'] if m['model']=='fixture-codex']
        self.assertEqual(len(same),2)
        self.assertEqual({m['provider']:m['installedEfforts'] for m in same},{'codex':['medium'],'claude':['low']})
        settings=self.rpc('studio/settings');settings['agent_overrides']['agent:writer']={'provider':'claude','model':'fixture-codex','effort':'medium'}
        self.assertIn('not listed',str(self.rpc('studio/settings',{'settings':settings},allow_error=True)))

    def test_disconnected_inherited_agent_can_be_disabled_and_legacy_flags_preserved(self):
        self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        settings=self.rpc('studio/settings')
        settings['agent_overrides']['agent:researcher']={'enabled':False,'provider':'codex','model':'fixture-codex','effort':'medium','allowed_selections':[]}
        self.rpc('studio/settings',{'settings':settings})
        self.assertFalse(next(a for a in self.rpc('studio/agents') if a['name']=='researcher')['enabled'])

    def test_logical_roles_deduplicate_and_preserve_legacy_provider_override(self):
        (self.home/'.claude/agents/researcher.md').write_text('---\nname: researcher\nmodel: sonnet\neffort: high\n---\nClaude research instructions')
        settings=self.rpc('studio/settings')
        settings['agent_overrides']['claude:researcher']={'model':'sonnet','effort':'high'}
        self.rpc('studio/settings',{'settings':settings})
        agents=self.rpc('studio/agents',{'workspace':str(self.project)})
        self.assertEqual(len(agents),2)
        researcher=next(a for a in agents if a['name']=='researcher')
        self.assertEqual(researcher['id'],'agent:researcher')
        self.assertEqual(researcher['provider'],'claude')
        self.assertEqual(set(researcher['aliases']),{'codex:researcher','claude:researcher'})
        self.assertNotIn('instructions',researcher)
        self.assertTrue(all('instructions' not in source for source in researcher['sources']))

    def test_disconnect_prefers_explicit_replacement_and_persists_without_rewriting_sources(self):
        original=(self.home/'.codex/agents/researcher.toml').read_bytes()
        settings=self.rpc('studio/settings')
        settings['agent_overrides']['codex:researcher']={
            'enabled':False,'provider':'codex','model':'fixture-codex','effort':'medium',
            'allowed_models':['fixture-codex'],'allowed_efforts':['medium'],
            'allowed_selections':[{'provider':'claude','model':'sonnet','effort':'high'}]}
        self.rpc('studio/settings',{'settings':settings})
        changed=self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        override=changed['agent_overrides']['agent:researcher']
        self.assertEqual((override['provider'],override['model'],override['effort']),('claude','sonnet','high'))
        self.assertFalse(override['enabled'])
        self.assertEqual(override['allowed_models'],[]);self.assertEqual(override['allowed_efforts'],[])
        self.assertEqual((self.home/'.codex/agents/researcher.toml').read_bytes(),original)
        self.cleanup();self.daemon=None;self.launch()
        self.assertEqual(self.rpc('studio/settings'),changed)
        self.rpc('studio/connection/set',{'provider':'codex','enabled':True})
        agent=next(a for a in self.rpc('studio/agents') if a['name']=='researcher')
        self.assertEqual((agent['provider'],agent['model'],agent['effort']),('claude','sonnet','high'))

    def test_disconnect_uses_same_tier_and_resets_unsupported_effort(self):
        (self.home/'.codex/agents/researcher.toml').write_text('model = "gpt-6.1-sol"\nmodel_reasoning_effort = "medium"\ndeveloper_instructions = "Research"\n')
        settings=self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        selected=settings['agent_overrides']['agent:researcher']
        self.assertEqual((selected['provider'],selected['model'],selected['effort']),('claude','sonnet',None))
        agent=next(a for a in self.rpc('studio/agents') if a['name']=='researcher')
        self.assertTrue(agent['available']);self.assertEqual(agent['status'],'definition_loaded')
        self.assertEqual(agent['sources'][0]['model'],'gpt-6.1-sol')

    def test_disconnect_without_similar_model_keeps_selection_and_marks_agent_disconnected(self):
        settings=self.rpc('studio/settings')
        settings['agent_overrides']['agent:researcher']={'enabled':True,'provider':'codex','model':'fixture-codex','effort':'medium'}
        self.rpc('studio/settings',{'settings':settings})
        changed=self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        self.assertEqual(changed['agent_overrides'],settings['agent_overrides'])
        agent=next(a for a in self.rpc('studio/agents') if a['name']=='researcher')
        self.assertEqual((agent['provider'],agent['model']),('codex','fixture-codex'))
        self.assertFalse(agent['available']);self.assertEqual(agent['status'],'disconnected')
        # No active provider is also a recoverable unavailable selection, never a mock substitute.
        changed=self.rpc('studio/connection/set',{'provider':'claude','enabled':False})
        self.assertEqual(changed['agent_overrides'],settings['agent_overrides'])
        self.assertTrue(all(not a['available'] for a in self.rpc('studio/agents')))

    def test_settings_disconnect_replaces_project_only_agent_with_exact_model_before_tier(self):
        agents=self.project/'.codex/agents';agents.mkdir(parents=True)
        (agents/'designer.toml').write_text('model = "claude-sonnet-5-5"\nmodel_reasoning_effort = "high"\ndeveloper_instructions = "Design"\n')
        (self.home/'.claude/agents/exact.md').write_text('---\nname: exact\nmodel: claude-sonnet-5-5\neffort: high\n---\nExact model')
        self.rpc('studio/project/create',{'path':str(self.project)})
        settings=self.rpc('studio/settings');settings['provider_connections']['codex']=False
        changed=self.rpc('studio/settings',{'settings':settings})
        selected=changed['agent_overrides']['agent:designer']
        self.assertEqual((selected['provider'],selected['model'],selected['effort']),('claude','claude-sonnet-5-5','high'))
        self.assertEqual(self.rpc('studio/settings'),changed)

    def test_disconnect_replaces_claude_flagship_with_codex_flagship(self):
        (self.home/'.claude/agents/writer.md').write_text('---\nname: writer\nmodel: claude-opus-4-6\neffort: high\n---\nWrite')
        (self.home/'.codex/agents/flagship.toml').write_text('model = "gpt-6-astra"\nmodel_reasoning_effort = "high"\ndeveloper_instructions = "Research"\n')
        changed=self.rpc('studio/connection/set',{'provider':'claude','enabled':False})
        selected=changed['agent_overrides']['agent:writer']
        self.assertEqual((selected['provider'],selected['model'],selected['effort']),('codex','gpt-6-astra','high'))
        self.assertEqual(changed['superagent']['provider'],'codex')

    def test_role_can_use_other_provider_model_and_reject_wrong_effort(self):
        settings=self.rpc('studio/settings')
        settings['agent_overrides']['agent:researcher']={'provider':'claude','model':'sonnet','effort':'high','allowed_selections':[]}
        self.rpc('studio/settings',{'settings':settings})
        cid=self.conversation();self.send(cid,'Research and review');detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'completed')
        worker=next(t for t in detail['tasks'] if t['agent_id']=='agent:researcher')
        self.assertEqual((worker['provider'],worker['model'],worker['effort']),('claude','sonnet','high'))
        settings['agent_overrides']['agent:researcher']['effort']='medium'
        self.assertIn('not listed',str(self.rpc('studio/settings',{'settings':settings},allow_error=True)))
        settings['agent_overrides']['agent:researcher'].update(model='unknown',effort=None)
        self.assertIn('catalog',str(self.rpc('studio/settings',{'settings':settings},allow_error=True)))

    def test_tuple_selection_null_effort_and_disconnected_default(self):
        settings=self.rpc('studio/settings')
        settings['agent_overrides']['agent:researcher']={'provider':'codex','model':'fixture-codex','effort':'medium','allowed_selections':[{'provider':'claude','model':'sonnet','effort':None}]}
        self.rpc('studio/settings',{'settings':settings})
        settings['superagent']={'provider':'claude','model':'sonnet','effort':'high'}
        self.rpc('studio/settings',{'settings':settings})
        self.rpc('studio/connection/set',{'provider':'codex','enabled':False})
        fixture=Path(self.env['AGENTKIT_CLAUDE_BIN'])
        source=fixture.read_text().replace("write='WRITE_TASK' in text", "write='WRITE_TASK' in text").replace("if 'INVALID_MODEL' in text:", "tasks[0].update(provider='claude',model='sonnet',effort=None)\n        if 'INVALID_MODEL' in text:")
        fixture.write_text(source)
        cid=self.conversation();self.send(cid,'Research and review');detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'completed',detail)
        worker=next(t for t in detail['tasks'] if t['agent_id']=='agent:researcher')
        self.assertEqual(worker['provider'],'claude');self.assertIsNone(worker['effort'])
        current=self.rpc('studio/settings');current['concurrency']=2
        self.rpc('studio/settings',{'settings':current})

    def conversation(self):
        project=self.rpc('studio/project/create',{'path':str(self.project)})
        conversation=self.rpc('studio/conversation/create',{'project_id':project['id']})
        return conversation['id']

    def get(self,cid):return self.rpc('studio/conversation/get',{'conversation_id':cid})
    def send(self,cid,text):return self.rpc('studio/message/send',{'conversation_id':cid,'text':text})
    def wait(self,cid,predicate,timeout=15):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            detail=self.get(cid)
            if predicate(detail):return detail
            time.sleep(.05)
        self.fail('Conversation never reached expected state: '+json.dumps(detail))
    def done(self,cid):return self.wait(cid,lambda d:d['conversation']['status'] in TERMINAL)

    def mcp(self, messages):
        process=subprocess.run([str(self.binary),'mcp-server'],input=''.join(json.dumps(m)+'\n' for m in messages),text=True,capture_output=True,env=self.env,timeout=15)
        self.assertEqual(process.returncode,0,process.stderr)
        return [json.loads(line) for line in process.stdout.splitlines()]

    def test_mcp_gate_and_delegation_keep_approval_boundary(self):
        initialize={'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'fixture','version':'1'}}}
        listing={'jsonrpc':'2.0','id':2,'method':'tools/list'}
        call={'jsonrpc':'2.0','id':3,'method':'tools/call','params':{'name':'list_agents','arguments':{'workspace':str(self.project)}}}
        replies=self.mcp([initialize,listing,call])
        self.assertEqual(len(replies[1]['result']['tools']),5)
        self.assertTrue(replies[2]['result']['isError'])
        settings=self.rpc('studio/settings');settings['mcp_enabled']=True
        self.rpc('studio/settings',{'settings':settings})
        reply=self.mcp([call])[0]
        self.assertFalse(reply['result']['isError'])
        cid=self.conversation()
        delegate={'jsonrpc':'2.0','id':4,'method':'tools/call','params':{'name':'delegate','arguments':{'conversation_id':cid,'text':'WRITE_TASK'}}}
        self.assertFalse(self.mcp([delegate])[0]['result']['isError'])
        self.wait(cid,lambda d:d['conversation']['status']=='awaiting_approval')
        self.assertFalse((self.project/'report.txt').exists())
        names=[tool['name'] for tool in replies[1]['result']['tools']]
        self.assertNotIn('approve',names)

    def test_settings_and_agent_overrides_do_not_modify_definitions(self):
        definition=self.home/'.codex/agents/researcher.toml'
        before=definition.read_bytes()
        settings=self.rpc('studio/settings')
        settings['agent_overrides']={'codex:researcher':{'enabled':False,'model':'fixture-codex','effort':'medium','allowed_models':['fixture-codex'],'allowed_efforts':['medium']}}
        self.rpc('studio/settings',{'settings':settings})
        agents=self.rpc('studio/agents',{'workspace':str(self.project)})
        researcher=next(a for a in agents if a['id']=='agent:researcher')
        self.assertFalse(researcher['enabled'])
        self.assertEqual(definition.read_bytes(),before)
        cid=self.conversation();self.send(cid,'Research and review')
        self.assertEqual(self.done(cid)['conversation']['status'],'failed')

    def test_crash_marks_active_request_interrupted_without_retry(self):
        cid=self.conversation();self.send(cid,'HOLD_TASK')
        self.wait(cid,lambda d:any(t['status']=='running' for t in d['tasks']))
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            if self.log.exists() and '\"method\": \"turn/start\"' in self.log.read_text():break
            time.sleep(.02)
        else:self.fail('Fixture turn did not start before crash test')
        self.daemon.kill();self.daemon.wait();self.daemon.stderr.close()
        # Provider cleanup after a daemon hard-kill is documented separately.
        # This fixture starts no descendants, but terminate its owned process group.
        if self.log.exists():
            import signal
            for line in self.log.read_text().splitlines():
                entry=json.loads(line)
                if entry.get('event')=='started':
                    try:os.killpg(entry['pid'],signal.SIGKILL)
                    except ProcessLookupError:pass
        before=self.log.read_text() if self.log.exists() else ''
        self.launch();detail=self.get(cid)
        self.assertEqual(detail['conversation']['status'],'interrupted')
        time.sleep(.3)
        self.assertEqual(self.log.read_text() if self.log.exists() else '',before)

    def test_legacy_migration_keeps_history_and_creates_backup(self):
        self.cleanup()
        legacy=self.root/'legacy';legacy.mkdir()
        db=sqlite3.connect(str(legacy/'history.sqlite3'))
        db.executescript("CREATE TABLE runs(id TEXT PRIMARY KEY,input TEXT NOT NULL,run TEXT NOT NULL); CREATE TABLE events(seq INTEGER PRIMARY KEY AUTOINCREMENT,run_id TEXT NOT NULL REFERENCES runs(id),event TEXT NOT NULL);")
        run={'id':'legacy-run','provider':'mock','workspace':str(self.project),'prompt':'Historical request','status':'completed','created_at':'1','updated_at':'2','provider_session_id':None}
        inp={'provider':'mock','workspace':str(self.project),'prompt':'Historical request','attachments':[]}
        event={'seq':0,'run_id':'legacy-run','timestamp':'2','agent_id':None,'parent_agent_id':None,'kind':'text','payload':{'text':'Historical answer'}}
        db.execute('INSERT INTO runs VALUES(?,?,?)',('legacy-run',json.dumps(inp),json.dumps(run)))
        db.execute('INSERT INTO events(run_id,event) VALUES(?,?)',('legacy-run',json.dumps(event)))
        db.commit();db.close()
        self.data=legacy;self.env['AGENTKIT_DATA_DIR']=str(legacy);self.launch()
        detail=self.get('legacy-run')
        self.assertEqual(detail['conversation']['legacy_run_id'],'legacy-run')
        self.assertEqual(self.rpc('get',{'run_id':'legacy-run'})['prompt'],'Historical request')
        self.assertEqual(self.rpc('subscribe',{'run_id':'legacy-run'})['events'][0]['payload']['text'],'Historical answer')
        self.assertEqual(len(list(legacy.glob('history.pre-studio-v1-*.sqlite3'))),1)
        self.cleanup();self.launch()
        self.assertEqual(len(list(legacy.glob('history.pre-studio-v1-*.sqlite3'))),1)

    def test_direct_answer_and_followup_keep_one_conversation(self):
        cid=self.conversation();self.send(cid,'DIRECT_QUESTION فارسی 👋')
        first=self.done(cid)
        self.assertEqual(first['conversation']['status'],'completed')
        self.assertTrue(any('پاسخ مستقیم' in json.dumps(m,ensure_ascii=False) for m in first['messages']))
        self.send(cid,'DIRECT_QUESTION follow up');second=self.done(cid)
        self.assertGreater(len(second['messages']),len(first['messages']))
        self.assertEqual(second['conversation']['id'],cid)

    def test_mixed_providers_complete_and_collect_results(self):
        cid=self.conversation();self.send(cid,'Research and review')
        detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'completed',detail)
        self.assertTrue(any('Combined fixture result' in json.dumps(m) for m in detail['messages']))
        logs=self.log.read_text()
        self.assertIn('app-server',logs);self.assertIn('--print',logs)

    def test_writes_wait_for_approval_and_deny_does_not_execute(self):
        cid=self.conversation();self.send(cid,'WRITE_TASK create a report')
        self.wait(cid,lambda d:d['conversation']['status']=='awaiting_approval')
        self.assertFalse((self.project/'report.txt').exists())
        self.rpc('studio/plan/approve',{'conversation_id':cid,'decision':False})
        self.done(cid);self.assertFalse((self.project/'report.txt').exists())

    def test_approved_plan_executes_write(self):
        cid=self.conversation();self.send(cid,'WRITE_TASK create a report')
        self.wait(cid,lambda d:d['conversation']['status']=='awaiting_approval')
        self.rpc('studio/plan/approve',{'conversation_id':cid,'decision':True})
        result=self.done(cid)
        self.assertEqual(result['conversation']['status'],'completed',result)
        self.assertEqual((self.project/'report.txt').read_text(),'fixture report')

    def test_disallowed_model_and_invalid_dependency_are_rejected(self):
        for text in ('INVALID_MODEL','BAD_DEPENDENCY'):
            cid=self.conversation();self.send(cid,text)
            detail=self.done(cid)
            self.assertEqual(detail['conversation']['status'],'failed',detail)

    def test_parallel_conversations_stay_independent(self):
        ids=[self.conversation() for _ in range(3)]
        for cid in ids:self.send(cid,'DIRECT_QUESTION '+cid)
        for cid in ids:
            detail=self.done(cid)
            self.assertEqual(detail['conversation']['status'],'completed')
            self.assertTrue(any(cid in json.dumps(m) for m in detail['messages']))

    def test_stop_and_restart_do_not_replay_work(self):
        cid=self.conversation();self.send(cid,'HOLD_TASK')
        self.wait(cid,lambda d:any(t['status']=='running' for t in d['tasks']))
        self.rpc('studio/conversation/stop',{'conversation_id':cid})
        result=self.done(cid);self.assertEqual(result['conversation']['status'],'cancelled')
        self.cleanup();self.launch()
        self.assertEqual(self.get(cid)['conversation']['status'],'cancelled')


    def retry_scenario(self,scenario,allow_backup=False,tuple_selections=False):
        (self.project/'.fixture-scenario').write_text(scenario)
        (self.home/'.codex/agents/researcher.toml').write_text('model = "fixture-codex"\nmodel_reasoning_effort = "medium"\ndeveloper_instructions = "RETRY_TARGET Read project files."\n')
        if allow_backup:
            (self.home/'.codex/agents/backup.toml').write_text('model = \"fixture-backup\"\nmodel_reasoning_effort = \"medium\"\n')
        settings=self.rpc('studio/settings')
        settings['agent_overrides']['codex:researcher']={'enabled':True,'model':'fixture-codex','effort':'medium','allowed_models':['fixture-codex','fixture-backup'] if allow_backup else ['fixture-codex'],'allowed_efforts':['medium']}
        if tuple_selections:
            settings['agent_overrides']['codex:researcher']={'enabled':True,'provider':'codex','model':'fixture-codex','effort':'medium','allowed_selections':[{'provider':'codex','model':'fixture-backup','effort':'medium'}]}
        self.rpc('studio/settings',{'settings':settings})
        cid=self.conversation();self.send(cid,'Research and review')
        return cid

    def research_task(self,detail):
        return next((t for t in detail['tasks'] if t['agent_id']=='agent:researcher'),None)

    def wait_retry(self,cid):
        return self.wait(cid,lambda d: self.research_task(d) is not None and self.research_task(d).get('retry_at') is not None and self.research_task(d)['status']=='queued')

    def test_safe_pre_submission_failure_retries_once_with_durable_evidence(self):
        cid=self.retry_scenario('transient_once');detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'completed',detail)
        task=self.research_task(detail);self.assertEqual(len(task['attempts']),2)
        self.assertEqual(task['attempts'][0]['status'],'failed')
        self.assertTrue(task['attempts'][0]['retry_evidence']['safe_to_retry'])
        self.assertEqual(task['attempts'][1]['status'],'completed')
        self.assertTrue(any('Retrying after' in m['text'] for m in detail['messages']))
        self.cleanup();self.launch()
        self.assertEqual(self.research_task(self.get(cid))['attempts'],task['attempts'])

    def test_retry_exhaustion_is_bounded_and_dependents_do_not_run(self):
        cid=self.retry_scenario('transient_always');detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'failed')
        task=self.research_task(detail)
        self.assertEqual(len(task['attempts']),3);self.assertTrue(task['retry_exhausted'])
        self.assertEqual((self.project/'.fixture-attempts').read_text(),'3')
        writer=next(t for t in detail['tasks'] if t['agent_id']=='agent:writer')
        self.assertEqual(writer['status'],'cancelled');self.assertIsNone(writer['run_id'])

    def test_allowlisted_same_provider_model_fallback_preserves_effort(self):
        cid=self.retry_scenario('model_fallback',allow_backup=True);detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'completed',detail)
        task=self.research_task(detail)
        self.assertEqual([a['model'] for a in task['attempts']],['fixture-codex','fixture-backup'])
        self.assertTrue(all(a['effort']=='medium' for a in task['attempts']))
        self.assertEqual(task['provider'],'codex')
        self.assertTrue(any('Switching to allowed model' in m['text'] for m in detail['messages']))

    def test_tuple_allowlist_supports_same_provider_fallback(self):
        cid=self.retry_scenario('model_fallback',allow_backup=True,tuple_selections=True);detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'completed',detail)
        task=self.research_task(detail)
        self.assertEqual([a['model'] for a in task['attempts']],['fixture-codex','fixture-backup'])
        self.assertTrue(all(a['effort']=='medium' for a in task['attempts']))

    def test_missing_model_does_not_fallback_outside_allowed_set(self):
        cid=self.retry_scenario('model_fallback');detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'failed')
        task=self.research_task(detail);self.assertEqual(len(task['attempts']),1)
        self.assertEqual(task['model'],'fixture-codex')
        self.assertIn('No untried model',task['retry_notice'])

    def test_started_and_unknown_failures_are_not_replayed(self):
        for scenario in ('started_failure','unknown_prestart','started_before_reply'):
            with self.subTest(scenario=scenario):
                (self.project/'.fixture-attempts').unlink(missing_ok=True)
                cid=self.retry_scenario(scenario);detail=self.done(cid)
                self.assertEqual(detail['conversation']['status'],'failed')
                self.assertEqual(len(self.research_task(detail)['attempts']),1)
                self.assertEqual((self.project/'.fixture-attempts').read_text(),'1')
        self.assertEqual((self.project/'.fixture-side-effects').read_text(),'1')

    def test_cancel_during_backoff_prevents_new_attempt(self):
        cid=self.retry_scenario('transient_always');self.wait_retry(cid)
        self.rpc('studio/conversation/stop',{'conversation_id':cid})
        time.sleep(1.4)
        self.assertEqual(self.get(cid)['conversation']['status'],'cancelled')
        self.assertEqual((self.project/'.fixture-attempts').read_text(),'1')

    def test_changed_agent_policy_blocks_pending_retry(self):
        cid=self.retry_scenario('transient_once');self.wait_retry(cid)
        settings=self.rpc('studio/settings');settings['agent_overrides']['codex:researcher']['enabled']=False
        self.rpc('studio/settings',{'settings':settings})
        detail=self.done(cid)
        self.assertEqual(detail['conversation']['status'],'failed')
        self.assertEqual((self.project/'.fixture-attempts').read_text(),'1')
        self.assertIn('disabled',self.research_task(detail)['error'])

    def test_restart_during_backoff_does_not_repeat_work(self):
        cid=self.retry_scenario('transient_always');self.wait_retry(cid)
        self.cleanup();self.launch();time.sleep(1.1)
        self.assertEqual(self.get(cid)['conversation']['status'],'interrupted')
        self.assertEqual((self.project/'.fixture-attempts').read_text(),'1')


    def test_backoff_releases_global_slot_for_another_conversation(self):
        settings=self.rpc('studio/settings');settings['concurrency']=1
        self.rpc('studio/settings',{'settings':settings})
        failing=self.retry_scenario('transient_always');self.wait_retry(failing)
        other=self.conversation();self.send(other,'DIRECT_QUESTION while another task backs off')
        good=self.done(other);bad=self.done(failing)
        self.assertEqual(good['conversation']['status'],'completed')
        planner=next(t for t in good['tasks'] if t['kind']=='planner')
        retry=self.research_task(bad)['attempts'][1]
        self.assertLess(int(planner['attempts'][0]['started_at']),int(retry['started_at']))

    def test_fallback_policy_is_rechecked_after_backoff(self):
        cid=self.retry_scenario('model_fallback',allow_backup=True);self.wait_retry(cid)
        settings=self.rpc('studio/settings')
        settings['agent_overrides']['codex:researcher']['allowed_models']=['fixture-codex']
        self.rpc('studio/settings',{'settings':settings})
        detail=self.done(cid);task=self.research_task(detail)
        self.assertEqual(detail['conversation']['status'],'failed')
        self.assertEqual(len(task['attempts']),1)
        self.assertIn('allowed set',task['error'])
        self.assertEqual((self.project/'.fixture-attempts').read_text(),'1')

if __name__=='__main__':unittest.main()
