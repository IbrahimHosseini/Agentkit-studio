<script lang="ts">
  import InlineText from './InlineText.svelte';
  let {text}:{text:string}=$props();
  type Block={kind:'text'|'code'|'heading'|'list'|'table';text:string;items?:string[];language?:string;headers?:string[];rows?:string[][];align?:('left'|'right'|'center'|undefined)[]};
  function cells(value:string):string[] {
    let line=value.trim().replace(/^\\\|/,'|').replace(/\\$/,'').trim();
    if(line.startsWith('|'))line=line.slice(1);
    if(line.endsWith('|')&&!line.endsWith('\\|'))line=line.slice(0,-1);
    const result:string[]=[];let cell='';let code=false;
    for(let i=0;i<line.length;i++) {
      if(line[i]==='\\'&&line[i+1]==='|'){cell+='|';i++;continue;}
      if(line[i]==='`')code=!code;
      if(line[i]==='|'&&!code){result.push(cell.trim());cell='';}else cell+=line[i];
    }
    result.push(cell.trim());return result;
  }
  function parse(value:string):Block[]{
    const blocks:Block[]=[];let paragraph:string[]=[];let code:string[]|null=null;let language='';
    const flush=()=>{if(paragraph.length){blocks.push({kind:'text',text:paragraph.join('\n')});paragraph=[];}};
    const lines=value.split('\n');
    for(let index=0;index<lines.length;index++){
      const line=lines[index];
      if(line.startsWith('```')){flush();if(code){blocks.push({kind:'code',text:code.join('\n'),language});code=null;}else{code=[];language=line.slice(3).trim();}continue;}
      if(code){code.push(line);continue;}
      if(!line.trim()){flush();continue;}
      const header=cells(line), separator=index+1<lines.length?cells(lines[index+1]):[];
      if(header.length>1&&separator.length===header.length&&separator.every(cell=>/^:?-{3,}:?$/.test(cell))){
        flush();index++;const rows:string[][]=[];
        while(index+1<lines.length&&lines[index+1].includes('|')&&lines[index+1].trim()){
          const row=cells(lines[index+1]);if(row.length<2)break;
          rows.push(header.map((_,i)=>row[i]??''));index++;
        }
        blocks.push({kind:'table',text:'',headers:header,rows,align:separator.map(cell=>cell.startsWith(':')&&cell.endsWith(':')?'center':cell.endsWith(':')?'right':undefined)});continue;
      }
      const heading=line.match(/^#{1,6}\s+(.+)$/);if(heading){flush();blocks.push({kind:'heading',text:heading[1]});continue;}
      const bullet=line.match(/^\s*(?:[-*+] |\d+\. )(.+)$/);if(bullet){flush();const last=blocks.at(-1);if(last?.kind==='list')last.items?.push(bullet[1]);else blocks.push({kind:'list',text:'',items:[bullet[1]]});continue;}
      paragraph.push(line);
    }
    flush();if(code)blocks.push({kind:'code',text:code.join('\n'),language});return blocks;
  }
  const blocks=$derived(parse(text));
</script>
<div class="message-body">
{#each blocks as block}
  {#if block.kind==='code'}<div class="code-container">{#if block.language}<div class="code-language" dir="ltr">{block.language}</div>{/if}<pre class="code-block" dir="ltr"><code>{block.text}</code></pre></div>
  {:else if block.kind==='table'}<div class="prose-table-wrap" dir="auto" tabindex="0" role="region" aria-label="Message table"><table class="prose-table"><thead><tr>{#each block.headers??[] as cell,i}<th scope="col" dir="auto" style:text-align={block.align?.[i]}><InlineText text={cell}/></th>{/each}</tr></thead><tbody>{#each block.rows??[] as row}<tr>{#each row as cell,i}<td dir="auto" style:text-align={block.align?.[i]}><InlineText text={cell}/></td>{/each}</tr>{/each}</tbody></table></div>
  {:else if block.kind==='heading'}<h3 class="prose-heading" dir="auto"><InlineText text={block.text}/></h3>
  {:else if block.kind==='list'}<ul class="prose-list">{#each block.items??[] as item}<li dir="auto"><InlineText text={item}/></li>{/each}</ul>
  {:else}<div class="prose" dir="auto"><InlineText text={block.text}/></div>{/if}
{/each}
</div>
