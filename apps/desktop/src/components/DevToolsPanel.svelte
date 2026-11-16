<script lang="ts">
import {onMount} from 'svelte';
import {call,names,type Tool} from '../lib/devtools';
import BrowserPanel from './BrowserPanel.svelte';
import DevicePanel from './DevicePanel.svelte';
import TerminalPanel from './TerminalPanel.svelte';
let {conversationId,tool,onclose,preferredDeviceId='',preferredTerminalId=''}:{conversationId:string;tool:Tool;preferredDeviceId?:string;preferredTerminalId?:string;onclose:()=>void}=$props();
let grants=$state<Record<string,boolean>>({}),consent=$state(false),error=$state(''),wide=$state(false),width=$state(480);
onMount(()=>{call(conversationId,'access','get').then(v=>grants=v).catch(e=>error=String(e));});
async function access(allow:boolean){try{grants=await call(conversationId,'access',allow?'grant':'revoke',{target:tool});consent=false;}catch(e){error=String(e);}}
</script>
<aside class="devtools-pane" class:expanded={wide} style:width={`${width}px`} aria-label={names[tool]}>
<div class="devtools-heading"><strong>{names[tool]}</strong><button onclick={()=>wide=!wide} aria-label="Expand panel">⤢</button><button onclick={onclose} aria-label="Close tools panel">×</button></div>
<label class="devtools-width">Panel width <input type="range" min="320" max="900" step="20" bind:value={width}/></label>
<div class="devtools-access"><button onclick={()=>grants[tool]?access(false):consent=true}>{grants[tool]?'Revoke agent access':'Allow agent access'}</button></div>
{#if consent}<section class="devtools-consent"><strong>Let agents use {names[tool]}?</strong><p>{#if tool==='ios'||tool==='android'}Agents in this chat can view and control all {tool==='ios'?'iOS simulators':'Android emulators'}, not just the selected device.{:else if tool==='terminal'}Agents in this chat can read terminal output and run host commands from this project directory. Commands may affect files and services accessible to your account.{:else}Agents in this chat can view and control all browser tabs in this chat.{/if} Screenshots and output may be sent to your selected model provider. Use test accounts and data.</p><button onclick={()=>access(true)}>Allow</button><button onclick={()=>consent=false}>Cancel</button></section>{/if}
{#if error}<p role="alert">{error}</p>{/if}
{#key tool}{#if tool==='browser'}<BrowserPanel {conversationId}/>{:else if tool==='terminal'}<TerminalPanel {conversationId} {preferredTerminalId}/>{:else}<DevicePanel {conversationId} {preferredDeviceId} platform={tool}/>{/if}{/key}
</aside>
