#!/usr/bin/env python3
"""Offline provider protocol fixture for Studio orchestration; no model calls."""
import json
import os
from pathlib import Path
import sys
import time
import uuid

def emit(value):
    print(json.dumps(value, ensure_ascii=False),flush=True)

def record(value):
    with open(os.environ['STUDIO_FIXTURE_LOG'],'a',encoding='utf-8') as stream:
        stream.write(json.dumps(value,ensure_ascii=False)+'\n')

def answer(text):
    if 'STUDIO_CHAT_DIRECT_V1' in text:
        return 'Direct chat fixture result — پاسخ مستقیم'
    if 'STUDIO_PLANNER_V1' in text:
        if 'DIRECT_QUESTION' in text:
            return json.dumps({'summary':'Answer directly','response':'پاسخ مستقیم fixture','tasks':[]},ensure_ascii=False)
        write='WRITE_TASK' in text
        tasks=[{'id':'research','agent_id':'codex:researcher','title':'Research','prompt':'Read fixture context','model':None,'effort':None,'read_only':True,'dependencies':[],'files':[],'reason':'Installed default'}]
        tasks.append({'id':'report','agent_id':'claude:writer','title':'Write report' if write else 'Review','prompt':'WRITE_TASK' if write else 'Review research','model':None,'effort':None,'read_only':not write,'dependencies':['research'],'files':['report.txt'] if write else [],'reason':'Installed default'})
        if 'CODEX_WRITE_TASK' in text:
            tasks[-1]['agent_id']='codex:researcher'
        if 'INVALID_MODEL' in text:
            tasks[0]['model']='not-an-allowed-model'
        if 'BAD_DEPENDENCY' in text:
            tasks[0]['dependencies']=['missing']
        return json.dumps({'summary':'Research and review with two runtimes','response':None,'tasks':tasks})
    if 'STUDIO_SUMMARIZER_V1' in text:
        return 'Combined fixture result — نتیجهٔ نهایی'
    if 'WRITE_TASK' in text:
        Path('report.txt').write_text('fixture report',encoding='utf-8')
    return 'Worker fixture result'

def main():
    if 'generate-json-schema' in sys.argv:
        schema=Path(sys.argv[sys.argv.index('--out')+1])/'v2';schema.mkdir(parents=True)
        for name in ('ThreadStartParams','ThreadResumeParams','TurnStartParams'):
            (schema/(name+'.json')).write_text(json.dumps({'properties':{'approvalsReviewer':{}},'definitions':{'ApprovalsReviewer':{'enum':['user','auto_review']}}}))
        return
    if '--version' in sys.argv:
        print('codex-cli 0.150.0' if 'codex' in Path(sys.argv[0]).name else '2.1.296 (Claude Code)')
        return
    if '--help' in sys.argv:
        time.sleep(float(os.environ.get('STUDIO_FIXTURE_HELP_DELAY','0')))
        print('app-server --safe-mode --model --effort --agent --agents --input-format --output-format --permission-mode --permission-prompts host auto bypassPermissions default --disallowedTools --tools --mcp-config --strict-mcp-config --json-schema --append-system-prompt --forward-subagent-text')
        return
    record({'argv':sys.argv[1:],'pid':os.getpid(),'event':'started'})
    if 'app-server' in sys.argv:
        instructions=''
        for line in sys.stdin:
            msg=json.loads(line)
            record({'rpc':msg,'pid':os.getpid()})
            method=msg.get('method')
            p=msg.get('params',{})
            if method=='initialize': emit({'id':msg['id'],'result':{'userAgent':'studio-fixture'}})
            elif method in ('thread/start','thread/resume'):
                instructions=json.dumps(p)
                scenario_file=Path('.fixture-scenario')
                scenario=scenario_file.read_text().strip() if scenario_file.exists() else ''
                if scenario=='direct_model_unavailable' and 'STUDIO_CHAT_DIRECT_V1' in instructions:
                    emit({'id':msg['id'],'error':{'code':-32600,'message':'Selected model unavailable','data':{'codexErrorInfo':'modelNotFound'}}})
                    continue
                if 'RETRY_TARGET' in instructions:
                    count_file=Path('.fixture-attempts')
                    count=int(count_file.read_text())+1 if count_file.exists() else 1
                    count_file.write_text(str(count))
                    record({'event':'worker_attempt','count':count,'model':p.get('model'),'time':time.time()})
                    if scenario=='transient_always' or scenario=='transient_once' and count==1:
                        emit({'id':msg['id'],'error':{'code':-32001,'message':'Server overloaded; retry later.'}})
                        continue
                    if scenario=='model_fallback' and p.get('model')=='fixture-codex':
                        emit({'id':msg['id'],'error':{'code':-32600,'message':'Selected model unavailable','data':{'codexErrorInfo':'modelNotFound'}}})
                        continue
                    if scenario=='started_before_reply':
                        emit({'method':'turn/started','params':{'turn':{'id':'unexpected-start'}}})
                        emit({'id':msg['id'],'error':{'code':-32001,'message':'Server overloaded; retry later.'}})
                        continue
                    if scenario=='unknown_prestart':
                        emit({'id':msg['id'],'error':{'code':-32603,'message':'Transient error, please retry'}})
                        continue
                emit({'id':msg['id'],'result':{'thread':{'id':'fixture-'+str(uuid.uuid4())}}})
            elif method=='config/read': emit({'id':msg['id'],'result':{'config':{'mcp_servers':{}}}})
            elif method=='model/list':
                emit({'id':msg['id'],'result':{'data':[{'id':'fixture-codex','model':'fixture-codex','displayName':'Fixture','defaultReasoningEffort':'medium','supportedReasoningEfforts':[{'reasoningEffort':'medium'},{'reasoningEffort':'high'}]}],'nextCursor':None}})
            elif method=='account/read': emit({'id':msg['id'],'result':{'account':{'type':'chatgpt'},'requiresOpenaiAuth':True}})
            elif method=='turn/start':
                text=instructions+' '+json.dumps(p)
                if 'RETRY_TARGET' in instructions and scenario=='started_failure':
                    side_effect=Path('.fixture-side-effects')
                    count=int(side_effect.read_text())+1 if side_effect.exists() else 1
                    side_effect.write_text(str(count))
                    emit({'method':'turn/started','params':{'turn':{'id':'turn-fixture'}}})
                    emit({'id':msg['id'],'error':{'code':-32001,'message':'Server overloaded; retry later.'}})
                    continue
                emit({'id':msg['id'],'result':{'turn':{'id':'turn-fixture'}}})
                current = text.rsplit('Current user message:',1)[-1] if 'STUDIO_CHAT_DIRECT_V1' in text else text
                if 'HOLD_TASK' in current:
                    time.sleep(30)
                output=answer(text)
                emit({'method':'item/agentMessage/delta','params':{'threadId':'fixture','delta':output}})
                emit({'method':'turn/completed','params':{'turn':{'status':'completed'}}})
            elif method=='turn/interrupt':
                emit({'id':msg['id'],'result':{}})
                break
    else:
        msg=json.loads(sys.stdin.readline())
        record({'input':msg,'pid':os.getpid()})
        text=json.dumps(msg)+' '+' '.join(sys.argv)
        emit({'type':'system','subtype':'init','session_id':'claude-fixture'})
        output=answer(text)
        emit({'type':'stream_event','session_id':'claude-fixture','event':{'delta':{'text':output}}})
        emit({'type':'result','session_id':'claude-fixture','subtype':'success','is_error':False,'result':output})

if __name__=='__main__': main()
