<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { readChatSort, chatSelectionAvailable, type ChatSort, type SortDirection, type ModelCatalog } from "./lib/model-options";
  import Sidebar from "./components/Sidebar.svelte";
  import Composer from "./components/Composer.svelte";
  import ConversationView from "./components/ConversationView.svelte";
  import TaskPanel from "./components/TaskPanel.svelte";
  import SettingsPage from "./components/SettingsPage.svelte";
  import { request } from "./lib/api";
  import { createAttachmentController } from "./lib/attachments";
  import type { Project, Conversation, Snapshot, Task, Settings, Agent, Connections, Page, Preview, FileInfo, RunEvent, Approval, Message, ModelSelection } from "./lib/protocol";
  import { isActive } from "./lib/protocol";
  let page = $state<Page>("chat"), connected = $state(false), connecting = $state(true), busy = $state(false), error = $state(""), dragging = $state(false);
  let projects = $state<Project[]>([]), conversations = $state<Conversation[]>([]), projectId = $state(""), conversationId = $state(""), snapshot = $state<Snapshot | null>(null);
  let prompt = $state(""), attachments = $state<Preview[]>([]), showActivity = $state(true), events = $state<RunEvent[]>([]);
  let agents = $state<Agent[]>([]), connections = $state<Connections | null>(null), settings = $state<Settings | null>(null);
  let catalog = $state<ModelCatalog>({models:[],providers:[]});
  let chatDefault = $state<ModelSelection | null>(null);
  let newChatSelection = $state<ModelSelection | null | undefined>(undefined);
  const chatProviders = $derived(Object.entries(settings?.provider_connections??{}).filter(([,enabled])=>enabled).map(([provider])=>provider));
  const chatSelection = $derived(conversationId ? snapshot?.conversation.chat_selection??null : newChatSelection === undefined ? chatDefault : newChatSelection);
  const chatReady = $derived(!!projectId || chatSelectionAvailable(catalog,chatProviders,chatSelection));
  async function changeChatSelection(selection: ModelSelection) {
      if(busy || active || attachmentsBusy || projectId) return;
      if(!conversationId) { newChatSelection=selection; rememberDraft(); return; }
      await act(async()=>{
          mergeSnapshot(await request<Snapshot>('studio/conversation/selection',{conversation_id:conversationId,selection}));
          await refreshLists();
      });
  }
  let sort = $state<ChatSort>('updated_at'), direction = $state<SortDirection>('desc');
  let deletion = $state<{kind:'project'|'conversation';id:string;name:string}|null>(null);
  let deleteDialog: HTMLDialogElement;
  $effect(() => {
      if (deletion && deleteDialog && !deleteDialog.open) deleteDialog.showModal();
      else if (!deletion && deleteDialog?.open) deleteDialog.close();
  });
  async function changeSort(next: ChatSort, order: SortDirection) {
      sort = next; direction = order;
      try { localStorage.setItem('studio.chat-sort',JSON.stringify({sort,direction})); } catch { /* Storage is optional. */ }
      await act(refreshLists);
  }
  function requestDelete(kind:'project'|'conversation', id:string, name:string) {
      if (busy || attachmentsBusy) return;
      error = ''; deletion = {kind,id,name};
  }
  async function confirmDelete() {
      const selected = deletion;
      if (!selected || busy) return;
      await act(async () => {
          await request(`studio/${selected.kind}/delete`, selected.kind==='project'?{project_id:selected.id}:{conversation_id:selected.id});
          if (selected.kind==='conversation') {
              const draft = drafts.get(selected.id);
              draft?.attachments.forEach(a=>{if(a.preview)URL.revokeObjectURL(a.preview);});
              drafts.delete(selected.id);
              if (conversationId===selected.id) {
                  clearDraft(); conversationId=''; projectId=''; snapshot=null;
                  olderMessages=[]; olderTasks=[]; events=[]; page='chat'; restoreDraft();
              }
          } else if (projectId===selected.id) {
              projectId='';
              if (conversationId) await refreshSnapshot();
          }
          deletion=null;
          await refreshLists(); await refreshSettings();
      });
  }
  async function refreshCatalog() {
      const g=generation;
      const result=await request<ModelCatalog>('studio/model-catalog',{workspace:project?.path??''});
      if(!disposed&&g===generation)catalog=result;
  }
  let eventRun = "", eventCursor = 0;
  let moreEvents = $state(false);
  let olderMessages: Message[] = [], olderTasks: Task[] = [];
  function mergeSnapshot(data: Snapshot) {
      const messageIds = new Set(data.messages.map(m => m.id));
      const taskIds = new Set(data.tasks.map(t => t.id));
      snapshot = { ...data, messages: [...olderMessages.filter(m => !messageIds.has(m.id)), ...data.messages], tasks: [...olderTasks.filter(t => !taskIds.has(t.id)), ...data.tasks] };
  }
  async function loadEarlierMessages() {
      await act(async () => {
          if (!snapshot?.messages.length)
              return;
          const result = await request<{
              messages: Message[];
              has_more: boolean;
          }>("studio/messages", { conversation_id: conversationId, before_id: snapshot.messages[0].id, limit: 50 });
          olderMessages = [...result.messages, ...olderMessages];
          snapshot = { ...snapshot, messages: [...result.messages, ...snapshot.messages] };
      });
  }
  async function loadEarlierTasks() {
      await act(async () => {
          if (!snapshot?.tasks.length)
              return;
          const result = await request<{
              tasks: Task[];
              has_more: boolean;
          }>("studio/tasks", { conversation_id: conversationId, before_id: snapshot.tasks[0].id, limit: 40 });
          olderTasks = [...result.tasks, ...olderTasks];
          snapshot = { ...snapshot, tasks: [...result.tasks, ...snapshot.tasks] };
      });
  }
  let disposed = false, polling = false, generation = 0;
  const drafts = new Map<string, {
      text: string;
      attachments: Preview[];
      selection?: ModelSelection | null;
  }>();
  const draftKey = () => conversationId || "new";
  function rememberDraft() {
      drafts.set(draftKey(), { text: prompt, attachments: [...attachments], selection:newChatSelection });
  }
  function restoreDraft() {
      const draft = drafts.get(draftKey());
      prompt = draft?.text ?? "";
      attachments = draft?.attachments ?? [];
      if(!conversationId) newChatSelection=draft?.selection;
  }
  let pendingAttachments = $state<Record<string, number>>({});
  const attachmentsBusy = $derived((pendingAttachments[draftKey()] ?? 0) > 0);
  const attachmentController = createAttachmentController({
      read: key => key === draftKey() ? attachments : drafts.get(key)?.attachments ?? [],
      write: (key, items) => {
          if (disposed)
              return;
          if (key === draftKey())
              attachments = items;
          else
              drafts.set(key, { ...drafts.get(key), text: drafts.get(key)?.text ?? "", attachments: items });
      },
      pending: (key, count) => {
          if (!disposed)
              pendingAttachments = { ...pendingAttachments, [key]: count };
      },
      disposed: () => disposed,
      createPreview: info => info.preview_bytes ? URL.createObjectURL(new Blob([new Uint8Array(info.preview_bytes)], { type: info.mime })) : undefined,
      revokePreview: url => URL.revokeObjectURL(url),
      removeStaged: path => invoke("remove_staged_attachment", { path }),
      usedByOtherDraft: (key, path) => (key !== draftKey() && attachments.some(item => item.path === path)) || [...drafts].some(([other, draft]) => other !== key && other !== draftKey() && draft.attachments.some(item => item.path === path)),
  });
  const project = $derived(projects.find(p => p.id === projectId));
  const active = $derived(snapshot ? isActive(snapshot.conversation.status) : false);
  const approvals = $derived((snapshot?.tasks ?? []).flatMap(task => (task.approvals ?? []).map(a => ({ ...a, run_id: task.run_id ?? "" }))).filter(a => a.run_id));
  function fail(e: unknown) {
      error = e instanceof Error ? e.message : String(e);
  }
  async function refreshLists() {
      const g = generation, pid = projectId;
      const [ps, cs] = await Promise.all([request<Project[]>("studio/projects"), request<Conversation[]>("studio/conversations", {sort,direction})]);
      if (disposed || g !== generation || pid !== projectId)
          return;
      projects = ps;
      conversations = cs;
  }
  async function refreshSnapshot() {
      const id = conversationId, g = generation;
      if (!id)
          return;
      const data = await request<Snapshot>("studio/conversation/get", { conversation_id: id });
      if (!disposed && g === generation && conversationId === id)
          mergeSnapshot(data);
  }
  async function refreshSettings() {
      const g = generation;
      const [s, a, c, models, defaultSelection] = await Promise.all([request<Settings>("studio/settings"), request<Agent[]>("studio/agents", { workspace: project?.path ?? "" }), request<Connections>("studio/connections"), request<ModelCatalog>("studio/model-catalog",{workspace:project?.path??""}), request<ModelSelection|null>("studio/chat-defaults")]);
      if (disposed || g !== generation)
          return;
      settings = s;
      agents = a;
      connections = c;
      catalog = models;
      chatDefault = defaultSelection;
  }
  async function connect() {
      connecting = true;
      error = "";
      try {
          await request("handshake");
          connected = true;
          await refreshLists();
          await refreshLists();
          await refreshSettings();
          await refreshSnapshot();
      }
      catch (e) {
          connected = false;
          fail(e);
      }
      finally {
          connecting = false;
      }
  }
  async function tick() {
      if (!connected || busy || polling || document.hidden || disposed)
          return;
      polling = true;
      try {
          await Promise.all([refreshLists(), refreshSnapshot()]);
      }
      catch (e) {
          connected = false;
          fail(e);
      }
      finally {
          polling = false;
      }
  }
  async function act(fn: () => Promise<void>) {
      if (busy)
          return;
      generation++;
      busy = true;
      error = "";
      try {
          await fn();
      }
      catch (e) {
          fail(e);
      }
      finally {
          busy = false;
      }
  }
  function clearDraft() {
      drafts.delete(draftKey());
      attachments.forEach(a => {
          if (a.preview)
              URL.revokeObjectURL(a.preview);
      });
      prompt = "";
      attachments = [];
  }
  function newConversation() {
      if (busy)
          return;
      rememberDraft();
      generation++;
      conversationId = "";
      projectId = "";
      snapshot = null;
      olderMessages = [];
      olderTasks = [];
      events = [];
      page = "chat";
      error = "";
      restoreDraft();
  }
  async function changeProject(id: string | null) {
      if (busy || active || attachmentsBusy) return;
      await act(async () => {
          if (conversationId) {
              mergeSnapshot(await request<Snapshot>("studio/conversation/project", {
                  conversation_id: conversationId, project_id: id,
              }));
          }
          projectId = id ?? "";
          await refreshLists();
          await refreshSettings();
      });
  }
  async function setConnection(provider: string, enabled: boolean) {
      await act(async () => {
          settings = await request<Settings>("studio/connection/set", { provider, enabled });
          await refreshSettings();
      });
  }
  async function chooseProject(p: Project) {
      if (busy)
          return;
      rememberDraft();
      generation++;
      projectId = p.id;
      conversationId = "";
      snapshot = null;
      olderMessages = [];
      olderTasks = [];
      events = [];
      page = "chat";
      restoreDraft();
      await act(async () => {
          await refreshLists();
          await refreshSettings();
      });
  }
  async function chooseConversation(c: Conversation) {
      if (busy)
          return;
      rememberDraft();
      generation++;
      conversationId = c.id;
      projectId = c.project_id ?? "";
      snapshot = null;
      olderMessages = [];
      olderTasks = [];
      events = [];
      page = "chat";
      restoreDraft();
      await act(refreshSnapshot);
  }
  async function addProject() {
      await act(async () => {
          const path = await open({ directory: true, multiple: false, title: "Open project folder" });
          if (typeof path === "string") {
              const p = await request<Project>("studio/project/create", { path });
              rememberDraft();
              projectId = p.id;
              conversationId = "";
              snapshot = null;
              olderMessages = [];
              olderTasks = [];
              events = [];
              page = "chat";
              restoreDraft();
              await refreshLists();
              await refreshSettings();
          }
      });
  }
  async function navigate(next: Page) {
      if (busy)
          return;
      page = next;
      if (next !== "chat" && connected)
          await act(refreshSettings);
  }
  async function send() {
      if (!connected || busy || attachmentController.isPending(draftKey()) || active || !chatReady || !prompt.trim())
          return;
      if (new TextEncoder().encode(prompt).byteLength > 65536) {
          error = "Messages must be at most 65,536 UTF-8 bytes.";
          return;
      }
      const selection = !projectId ? chatSelection : null;
      await act(async () => {
          if (!conversationId) {
              const c = await request<Conversation>("studio/conversation/create", { project_id: projectId || null, ...(!projectId && selection ? {selection} : {}) });
              drafts.delete(draftKey());
              conversationId = c.id;
              generation++;
          }
          const data = await request<Snapshot>("studio/message/send", { conversation_id: conversationId, text: prompt, ...(!projectId && selection ? {selection} : {}), attachments: attachments.map(({ id, kind, path, mime }) => ({ id, kind, path, mime })) });
          mergeSnapshot(data);
          clearDraft();
          await refreshLists();
      });
  }
  async function decidePlan(decision: boolean) {
      await act(async () => {
          mergeSnapshot(await request<Snapshot>("studio/plan/approve", { conversation_id: conversationId, decision }));
          await refreshLists();
      });
  }
  async function stopConversation() {
      await act(async () => {
          mergeSnapshot(await request<Snapshot>("studio/conversation/stop", { conversation_id: conversationId }));
      });
  }
  async function stopTask(task: Task) {
      await act(async () => {
          await request("studio/task/stop", { task_id: task.id });
          await refreshSnapshot();
      });
  }
  async function approve(a: Approval, decision: boolean) {
      await act(async () => {
          await request("approve", { run_id: a.run_id, request_id: a.request_id, decision });
          await refreshSnapshot();
      });
  }
  async function readEvents() {
      if (!eventRun)
          return;
      const result = await request<{
          events: RunEvent[];
          next_seq: number;
      }>("subscribe", { run_id: eventRun, after_seq: eventCursor, limit: 100 });
      events = [...events, ...result.events].slice(-500);
      eventCursor = result.next_seq;
      moreEvents = result.events.length > 0;
  }
  async function inspect(task: Task) {
      await act(async () => {
          if (!task.run_id)
              return;
          eventRun = task.run_id;
          eventCursor = 0;
          events = [];
          moreEvents = false;
          await readEvents();
      });
  }
  async function saveSettings(s: Settings) {
      if (busy)
          return;
      busy = true;
      error = "";
      try {
          settings = await request<Settings>("studio/settings", { settings: s, workspace: project?.path??"" });
          agents = await request<Agent[]>("studio/agents", {workspace:project?.path??""});
          await refreshCatalog();
      }
      catch (e) {
          fail(e);
          throw e;
      }
      finally {
          busy = false;
      }
  }
  async function readPaths(key: string, paths: string[]) {
      const unique = [...new Set(paths)];
      for (let offset = 0; offset < unique.length; offset += 12) {
          if (disposed)
              return;
          const infos = await invoke<FileInfo[]>("attachment_info", { paths: unique.slice(offset, offset + 12) });
          for (const info of infos)
              await attachmentController.add(key, info);
      }
  }
  async function addPaths(paths: string[]) {
      if (busy || page !== "chat" || disposed)
          return;
      const key = draftKey();
      try {
          await attachmentController.run(key, () => readPaths(key, paths));
      }
      catch (e) {
          if (!disposed)
              fail(e);
      }
  }
  async function pickFiles() {
      if (busy || disposed)
          return;
      const key = draftKey();
      try {
          await attachmentController.run(key, async () => {
              const paths = await open({ directory: false, multiple: true, title: "Attach files or images" });
              if (paths && !disposed)
                  await readPaths(key, Array.isArray(paths) ? paths : [paths]);
          });
      }
      catch (e) {
          if (!disposed)
              fail(e);
      }
  }
  async function removeAttachment(id: string) {
      if (busy || disposed)
          return;
      const key = draftKey();
      try {
          await attachmentController.remove(key, id);
      }
      catch (e) {
          if (!disposed)
              fail(e);
      }
  }
  async function pasteImage(event: ClipboardEvent) {
      const files = [...(event.clipboardData?.files ?? [])];
      if (!files.length)
          return;
      event.preventDefault();
      if (busy || disposed)
          return;
      const key = draftKey();
      try {
          if (!isTauri())
              throw new Error("Paste images in the desktop app.");
          if (files.length > 12)
              throw new Error("Attach up to 12 files.");
          for (const file of files) {
              if (!["image/png", "image/jpeg", "image/gif", "image/webp"].includes(file.type) || file.size > 16 * 1024 * 1024)
                  throw new Error("Paste PNG, JPEG, GIF or WebP images up to 16 MiB each.");
          }
          await attachmentController.run(key, async () => {
              for (const file of files) {
                  if (disposed)
                      return;
                  const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
                  if (disposed)
                      return;
                  const info = await invoke<FileInfo>("stage_clipboard_image", { bytes, mime: file.type });
                  await attachmentController.add(key, info, true);
              }
          });
      }
      catch (e) {
          if (!disposed)
              fail(e);
      }
  }
  onMount(() => {
      try { const saved=readChatSort(localStorage.getItem("studio.chat-sort")); sort=saved.sort; direction=saved.direction; } catch { /* Defaults work without storage. */ }
      void connect();
      const timer = window.setInterval(() => void tick(), 700);
      let unlisten: (() => void) | undefined;
      const key = (e: KeyboardEvent) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "n") {
              e.preventDefault();
              newConversation();
          }
      };
      window.addEventListener("keydown", key);
      if (isTauri())
          getCurrentWebview().onDragDropEvent(({ payload }) => {
              dragging = payload.type === "enter" || payload.type === "over";
              if (payload.type === "drop") {
                  dragging = false;
                  void addPaths(payload.paths);
              }
          }).then(fn => {
              if (disposed)
                  fn();
              else
                  unlisten = fn;
          }).catch(fail);
      return () => {
          disposed = true;
          generation++;
          clearInterval(timer);
          window.removeEventListener("keydown", key);
          unlisten?.();
          const urls = new Set([...attachments, ...[...drafts.values()].flatMap(d => d.attachments)].map(a => a.preview).filter((u): u is string => !!u));
          urls.forEach(u => URL.revokeObjectURL(u));
      };
  });
