<script lang="ts">
  import type { Agent, AgentOverride, Connections, Settings, Page } from "../lib/protocol";
  import { label } from "../lib/protocol";
  import { request } from "../lib/api";
  import { untrack } from "svelte";
  import ModelSettings from "./ModelSettings.svelte";
  import { mergeSettings, agentOverrideFor, chatSelectionAvailable, type ModelCatalog } from "../lib/model-options";
  let catalogBusy = $state(""), catalogError = $state<Record<string,string>>({});
  const catalogFor = (provider: string) => catalog.providers.find(c=>c.provider===provider);
  async function loadCatalog(provider: string) {
      catalogBusy = provider;
      catalogError[provider] = "";
      try {
          await request("studio/models", {provider, workspace});
          await onCatalogRefresh();
      } catch (e) { catalogError[provider] = String(e); }
      finally { catalogBusy = ""; }
  }
  let { page, agents, connections, settings, busy, workspace, onSave, onRefresh, onConnection, catalog, onCatalogRefresh }: {
      page: Page;
      catalog: ModelCatalog;
      onCatalogRefresh: () => Promise<void>;
      agents: Agent[];
      connections: Connections | null;
      settings: Settings | null;
      busy: boolean;
      workspace: string;
      onSave: (s: Settings) => Promise<void>;
      onRefresh: () => void;
      onConnection: (provider: string, enabled: boolean) => Promise<void>;
  } = $props();
  const connectedProviders = $derived(Object.entries(settings?.provider_connections ?? {}).filter(([,enabled])=>enabled).map(([provider])=>provider));
  let draft = $state<Settings | null>(null), saved = $state(false), search = $state("");
  let previousSettings: Settings | null = null;
  $effect(() => {
      const incoming = settings ? structuredClone($state.snapshot(settings)) : null;
      draft = incoming ? mergeSettings(previousSettings, untrack(() => draft ? $state.snapshot(draft) : null), incoming) : null;
      previousSettings = incoming;
      saved = false;
  });
  const filtered = $derived(agents.filter(a => (a.name + " " + a.provider + " " + a.description).toLowerCase().includes(search.toLowerCase())));
  function config(a: Agent): AgentOverride {
      return agentOverrideFor(a, draft?.agent_overrides ?? {});
  }
  function change(a: Agent, field: keyof AgentOverride, value: string | boolean) {
      if (!draft)
          return;
      const v = field === "allowed_models" || field === "allowed_efforts" ? String(value).split(",").map(s => s.trim()).filter(Boolean) : typeof value === "string" ? (value.trim() || null) : value;
      draft.agent_overrides = { ...draft.agent_overrides, [a.id]: { ...config(a), [field]: v } };
      saved = false;
  }
  async function save() {
      if (draft) {
          try {
              await onSave($state.snapshot(draft));
              saved = true;
          }
          catch {
              saved = false;
          }
      }
  }
</script>
<section class="settings-page">
<div class="settings-heading">
<div>
<span class="eyebrow">WORKSPACE SETTINGS</span>
<h1>{page==='agents'?'Your agents':page==='connections'?'Connections':'Preferences'}</h1>
<p>{page==='agents'?'A team you already know. Defaults you control.':page==='connections'?'Use the accounts and tools you already have.':'Set the boundaries. Let your team handle the details.'}</p>
</div>
<button onclick={onRefresh} disabled={busy}>Refresh</button>
</div>

