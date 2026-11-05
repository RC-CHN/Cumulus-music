<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';

const props = defineProps<{
  playId: string;
  embedded?: boolean;
  state: 'idle' | 'loading' | 'ready' | 'error';
  card: { readonly metas: readonly { readonly title: string; readonly content: string }[] } | null;
}>();
const emit = defineEmits<{ retry: [] }>();
const scroll = ref<HTMLElement>();
const state = computed(() =>
  props.state === 'ready' && props.card === null ? 'empty' : props.state,
);

watch(
  () => props.playId,
  () => {
    if (!props.embedded && scroll.value) scroll.value.scrollTop = 0;
  },
);

async function retry(event: MouseEvent) {
  if (props.state !== 'error') return;
  const focused = event.currentTarget === document.activeElement;
  const playId = props.playId;
  emit('retry');
  await nextTick();
  if (focused && playId === props.playId && document.activeElement === document.body) {
    scroll.value?.focus({ preventScroll: true });
  }
}
</script>

<template>
  <section
    ref="scroll"
    class="vinyl-wiki-pane"
    :class="{ 'vinyl-wiki-embedded': embedded }"
    data-vinyl-wiki-pane
    :data-state="state"
    :aria-busy="state === 'loading'"
    aria-label="音乐百科"
    :tabindex="embedded ? -1 : 0"
  >
    <article v-if="state === 'ready' && card" class="vinyl-wiki-card" data-vinyl-wiki-card>
      <h2>音乐百科</h2>
      <dl v-if="card.metas.length" class="vinyl-wiki-metas">
        <div
          v-for="(meta, index) in card.metas"
          :key="index"
          class="vinyl-wiki-meta"
          data-vinyl-wiki-meta
        >
          <dt>{{ meta.title }}</dt>
          <dd>{{ meta.content }}</dd>
        </div>
      </dl>
    </article>
    <p v-else-if="state === 'loading'" class="vinyl-wiki-status" role="status">正在加载百科…</p>
    <p v-else-if="state === 'empty'" class="vinyl-wiki-status" role="status">暂无百科信息</p>
    <div v-else-if="state === 'error'" class="vinyl-wiki-error">
      <p class="vinyl-wiki-status" role="status">百科加载失败</p>
      <button type="button" data-vinyl-wiki-retry @click="retry">重试</button>
    </div>
  </section>
</template>

<style scoped>
.vinyl-wiki-pane {
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  overflow: hidden auto;
  overscroll-behavior-y: contain;
  scrollbar-width: thin;
}

.vinyl-wiki-embedded {
  flex: none;
  overflow: visible;
}

.vinyl-wiki-pane:focus-visible {
  border-radius: 8px;
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.vinyl-wiki-card {
  display: flex;
  flex: none;
  flex-direction: column;
  gap: 20px;
  min-width: 0;
  padding: 20px;
  border-radius: 12px;
  background: color-mix(in srgb, var(--surface-alt) 82%, transparent);
}

.vinyl-wiki-card h2 {
  margin: 0;
  color: var(--text);
  font-size: 20px;
  font-weight: 600;
  line-height: 24px;
  letter-spacing: 0.5px;
}

.vinyl-wiki-metas {
  display: grid;
  gap: 14px;
  min-width: 0;
  margin: 0;
}

.vinyl-wiki-meta {
  display: grid;
  grid-template-columns: min(80px, 40%) minmax(0, 1fr);
  column-gap: 20px;
  min-width: 0;
  font-size: 16px;
  line-height: 24px;
  overflow-wrap: anywhere;
}

.vinyl-wiki-meta dt {
  min-width: 0;
  color: var(--muted);
}

.vinyl-wiki-meta dd {
  min-width: 0;
  margin: 0;
  color: var(--text-secondary);
}

.vinyl-wiki-status {
  margin: 0;
  padding-block: 4px;
  color: var(--muted);
  font-size: 13px;
}

.vinyl-wiki-error {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 10px;
}

.vinyl-wiki-error button {
  color: var(--accent);
  font-size: 13px;
}
</style>
