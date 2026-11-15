import type { Agent, AgentOverride, Settings, ModelSelection } from './protocol';
export interface CatalogModel {
    id: string;
    provider?: string;
    defaultReasoningEffort?: string | null;
    model?: string;
    displayName?: string;
    verification?: string;
    installedEfforts?: string[];
    supportedReasoningEfforts?: { reasoningEffort: string }[];
}
export interface Catalog { provider: string; models: CatalogModel[]; catalog_error?: string }
export interface Option { value: string; label: string }
export function modelOptions(provider: string, agents: Agent[], catalog: Catalog | undefined, saved: (string | null)[]): Option[] {
    const options = new Map<string, string>();
    for (const a of agents.filter(a => a.provider === provider)) {
        const installed = "source_model" in a ? a.source_model : a.model;
        if (installed) options.set(installed, `${installed} · Installed default`);
    }
    for (const value of saved) if (value && !options.has(value)) options.set(value, `${value} · Saved setting`);
    for (const m of catalog?.provider === provider ? catalog.models : []) {
        const id = m.model || m.id;
        options.set(id, `${m.displayName || id} · ${m.verification === 'installed' ? 'Installed' : 'Advertised'}`);
    }
    return [...options].map(([value, label]) => ({ value, label }));
}
export function effortOptions(provider: string, model: string | null, agents: Agent[], catalog?: Catalog): Option[] {
    const options = new Map<string, string>();
    for (const a of agents.filter(a => a.provider === provider && ("source_model" in a ? a.source_model : a.model) === model)) {
        const effort = "source_effort" in a ? a.source_effort : a.effort;
        if (effort) options.set(effort, `${effort} · Installed default`);
    }
    for (const m of catalog?.provider === provider ? catalog.models : []) {
        if ((m.model || m.id) !== model) continue;
        for (const effort of m.installedEfforts ?? []) options.set(effort, `${effort} · Installed`);
        for (const effort of m.supportedReasoningEfforts ?? []) options.set(effort.reasoningEffort, effort.reasoningEffort);
    }
    return [...options].map(([value, label]) => ({ value, label }));
}
export function validEffort(effort: string | null, options: Option[]): string | null {
    return options.some(o => o.value === effort) ? effort : null;
}
export function mergeSettings(previous: Settings | null, draft: Settings | null, incoming: Settings): Settings {
    const next = structuredClone(incoming);
    if (previous && draft) {
        for (const key of ['superagent', 'agent_overrides', 'concurrency', 'mcp_enabled', 'claude_auth_mode'] as const) {
            if (JSON.stringify(previous[key]) === JSON.stringify(incoming[key]))
                Object.assign(next, { [key]: structuredClone(draft[key]) });
        }
    }
    next.provider_connections = { ...incoming.provider_connections };
    next.claude_auth_mode ??= 'configured';
    return next;
}

// A chosen default must remain executable under a nonempty automatic-selection limit.
export function defaultSelection(
    model: string | null,
    effort: string | null,
    allowedModels?: string[],
    allowedEfforts?: string[],
) {
    return {
        model,
        effort,
        ...(allowedModels ? { allowed_models: allowedModels.length && model && !allowedModels.includes(model) ? [...allowedModels, model] : [...allowedModels] } : {}),
        ...(allowedEfforts ? { allowed_efforts: allowedEfforts.length && effort && !allowedEfforts.includes(effort) ? [...allowedEfforts, effort] : [...allowedEfforts] } : {}),
    };
}


