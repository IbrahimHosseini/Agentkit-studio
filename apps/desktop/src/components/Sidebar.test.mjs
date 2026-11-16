import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';

const directory = await mkdtemp(fileURLToPath(new URL('../../node_modules/.sidebar-test-', import.meta.url)));
after(() => rm(directory, { recursive: true, force: true }));
const source = new URL('./Sidebar.svelte', import.meta.url);
const { js } = compile(await readFile(source, 'utf8'), { filename:fileURLToPath(source), generate:'server' });
const code = js.code.replaceAll('"../lib/protocol"',JSON.stringify(new URL('../lib/protocol.ts',import.meta.url).href));
await writeFile(`${directory}/Sidebar.mjs`,code);
const { default: Sidebar } = await import(pathToFileURL(`${directory}/Sidebar.mjs`).href);
const projects = [{id:'alpha',name:'Alpha',path:'/tmp/alpha'},{id:'beta',name:'Beta',path:'/tmp/beta'}];
const chat = (id,project_id,title,status='completed') => ({id,project_id,title,status});
const conversations = [chat('a1','alpha','First Alpha'),chat('b1','beta','Only Beta'),chat('a2','alpha','دوم Alpha'),chat('direct',null,'Direct chat')];
const props = { projects,conversations,projectId:'alpha',conversationId:'a2',page:'chat',connected:true,busy:false,sort:'updated_at',direction:'desc',onSort(){},onDeleteProject(){},onDeleteConversation(){},onProject(){},onConversation(){},onNew(){},onAdd(){},onPage(){} };
const html = overrides => render(Sidebar,{props:{...props,...overrides}}).body.replace(/<!--[^]*?-->/g,'');
const projectNav = (output,name) => output.match(new RegExp(`<nav[^>]*aria-label="Conversations in ${name}"[^>]*>([^]*?)</nav>`))?.[0]??'';
const allNav = output => output.match(/<nav class="conversation-list"[^>]*>([^]*?)<\/nav>/)?.[0]??'';

test('project chats are grouped by project while every chat remains in All conversations', () => {
  const output = html();
  const nested = projectNav(output,'Alpha');
  assert.ok(nested.includes('First Alpha'));
  assert.ok(nested.includes('دوم Alpha'));
  assert.ok(!nested.includes('Only Beta'));
  assert.ok(!nested.includes('Direct chat'));
  for (const conversation of conversations) assert.ok(allNav(output).includes(conversation.title));
  assert.ok(nested.indexOf('First Alpha')<nested.indexOf('دوم Alpha'));
  assert.match(nested,/<span dir="auto">دوم Alpha<\/span>/);
});

test('open project retains its existing icon and closed projects use the closed folder', () => {
  const output = html();
  assert.match(output,/<button class="project-row selected"[^>]*aria-expanded="true"[^]*?M3 17V6/);
  assert.match(output,/<button class="project-row"[^>]*aria-expanded="false"[^]*?M3 7a2 2/);
  assert.match(projectNav(output,'Beta'),/ hidden/);
  const closed = html({projectId:'',conversationId:''});
  assert.equal((closed.match(/aria-expanded="false" aria-controls="[^\"]+-project-/g)??[]).length,2);
});

test('selected conversation and active status appear in both project and global lists', () => {
  const output = html({conversations:conversations.map(c=>c.id==='a2'?{...c,status:'running'}:c)});
  for (const list of [projectNav(output,'Alpha'),allNav(output)]) {
    assert.match(list,/class="conversation-row selected"[^>]*aria-current="page"/);
    assert.match(list,/class="dot active" aria-label="running"/);
    assert.match(list,/aria-label="Delete conversation دوم Alpha"[^>]* disabled/);
  }
});

test('long project lists start with five chats and expose Show more without limiting All conversations', () => {
  const many = Array.from({length:8},(_,i)=>chat(`a${i}`,'alpha',`Alpha chat ${i}`));
  const output = html({conversations:many,conversationId:''});
  const nested = projectNav(output,'Alpha');
  assert.equal((nested.match(/class="conversation-row/g)??[]).length,5);
  assert.match(nested,/>Show more<\/button>/);
  assert.ok(!nested.includes('Alpha chat 7'));
  assert.ok(allNav(output).includes('Alpha chat 7'));
  const selected = projectNav(html({conversations:many,conversationId:'a7'}),'Alpha');
  assert.ok(selected.includes('Alpha chat 7'));
  assert.match(selected,/>Show less<\/button>/);
});

test('empty groups and escaped chat titles remain readable without rendering HTML', () => {
  assert.match(projectNav(html({projects:[...projects,{id:'empty',name:'Empty',path:'/tmp/empty'}],projectId:'empty',conversationId:''}),'Empty'),/No conversations yet\./);
  const output = html({conversations:[chat('unsafe','alpha','<img src=x onerror=alert(1)>')],conversationId:'unsafe'});
  assert.ok(!output.includes('<img'));
  assert.match(output,/&lt;img/);
});