{#if !settings}
<div class="empty-card">Connect to the local engine to load settings.</div>
{:else if draft}


{#if page==='agents'}

 <div class="settings-intro">
<span>{agents.length} roles · One role across connected providers. Studio overrides never rewrite installed definitions.</span>
<input class="agent-search" aria-label="Search agents" bind:value={search} placeholder="Find an agent…"/>
</div>
 <div class="agent-grid">
{#each filtered as agent(agent.id)}
{@const conf=config(agent)}
{@const unavailable=!chatSelectionAvailable(catalog,connectedProviders,{provider:conf.provider??agent.provider,model:conf.model,effort:conf.effort})}
<article class="agent-card" class:model-unavailable={unavailable}>
<div class="agent-card-heading">
<div class="avatar">{agent.name.slice(0,1).toUpperCase()}</div>
<div>
<h2>{agent.name}</h2>
<span class="provider-tag">{conf.provider??agent.provider} · {agent.scope}</span>
</div>
<label class="toggle-label">
<input type="checkbox" checked={conf.enabled} onchange={e=>change(agent,'enabled',e.currentTarget.checked)}/>
<span class="sr-only">Enable {agent.id}</span>
</label>
</div>
<p class="agent-description" dir="auto">{agent.description||'Installed agent definition'}</p>
<div class="default-info">Installed default: <bdi>{('source_model' in agent?agent.source_model:agent.model)||'Runtime default'}</bdi> · {('source_effort' in agent?agent.source_effort:agent.effort)||'Runtime default'}</div>
<ModelSettings provider={conf.provider??agent.provider} providers={connectedProviders} {catalog} model={conf.model} effort={conf.effort} allowedSelections={conf.allowed_selections??[]} onChange={patch=>{if(draft){draft.agent_overrides={...draft.agent_overrides,[agent.id]:{...config(agent),...patch}};saved=false;}}}/>
{#if unavailable}<p class="inline-error" role="status">No available replacement for this model and effort. Connect its provider or select an available model.</p>{/if}
<details class="agent-options"><summary>Definition sources ({agent.sources?.length??1})</summary>
{#each agent.sources??[{id:agent.id,provider:agent.provider,source:agent.source,model:agent.source_model??null,effort:agent.source_effort??null}] as source(source.id)}
<p class="hint">{source.provider} · {source.model??'Runtime default'} · {source.effort??'Runtime default'}</p>
<code class="source-path" dir="ltr">{source.source}</code>
{/each}
</details>
<div class="definition-status">{agent.available?'Definition loaded · execution unverified':label(agent.status)}</div>
{#each agent.warnings??[] as warning}
<p class="inline-error" dir="auto">{warning}</p>
{/each}
</article>
{:else}
<div class="empty-card">{search?'No agents match your search.':'No agent definitions found. Add agents to Codex or Claude Code, then refresh.'}</div>
{/each}
</div>

{:else if page==='connections'}

 <div class="connection-grid">
{#each connections?.providers??[] as provider}
<article class="connection-card">
<div class="connection-logo">{provider.provider==='codex'?'⌘':'✳'}</div>
<div>
<h2>{provider.provider==='codex'?'Codex':'Claude Code'}</h2>
<p>{provider.provider==='codex'?'Uses your existing ChatGPT sign-in':(settings.claude_auth_mode??'configured')==='native'?'Uses your official Claude sign-in':'Uses your existing Claude routing configuration'}</p>
</div>
<span class="connection-badge" class:good={provider.enabled&&provider.installed&&provider.compatible}>{label(provider.status)}</span>
<div class="connection-details">
<p>Studio access: {provider.enabled?'Enabled':'Disconnected'}. {provider.enabled&&!provider.compatible?'The runtime needs attention before tasks can run.':''}</p>
<p>{provider.detail}</p>
<button disabled={busy} onclick={()=>onConnection(provider.provider, !(settings.provider_connections?.[provider.provider]??false))}>{(settings.provider_connections?.[provider.provider]??false)?'Disconnect from Studio':'Connect to Studio'}</button>
<p class="hint">Controls execution in Studio only. This does not sign in or sign out of your account. Stop active work before disconnecting.</p>
{#if provider.version}
<code>{provider.version}</code>
{/if}
<p class="hint">Runtime compatibility does not verify account access or a successful model run.</p>
{#if provider.provider==='claude'}
<label for="claude-auth-mode">Claude connection route</label>
<select id="claude-auth-mode" bind:value={draft.claude_auth_mode} onchange={()=>saved=false} disabled={busy} aria-describedby="claude-auth-description">
  <option value="configured">Existing configuration</option>
  <option value="native">Official Claude account</option>
</select>
<p id="claude-auth-description" class="hint">
{#if draft.claude_auth_mode==='native'}
Use the official Claude OAuth sign-in for Studio runs. Studio ignores configured gateways, API-key routing and cloud-provider routing in this mode. Sign in through Claude Code’s official login before starting a task.
{:else}
Keep the existing Claude Code environment and routing configuration, including a configured gateway or API provider. Studio does not replace that route with official Claude login.
{/if}
</p>
<p class="hint">This preference changes Studio only. Installed Claude settings are not modified.</p>
{#if draft.claude_auth_mode!==(settings.claude_auth_mode??'configured')}<p class="hint" role="status">Unsaved change. Save changes to apply this route to future executions.</p>{/if}
{/if}
</div>
</article>
{:else}
<div class="empty-card">No connection information available. Refresh to inspect installed runtimes.</div>
{/each}
</div>
 <article class="settings-card">
<h2>Studio MCP bridge</h2>
<p>Let compatible apps access Studio’s coordination tools. Configure the connection in the external app yourself.</p>
<label class="check-row">
<input type="checkbox" bind:checked={draft.mcp_enabled}/> Enable external MCP access</label>
<pre class="code-block" dir="ltr">{JSON.stringify({mcpServers:{agentkitStudio:{command:connections?.mcp.command??'agentkit',args:connections?.mcp.args??['mcp-server']}}},null,2)}</pre>
<p class="hint">Use the full path to this Studio build’s CLI if it is not on PATH. Existing app configurations remain unchanged.</p>
</article>

{:else}

 <article class="settings-card">
<h2>Superagent</h2>
<p>The coordinator chooses agents and models for each request. It asks before making broad changes.</p>
<ModelSettings provider={draft.superagent.provider} providers={connectedProviders} {catalog} model={draft.superagent.model} effort={draft.superagent.effort} onChange={patch=>{if(draft){draft.superagent={provider:patch.provider??draft.superagent.provider,model:patch.model===undefined?draft.superagent.model:patch.model,effort:patch.effort===undefined?draft.superagent.effort:patch.effort};saved=false;}}}/>
</article>
 <article class="settings-card">
<h2>Parallel work</h2>
<p>Limit simultaneous provider executions across all conversations, including the superagent.</p>
<label>Maximum active executions<input type="number" min="1" max="16" step="1" bind:value={draft.concurrency}/>
</label>
<p class="hint">Changes to the same project are serialized to prevent overlapping writes.</p>
</article>

{/if}

<article class="settings-card">
<h2>Model catalogs</h2>
<p class="hint">Shared by agent and superagent settings. Installed definitions and advertised capabilities do not verify execution access.</p>
{#each connectedProviders as provider}
{@const metadata=catalogFor(provider)}
<div class="catalog-provider">
<h3>{provider==='codex'?'Codex':provider==='claude'?'Claude Code':provider}</h3>
<button onclick={()=>loadCatalog(provider)} disabled={!!catalogBusy||busy}>{catalogBusy===provider?'Reading catalog…':'Refresh model catalog'}</button>
{#if catalogError[provider]||metadata?.catalog_error}<p class="inline-error" role="alert">{catalogError[provider]||metadata?.catalog_error}</p>{/if}
{#if metadata?.stale}<p class="hint">Showing previously saved models; the latest refresh did not succeed.</p>{/if}
{#if metadata?.detail}<p class="hint">{metadata.detail}</p>{/if}
<div class="model-catalog">
{#each catalog.models.filter(m=>m.provider===provider) as model(model.model||model.id)}<div><strong>{model.displayName||model.id}</strong><code>{model.model||model.id}</code><span>{model.verification==='installed'?'Installed definition':'Advertised'} · {[...(model.supportedReasoningEfforts??[]).map(e=>e.reasoningEffort),...(model.installedEfforts??[])].join(' · ')||'Runtime default'}</span></div>{/each}
</div>
</div>
{:else}<p class="hint">No providers are connected to Studio. Connect a provider to load its models.</p>
{/each}
</article>

<div class="settings-save">
<span role="status">{saved?'Settings saved.': 'Changes apply to future executions.'}</span>
<button class="primary" onclick={save} disabled={busy}>{busy?'Saving…':'Save changes'}</button>
</div>

{/if}
</section>

<style>
  .agent-card.model-unavailable { border-color: var(--error); box-shadow: 0 0 0 1px var(--error); }
</style>
