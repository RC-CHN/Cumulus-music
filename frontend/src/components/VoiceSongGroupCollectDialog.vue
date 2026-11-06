<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import type { VoiceSongGroupCollectModel } from '../composables/useVoiceSongGroupCollect';
import { failure, type PlaylistView } from '../services/contracts';
import Icon from './Icon.vue';
import PlaylistCollectList from './PlaylistCollectList.vue';
import '../styles/playlist-collect.css';

const props = defineProps<{ model: VoiceSongGroupCollectModel }>();
const emit = defineEmits<{ expired: [] }>();
const { phase, plan, selected, message, saving } = props.model;
const dialog = ref<HTMLDialogElement>();
const closeButton = ref<HTMLButtonElement>();
const reconcileButton = ref<HTMLButtonElement>();
const sorts = [
  { value: 'default', label: '默认排序' },
  { value: 'recently', label: '常用优先' },
] as const;
type Sort = (typeof sorts)[number]['value'];
const preference = 'cumulus.playlist-collect-sort.v1';
function remembered(): Sort {
  try {
    return localStorage.getItem(preference) === 'recently' ? 'recently' : 'default';
  } catch {
    return 'default';
  }
}
const sort = ref<Sort>(remembered());
function lane() {
  return {
    rows: [] as PlaylistView[],
    next: 0 as number | null,
    loading: false,
    error: '',
    top: 0,
  };
}
const pages = reactive({ default: lane(), recently: lane() });
const current = computed(() => pages[sort.value]);
let alive = true;
onBeforeUnmount(() => {
  alive = false;
});
onMounted(() => dialog.value?.showModal());

async function load(automatic = false) {
  const state = current.value;
  const next = state.next;
  const owner = plan.value;
  if (!alive || !owner || next === null || state.loading || (automatic && state.error)) return;
  const request = props.model.destinations(sort.value, next);
  if (!request) return;
  state.loading = true;
  try {
    const reply = await request;
    if (!alive || plan.value !== owner || phase.value !== 'picking') return;
    if (reply.epoch !== owner.epoch) throw new Error('歌单列表已失效，请重试');
    const rows = next === 0 ? [] : state.rows.slice();
    const seen = new Set(rows.map((playlist) => playlist.id));
    for (const row of reply.value.playlists) {
      if (!row.owned || row.favorite || seen.has(row.id)) continue;
      seen.add(row.id);
      rows.push(row);
    }
    state.rows = rows;
    const following = reply.value.nextPage;
    state.next = following !== null && following > next ? following : null;
    state.error = '';
  } catch (error) {
    if (!alive || plan.value !== owner || phase.value !== 'picking') return;
    const problem = failure(error);
    state.error = problem.message;
    if (problem.code === 'session_expired' || problem.code === 'profile_changed') emit('expired');
  } finally {
    if (alive) state.loading = false;
  }
}
function selectSort(value: Sort) {
  if (value === sort.value || phase.value !== 'picking') return;
  sort.value = value;
  try {
    localStorage.setItem(preference, value);
  } catch {
    // Use the selected sort for this dialog.
  }
  if (current.value.next === 0) void load(true);
}
function sortKeys(event: KeyboardEvent) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const next =
    event.key === 'Home'
      ? 'default'
      : event.key === 'End'
        ? 'recently'
        : sort.value === 'default'
          ? 'recently'
          : 'default';
  selectSort(next);
  void nextTick(() =>
    dialog.value?.querySelector<HTMLElement>(`[data-collect-sort="${next}"]`)?.focus(),
  );
}
watch(
  phase,
  async (kind) => {
    if (kind === 'picking') void load(true);
    await nextTick();
    if (!alive || phase.value !== kind) return;
    if (kind === 'picking')
      dialog.value?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
    else if (kind === 'pending') reconcileButton.value?.focus();
    else if (!saving.value) closeButton.value?.focus();
  },
  { immediate: true },
);
function choose(id: string) {
  if (current.value.rows.some((row) => row.id === id)) void props.model.choose(id);
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-collect-dialog"
    data-voice-song-collect
    :data-voice-song-collect-stage="phase"
    aria-labelledby="voice-song-collect-title"
    :aria-busy="saving || phase === 'preparing'"
    @cancel.prevent="model.close"
  >
    <header>
      <h2 id="voice-song-collect-title">收藏到歌单</h2>
      <button
        ref="closeButton"
        type="button"
        class="icon-button collect-close"
        aria-label="关闭收藏"
        :disabled="saving"
        @click="model.close"
      >
        <Icon name="close" />
      </button>
    </header>
    <p v-if="plan" data-voice-song-collect-summary class="collect-feedback">
      已选 {{ plan.selectedCount }} 首，可收藏 {{ plan.eligibleCount }} 首<span
        v-if="plan.omittedIndices.length"
        >，跳过 {{ plan.omittedIndices.length }} 首</span
      >
    </p>
    <p
      v-if="phase !== 'picking'"
      data-voice-song-collect-status
      class="collect-feedback"
      role="status"
    >
      {{
        phase === 'preparing'
          ? '正在确认可收藏的歌曲…'
          : phase === 'saving'
            ? '正在收藏…'
            : phase === 'reading'
              ? '正在核对收藏结果…'
              : message
      }}
    </p>
    <footer v-if="phase === 'pending'" class="group-collect-footer">
      <button ref="reconcileButton" type="button" class="text-button" @click="model.reconcile">
        核对结果
      </button>
    </footer>
    <div v-if="phase === 'picking'" class="collect-body">
      <div class="collect-sort-tabs" role="tablist" aria-label="收藏歌单排序" @keydown="sortKeys">
        <button
          v-for="option in sorts"
          :key="option.value"
          type="button"
          role="tab"
          :data-collect-sort="option.value"
          :aria-selected="sort === option.value"
          :tabindex="sort === option.value ? 0 : -1"
          @click="selectSort(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
      <div v-if="current.error || current.loading" class="collect-load" role="status">
        {{ current.error || '正在读取歌单…' }}
        <button v-if="current.error" type="button" class="text-button" @click="load()">重试</button>
      </div>
      <PlaylistCollectList
        :key="sort"
        :include-create="false"
        :playlists="current.rows"
        :disabled="false"
        :selected-id="selected"
        :scroll-top="current.top"
        :loading="current.loading || !!current.error || current.next !== null"
        @scroll="current.top = $event"
        @near-end="load(true)"
        @select="choose"
      />
      <button
        v-if="current.next !== null && current.next !== 0 && !current.error"
        type="button"
        class="text-button collect-more"
        :disabled="current.loading"
        @click="load()"
      >
        加载更多歌单
      </button>
    </div>
  </dialog>
</template>

<style scoped>
.group-collect-footer {
  display: flex;
  justify-content: center;
  padding: 8px 20px 18px;
}
</style>
