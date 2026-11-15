import test from 'node:test';
import assert from 'node:assert/strict';
import { modelOptions, effortOptions, validEffort, mergeSettings, defaultSelection } from './model-options.ts';
const agents = [
  {provider:'codex',model:'codex-installed',effort:'high'},
  {provider:'claude',model:'claude-installed',effort:'medium'},
];
test('catalog choices retain installed and saved allowed models with provider isolation', () => {
  const options = modelOptions('codex',agents,{provider:'codex',models:[{id:'advertised',verification:'advertised'}]},['saved-allowed',null]);
  assert.deepEqual(options.map(o=>o.value),['codex-installed','saved-allowed','advertised']);
  assert.match(options[0].label,/Installed/);
  assert.match(options[1].label,/Saved/);
  assert.match(options[2].label,/Advertised/);
  assert.deepEqual(modelOptions('claude',agents,{provider:'codex',models:[{id:'wrong'}]},[]).map(o=>o.value),['claude-installed']);
});
test('effort changes reset unsupported choices and retain explicitly installed effort evidence', () => {
  const catalog = {provider:'claude',models:[{id:'claude-installed',verification:'installed',installedEfforts:['medium'],supportedReasoningEfforts:[]}]};
  const options = effortOptions('claude','claude-installed',agents,catalog);
  assert.deepEqual(options,[{value:'medium',label:'medium · Installed'}]);
  assert.equal(validEffort('high',options),null);
  assert.equal(validEffort('medium',options),'medium');
  assert.deepEqual(effortOptions('codex','claude-installed',agents,catalog),[]);
});
test('connection response preserves unsaved preferences without reverting immediate connection change', () => {
  const original = {claude_auth_mode:'configured',provider_connections:{codex:true,claude:true},superagent:{provider:'codex',model:null,effort:null},agent_overrides:{},concurrency:4,mcp_enabled:false};
  const draft = structuredClone(original); draft.concurrency=8; draft.claude_auth_mode='native';
  const incoming=structuredClone(original); incoming.provider_connections.claude=false;
  const merged=mergeSettings(original,draft,incoming);
  assert.equal(merged.provider_connections.claude,false);
  assert.equal(merged.concurrency,8);
  assert.equal(merged.claude_auth_mode,'native');
  const saved=structuredClone(incoming);saved.concurrency=6;
  assert.equal(mergeSettings(original,draft,saved).concurrency,6);
});

test('choosing defaults extends nonempty limits while empty lists keep default-only semantics', () => {
  assert.deepEqual(defaultSelection('new','high',['old'],['medium']), {
    model:'new',effort:'high',allowed_models:['old','new'],allowed_efforts:['medium','high']
  });
  assert.deepEqual(defaultSelection('new',null,['old'],['medium']), {
    model:'new',effort:null,allowed_models:['old','new'],allowed_efforts:['medium']
  });
  assert.deepEqual(defaultSelection('new','high',[],[]), {
    model:'new',effort:'high',allowed_models:[],allowed_efforts:[]
  });
  assert.deepEqual(defaultSelection('old','medium',['old'],['medium']), {
    model:'old',effort:'medium',allowed_models:['old'],allowed_efforts:['medium']
  });
});

