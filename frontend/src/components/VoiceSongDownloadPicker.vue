<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import type { VoiceSongDownloadModel } from '../composables/useVoiceSongDownload';
import { failure, type PlaylistView } from '../services/contracts';
import { useCore } from '../services/core';
import type { DownloadTab } from '../services/downloads';
import { playlistLength } from '../services/playlists';
import Icon from './Icon.vue';
import PlaylistCollectList from './PlaylistCollectList.vue';
import '../styles/playlist-collect.css';

const props = defineProps<{ model: VoiceSongDownloadModel; epoch: string }>();
const emit = defineEmits<{ downloads: [tab: DownloadTab]; expired: [] }>();
const { stage, busy, message } = props.model;
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const closeButton = ref<HTMLButtonElement>();
const nameInput = ref<HTMLInputElement>();
const name = ref('');
const privateList = ref(false);
const selected = ref('');
const title = computed(() =>
  stage.value.kind === 'create'
    ? '创建新歌单'
    : stage.value.kind === 'pick'
      ? '下载到歌单'
      : '下载歌曲',
);
const canClose = computed(() => !busy.value || stage.value.kind === 'checking');
const invalid = computed(
  () => !name.value.trim() || /[@#]/.test(name.value) || playlistLength(name.value.trim()) > 40,
);

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
  const order = sort.value;
  const state = pages[order];
  const next = state.next;
  if (
    !alive ||
    stage.value.kind !== 'pick' ||
    busy.value ||
    state.loading ||
    next === null ||
    (automatic && state.error)
  )
    return;
  const owner = props.epoch;
  state.loading = true;
  try {
    const reply = await core.playlistCollectPage(owner, order, next);
    if (!alive || props.epoch !== owner || reply.epoch !== owner) return;
    const rows = next === 0 ? [] : state.rows.slice();
    const seen = new Set(rows.map((playlist) => playlist.id));
    for (const row of reply.value.playlists) {
      if (!row.owned || row.favorite || seen.has(row.id)) continue;
      seen.add(row.id);
      rows.push(row);
    }
    state.rows = rows;
    state.next = reply.value.nextPage;
    state.error = '';
  } catch (error) {
    if (!alive || props.epoch !== owner) return;
    const issue = failure(error);
    state.error = issue.message;
    if (issue.code === 'profile_changed' || issue.code === 'session_expired') emit('expired');
  } finally {
    if (alive) state.loading = false;
  }
}
function selectSort(value: Sort) {
  if (value === sort.value || busy.value) return;
  sort.value = value;
  try {
    localStorage.setItem(preference, value);
  } catch {
    /* Keep the current session order. */
  }
  if (!current.value.error && current.value.next === 0) void load();
}
function sortKeys(event: KeyboardEvent) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key) || busy.value) return;
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
  () => stage.value.kind,
  async (kind) => {
    if (kind === 'pick') void load(true);
    if (kind === 'create') {
      name.value = '';
      privateList.value = false;
    }
    await nextTick();
    if (!alive || stage.value.kind !== kind) return;
    if (kind === 'create') nameInput.value?.focus();
    else if (kind === 'pick') dialog.value?.querySelector<HTMLElement>('.collect-list')?.focus();
    else closeButton.value?.focus();
  },
  { immediate: true },
);

function choose(playlistId: string) {
  selected.value = playlistId;
  void props.model.selectPlaylist(playlistId);
}
function openDownloads() {
  const current = stage.value;
  if (current.kind !== 'notice' || !current.downloadTab) return;
  props.model.close();
  emit('downloads', current.downloadTab);
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-collect-dialog"
    data-voice-download-picker
    :data-voice-download-stage="stage.kind"
    aria-labelledby="voice-download-picker-title"
    :aria-busy="busy"
    @cancel.prevent="model.close"
  >
    <header>
      <h2 id="voice-download-picker-title">{{ title }}</h2>
      <button
        ref="closeButton"
        type="button"
        class="icon-button collect-close"
        aria-label="关闭歌曲下载"
        :disabled="!canClose"
        @click="model.close"
      >
        <Icon name="close" />
      </button>
    </header>
    <p v-if="stage.kind !== 'idle'" class="sr-only">下载 {{ stage.opening.title }}</p>
    <p v-if="message" class="collect-feedback" role="status">{{ message }}</p>
    <p v-if="stage.kind === 'checking'" class="collect-feedback" role="status">
      正在确认歌曲的下载状态…
    </p>
    <div v-else-if="stage.kind === 'pick'" class="collect-body">
      <div class="collect-sort-tabs" role="tablist" aria-label="下载歌单排序" @keydown="sortKeys">
        <button
          v-for="option in sorts"
          :key="option.value"
          type="button"
          role="tab"
          :data-collect-sort="option.value"
          :aria-selected="sort === option.value"
          :tabindex="sort === option.value ? 0 : -1"
          :disabled="busy"
          @click="selectSort(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
      <div v-if="current.error || current.loading" class="collect-load" role="status">
        {{ current.error || '正在读取歌单…' }}
        <button
          v-if="current.error"
          type="button"
          class="text-button"
          :disabled="busy || current.loading"
          @click="load()"
        >
          重试
        </button>
      </div>
      <PlaylistCollectList
        :key="sort"
        purpose="download"
        :playlists="current.rows"
        :disabled="busy"
        :selected-id="selected"
        :scroll-top="current.top"
        :loading="current.loading || !!current.error || current.next !== null"
        @scroll="current.top = $event"
        @near-end="load(true)"
        @select="choose"
        @create="model.openCreate"
      />
      <button
        v-if="current.next !== null && current.next !== 0 && !current.error"
        type="button"
        class="text-button collect-more"
        :disabled="busy || current.loading"
        @click="load()"
      >
        加载更多歌单
      </button>
    </div>
    <form
      v-else-if="stage.kind === 'create'"
      class="collect-create"
      @submit.prevent="model.submitCreate(name, privateList)"
    >
      <fieldset :disabled="busy">
        <label
          >歌单名称<input
            ref="nameInput"
            v-model="name"
            aria-label="歌单名称"
            aria-describedby="voice-download-name-length"
            :aria-invalid="!!name && invalid"
            placeholder="输入歌单标题"
            maxlength="80"
          />
          <small id="voice-download-name-length"
            >{{ playlistLength(name.trim()) }} / 40 · 中文计 2 个字符</small
          >
        </label>
        <label class="private-checkbox"
          ><input v-model="privateList" type="checkbox" />设为私密歌单</label
        >
        <p v-if="!privateList" class="muted">公开歌单可被其他用户查看和收藏。</p>
        <p v-if="name && invalid" class="form-warning">
          名称不能包含 @ 或 #，并请保持在字数上限内。
        </p>
      </fieldset>
      <footer>
        <button type="submit" class="primary" :disabled="invalid || busy">
          {{ busy ? '正在创建…' : '创建' }}
        </button>
      </footer>
    </form>
    <div v-else-if="stage.kind === 'recover'" class="collect-feedback">
      <button
        v-if="stage.canResumeAdd"
        type="button"
        class="primary"
        :disabled="busy"
        @click="model.resumeAdd"
      >
        加入已创建的歌单
      </button>
      <button v-else type="button" class="secondary" :disabled="busy" @click="model.reconcile">
        核对结果
      </button>
    </div>
    <div v-else-if="stage.kind === 'notice' && stage.downloadTab" class="collect-feedback">
      <button type="button" class="secondary" :disabled="busy" @click="openDownloads">
        打开下载管理
      </button>
    </div>
  </dialog>
</template>