</script>
<svelte:head>
<title>Agentkit Studio</title>
</svelte:head>
<div class="app-shell">
<Sidebar busy={busy||attachmentsBusy} {sort} {direction} onSort={changeSort} onDeleteProject={p=>requestDelete("project",p.id,p.name)} onDeleteConversation={c=>requestDelete("conversation",c.id,c.title)} {projects} {conversations} {projectId} {conversationId} {page} {connected} onProject={chooseProject} onConversation={chooseConversation} onNew={newConversation} onAdd={addProject} onPage={navigate}/>
<main>
<header class="topbar">
<div class="breadcrumb">
<span aria-hidden="true"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" style="display:block;flex-shrink:0"><path d="M3 17V6a2 2 0 0 1 2-2h4l2 3h7a2 2 0 0 1 2 2v2"/><path d="M3 18l2.6-6a1.7 1.7 0 0 1 1.6-1h13a1.5 1.5 0 0 1 1.4 2l-2.2 5.7A2 2 0 0 1 17.5 20H4.4A1.5 1.5 0 0 1 3 18Z"/></svg></span>
<bdi>{project?.name??'No project'}</bdi>
<span class="breadcrumb-separator">/</span>
<span dir="auto">{page==='chat'?(snapshot?.conversation.title??'New conversation'):page==='agents'?'Agents':page==='connections'?'Connections':'Settings'}</span>
</div>
{#if page==='chat'}
<button class="subtle-button" onclick={()=>showActivity=!showActivity} aria-expanded={showActivity}>◫ <span>Activity</span>
</button>
{/if}
</header>

{#if error}
<div class="error-banner" role="alert">
<span dir="auto">{error}</span>
{#if !connected}
<button onclick={connect} disabled={connecting}>{connecting?'Connecting…':'Reconnect'}</button>
{/if}
<button class="icon" onclick={()=>error=''} aria-label="Dismiss error">×</button>
</div>
{/if}


{#if page==='chat'}
<div class="chat-layout" class:with-panel={showActivity&&!!snapshot}>
<section class="chat-column" aria-label="Chat">
{#if snapshot}
<ConversationView hasEarlier={(snapshot.total_messages??snapshot.messages.length)>snapshot.messages.length} onEarlier={loadEarlierMessages} tasks={snapshot.tasks} messages={snapshot.messages} plan={snapshot.plan} {approvals} {busy} {active} onPlan={decidePlan} onApproval={approve}/>
{:else}
<div class="welcome">
<div class="welcome-mark">a<span>·</span>
</div>
<p class="eyebrow">{projectId?'YOUR WORKSPACE, WORKING TOGETHER':'YOUR MODELS, ONE CONVERSATION'}</p>
<h1>{projectId?'What should we build?':'How can I help?'}</h1>
<p>{projectId?'One conversation. The right team for every task.':'Choose a model and start a conversation.'}</p>
{#if !project}
<button class="primary" onclick={addProject} disabled={!connected}>＋ Open a project</button>
{/if}
<div class="suggestions">
{#each (projectId?['Explore this project','Find and explain a bug','Plan a new feature']:['Explain a concept','Help me write something','Brainstorm ideas']) as suggestion}
<button onclick={()=>prompt=suggestion}>
<span>{suggestion}</span>
<span aria-hidden="true">↗</span>
</button>
{/each}
</div>
</div>
{/if}
<Composer selection={chatSelection} providers={chatProviders} {catalog} selectionReady={chatReady} onSelection={changeChatSelection} bind:text={prompt} {attachments} disabled={!connected} busy={busy||attachmentsBusy} {active} {projects} {projectId} onProjectChange={changeProject} onSend={send} onAttach={pickFiles} onRemove={removeAttachment} onPaste={pasteImage} onStop={stopConversation}/>
</section>
{#if showActivity&&snapshot}
<TaskPanel {moreEvents} onMoreEvents={()=>act(readEvents)} hasEarlier={(snapshot.total_tasks??snapshot.tasks.length)>snapshot.tasks.length} onEarlier={loadEarlierTasks} tasks={snapshot.tasks} {events} {busy} onStop={stopTask} onInspect={inspect} onClose={()=>showActivity=false}/>
{/if}
</div>
{:else}
<SettingsPage {catalog} onCatalogRefresh={refreshCatalog} {page} {agents} {connections} {settings} {busy} workspace={project?.path??''} onConnection={setConnection} onSave={saveSettings} onRefresh={()=>act(refreshSettings)}/>
{/if}
</main>
{#if dragging}
<div class="drop-overlay">
<strong>Drop files into your conversation</strong>
<p>Up to 12 files · 16 MiB each · 40 MiB total</p>
</div>
{/if}
</div>

<dialog class="delete-dialog" bind:this={deleteDialog} oncancel={e=>{if(busy)e.preventDefault();else deletion=null;}}>
{#if deletion}
<h2>{deletion.kind==='project'?'Remove project from Studio?':'Delete conversation?'}</h2>
<p dir="auto">{deletion.name}</p>
<p class="hint">{deletion.kind==='project'?'Its conversations will stay as chats without a project. Files in the project folder will not be deleted.':'This removes the conversation and its history from Studio. This cannot be undone.'}</p>
{#if error}<p class="inline-error" role="alert" dir="auto">{error}</p>{/if}
<div class="dialog-actions"><button onclick={()=>deletion=null} disabled={busy}>Cancel</button><button class="danger" onclick={confirmDelete} disabled={busy}>{busy?'Deleting…':deletion.kind==='project'?'Remove project':'Delete conversation'}</button></div>
{/if}
</dialog>