import { connectedModels, modelKey, modelEfforts, switchModel, selectionOptions, selectionKey, readChatSort } from './model-options.ts';
const connectedCatalog = {
  models: [
    {provider:'codex',id:'shared',verification:'advertised',defaultReasoningEffort:'medium',supportedReasoningEfforts:[{reasoningEffort:'medium'},{reasoningEffort:'high'}],installedEfforts:['unsupported']},
    {provider:'claude',id:'shared',verification:'installed',installedEfforts:['low']},
    {provider:'future',id:'new',verification:'advertised',supportedReasoningEfforts:[]},
  ],
  providers: [{provider:'codex',runtimeDefaultEfforts:['high']},{provider:'claude'}],
};
test('connected catalogs use provider plus model identity and exclude disconnected providers', () => {
  const models=connectedModels(connectedCatalog,['codex','claude']);
  assert.equal(models.length,2);
  assert.notEqual(modelKey(models[0].provider,models[0].id),modelKey(models[1].provider,models[1].id));
  assert.deepEqual(connectedModels(connectedCatalog,['future']).map(m=>m.id),['new']);
  assert.deepEqual(connectedModels(connectedCatalog,[]),[]);
});
test('switching provider resets effort to that model default and advertised capabilities take precedence', () => {
  assert.deepEqual(switchModel(connectedCatalog,'codex','shared'),{provider:'codex',model:'shared',effort:'medium'});
  assert.deepEqual(switchModel(connectedCatalog,'claude','shared'),{provider:'claude',model:'shared',effort:null});
  assert.deepEqual(modelEfforts(connectedCatalog,'codex','shared'),['medium','high']);
  assert.deepEqual(modelEfforts(connectedCatalog,'claude','shared'),['low']);
  assert.deepEqual(modelEfforts(connectedCatalog,'codex',null),['high']);
});
test('automatic choices are exact supported tuples, never a cross-provider Cartesian product', () => {
  const options=selectionOptions(connectedCatalog,['codex','claude']);
  assert.ok(options.some(o=>selectionKey(o)===selectionKey({provider:'claude',model:'shared',effort:'low'})));
  assert.ok(!options.some(o=>o.provider==='claude'&&o.effort==='high'));
  assert.ok(!options.some(o=>o.provider==='future'));
  assert.equal(new Set(options.map(selectionKey)).size,options.length);
});
test('sort preferences survive reload and malformed storage safely falls back', () => {
  assert.deepEqual(readChatSort('{"sort":"title","direction":"asc"}'),{sort:'title',direction:'asc'});
  for(const raw of [null,'invalid','{"sort":"unsupported","direction":"asc"}','{"sort":"title","direction":"sideways"}'])
    assert.deepEqual(readChatSort(raw),{sort:'updated_at',direction:'desc'});
});

import { agentOverrideFor } from './model-options.ts';
test('sparse and legacy overrides inherit effective fields without crashing or losing defaults', () => {
  const a={id:'agent:backend',aliases:['codex:backend'],provider:'codex',model:'shared',effort:'medium'};
  const sparse=agentOverrideFor(a,{'agent:backend':{enabled:false}});
  assert.equal(sparse.enabled,false);assert.equal(sparse.model,'shared');assert.equal(sparse.effort,'medium');
  assert.deepEqual(sparse.allowed_models,[]);
  assert.deepEqual(sparse.allowed_selections,[{provider:'codex',model:'shared',effort:'medium'}]);
  assert.equal(agentOverrideFor(a,{'codex:backend':{enabled:true,model:null,effort:null}}).model,null);
  assert.equal(agentOverrideFor(a,{'agent:backend':{enabled:false},'codex:backend':{enabled:true}}).enabled,false);
});


import { chatSelectionAvailable } from './model-options.ts';
test('direct chat selection requires its saved provider/model/effort, without disconnected fallback', () => {
  const selected={provider:'codex',model:'shared',effort:'high'};
  assert.equal(chatSelectionAvailable(connectedCatalog,['codex','claude'],selected),true);
  assert.equal(chatSelectionAvailable(connectedCatalog,['claude'],selected),false);
  assert.deepEqual(selected,{provider:'codex',model:'shared',effort:'high'});
  assert.equal(chatSelectionAvailable(connectedCatalog,['claude'],{provider:'claude',model:'shared',effort:'high'}),false);
  assert.equal(chatSelectionAvailable(connectedCatalog,['claude'],{provider:'claude',model:'shared',effort:'low'}),true);
  assert.equal(chatSelectionAvailable(connectedCatalog,['codex'],{provider:'codex',model:'removed',effort:null}),false);
  assert.equal(chatSelectionAvailable(connectedCatalog,[],null),false);
});
test('direct chat runtime-default model accepts only its own evidenced effort', () => {
  assert.equal(chatSelectionAvailable(connectedCatalog,['codex'],{provider:'codex',model:null,effort:'high'}),true);
  assert.equal(chatSelectionAvailable(connectedCatalog,['claude'],{provider:'claude',model:null,effort:'high'}),false);
  assert.equal(chatSelectionAvailable(connectedCatalog,['claude'],{provider:'claude',model:null,effort:null}),true);
});
