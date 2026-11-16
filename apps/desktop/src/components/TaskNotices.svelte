<script lang="ts">
  import type { Task } from '../lib/protocol';
  let { task }: { task: Task } = $props();
  const notices = $derived([...new Set([task.retry_notice,task.permission_notice].filter((notice): notice is string => !!notice))]);
</script>
{#each notices as notice}<p class="retry-notice" role="status" dir="auto">{notice}</p>{/each}
