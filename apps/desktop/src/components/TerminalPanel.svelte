<script lang="ts">
import {onMount,untrack} from 'svelte';
import {Terminal} from '@xterm/xterm';
import {FitAddon} from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import {call} from '../lib/devtools';
let {conversationId,preferredTerminalId=''}:{conversationId:string;preferredTerminalId?:string}=$props();
let host:HTMLDivElement;let terminals=$state<any[]>([]),selected=$state(''),error=$state('');let term:Terminal,fit:FitAddon,alive=true,cursor=0,revision=0;
const rpc=(action:string,p={})=>call(conversationId,'terminal',action,p);
async function refresh(){const v=await rpc('list');if(alive)terminals=v.terminals;}
function choose(id:string){revision++;selected=id;cursor=0;term?.reset();}
$effect(()=>{const id=preferredTerminalId;if(id)untrack(()=>{choose(id);void refresh().catch(e=>error=String(e));});});
async function create(){try{const v=await rpc('create');await refresh();choose(v.id);}catch(e){error=String(e);}}
async function close(){try{await rpc('close',{id:selected});await refresh();choose(terminals[0]?.id??'');}catch(e){error=String(e);}}
onMount(()=>{const colors=getComputedStyle(host);term=new Terminal({cursorBlink:true,fontSize:12,convertEol:false,theme:{background:colors.getPropertyValue('--surface').trim(),foreground:colors.getPropertyValue('--text').trim(),cursor:colors.getPropertyValue('--accent').trim()}});fit=new FitAddon();term.loadAddon(fit);term.open(host);fit.fit();const sub=term.onData(data=>{if(selected)rpc('write',{id:selected,data}).catch(e=>error=String(e));});const observer=new ResizeObserver(()=>{fit.fit();if(selected)rpc('resize',{id:selected,cols:term.cols,rows:term.rows}).catch(()=>{});});observer.observe(host);let timer:ReturnType<typeof setTimeout>;
async function poll(){const id=selected,version=revision;try{await refresh();if(alive&&version===revision&&selected&&!terminals.some(t=>t.id===selected))choose(terminals[0]?.id??'');if(id&&version===revision){const v=await rpc('read',{id,after:cursor});if(alive&&id===selected&&version===revision){cursor=v.cursor;term.write(Uint8Array.from(atob(v.data),c=>c.charCodeAt(0)));}}}catch(e){if(alive)error=String(e);}if(alive)timer=setTimeout(poll,200);}
refresh().then(()=>{if(!alive)return;if(preferredTerminalId&&terminals.some(t=>t.id===preferredTerminalId))choose(preferredTerminalId);else if(terminals.length)choose(terminals[0].id);else void create();void poll();}).catch(e=>error=String(e));return()=>{alive=false;clearTimeout(timer);observer.disconnect();sub.dispose();term.dispose();};});
</script>
<div class="devtools-row"><select aria-label="Terminal tab" value={selected} onchange={e=>choose(e.currentTarget.value)}>{#each terminals as t}<option value={t.id}>{t.title||'Terminal'}{t.exited?' (exited)':''}</option>{/each}</select><button onclick={create}>＋</button><button onclick={close} disabled={!selected}>Close tab</button></div>
{#if error}<p role="alert">{error}</p>{/if}<div class="terminal-host" bind:this={host} dir="ltr"></div>
