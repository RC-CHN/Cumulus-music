<script setup lang="ts">
defineProps<{ issue: string; previous?: string; busy: boolean }>();
const emit = defineEmits<{ retry: [event: Event] }>();
</script>
<template>
  <section class="fm-state-recovery" role="alert" :aria-busy="busy">
    <p>暂时无法读取最新状态，请重新读取后再提交操作。</p>
    <p v-if="previous" class="fm-state-previous">上次提交：{{ previous }}</p>
    <p class="fm-state-read-issue">状态读取：{{ issue }}</p>
    <button class="secondary" :disabled="busy" @click="emit('retry', $event)">
      {{ busy ? '正在读取状态…' : '重新读取状态' }}
    </button>
    <small>重新读取不会重复提交。</small>
  </section>
</template>
<style scoped>
.fm-state-recovery {
  padding: 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--accent-soft);
  color: var(--text);
  line-height: 1.6;
  overflow-wrap: anywhere;
}
.fm-state-recovery p {
  margin: 0 0 8px;
}
.fm-state-previous,
.fm-state-read-issue {
  font-size: 12px;
}
.fm-state-recovery small {
  display: block;
  color: var(--muted);
  margin-top: 8px;
  font-size: 11px;
}
</style>
