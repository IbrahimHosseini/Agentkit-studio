<script lang="ts">
  import { tick } from 'svelte';
  import type { PermissionMode } from '../lib/protocol';
  import { permissionChoices } from '../lib/permissions';

  let { value, disabled = false, hasProject, onChange }: {
    value: PermissionMode;
    disabled?: boolean;
    hasProject: boolean;
    onChange: (value: PermissionMode) => void | Promise<void>;
  } = $props();
  const id = $props.id();
  let expanded = $state(false);
  let help = $state(false);
  let root: HTMLDivElement;
  let trigger: HTMLButtonElement;
  let menu: HTMLDivElement;
  const selected = $derived(permissionChoices.find(choice => choice.value === value) ?? permissionChoices[0]);
  function close(focus = false) {
    expanded = false;
    if (focus) trigger?.focus();
  }
  async function show(index = permissionChoices.findIndex(choice => choice.value === value)) {
    if (disabled) return;
    expanded = true;
    await tick();
    menu?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')[Math.max(0,index)]?.focus();
  }
  function outside(event: MouseEvent) {
    if (expanded && !root?.contains(event.target as Node)) close();
  }
  function focusOutside(event: FocusEvent) {
    if (expanded && !root?.contains(event.target as Node)) close();
  }
  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && expanded) { event.preventDefault(); event.stopPropagation(); close(true); return; }
    if (!expanded && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      event.preventDefault(); void show(event.key === 'ArrowUp' ? permissionChoices.length - 1 : undefined); return;
    }
    if (!expanded || !['ArrowDown','ArrowUp','Home','End'].includes(event.key)) return;
    const options = [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')];
    const current = options.indexOf(document.activeElement as HTMLButtonElement);
    const index = event.key === 'Home' ? 0 : event.key === 'End' ? options.length - 1 : (current + (event.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length;
    event.preventDefault(); options[index]?.focus();
  }
  function choose(mode: PermissionMode) {
    if (disabled) return;
    close(true);
    if (mode !== value) void onChange(mode);
  }
  $effect(() => { if (disabled) expanded = false; });
</script>

<svelte:window onclick={outside} onfocusin={focusOutside} onkeydown={event=>{if(root?.contains(event.target as Node))keydown(event);}}/>
<div class="permission-control" bind:this={root} role="group" aria-label="Chat permissions" dir="ltr">
  <button type="button" class="permission-trigger" class:full-access={value==='full_access'} bind:this={trigger} {disabled} aria-haspopup="menu" aria-expanded={expanded} aria-controls={id} title={disabled?'Permissions are unavailable while disconnected or a request is active':`Permissions: ${selected.label}`} onclick={()=>expanded?close():void show()}>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 4 6v6c0 4.5 8 9 8 9s8-4.5 8-9V6z"/>{#if value==='full_access'}<path d="M12 8v5m0 3h.01"/>{:else if value==='auto'}<path d="m8.5 12 2.5 2.5 4.5-5"/>{:else}<path d="M12 8v5m0 3h.01"/>{/if}</svg>
    <span>{selected.label}</span>
    <svg class="permission-chevron" width="10" height="10" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="m4 6 4 4 4-4"/></svg>
  </button>
  <div class="permission-popover" id={id} hidden={!expanded}>
    <div class="permission-heading"><span id={`${id}-heading`}>How should actions be approved?</span><button type="button" class="permission-learn" aria-expanded={help} aria-controls={`${id}-help`} onclick={()=>help=!help}>Learn more</button></div>
    <div role="menu" aria-labelledby={`${id}-heading`} bind:this={menu}>
      {#each permissionChoices as choice}
      <button type="button" role="menuitemradio" aria-checked={value===choice.value} class="permission-option" class:full-access={choice.value==='full_access'} tabindex={value===choice.value?0:-1} disabled={disabled} onclick={()=>choose(choice.value)}>
        <svg class="permission-option-icon" width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          {#if choice.value==='ask'}<path d="M8 12V5a1.5 1.5 0 0 1 3 0v6-8a1.5 1.5 0 0 1 3 0v8-6a1.5 1.5 0 0 1 3 0v7-4a1.5 1.5 0 0 1 3 0v6c0 4-2 7-6 7h-1c-2 0-3-1-4-3l-4-5a1.5 1.5 0 0 1 2-2l2 2"/>
          {:else}<path d="M12 3 4 6v6c0 4.5 8 9 8 9s8-4.5 8-9V6z"/>{#if choice.value==='auto'}<path d="m8.5 12 2.5 2.5 4.5-5"/>{:else}<path d="M12 8v5m0 3h.01"/>{/if}{/if}
        </svg>
        <span class="permission-option-copy"><span class="permission-option-label">{choice.label}</span><span class="permission-option-description">{choice.description}</span></span>
        {#if value===choice.value}<svg class="permission-check" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12 4 4L19 6"/></svg>{/if}
      </button>
      {/each}
    </div>
    {#if !hasProject}<p class="permission-note">Chats without a project stay read-only. Attach a project to allow changes.</p>{/if}
    <p class="permission-note" id={`${id}-help`} hidden={!help}>This choice is saved for this chat. Provider policies still apply. Automatic review may use additional model calls. Full access removes the project sandbox and approval prompts.</p>
  </div>
</div>
