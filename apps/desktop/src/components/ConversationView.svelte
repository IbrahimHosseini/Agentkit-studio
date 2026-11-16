<script lang="ts">
  import type { Message, Plan, Approval, Task } from "../lib/protocol";
  import MessageBody from "./MessageBody.svelte";
  import TaskNotices from './TaskNotices.svelte';
  import { isActive } from '../lib/protocol';
  import { tick } from "svelte";
  let container: HTMLDivElement;
  let follow = $state(true);
  $effect(() => {
      messages;
      tasks;
      plan;
      approvals;
      void tick().then(() => {
          if (follow && container)
              container.scrollTop = container.scrollHeight;
      });
  });
  let { messages, plan, approvals, tasks, busy, active, onPlan, onApproval, hasEarlier, onEarlier }: {
      hasEarlier: boolean;
      onEarlier: () => void;
      messages: Message[];
      tasks: Task[];
      plan: Plan | null;
      approvals: Approval[];
      busy: boolean;
      active: boolean;
      onPlan: (allow: boolean) => void;
      onApproval: (a: Approval, allow: boolean) => void;
  } = $props();
</script>
<div bind:this={container} onscroll={()=>{follow=container.scrollHeight-container.scrollTop-container.clientHeight<90;}} class="messages" role="log" aria-label="Conversation messages" aria-live="polite">

{#if hasEarlier}<button class="load-history" onclick={()=>{follow=false;onEarlier();}} disabled={busy}>Load earlier messages</button>{/if}
{#each messages as message(message.id)}
<article class="message" class:user={message.role==='user'} class:notice={message.role==='notice'}>
<div class="message-author">{message.role==='user'?'You':message.role==='notice'?'Activity':message.provider?`${message.provider} · ${message.model??'Runtime default'}`:'Superagent'}</div>
<MessageBody text={message.text}/>
{#if message.attachments?.length}
<div class="message-attachments">
{#each message.attachments as attachment}
<span title={attachment.path}>▧ <bdi>{attachment.path.split(/[\\/]/).pop()}</bdi>
</span>
{/each}
</div>
{/if}
</article>
{/each}


{#if plan?.status==='awaiting_approval'}
<section class="plan-card" aria-label="Plan awaiting approval">
<div class="card-eyebrow">YOUR APPROVAL NEEDED</div>
<h2>Here’s the plan</h2>
<p dir="auto">{plan.summary}</p>
<ol>
{#each plan.tasks as task}
<li>
<strong dir="auto">{task.title}</strong>
<div>
<bdi>{task.agent_id}</bdi> · {task.model||'Runtime default'} · {task.effort||'Default effort'}</div>
{#if task.files?.length}
<small dir="ltr">{task.files.join(', ')}</small>
{/if}
</li>
{/each}
</ol>
<div class="button-row">
<button class="primary" onclick={()=>onPlan(true)} disabled={busy}>Approve and start</button>
<button onclick={()=>onPlan(false)} disabled={busy}>Decline plan</button>
</div>
</section>
{/if}


{#each approvals as approval(approval.run_id+approval.request_id)}
<section class="plan-card">
<div class="card-eyebrow">PERMISSION REQUEST</div>
<h2>Allow this step?</h2>
<p dir="auto">{approval.reason}</p>
{#if approval.command}
<pre class="code-block" dir="ltr">{approval.command}</pre>
{/if}
<div class="button-row">
<button class="primary" disabled={busy} onclick={()=>onApproval(approval,true)}>Allow once</button>
<button disabled={busy} onclick={()=>onApproval(approval,false)}>Deny</button>
</div>
</section>
{/each}


{#each tasks.filter(t=>t.kind!=='planner'&&t.status==='running'&&t.result) as task(task.id)}
<article class="message streaming">
<div class="message-author">{task.kind==='direct'?`${task.provider} · ${task.model??'Runtime default'}`:task.kind==='summary'?'Superagent':task.agent_id} <span class="stream-label">Live</span>
</div>
<MessageBody text={task.result}/>
</article>
{/each}


{#if active&&plan?.status!=='awaiting_approval'}
{#each tasks.filter(t=>(isActive(t.status)||t.status==='starting')&&(t.retry_notice||t.permission_notice)) as task(task.id)}
<article class="message notice">
<div class="message-author">Activity · <bdi>{task.kind==='direct'?task.provider:task.agent_id}</bdi></div>
<TaskNotices {task}/>
</article>
{/each}
<div class="working" role="status">
<span class="dot active">
</span> {tasks.some(t=>t.kind==='direct'&&['queued','starting','running'].includes(t.status))?'Generating a response':'Your team is working'}<span class="working-dots">…</span>
</div>
{/if}

</div>
