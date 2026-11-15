import test from 'node:test';
import assert from 'node:assert/strict';
import {createAttachmentController} from './attachments.ts';

function fixture() {
  const drafts = new Map(), pending = new Map(), removed = [], revoked = [];
  let current = 'first', closed = false;
  const host = {
    read: key => drafts.get(key) ?? [],
    write: (key, items) => drafts.set(key, items),
    pending: (key, count) => pending.set(key, count),
    disposed: () => closed,
    createPreview: () => 'blob:test',
    revokePreview: url => revoked.push(url),
    removeStaged: async path => {removed.push(path);},
  };
  return {controller:createAttachmentController(host),host,drafts,pending,removed,revoked,
    get current(){return current;},switchDraft(key){current=key;},close(){closed=true;}};
}
const info = (path='/tmp/image.png',size=100) => ({path,name:path.split('/').pop(),size,kind:'image',mime:'image/png',preview_bytes:null});
function deferred(){let resolve;const promise=new Promise(r=>resolve=r);return{promise,resolve};}

test('an asynchronous drop stays in its captured draft when the user switches conversations',async()=>{
  const f=fixture(),gate=deferred(),origin=f.current;
  const operation=f.controller.run(origin,async()=>{await gate.promise;await f.controller.add(origin,info());});
  assert.equal(f.controller.isPending(origin),true);
  f.switchDraft('second');
  assert.equal(f.controller.isPending(f.current),false);
  gate.resolve();await operation;
  assert.equal(f.drafts.get('first').length,1);
  assert.equal(f.drafts.has('second'),false);
  assert.equal(f.controller.isPending(origin),false);
});

test('concurrent attachment operations keep send blocked until both finish',async()=>{
  const f=fixture(),a=deferred(),b=deferred();
  const first=f.controller.run('first',()=>a.promise),second=f.controller.run('first',()=>b.promise);
  a.resolve();await first;assert.equal(f.controller.isPending('first'),true);
  b.resolve();await second;assert.equal(f.controller.isPending('first'),false);
});

test('duplicate canonical paths do not consume slots or byte budget at the limit',async()=>{
  const f=fixture();
  for(let n=0;n<12;n++)await f.controller.add('first',info(`/tmp/${n}.png`));
  await f.controller.add('first',info('/tmp/0.png',40*1024*1024));
  assert.equal(f.drafts.get('first').length,12);
  assert.deepEqual(f.removed,[]);
});

test('a rejected staged image is cleaned up but an original file is never removed',async()=>{
  const f=fixture();
  await f.controller.add('first',info('/tmp/one',16*1024*1024));
  await f.controller.add('first',info('/tmp/two',16*1024*1024));
  await assert.rejects(f.controller.add('first',info('/tmp/staged',16*1024*1024),true),/40 MiB/);
  await assert.rejects(f.controller.add('first',info('/tmp/original',16*1024*1024)),/40 MiB/);
  assert.deepEqual(f.removed,['/tmp/staged']);
  assert.equal(f.drafts.get('first').length,2);
});

test('a late staged result after disposal is removed without mutating a draft',async()=>{
  const f=fixture();f.close();
  await assert.rejects(f.controller.add('first',info(),true),/closed/);
  assert.deepEqual(f.removed,['/tmp/image.png']);assert.equal(f.drafts.size,0);
});

test('history retention refusal preserves the draft attachment and its preview',async()=>{
  const f=fixture();await f.controller.add('first',info(),true);
  f.host.removeStaged=async()=>{throw new Error('Image retained by history');};
  await assert.rejects(f.controller.remove('first',f.drafts.get('first')[0].id),/retained/);
  assert.equal(f.drafts.get('first').length,1);assert.deepEqual(f.revoked,[]);
  assert.equal(f.controller.isPending('first'),false);
});

test('removing from a captured draft cannot remove an attachment in the newly selected draft',async()=>{
  const f=fixture(),gate=deferred();await f.controller.add('first',info(),true);await f.controller.add('second',info('/tmp/second.png'));
  f.host.removeStaged=()=>gate.promise;
  const removal=f.controller.remove('first',f.drafts.get('first')[0].id);f.switchDraft('second');
  gate.resolve();await removal;assert.equal(f.drafts.get('first').length,0);assert.equal(f.drafts.get('second').length,1);
});

test('an image shared by another unsent draft is detached without deleting its staged file',async()=>{
  const f=fixture();await f.controller.add('first',info(),true);await f.controller.add('second',info());
  f.host.usedByOtherDraft=(key,path)=>[...f.drafts].some(([other,items])=>other!==key&&items.some(item=>item.path===path));
  await f.controller.remove('first',f.drafts.get('first')[0].id);
  assert.deepEqual(f.removed,[]);assert.equal(f.drafts.get('first').length,0);assert.equal(f.drafts.get('second').length,1);
});
