export interface Attachment {
    id: string;
    kind: string;
    path: string;
    mime: string;
}
export interface Preview extends Attachment {
    name: string;
    size: number;
    preview?: string;
    staged?: boolean;
}
export interface FileInfo {
    path: string;
    name: string;
    size: number;
    kind: string;
    mime: string;
    preview_bytes: number[] | null;
}
export interface Project {
    id: string;
    path: string;
    name: string;
    created_at: string;
}
export interface Conversation {
    chat_selection?: ModelSelection | null;
    id: string;
    project_id: string | null;
    title: string;
    status: string;
    created_at: string;
    updated_at: string;
}
export interface Message {
    provider?: string;
    model?: string | null;
    effort?: string | null;
    task_id?: string;
    id: string;
    conversation_id: string;
    role: string;
    text: string;
    attachments: Attachment[];
    created_at: string;
}
export interface TaskAttempt {
    number: number;
    run_id: string;
    model: string | null;
    effort: string | null;
    status: string;
    started_at: string;
    finished_at?: string;
    error: string | null;
    retry_evidence?: {safe_to_retry:boolean;category:'transient'|'model_unavailable';phase:string;code:string;evidence:string} | null;
}
export interface Task {
    attempts?: TaskAttempt[];
    retry_at?: number | null;
    retry_exhausted?: boolean;
    retry_notice?: string;

    kind?: string;
    actual_model?: string;
    actual_effort?: string;
    output?: {
        available: boolean;
        truncated?: boolean;
        files: {path:string;status:string;diff:string|null}[];
        note: string;
    };
    approvals?: {
        request_id: string;
        reason: string;
        command?: string;
    }[];
    id: string;
    conversation_id: string;
    run_id: string | null;
    agent_id: string;
    provider: string;
    model: string | null;
    effort: string | null;
    title: string;
    prompt: string;
    status: string;
    read_only: boolean;
    dependencies: string[];
    files: string[];
    reason: string;
    result: string;
    error: string | null;
    created_at: string;
}
export interface Plan {
    summary: string;
    tasks: Task[];
    status: string;
}
export interface Snapshot {
    total_messages?: number;
    total_tasks?: number;
    truncated?: boolean;
    conversation: Conversation;
    messages: Message[];
    tasks: Task[];
    plan: Plan | null;
}
export interface ModelSelection {
    provider: string;
    model: string | null;
    effort: string | null;
}
export interface Agent {
    enabled?: boolean;
    aliases?: string[];
    sources?: {id:string;provider:string;source:string;model:string|null;effort:string|null}[];
    allowed_selections?: ModelSelection[];
    allowed_models?: string[];
    allowed_efforts?: string[];
    source_model?: string | null;
    source_effort?: string | null;
    id: string;
    name: string;
    provider: string;
    description: string;
    source: string;
    scope: string;
    model: string | null;
    effort: string | null;
    available: boolean;
    status: string;
    warnings: string[];
}
export interface AgentOverride {
    provider?: string;
    allowed_selections?: ModelSelection[];
    enabled: boolean;
    model: string | null;
    effort: string | null;
    allowed_models: string[];
    allowed_efforts: string[];
}
export interface Settings {
    provider_connections: Record<string, boolean>;
    claude_auth_mode: 'configured' | 'native';
    concurrency: number;
    superagent: {
        provider: string;
        model: string | null;
        effort: string | null;
    };
    agent_overrides: Record<string, AgentOverride>;
    mcp_enabled: boolean;
}
export interface ProviderConnection {
    enabled: boolean;
    provider: string;
    installed: boolean;
    version: string | null;
    compatible: boolean;
    status: string;
    detail: string;
    capabilities: {
        model: boolean;
        effort: boolean;
        read_only: boolean;
    };
}
export interface Connections {
    providers: ProviderConnection[];
    mcp: {
        transport: string;
        command: string;
        args: string[];
        automatic_config: boolean;
    };
}
export interface RunEvent {
    seq: number;
    run_id: string;
    timestamp: string;
    agent_id: string | null;
    parent_agent_id: string | null;
    kind: string;
    payload: Record<string, unknown>;
}
export interface Approval {
    run_id: string;
    request_id: string;
    reason: string;
    command?: string;
}
export type Page = 'chat' | 'agents' | 'connections' | 'settings';
export const isActive = (status: string) => ['queued', 'planning', 'running', 'awaiting_approval', 'stopping', 'pending', 'summarizing'].includes(status);
export const label = (value: string) => value.replaceAll('_', ' ').replace(/^./, c => c.toUpperCase());
