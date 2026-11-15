<script lang="ts">
  import type { Project, Conversation, Page } from "../lib/protocol";
  import type { ChatSort, SortDirection } from "../lib/model-options";
  import { isActive } from "../lib/protocol";
  let { projects, conversations, projectId, conversationId, page, connected, onProject, onConversation, onNew, onAdd, onPage, busy, sort, direction, onSort, onDeleteProject, onDeleteConversation }: {
      busy: boolean;
      sort: ChatSort;
      direction: SortDirection;
      onSort: (sort: ChatSort, direction: SortDirection) => void;
      onDeleteProject: (project: Project) => void;
      onDeleteConversation: (conversation: Conversation) => void;
      projects: Project[];
      conversations: Conversation[];
      projectId: string;
      conversationId: string;
      page: Page;
      connected: boolean;
      onProject: (p: Project) => void;
      onConversation: (c: Conversation) => void;
      onNew: () => void;
      onAdd: () => void;
      onPage: (p: Page) => void;
  } = $props();
  let sortOpen = $state(false);
  let sortRoot: HTMLDivElement;
  function closeSort(event: MouseEvent) {
      if (sortOpen && !sortRoot?.contains(event.target as Node)) sortOpen = false;
  }
  function escapeSort(event: KeyboardEvent) {
      if (event.key === 'Escape' && sortOpen) {
          sortOpen = false;
          sortRoot?.querySelector('button')?.focus();
      }
  }
</script>
<svelte:window onclick={closeSort} onkeydown={escapeSort}/>
<aside class="sidebar" aria-label="Workspace navigation">
  <div class="brand">
<span class="brand-symbol">a<span>·</span>
</span>
<strong>agentkit <span>studio</span>
</strong>
</div>
  <button class="nav-button new-chat" onclick={onNew}>
<span aria-hidden="true">＋</span> New conversation <kbd>⌘ N</kbd>
</button>
  <div class="sidebar-section">
<span>Projects</span>
<button class="icon" onclick={onAdd} disabled={!connected} aria-label="Add project">＋</button>
</div>
  <div class="project-list">
{#each projects as project (project.id)}
<div class="sidebar-item">
<button class="project-row" class:selected={projectId===project.id} onclick={()=>onProject(project)} title={project.path}>
<span aria-hidden="true"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" style="display:block;flex-shrink:0"><path d="M3 17V6a2 2 0 0 1 2-2h4l2 3h7a2 2 0 0 1 2 2v2"/><path d="M3 18l2.6-6a1.7 1.7 0 0 1 1.6-1h13a1.5 1.5 0 0 1 1.4 2l-2.2 5.7A2 2 0 0 1 17.5 20H4.4A1.5 1.5 0 0 1 3 18Z"/></svg></span>
<bdi>{project.name}</bdi>
</button>
<button class="delete-item icon" aria-label={`Delete project ${project.name}`} title="Remove project from Studio" disabled={busy||!connected||conversations.some(c=>c.project_id===project.id&&isActive(c.status))} onclick={()=>onDeleteProject(project)}>×</button>
</div>
{:else}
<p class="sidebar-hint">Projects are optional. Add a folder for workspace tasks.</p>
{/each}
</div>
  <div class="sidebar-section conversations-heading">
<span>All conversations</span>
<span class="conversation-count">{conversations.length}</span>
<div class="sort-options" bind:this={sortRoot}>
<button class="icon sort-trigger" disabled={busy} aria-label="Sort conversations" title="Sort conversations" aria-expanded={sortOpen} aria-controls="chat-sort-options" onclick={()=>sortOpen=!sortOpen}>
<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><path d="M4 6h16M7 12h10M10 18h4"/></svg>
</button>
{#if sortOpen}
<div class="compact-popover sort-panel" id="chat-sort-options" role="group" aria-label="Conversation sorting">
<label for="chat-sort">Sort by</label>
<select id="chat-sort" value={sort} disabled={busy} onchange={e=>onSort(e.currentTarget.value as ChatSort,direction)}>
<option value="updated_at">Last activity</option><option value="created_at">Date created</option><option value="title">Title</option>
</select>
<label for="chat-sort-direction">Order</label>
<select id="chat-sort-direction" value={direction} disabled={busy} onchange={e=>onSort(sort,e.currentTarget.value as SortDirection)}>
<option value="asc">Ascending</option><option value="desc">Descending</option>
</select>
</div>
{/if}
</div>
</div>
  <nav class="conversation-list" aria-label="Conversations">
{#each conversations as conversation (conversation.id)}
<div class="sidebar-item">
<button class="conversation-row" class:selected={page==='chat'&&conversationId===conversation.id} onclick={()=>onConversation(conversation)} aria-current={conversationId===conversation.id?'page':undefined}>
<span dir="auto">{conversation.title}</span>
{#if isActive(conversation.status)}
<span class="dot active" aria-label={conversation.status}>
</span>
{/if}
</button>
<button class="delete-item icon" aria-label={`Delete conversation ${conversation.title}`} title="Delete conversation" disabled={busy||!connected||isActive(conversation.status)} onclick={()=>onDeleteConversation(conversation)}>×</button>
</div>
{:else}
<p class="sidebar-hint">Your conversations will appear here.</p>
{/each}
</nav>
  <div class="sidebar-bottom">
<button class="nav-button" class:selected={page==='agents'} onclick={()=>onPage('agents')}>
<span aria-hidden="true">♧</span> Agents</button>
<button class="nav-button" class:selected={page==='connections'} onclick={()=>onPage('connections')}>
<span aria-hidden="true">⌁</span> Connections</button>
<button class="nav-button" class:selected={page==='settings'} onclick={()=>onPage('settings')}>
<span aria-hidden="true">⚙</span> Settings</button>
<div class="engine-state">
<span class="dot" class:online={connected}>
</span>{connected?'Local engine connected':'Engine disconnected'}</div>
</div>
</aside>
