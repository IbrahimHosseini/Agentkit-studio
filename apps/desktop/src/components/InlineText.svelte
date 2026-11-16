<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  let {text}:{text:string}=$props();
  type Part = {kind:'text'|'code'|'strong'|'link';text:string;href?:string;path?:string;label?:Part[]};
  let error=$state('');
  function parse(value:string,links=true):Part[] {
    const parts:Part[]=[];
    // Consume a whole link before parsing its label, so code in a label cannot
    // split the link. Count URL parentheses instead of stopping at the first ).
    const tokens=/`[^`\n]+`|\*\*[^*\n]+\*\*|\[[^\]\n]+\]\\?\(/g;
    let end=0;
    for(let match=tokens.exec(value);match;match=tokens.exec(value)) {
      const index=match.index;
      if(index>end)parts.push({kind:'text',text:value.slice(end,index)});
      let token=match[0];
      if(token.startsWith('`'))parts.push({kind:'code',text:token.slice(1,-1)});
      else if(token.startsWith('**'))parts.push({kind:'strong',text:token.slice(2,-2)});
      else {
        const escaped=token.endsWith('\\(');
        let depth=0,stop=tokens.lastIndex,closed=false;
        const start=stop;
        const angled=value[start]==='<';
        for(;stop<value.length;stop++) {
          const char=value[stop];
          if(char==='\n')break;
          if(char==='\\') {
            if(escaped&&value[stop+1]===')'&&depth===0){closed=true;break;}
            stop++;continue;
          }
          if(angled&&value[stop-1]!=='>')continue;
          if(char===')'&&depth===0){closed=true;break;}
          if(!angled&&/\s/.test(char))break;
          if(char==='(')depth++;
          if(char===')')depth--;
        }
        if(closed) {
          const raw=value.slice(start,stop).replace(/^<([\s\S]*)>$/,'$1').replace(/\\([\\():/])/g,'$1');
          token=value.slice(index,stop+(value[stop]==='\\'?2:1));
          tokens.lastIndex=index+token.length;
          let href:string|undefined,path:string|undefined;
          if(links&&!/[\u0000-\u001f\u007f]/.test(raw)) {
            if((raw.startsWith('/')&&!raw.startsWith('//'))||/^[A-Za-z]:[\\/]/.test(raw)) {
              path=raw.replace(/:\d+(?::\d+)?$/,'');
              href=raw;
            } else {
              try {const url=new URL(raw);if(['https:','http:'].includes(url.protocol))href=url.href;}catch{/* Invalid targets remain literal text. */}
            }
          }
          parts.push(href?{kind:'link',text:raw,href,path,label:parse(match[0].slice(1,match[0].lastIndexOf(']')),false)}:{kind:'text',text:token});
        } else parts.push({kind:'text',text:token});
      }
      end=index+token.length;
    }
    if(end<value.length)parts.push({kind:'text',text:value.slice(end)});
    return parts;
  }
  async function follow(event:MouseEvent,part:Part) {
    if(!part.path)return;
    event.preventDefault();
    error='';
    try {await invoke('reveal_source_file',{path:part.path});}
    catch {error=`Could not reveal ${part.text}. The file may have moved or be on another computer.`;}
  }
  const parts=$derived(parse(text));
</script>
{#snippet label(parts:Part[])}{#each parts as part}{#if part.kind==='code'}<code dir="ltr">{part.text}</code>{:else if part.kind==='strong'}<strong>{part.text}</strong>{:else}{part.text}{/if}{/each}{/snippet}
{#each parts as part}{#if part.kind==='link'}<a href={part.href} title={part.path?`Reveal file: ${part.text}`:undefined} target={part.path?undefined:'_blank'} rel="noopener noreferrer" onclick={(event)=>follow(event,part)}>{@render label(part.label??[])}</a>{:else}{@render label([part])}{/if}{/each}{#if error}<span role="alert" dir="auto">{error}</span>{/if}
