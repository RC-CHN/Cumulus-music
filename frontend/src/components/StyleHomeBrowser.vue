<script setup lang="ts">
import { nextTick, onUnmounted, ref, watch } from 'vue';
import { artworkAt } from '../services/artwork';
import { failure, type StyleSeedHandle, type StyleSeedPage } from '../services/contracts';
import { useCore } from '../services/core';
import Icon from './Icon.vue';

const props = defineProps<{ epoch: string }>();
const emit = defineEmits<{ open: [seed: StyleSeedHandle]; expired: [] }>();
const core = useCore();
const trigger = ref<HTMLButtonElement>();
const dialog = ref<HTMLDialogElement>();
const open = ref(false);
const loading = ref(false);
const error = ref('');
const page = ref<StyleSeedPage>();
const missingArt = ref(new Set<string>());
let serial = 0;

async function load() {
  if (!open.value || loading.value) return;
  const request = ++serial;
  const owner = props.epoch;
  const active = () => request === serial && owner === props.epoch && open.value;
  loading.value = true;
  error.value = '';
  page.value = undefined;
  missingArt.value = new Set();
  try {
    const reply = await core.styleHomeSeeds(owner);
    if (active() && reply.epoch === owner) page.value = reply.value;
  } catch (issue) {
    if (!active()) return;
    const problem = failure(issue);
    error.value = problem.message;
    if (['session_expired', 'profile_changed'].includes(problem.code)) emit('expired');
  } finally {
    if (active()) loading.value = false;
  }
}

async function show() {
  if (open.value) return;
  open.value = true;
  const owner = props.epoch;
  await nextTick();
  if (!open.value || owner !== props.epoch || !dialog.value) return;
  dialog.value.showModal();
  void load();
}

function close(restoreFocus = true) {
  serial++;
  dialog.value?.close();
  open.value = false;
  loading.value = false;
  error.value = '';
  page.value = undefined;
  if (restoreFocus) focus();
}

function choose(id: string) {
  if (!open.value || !page.value?.cards.some((card) => card.id === id)) return;
  const seed = { id, revision: page.value.revision };
  close(false);
  emit('open', seed);
}

function focus() {
  trigger.value?.focus({ preventScroll: true });
}
defineExpose({ focus });
watch(
  () => props.epoch,
  () => close(false),
);
onUnmounted(() => serial++);
</script>

<template>
  <button ref="trigger" type="button" class="text-button style-home-entry" @click="show">
    首页风格推荐
  </button>
  <dialog
    v-if="open"
    ref="dialog"
    class="style-home-browser"
    aria-label="首页风格推荐"
    @cancel.prevent="close()"
  >
    <header>
      <h2>首页风格推荐</h2>
      <button type="button" class="icon-button" aria-label="关闭首页风格推荐" @click="close()">
        <Icon name="close" />
      </button>
    </header>
    <p class="muted">选择首页推荐的风格，只浏览这组歌曲，不更改推荐偏好。</p>
    <div class="style-home-body">
      <p v-if="loading" role="status">正在读取首页风格推荐…</p>
      <div v-else-if="error">
        <p role="alert">{{ error }}</p>
        <button type="button" @click="load">重试</button>
      </div>
      <p v-else-if="!page?.cards.length" class="muted">暂无可用的首页风格推荐。</p>
      <ul v-else>
        <li v-for="card in page.cards" :key="card.id">
          <button type="button" class="style-home-card" @click="choose(card.id)">
            <img
              v-if="card.coverUrl && !missingArt.has(card.id)"
              :src="artworkAt(card.coverUrl, 80)"
              alt=""
              decoding="async"
              referrerpolicy="no-referrer"
              @error="missingArt.add(card.id)"
            />
            <Icon v-else name="music" />
            <span>{{ card.title }}</span>
          </button>
        </li>
      </ul>
    </div>
  </dialog>
</template>

<style scoped>
.style-home-browser {
  width: min(500px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  padding: 18px 22px;
  border: 1px solid var(--line);
  border-radius: 12px;
  color: var(--text);
  background: var(--surface-raised);
}
.style-home-browser[open] {
  display: flex;
  flex-direction: column;
}
.style-home-browser::backdrop {
  background: #0004;
}
header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
h2 {
  font-size: 19px;
  margin: 0;
}
.style-home-body {
  min-height: 0;
  overflow: auto;
}
ul {
  display: grid;
  gap: 8px;
  list-style: none;
  padding: 3px;
  margin: 0;
}
.style-home-card {
  display: flex;
  gap: 10px;
  align-items: center;
  width: 100%;
  text-align: start;
  overflow-wrap: anywhere;
  padding: 6px 10px;
}
.style-home-card img,
.style-home-card .icon {
  width: 40px;
  height: 40px;
  flex: none;
  object-fit: cover;
  border-radius: 6px;
}
</style>
