<script lang="ts">
  import { onMount } from 'svelte';
  import { call, type Frame } from '../lib/devtools';
  let { conversationId }: { conversationId: string } = $props();
  let pages = $state<any[]>([]), selected = $state('');
  let url = $state('http://localhost:3000'), addressFocused = $state(false);
  let frame = $state<Frame | null>(null), error = $state(''), logs = $state<any[]>([]);
  let alive = true, revision = 0, lastActive = '';
  let address: HTMLInputElement;
  const rpc = (action: string, p = {}) => call(conversationId, 'browser', action, { page_id: selected || undefined, ...p });
  function select(id: string) {
    if (selected !== id) { selected = id; frame = null; revision++; }
  }
  async function action(name: string, p = {}) {
    const version = ++revision;
    try {
      error = '';
      const v = await rpc(name, p);
      if (!alive || version !== revision) return;
      if (v.pages) { pages = v.pages; lastActive = v.active_page_id ?? ''; select(lastActive); }
    } catch (e) { if (alive) error = String(e); }
  }
  function navigate() {
    let value = url.trim();
    if (!/^https?:\/\//.test(value) && value !== 'about:blank') value = 'http://' + value;
    void action(selected ? 'navigate' : 'open', { url: value });
  }
  function key(e: KeyboardEvent) {
    // Escape exits the remote page. Tab stays in Studio's normal focus order.
    if (e.key === 'Escape') { e.preventDefault(); address.focus(); return; }
    if (e.key === 'Tab') return;
    if (['Meta', 'Control', 'Alt', 'Shift'].includes(e.key)) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'v') return;
    e.preventDefault();
    if (e.metaKey || e.ctrlKey || e.altKey || e.key.length !== 1) {
      const parts = [e.metaKey ? 'Meta' : '', e.ctrlKey ? 'Control' : '', e.altKey ? 'Alt' : '', e.shiftKey ? 'Shift' : '', e.key === ' ' ? 'Space' : e.key].filter(Boolean);
      void action('key', { key: parts.join('+') });
    } else void action('type', { text: e.key });
  }
  onMount(() => {
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      const version = revision;
      try {
        const tabs = await rpc('list');
        if (!alive) return;
        if (version === revision) {
          pages = tabs.pages;
          const active = tabs.active_page_id ?? '';
          if (!pages.some(p => p.id === selected) || active !== lastActive) select(active);
          lastActive = active;
        }
        const id = selected, frameVersion = revision;
        if (id) {
          const f = await rpc('frame', { page_id: id });
          if (alive && selected === id && revision === frameVersion) {
            frame = f;
            if (!addressFocused && f.url) url = f.url;
          }
        }
      } catch (e) { if (alive) error = String(e); }
      if (alive) timer = setTimeout(poll, 450);
    }
    void poll();
    return () => { alive = false; clearTimeout(timer); };
  });
</script>
<div class="devtools-row"><select aria-label="Browser tab" value={selected} onchange={e=>{select(e.currentTarget.value);}}><option value="">Select tab</option>{#each pages as p}<option value={p.id}>{p.title||p.url||'New tab'}</option>{/each}</select><button onclick={()=>action('open',{url:'about:blank'})}>＋</button><button disabled={!selected} onclick={()=>action('close')}>Close tab</button></div>
<form class="devtools-row" onsubmit={e=>{e.preventDefault();navigate();}}><button type="button" disabled={!selected} onclick={()=>action('back')} aria-label="Back">←</button><button type="button" disabled={!selected} onclick={()=>action('forward')} aria-label="Forward">→</button><button type="button" disabled={!selected} onclick={()=>action('reload')} aria-label="Reload">↻</button><input bind:this={address} onfocus={()=>addressFocused=true} onblur={()=>addressFocused=false} aria-label="Page URL" bind:value={url} dir="ltr"/><button>Go</button></form>
{#if error}<p role="alert">{error}</p>{/if}
{#if frame}<button type="button" class="browser-screen" aria-label="Interactive browser page" onkeydown={key} onpaste={e=>{e.preventDefault();void action('type',{text:e.clipboardData?.getData('text')??''});}} onwheel={e=>{e.preventDefault();void action('scroll',{delta_x:Math.max(-5000,Math.min(5000,e.deltaX)),delta_y:Math.max(-5000,Math.min(5000,e.deltaY))});}} onclick={e=>{const r=e.currentTarget.getBoundingClientRect();e.currentTarget.focus();void action('click',{x:(e.clientX-r.left)/r.width*frame!.width,y:(e.clientY-r.top)/r.height*frame!.height});}}><img draggable="false" alt="Current browser page" src={`data:${frame.mime_type};base64,${frame.data}`}/></button><div class="devtools-row"><button onclick={()=>action('key',{key:'Tab'})}>Page Tab</button><button onclick={()=>action('key',{key:'Escape'})}>Page Escape</button></div>{:else}<p class="devtools-empty">Enter a URL to browse. Start your project's dev server in Terminal, then open its local URL here. Browser requires Node.js and Chrome, Chromium or Edge.</p>{/if}
<details><summary onclick={()=>rpc('console').then(v=>logs=v.entries).catch(()=>{})}>Console</summary>{#each logs as log}<pre dir="ltr">{log.type}: {log.text}</pre>{/each}</details>
