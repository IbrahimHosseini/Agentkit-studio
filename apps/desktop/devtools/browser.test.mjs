import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { browserUrl, boundedNumber, createBridge } from './browser.mjs';

test('browser URLs permit web projects and reject privileged schemes and credentials', () => {
  assert.equal(browserUrl('http://localhost:3000'), 'http://localhost:3000/');
  assert.equal(browserUrl('about:blank'), 'about:blank');
  for (const url of ['file:///etc/passwd','javascript:alert(1)','data:text/html,hello','https://user:secret@example.com','not a url'])
    assert.throws(() => browserUrl(url));
});
test('viewport and input bounds reject malformed and out of range numbers', () => {
  assert.equal(boundedNumber(undefined, 1024, 320, 1920), 1024);
  for (const n of [NaN,Infinity,-1,2000,'800',null]) assert.throws(() => boundedNumber(n, 1024, 320, 1920));
});
test('empty session does not launch a browser for list or close', async () => {
  const bridge=createBridge();
  assert.deepEqual(await bridge.dispatch('list'), {pages:[],active_page_id:null});
  await assert.rejects(bridge.dispatch('frame'), /Open a browser tab/);
  assert.deepEqual(await bridge.close(), {pages:[],active_page_id:null});
});

test('real Chromium local fixture: input, images, tabs, console, isolation', {skip:process.env.STUDIO_TEST_BROWSER !== '1'}, async () => {
  const server=createServer((_req,res)=>{res.setHeader('Content-Type','text/html');res.end('<title>Studio fixture</title><label>Name<input id="name"></label><button id="run" onclick="document.querySelector(\'output\').textContent=document.querySelector(\'input\').value;console.log(\'fixture-click\')">Run</button><output></output>');});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const url=`http://127.0.0.1:${server.address().port}`;
  const a=createBridge(),b=createBridge();
  try {
    const opened=await a.dispatch('open',{url});
    assert.equal(opened.pages.length,1);
    await a.dispatch('key',{key:'Tab'});
    await a.dispatch('type',{text:'سلام Studio'});
    await a.dispatch('key',{key:'Tab'});
    await a.dispatch('key',{key:'Enter'});
    assert.match((await a.dispatch('snapshot')).text,/سلام Studio/);
    assert.ok((await a.dispatch('console')).entries.some(e=>e.text==='fixture-click'));
    const frame=await a.dispatch('frame');
    assert.equal(frame.width,1024);assert.equal(frame.height,768);
    assert.equal(Buffer.from(frame.data,'base64').subarray(0,2).toString('hex'),'ffd8');
    await a.dispatch('open',{url:'about:blank'});
    assert.equal((await a.dispatch('list')).pages.length,2);
    await b.dispatch('open',{url});
    assert.doesNotMatch((await b.dispatch('snapshot')).text,/سلام Studio/);
    await assert.rejects(a.dispatch('navigate',{url:'file:///etc/passwd'}),/HTTP/);
    await a.dispatch('close',{page_id:opened.pages[0].id});
    assert.equal((await a.dispatch('list')).pages.length,1);
  } finally {await a.close();await b.close();await new Promise(resolve=>server.close(resolve));}
});
