<script lang="ts">
  let {text}:{text:string}=$props();
  type Part = {kind:'text'|'code'|'strong'|'link';text:string;href?:string};
  function parse(value:string):Part[] {
    const parts:Part[]=[];
    // Text is rendered by Svelte, never injected as HTML. Escaped URL punctuation
    // also supports links pasted from Markdown-escaped conversation exports.
    const tokens=/(`[^`\n]+`|\*\*[^*\n]+\*\*|\[[^\]\n]+\]\\?\((?:\\.|[^()\s]|\([^()\n]*\))+?\\?\))/g;
    let end=0;
    for(const match of value.matchAll(tokens)) {
      const index=match.index??0;
      if(index>end)parts.push({kind:'text',text:value.slice(end,index)});
      const token=match[0];
      if(token.startsWith('`'))parts.push({kind:'code',text:token.slice(1,-1)});
      else if(token.startsWith('**'))parts.push({kind:'strong',text:token.slice(2,-2)});
      else {
        const link=token.match(/^\[([^\]]+)\]\\?\(([\s\S]*)\)$/);
        const raw=link?.[2].replace(/\\$/,'').replace(/\\([\\():/])/g,'$1')??'';
        let href:string|undefined;
        try {const url=new URL(raw);if(['https:','http:'].includes(url.protocol))href=url.href;}catch{/* Invalid targets remain literal text. */}
        parts.push(href?{kind:'link',text:link![1],href}:{kind:'text',text:token});
      }
      end=index+token.length;
    }
    if(end<value.length)parts.push({kind:'text',text:value.slice(end)});
    return parts;
  }
  const parts=$derived(parse(text));
</script>
{#each parts as part}{#if part.kind==='code'}<code dir="ltr">{part.text}</code>{:else if part.kind==='strong'}<strong>{part.text}</strong>{:else if part.kind==='link'}<a href={part.href} target="_blank" rel="noopener noreferrer">{part.text}</a>{:else}{part.text}{/if}{/each}