export interface ModelCatalog {
    models: CatalogModel[];
    providers: {provider: string; updated_at?: string; catalog_error?: string; detail?: string; stale?: boolean; runtimeDefaultEfforts?: string[]}[];
}
export const modelKey = (provider: string, model: string | null) => JSON.stringify([provider, model]);
export const selectionKey = (selection: ModelSelection) => JSON.stringify([selection.provider, selection.model, selection.effort]);
export function connectedModels(catalog: ModelCatalog, providers: string[]) {
    const seen = new Set<string>();
    return catalog.models.filter(m => {
        const key = modelKey(m.provider ?? '', m.model || m.id);
        if (!m.provider || !providers.includes(m.provider) || seen.has(key)) return false;
        seen.add(key); return true;
    });
}
export function modelEfforts(catalog: ModelCatalog, provider: string, model: string | null): string[] {
    if (model === null) return [...new Set(catalog.providers.find(p => p.provider === provider)?.runtimeDefaultEfforts ?? [])];
    const m = catalog.models.find(m => m.provider === provider && (m.model || m.id) === model);
    return [...new Set(m?.verification === 'advertised'
        ? (m.supportedReasoningEfforts ?? []).map(e => e.reasoningEffort)
        : [...(m?.supportedReasoningEfforts ?? []).map(e => e.reasoningEffort), ...(m?.installedEfforts ?? [])])];
}
export function switchModel(catalog: ModelCatalog, provider: string, model: string | null): ModelSelection {
    const m = catalog.models.find(m => m.provider === provider && (m.model || m.id) === model);
    const efforts = modelEfforts(catalog, provider, model);
    const preferred = m?.defaultReasoningEffort;
    return {provider, model, effort: preferred && efforts.includes(preferred) ? preferred : null};
}
export function selectionOptions(catalog: ModelCatalog, providers: string[]): ModelSelection[] {
    return providers.flatMap(provider => [
        ...[null, ...modelEfforts(catalog, provider, null)].map(effort=>({provider,model:null,effort})),
        ...connectedModels(catalog, [provider]).flatMap(m => [null, ...modelEfforts(catalog, provider, m.model || m.id)]
            .map(effort => ({provider, model:m.model || m.id, effort}))),
    ]);
}
export const selectionLabel = (s: ModelSelection) => `${s.provider} · ${s.model ?? 'Runtime default'} · ${s.effort ?? 'Default effort'}`;
export type ChatSort = 'updated_at' | 'created_at' | 'title';
export type SortDirection = 'asc' | 'desc';
export function readChatSort(raw: string | null): {sort: ChatSort; direction: SortDirection} {
    try {
        const value = JSON.parse(raw ?? '{}');
        if (['updated_at','created_at','title'].includes(value.sort) && ['asc','desc'].includes(value.direction)) return value;
    } catch { /* Corrupt local preferences must not prevent startup. */ }
    return {sort:'updated_at',direction:'desc'};
}

export function agentOverrideFor(agent: Agent, overrides: Record<string, Partial<AgentOverride> | undefined>): AgentOverride {
    const saved = overrides[agent.id] ?? (agent.aliases ?? []).map(id=>overrides[id]).find(Boolean);
    const base = {enabled:agent.enabled??true,provider:agent.provider,model:agent.model,effort:agent.effort,
        allowed_models:agent.allowed_models??[],allowed_efforts:agent.allowed_efforts??[],
        ...(agent.allowed_selections === undefined ? {} : {allowed_selections:agent.allowed_selections}), ...saved};
    const provider = base.provider ?? agent.provider;
    const models = base.allowed_models ?? [], efforts = base.allowed_efforts ?? [];
    const selections = base.allowed_selections ?? (models.length ? models : [base.model])
        .flatMap(model=>(efforts.length ? efforts : [base.effort]).map(effort=>({provider,model,effort})));
    return {...base,provider,allowed_models:models,allowed_efforts:efforts,allowed_selections:selections};
}

// Saved chat choices remain visible when disconnected; only new chats get defaults.
export function chatSelectionAvailable(catalog: ModelCatalog, providers: string[], selection: ModelSelection | null): boolean {
    if (!selection || !providers.includes(selection.provider)) return false;
    if (selection.model !== null && !connectedModels(catalog, [selection.provider]).some(m=>(m.model||m.id)===selection.model)) return false;
    return selection.effort === null || modelEfforts(catalog, selection.provider, selection.model).includes(selection.effort);
}
