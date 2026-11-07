<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import type { VoiceSongGroupDestinationModel } from '../composables/useVoiceSongGroupDestination';
import { validVoiceSongCollectionName } from '../composables/useVoiceSongGroupCollect';
import { failure, type PlaylistView } from '../services/contracts';
import { playlistLength } from '../services/playlists';
import Icon from './Icon.vue';
import PlaylistCollectList from './PlaylistCollectList.vue';
import '../styles/playlist-collect.css';

const props = defineProps<{ model: VoiceSongGroupDestinationModel }>();
const emit = defineEmits<{ expired: [] }>();
const { phase, message, liked, likedLoading, likedError, busy } = props.model;
const dialog = ref<HTMLDialogElement>();
const nameInput = ref<HTMLInputElement>();
const name = ref('');
const privatePlaylist = ref(false);
const valid = computed(() => validVoiceSongCollectionName(name.value));
const options = [
  { value: 'default', label: '默认排序' },
  { value: 'recently', label: '常用优先' },
] as const;
type Sort = (typeof options)[number]['value'];
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
onMounted(() => dialog.value?.showModal());
onBeforeUnmount(() => {
  alive = false;
});

async function load(automatic = false) {
  const state = current.value;
  const page = state.next;
  if (state.loading || page === null || (automatic && state.error)) return;
  const request = props.model.page(sort.value, page);
  if (!request) return;
  state.loading = true;
  try {
    const reply = await request;
    if (!alive) return;
    const rows = page === 0 ? [] : [...state.rows];
    const seen = new Set(rows.map((row) => row.id));
    for (const row of reply.playlists) {
      if (!row.owned || row.favorite || seen.has(row.id)) continue;
      seen.add(row.id);
      rows.push(row);
    }
    state.rows = rows;
    state.next = reply.nextPage !== null && reply.nextPage > page ? reply.nextPage : null;
    state.error = '';
  } catch (error) {
    if (!alive || phase.value !== 'pick') return;
    const issue = failure(error);
    state.error = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') emit('expired');
  } finally {
    if (alive) state.loading = false;
  }
}
function selectSort(value: Sort) {
  if (value === sort.value || phase.value !== 'pick') return;
  sort.value = value;
  try {
    localStorage.setItem(preference, value);
  } catch {
    /* Keep this visit's sort. */
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
  async (value) => {
    if (value === 'pick') void load(true);
    else if (value === 'create') {
      name.value = '';
      privatePlaylist.value = false;
    }
    await nextTick();
    if (!alive || phase.value !== value) return;
    if (value === 'create') nameInput.value?.focus();
    else if (value === 'pick') dialog.value?.querySelector<HTMLElement>('.collect-list')?.focus();
    else {
      const selector =
        value === 'pending'
          ? '[data-voice-group-destination-reconcile]'
          : value === 'created_ready'
            ? '[data-voice-group-destination-resume]'
            : '.collect-close:not(:disabled)';
      dialog.value?.querySelector<HTMLElement>(selector)?.focus();
    }
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
    data-voice-group-destination
    :data-voice-group-destination-stage="phase"
    aria-labelledby="voice-group-destination-title"
    :aria-busy="busy"
    @cancel.prevent="model.close"
  >
    <header>
      <button
        v-if="phase === 'create'"
        type="button"
        class="icon-button collect-back"
        aria-label="返回下载歌单"
        @click="model.back"
      >
        <Icon name="back" />
      </button>
      <h2 id="voice-group-destination-title">
        {{ phase === 'create' ? '创建新歌单' : '下载到歌单' }}
      </h2>
      <button
        type="button"
        class="icon-button collect-close"
        aria-label="关闭歌曲下载"
        :disabled="busy"
        @click="model.close"
      >
        <Icon name="close" />
      </button>
    </header>
    <p v-if="message" class="collect-feedback" role="status">{{ message }}</p>
    <div v-if="phase === 'pick'" class="collect-body">
      <div class="collect-sort-tabs" role="tablist" aria-label="下载歌单排序" @keydown="sortKeys">
        <button
          v-for="option in options"
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
        <button v-if="current.error" class="text-button" type="button" @click="load()">重试</button>
      </div>
      <div v-if="!liked && (likedLoading || likedError)" class="collect-load" role="status">
        {{ likedError ? '暂时无法核对喜欢歌单' : '正在读取喜欢歌单…' }}
        <button v-if="likedError" class="text-button" type="button" @click="model.readLiked">
          重试
        </button>
      </div>
      <PlaylistCollectList
        :key="sort"
        purpose="download"
        :system-liked="!!liked"
        :playlists="current.rows"
        :disabled="false"
        selected-id=""
        :scroll-top="current.top"
        :loading="current.loading || !!current.error || current.next !== null"
        @scroll="current.top = $event"
        @near-end="load(true)"
        @select="choose"
        @liked="model.chooseLiked"
        @create="model.openCreate"
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
    <form
      v-else-if="phase === 'create'"
      class="collect-create"
      @submit.prevent="valid && model.create(name.trim(), privatePlaylist)"
    >
      <label>
        歌单名称<input
          ref="nameInput"
          v-model="name"
          aria-label="歌单名称"
          aria-describedby="voice-group-destination-name-count"
          :aria-invalid="!!name && !valid"
          placeholder="输入歌单标题"
          maxlength="80"
        />
        <small id="voice-group-destination-name-count"
          >{{ playlistLength(name.trim()) }} / 40</small
        >
      </label>
      <label class="private-checkbox"
        ><input v-model="privatePlaylist" type="checkbox" />设为私密歌单</label
      >
      <p v-if="!privatePlaylist" class="muted">公开歌单可被其他用户查看和收藏。</p>
      <p v-if="name && !valid" class="form-warning">名称不能包含 @ 或 #，并请保持在字数上限内。</p>
      <footer><button class="primary" type="submit" :disabled="!valid">创建</button></footer>
    </form>
    <div v-else-if="phase === 'pending' || phase === 'created_ready'" class="collect-feedback">
      <button
        v-if="phase === 'pending'"
        type="button"
        class="secondary"
        data-voice-group-destination-reconcile
        @click="model.reconcile"
      >
        核对结果
      </button>
      <button
        v-else
        type="button"
        class="primary"
        data-voice-group-destination-resume
        @click="model.addCreated"
      >
        加入已创建的歌单
      </button>
    </div>
  </dialog>
</template>
