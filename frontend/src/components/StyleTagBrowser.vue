<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue';
import { failure, type StyleCategory } from '../services/contracts';
import { useCore } from '../services/core';
import Icon from './Icon.vue';

const props = defineProps<{ epoch: string }>();
const core = useCore();
const trigger = ref<HTMLButtonElement>();
const dialog = ref<HTMLDialogElement>();
const open = ref(false);
const loading = ref(false);
const error = ref('');
const categories = ref<StyleCategory[]>([]);
const selected = ref<string>();
const category = computed(() => categories.value.find((item) => item.id === selected.value));
let request = 0;

async function load() {
  const epoch = props.epoch;
  const current = ++request;
  loading.value = true;
  error.value = '';
  try {
    const result = await core.styleRecommendationCategories(epoch);
    if (current !== request || props.epoch !== epoch || result.epoch !== epoch) return;
    categories.value = result.value;
    selected.value = result.value[0]?.id;
  } catch (issue) {
    if (current === request && props.epoch === epoch) error.value = failure(issue).message;
  } finally {
    if (current === request) loading.value = false;
  }
}
async function show() {
  open.value = true;
  categories.value = [];
  selected.value = undefined;
  await nextTick();
  if (!open.value || !dialog.value) return;
  dialog.value.showModal();
  void load();
}
function close() {
  request++;
  dialog.value?.close();
  open.value = false;
  trigger.value?.focus({ preventScroll: true });
}
watch(
  () => props.epoch,
  () => {
    if (open.value) close();
  },
);
onUnmounted(() => request++);
</script>

<template>
  <button ref="trigger" type="button" class="text-button style-tags-entry" @click="show">
    浏览风格标签
  </button>
  <dialog
    v-if="open"
    ref="dialog"
    class="style-tag-browser"
    aria-label="风格标签"
    @cancel.prevent="close"
  >
    <header>
      <h2>风格标签</h2>
      <button type="button" class="icon-button" aria-label="关闭风格标签" @click="close">
        <Icon name="close" />
      </button>
    </header>
    <p class="muted">浏览可用分类和标签，不会更改推荐偏好。</p>
    <p v-if="loading" role="status">正在读取风格标签…</p>
    <div v-else-if="error">
      <p role="alert">{{ error }}</p>
      <button type="button" @click="load">重试</button>
    </div>
    <p v-else-if="!categories.length">暂无可用风格分类。</p>
    <div v-else class="style-tag-body">
      <nav aria-label="风格分类">
        <button
          v-for="item in categories"
          :key="item.id"
          type="button"
          :aria-pressed="selected === item.id"
          @click="selected = item.id"
        >
          {{ item.name }}
        </button>
      </nav>
      <ul v-if="category?.tags.length" aria-label="当前分类标签">
        <li v-for="tag in category.tags" :key="tag.id">{{ tag.name }}</li>
      </ul>
      <p v-else>此分类暂无标签。</p>
    </div>
  </dialog>
</template>

<style scoped>
.style-tag-browser {
  width: min(574px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  padding: 18px 22px;
  border: 1px solid var(--line);
  border-radius: 12px;
  color: var(--text);
  background: var(--surface-raised);
}
.style-tag-browser[open] {
  display: flex;
  flex-direction: column;
}
.style-tag-browser::backdrop {
  background: #0004;
}
.style-tag-browser header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex: none;
}
.style-tag-browser h2 {
  margin: 0;
  font-size: 19px;
}
.style-tag-body {
  overflow: auto;
  min-height: 0;
}
.style-tag-body nav,
.style-tag-body ul {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.style-tag-body ul {
  list-style: none;
  padding: 0;
}
.style-tag-body li {
  padding: 6px 12px;
  border-radius: 24px;
  background: var(--surface-alt);
}
.style-tag-body button[aria-pressed='true'] {
  color: var(--accent);
}
</style>
