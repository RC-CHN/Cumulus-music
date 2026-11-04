<script setup lang="ts">
import type { Page } from '../composables/useMusic';
import { choiceTabs } from '../services/choiceNavigation';
defineProps<{ page: Page }>();
const emit = defineEmits<{ navigate: [page: Page] }>();
</script>

<template>
  <nav class="choice-tabs" aria-label="精选分类">
    <button
      v-for="tab in choiceTabs"
      :key="tab.label"
      type="button"
      :aria-current="page.kind === tab.page.kind ? 'page' : undefined"
      @click="page.kind !== tab.page.kind && emit('navigate', tab.page)"
    >
      {{ tab.label }}
    </button>
  </nav>
</template>

<style scoped>
.choice-tabs {
  display: flex;
  flex: none;
  align-items: center;
  gap: clamp(12px, 2.5vw, 34px);
  padding: 0 clamp(24px, 3.3vw, 48px);
  min-height: 44px;
  color: var(--text-muted);
  border-bottom: 1px solid var(--line);
}
button {
  min-height: 44px;
  padding-inline: 4px;
  background: none;
  color: inherit;
  border: 0;
  border-bottom: 2px solid transparent;
  white-space: nowrap;
}
button[aria-current='page'] {
  color: var(--text);
  border-bottom-color: var(--accent);
  font-weight: 650;
}
</style>
