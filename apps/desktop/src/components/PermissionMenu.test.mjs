import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import { permissionMode } from '../lib/permissions.ts';

const directory = await mkdtemp(fileURLToPath(new URL('../../node_modules/.permission-test-', import.meta.url)));
after(() => rm(directory, { recursive: true, force: true }));
async function component(name, imports = {}) {
  const source = new URL(`./${name}.svelte`, import.meta.url);
  const { js } = compile(await readFile(source, 'utf8'), { filename:fileURLToPath(source), generate:'server' });
  let code = js.code;
  for (const [specifier, target] of Object.entries(imports)) {
    code = code.replaceAll(`'${specifier}'`, JSON.stringify(target)).replaceAll(`"${specifier}"`, JSON.stringify(target));
  }
  const target = `${directory}/${name}.mjs`;
  await writeFile(target, code);
  return (await import(pathToFileURL(target).href)).default;
}
const PermissionMenu = await component('PermissionMenu', {
  '../lib/permissions': new URL('../lib/permissions.ts', import.meta.url).href,
});
const Composer = await component('Composer', {
  './PermissionMenu.svelte': pathToFileURL(`${directory}/PermissionMenu.mjs`).href,
  '../lib/model-options': new URL('../lib/model-options.ts', import.meta.url).href,
});
const TaskNotices = await component('TaskNotices');
const RetryDetails = await component('RetryDetails', {
  './TaskNotices.svelte': pathToFileURL(`${directory}/TaskNotices.mjs`).href,
  '../lib/protocol': new URL('../lib/protocol.ts', import.meta.url).href,
});
const html = (Component, props) => render(Component, { props }).body.replace(/<!--[^]*?-->/g, '');
const menu = props => html(PermissionMenu, { value:'ask', hasProject:true, onChange:()=>{}, ...props });

test('legacy or malformed saved modes never grant automatic or full access', () => {
  for (const value of [undefined,null,'',false,{},'full','bypassPermissions','AUTO','ask']) assert.equal(permissionMode(value),'ask');
  assert.equal(permissionMode('auto'),'auto');
  assert.equal(permissionMode('full_access'),'full_access');
});

test('permission choice menu exposes three radio choices and one saved selection', () => {
  for (const [value, label] of [['ask','Ask for approval'],['auto','Approve for me'],['full_access','Full access']]) {
    const output = menu({value});
    assert.equal((output.match(/role="menuitemradio"/g)??[]).length,3);
    assert.equal((output.match(/aria-checked="true"/g)??[]).length,1);
    assert.match(output,new RegExp(`aria-checked="true"[^]*?permission-option-label">${label}`));
    assert.match(output,new RegExp(`<span>${label}</span>`));
    assert.match(output,/aria-haspopup="menu" aria-expanded="false"/);
    assert.match(output,/How should actions be approved\?/);
    assert.match(output,/class="permission-popover"[^>]* hidden/);
  }
});

test('full access is explicitly distinguished and automatic review does not promise blanket approval', () => {
  const output = menu({value:'auto'});
  assert.match(output,/let the provider review elevated access/);
  assert.match(output,/Full access removes the project sandbox and approval prompts/);
  assert.match(output,/Automatic review may use additional model calls/);
  assert.match(output,/class="permission-option full-access"/);
  assert.ok(!output.includes('Only ask for actions detected as potentially unsafe'));
});

test('projectless chats clearly retain read-only access in all three modes', () => {
  for (const value of ['ask','auto','full_access']) {
    assert.match(menu({value,hasProject:false}),/Chats without a project stay read-only\. Attach a project to allow changes\./);
    assert.ok(!menu({value,hasProject:true}).includes('Chats without a project stay read-only'));
  }
});

test('disabled menu prevents trigger and choice interaction', () => {
  const output = menu({disabled:true});
  assert.equal((output.match(/ disabled/g)??[]).length,4);
});

const composerProps = {
  selection:null, providers:[], catalog:{models:[],providers:[]}, selectionReady:true,
  onSelection:()=>{}, permissionMode:'auto', onPermissionChange:()=>{}, attachments:[],
  disabled:false, busy:false, active:false, projects:[], projectId:'project',
  onProjectChange:()=>{}, onSend:()=>{}, onAttach:()=>{}, onRemove:()=>{}, onPaste:()=>{}, onStop:()=>{},
};
test('composer places saved chat permission below the form and disables changes while running or saving', () => {
  const output = html(Composer,composerProps);
  assert.ok(output.indexOf('</form>') < output.indexOf('permission-control'));
  assert.match(output,/<span>Approve for me<\/span>/);
  for (const state of [{active:true},{busy:true},{disabled:true}]) {
    const disabled = html(Composer,{...composerProps,...state});
    assert.match(disabled,/class="permission-trigger"[^>]* disabled/);
  }
});

test('automatic permission review progress is visible and not duplicated with connection notices', () => {
  const notice = 'Reviewing elevated access — بررسی دسترسی';
  for (const Component of [TaskNotices,RetryDetails]) {
    const output = html(Component,{task:{permission_notice:notice,retry_notice:notice}});
    assert.equal(output.split(notice).length-1,1);
    assert.match(output,/role="status" dir="auto"/);
    const distinct = html(Component,{task:{permission_notice:'Permission review approved',retry_notice:'Codex connection recovered'}});
    assert.ok(distinct.includes('Permission review approved'));
    assert.ok(distinct.includes('Codex connection recovered'));
  }
});

test('permission progress text is escaped and absent notices produce no status placeholder', () => {
  assert.equal(html(TaskNotices,{task:{}}),'');
  const output = html(TaskNotices,{task:{permission_notice:'Review denied: <script>unsafe</script>'}});
  assert.ok(!output.includes('<script>'));
  assert.ok(output.includes('&lt;script>'));
});
