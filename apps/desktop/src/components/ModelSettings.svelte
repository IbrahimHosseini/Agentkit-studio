<script lang="ts">
  import type { ModelSelection } from '../lib/protocol';
  import { connectedModels, modelEfforts, modelKey, selectionKey, selectionOptions, selectionLabel, switchModel, type ModelCatalog } from '../lib/model-options';
  let { provider, providers, catalog, model, effort, allowedSelections, onChange }: {
    provider: string; providers: string[]; catalog: ModelCatalog; model: string|null; effort: string|null;
    allowedSelections?: ModelSelection[];
    onChange: (change: {provider?:string;model?:string|null;effort?:string|null;allowed_selections?:ModelSelection[];allowed_models?:string[];allowed_efforts?:string[]}) => void;
  } = $props();
  const models = $derived(connectedModels(catalog, providers));
  const efforts = $derived(modelEfforts(catalog, provider, model));
  const currentKey = $derived(modelKey(provider, model));
  const currentAvailable = $derived(providers.includes(provider) && (model === null || models.some(m => modelKey(m.provider!, m.model || m.id) === currentKey)));
  const choices = $derived(selectionOptions(catalog, providers));
  const savedUnavailable = $derived((allowedSelections ?? []).filter(s => !choices.some(o => selectionKey(o) === selectionKey(s))));
  function selectModel(value: string) {
    const [nextProvider, nextModel] = JSON.parse(value) as [string, string|null];
    onChange({...switchModel(catalog, nextProvider, nextModel), allowed_models:[], allowed_efforts:[], ...(allowedSelections === undefined ? {} : {allowed_selections:allowedSelections.filter(s=>choices.some(o=>selectionKey(o)===selectionKey(s)))})});
  }
  function selectAllowed(event: Event) {
    const selected = Array.from((event.currentTarget as HTMLSelectElement).selectedOptions, o => o.value);
    onChange({allowed_selections: choices.filter(o => selected.includes(selectionKey(o))), allowed_models:[], allowed_efforts:[]});
  }
</script>
<div class="field-pair model-fields">
<label>Model<select value={currentKey} onchange={e=>selectModel(e.currentTarget.value)} disabled={!providers.length}>
{#if !currentAvailable}<option value={currentKey} disabled>{provider} · {model??'Runtime default'} · Unavailable</option>{/if}
{#each providers as runtime}<option value={modelKey(runtime,null)}>{runtime} · Runtime default</option>{/each}
{#each models as option(modelKey(option.provider!,option.model||option.id))}<option value={modelKey(option.provider!,option.model||option.id)}>{option.provider} · {option.displayName||option.model||option.id}</option>{/each}
</select></label>
<label>Effort<select value={effort??''} onchange={e=>onChange({effort:e.currentTarget.value||null,allowed_models:[],allowed_efforts:[]})} disabled={!currentAvailable}>
<option value="">Runtime default</option>
{#if effort && !efforts.includes(effort)}<option value={effort} disabled>{effort} · Unavailable</option>{/if}
{#each efforts as option}<option value={option}>{option}</option>{/each}
</select></label>
</div>
{#if !providers.length}<p class="hint">Connect a provider in Connections to choose a model.</p>{/if}
{#if !currentAvailable}<p class="hint">This saved model is unavailable. Connect its provider or choose a connected model.</p>{/if}
{#if allowedSelections !== undefined}
<details class="agent-options"><summary>Automatic selection limits</summary>
<p class="hint">The default above is always allowed. Choose additional model and effort combinations. Use Command or Control to select several.</p>
<label>Additional allowed configurations<select multiple size="5" value={allowedSelections.map(selectionKey)} onchange={selectAllowed} disabled={!providers.length}>
{#each savedUnavailable as option}<option value={selectionKey(option)} disabled>{selectionLabel(option)} · Unavailable</option>{/each}
{#each choices as option(selectionKey(option))}<option value={selectionKey(option)}>{selectionLabel(option)}</option>{/each}
</select></label>
{#if savedUnavailable.length}<p class="hint">Unavailable saved configurations cannot execute. Changing this list removes them.</p>{/if}
</details>
{/if}
