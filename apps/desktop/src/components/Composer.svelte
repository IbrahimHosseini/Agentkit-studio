<script lang="ts">
  import type { Preview, Project, ModelSelection } from "../lib/protocol";
  import { connectedModels, modelEfforts, modelKey, switchModel, type ModelCatalog } from '../lib/model-options';
  let { selection, providers, catalog, selectionReady, onSelection, text = $bindable(""), attachments, disabled, busy, active, projects, projectId, onProjectChange, onSend, onAttach, onRemove, onPaste, onStop }: {
      selection: ModelSelection | null;
      providers: string[];
      catalog: ModelCatalog;
      selectionReady: boolean;
      onSelection: (selection: ModelSelection) => void;
      text?: string;
      attachments: Preview[];
      disabled: boolean;
      busy: boolean;
      active: boolean;
      projects: Project[];
      projectId: string;
      onProjectChange: (id: string | null) => void;
      onSend: () => void;
      onAttach: () => void;
      onRemove: (id: string) => void;
      onPaste: (e: ClipboardEvent) => void;
      onStop: () => void;
  } = $props();
  const models = $derived(connectedModels(catalog,providers));
  const efforts = $derived(selection ? modelEfforts(catalog,selection.provider,selection.model) : []);
  const currentModel = $derived(selection ? modelKey(selection.provider,selection.model) : '');
  const modelAvailable = $derived(!!selection && providers.includes(selection.provider) && (selection.model === null || models.some(m=>modelKey(m.provider!,m.model||m.id)===currentModel)));
  const selectedModelLabel = $derived(!selection ? 'Choose a model' : `${selection.provider} · ${models.find(m=>modelKey(m.provider!,m.model||m.id)===currentModel)?.displayName || selection.model || 'Runtime default'}${modelAvailable ? '' : ' · Unavailable'}`);
  const selectedEffortLabel = $derived(selection?.effort || 'Default effort');
  function chooseModel(value: string) {
      const [provider,model] = JSON.parse(value) as [string,string|null];
      onSelection(switchModel(catalog,provider,model));
  }
  let optionsOpen = $state(false);
  let optionsRoot: HTMLDivElement;
  function closeOptions(event: MouseEvent) {
      if (optionsOpen && !optionsRoot?.contains(event.target as Node)) optionsOpen = false;
  }
  function escapeOptions(event: KeyboardEvent) {
      if (event.key === 'Escape' && optionsOpen) {
          optionsOpen = false;
          optionsRoot?.querySelector('button')?.focus();
      }
  }
  let bytes = $derived(new TextEncoder().encode(text).byteLength);
  function keydown(e: KeyboardEvent) {
      if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
          e.preventDefault();
          if (!disabled && selectionReady && !busy && !active && text.trim() && bytes <= 65536)
              onSend();
      }
  }
</script>
<svelte:window onclick={closeOptions} onkeydown={escapeOptions}/>
<div class="composer-wrap">
<form class="composer" onsubmit={(e)=>{e.preventDefault();onSend();}}>

{#if attachments.length}
<div class="attachments">
{#each attachments as item(item.id)}
<div class="attachment">
{#if item.preview}
<img src={item.preview} alt="Attachment preview"/>
{:else}
<span aria-hidden="true">▧</span>
{/if}
<bdi>{item.name}</bdi>
<button type="button" class="icon" onclick={()=>onRemove(item.id)} disabled={busy} aria-label={`Remove ${item.name}`}>×</button>
</div>
{/each}
</div>
{/if}

  <label class="sr-only" for="message">{projectId?'Message your team':'Message'}</label>
<textarea id="message" bind:value={text} dir="auto" placeholder={projectId?"Ask anything, or describe what you want to build…":"Message your model…"} rows="3" onkeydown={keydown} onpaste={onPaste} disabled={busy}>
</textarea>
  <div class="composer-actions">
<div class="composer-options" bind:this={optionsRoot}>
<button type="button" class="icon attach" onclick={()=>optionsOpen=!optionsOpen} disabled={disabled||busy} aria-label="Add attachments or choose project" aria-expanded={optionsOpen} aria-controls="composer-options">＋</button>
{#if optionsOpen}
<div class="compact-popover composer-options-panel" id="composer-options" role="group" aria-label="Message options">
<button type="button" class="popover-action" onclick={()=>{optionsOpen=false;onAttach();}} disabled={disabled||busy||attachments.length>=12}>Attach files or images</button>
<label class="composer-project">
<span>Conversation project</span>
<select aria-label="Conversation project" value={projectId} disabled={disabled||busy||active} onchange={e=>{onProjectChange(e.currentTarget.value||null);optionsOpen=false;}}>
<option value="">No project</option>
{#each projects as project}<option value={project.id}>{project.name}</option>{/each}
</select>
</label>
</div>
{/if}
</div>
{#if projectId}
<span class="composer-mode">Superagent</span>
{:else}
<div class="chat-model-controls">
<label class="chat-choice"><span class="sr-only">Chat model</span><span class="chat-choice-size" aria-hidden="true">{selectedModelLabel}</span><select title={selectedModelLabel} class="chat-model-select" aria-label="Chat model" value={currentModel} disabled={disabled||busy||active||!providers.length} onchange={e=>chooseModel(e.currentTarget.value)}>
{#if !selection}<option value="" disabled>Choose a model</option>{:else if !modelAvailable}<option value={currentModel} disabled>{selection.provider} · {selection.model??'Runtime default'} · Unavailable</option>{/if}
{#each providers as provider}<option value={modelKey(provider,null)}>{provider} · Runtime default</option>{/each}
{#each models as model(modelKey(model.provider!,model.model||model.id))}<option value={modelKey(model.provider!,model.model||model.id)}>{model.provider} · {model.displayName||model.model||model.id}</option>{/each}
</select></label>
<label class="chat-choice"><span class="sr-only">Chat effort</span><span class="chat-choice-size" aria-hidden="true">{selectedEffortLabel}</span><select title={selectedEffortLabel} class="chat-effort-select" aria-label="Chat effort" value={selection?.effort??''} disabled={disabled||busy||active||!modelAvailable} onchange={e=>{if(selection)onSelection({...selection,effort:e.currentTarget.value||null});}}>
<option value="">Default effort</option>
{#if selection?.effort && !efforts.includes(selection.effort)}<option value={selection.effort} disabled>{selection.effort} · Unavailable</option>{/if}
{#each efforts as effort}<option value={effort}>{effort}</option>{/each}
</select></label>
</div>
{/if}
{#if active}
<button type="button" class="send-button stop" onclick={onStop} disabled={busy} aria-label="Stop request">■</button>
{:else}
<button class="send-button" type="submit" disabled={disabled||busy||!selectionReady||!text.trim()||bytes>65536} aria-label="Send message">{busy?'…':'↑'}</button>
{/if}
</div>

{#if bytes>65536}
<p class="inline-error" role="alert">Message is too long: {bytes.toLocaleString()} / 65,536 UTF-8 bytes.</p>
{/if}

</form>
<div class="composer-footnote">
<span>{projectId?'Superagent chooses the right agents and models.':!providers.length?'Connect a provider in Connections to start chatting.':!selectionReady?'Choose an available model and effort, or reconnect its provider.':'Messages go directly to your selected model.'}</span>
<span>Shift + Enter for a new line</span>
</div>
</div>
