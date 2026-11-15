<script lang="ts">
  import type { Task, RunEvent } from "../lib/protocol";
  import { label, isActive } from "../lib/protocol";
  import RetryDetails from "./RetryDetails.svelte";
  let { tasks, events, busy, onStop, onInspect, onClose, hasEarlier, onEarlier, moreEvents, onMoreEvents }: {
      moreEvents:boolean;
      onMoreEvents:()=>void;
      hasEarlier: boolean;
      onEarlier: () => void;
      tasks: Task[];
      events: RunEvent[];
      busy: boolean;
      onStop: (task: Task) => void;
      onInspect: (task: Task) => void;
      onClose: () => void;
  } = $props();
</script>
<aside class="task-panel" aria-label="Execution details">
<div class="panel-title">
<h2>Activity</h2>
<button class="icon" onclick={onClose} aria-label="Close activity panel">×</button>
</div>
<p class="panel-description">Your team’s work, step by step.</p>

{#if hasEarlier}<button class="load-history" onclick={onEarlier} disabled={busy}>Load earlier tasks</button>{/if}
{#each tasks as task(task.id)}
<article class="task-card">
<div class="task-status">
<span class="dot" class:active={isActive(task.status)} class:online={task.status==='completed'} class:failed={task.status==='failed'}>
</span>{label(task.status)}
{#if isActive(task.status)}
<button class="text-button" disabled={busy} onclick={()=>onStop(task)}>Stop</button>
{/if}
</div>
<h3 dir="auto">{task.title}</h3>
<div class="task-agent">
<bdi>{task.agent_id}</bdi>
<span>{task.provider}</span>
</div>
<div class="task-model">
<bdi>{task.actual_model||task.model||'Runtime default'}</bdi> · {task.actual_effort||task.effort||'Default effort'}<small>{task.actual_model?'Reported by runtime':'Requested configuration'}</small>
</div>
<RetryDetails {task}/>
{#if task.reason}
<p dir="auto">{task.reason}</p>
{/if}

{#if task.dependencies?.length}
<small>After: {task.dependencies.join(', ')}</small>
{/if}

{#if task.error}
<p class="inline-error" dir="auto">{task.error}</p>
{/if}

{#if task.result}
<details>
<summary>Result</summary>
<pre dir="auto">{task.result}</pre>
</details>
{/if}

{#if task.output}
<details><summary>Changes · {task.output.files?.length??0}</summary>
{#each task.output.files??[] as file}<details><summary><bdi>{file.path}</bdi> · {file.status}</summary>{#if file.diff}<pre dir="ltr">{file.diff}</pre>{:else}<small>Text comparison unavailable.</small>{/if}</details>{/each}
{#if task.output.truncated}<small>The comparison was limited. Some files or contents are omitted.</small>{/if}
<small>{task.output.note}</small></details>
{/if}
{#if task.files?.length}
<details>
<summary>File scope · {task.files.length}</summary>
{#each task.files as file}
<code dir="ltr">{file}</code>
{/each}
<small>Planned scope; not a verified diff.</small>
</details>
{/if}

{#if task.run_id}
<button class="text-button inspect" onclick={()=>onInspect(task)}>Inspect events ↗</button>
{/if}
</article>
{:else}
<div class="panel-empty">
<span>◌</span>
<p>Tasks appear here when your team starts working.</p>
</div>
{/each}


{#if events.length}
<details open class="raw-events">
<summary>Provider events · {events.length} loaded</summary>
{#each events as event(event.seq)}
<div>
<strong>{label(event.kind)}</strong>
<pre dir={['output','command','raw'].includes(event.kind)?'ltr':'auto'}>{String(event.payload.error??event.payload.text??event.payload.reason??JSON.stringify(event.payload,null,2))}</pre>
</div>
{/each}
{#if moreEvents}<button class="load-history" disabled={busy} onclick={onMoreEvents}>Load more events</button>{/if}
</details>
{/if}
</aside>
