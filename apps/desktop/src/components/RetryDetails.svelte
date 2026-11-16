<script lang="ts">
  import type { Task } from "../lib/protocol";
  import { label } from "../lib/protocol";
  import TaskNotices from './TaskNotices.svelte';
  let { task }: {
      task: Task;
  } = $props();
  function when(value: number | string) {
      const date = new Date(typeof value === "string" && /^\d+$/.test(value) ? Number(value) : value);
      return Number.isNaN(date.valueOf()) ? "the scheduled time" : date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }
</script>
<TaskNotices {task}/>
{#if task.retry_at&&task.status==='queued'}<p class="retry-scheduled" role="status">Retry scheduled for {when(task.retry_at)}. You can stop this task while it waits.</p>{/if}
{#if task.retry_exhausted}<p class="inline-error">Automatic retries are exhausted. Review the failure before starting a new request.</p>{/if}
{#if task.attempts?.length}
<details class="attempt-history"><summary>{task.attempts.length===1?'1 execution attempt':`${task.attempts.length} execution attempts`}</summary>
{#each task.attempts as attempt(attempt.run_id)}<div class="attempt"><div><strong>Attempt {attempt.number}</strong><span>{label(attempt.status)}</span></div><p><bdi>{attempt.model||'Runtime default'}</bdi> · {attempt.effort||'Default effort'}</p><small>Started {when(attempt.started_at)}{#if attempt.finished_at} · Ended {when(attempt.finished_at)}{/if}</small>{#if attempt.error}<p class="inline-error" dir="auto">{attempt.error}</p>{/if}{#if attempt.retry_evidence}<small>{attempt.retry_evidence.safe_to_retry?'Runtime confirmed retry was safe before execution.':'Automatic repetition was not authorized by the runtime.'}</small>{/if}</div>{/each}
</details>
{/if}
