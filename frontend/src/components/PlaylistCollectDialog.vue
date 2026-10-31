<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import type { PlaylistManager } from '../composables/usePlaylists';
import { failure, type PlaylistView, type TrackView } from '../services/contracts';
import { useCore } from '../services/core';
import { playlistLength, playlistPending, type PlaylistMutationView } from '../services/playlists';
import Icon from './Icon.vue';
import PlaylistCollectList from './PlaylistCollectList.vue';
import '../styles/playlist-collect.css';

const props = defineProps<{
  manager: PlaylistManager;
  epoch: string;
  tracks: TrackView[];
  playlists: PlaylistView[];
  selectionSummary?: { total: number; unsupported: number; duplicates: number };
}>();
const emit = defineEmits<{ close: []; expired: [] }>();
const client = useCore();
const dialog = ref<HTMLDialogElement>();
const nameInput = ref<HTMLInputElement>();
const { busy, error, result } = props.manager;
const interrupted = shallowRef<PlaylistMutationView>();
const receipt = computed(() => result.value ?? interrupted.value);
const lists = shallowRef(props.playlists);
const loading = ref(false),
  loadError = ref('');
const creating = ref(false),
  name = ref(''),
  privateList = ref(true);
const selected = ref(''),
  created = ref(''),
  scrollTop = ref(0);
const own = computed(() => lists.value.filter((p) => p.owned && !p.favorite));
const pending = computed(() => !!receipt.value && playlistPending(receipt.value.phase));
const invalid = computed(
  () => !name.value.trim() || /[@#]/.test(name.value) || playlistLength(name.value.trim()) > 40,
);
let alive = true;
onBeforeUnmount(() => {
  alive = false;
});
onMounted(() => {
  props.manager.reset();
  dialog.value?.showModal();
  void reload();
});
async function reload() {
  if (loading.value) return;
  const retry = dialog.value?.querySelector('.collect-load button');
  const focused = retry && document.activeElement === retry;
  loading.value = true;
  try {
    const response = await client.playlists();
    if (!alive || response.epoch !== props.epoch) return;
    lists.value = response.value;
    loadError.value = '';
    if (focused) {
      await nextTick();
      dialog.value?.querySelector<HTMLElement>('.collect-list')?.focus({ preventScroll: true });
    }
  } catch (e) {
    if (!alive) return;
    const issue = failure(e);
    loadError.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
  } finally {
    if (alive) loading.value = false;
  }
}
function close() {
  if (!busy.value) emit('close');
}
function startCreate() {
  if (busy.value || pending.value) return;
  props.manager.reset();
  created.value = '';
  creating.value = true;
}
async function back() {
  if (busy.value || pending.value) return;
  props.manager.reset();
  creating.value = false;
  await reload();
}
function entered() {
  if (creating.value) nameInput.value?.focus();
  else dialog.value?.querySelector<HTMLElement>('.collect-list')?.focus({ preventScroll: true });
}
async function perform(
  operation: () => Promise<PlaylistMutationView | undefined>,
  kind: 'create' | 'tracks',
  id?: string,
) {
  const before = new Set(props.manager.operations.value.map((entry) => entry.operationId));
  interrupted.value = undefined;
  const value = await operation();
  if (!alive) return;
  if (!value) {
    // The core can persist an operation and then fail its authoritative readback.
    // Recover only this attempt's new journal entry, never a previous creation.
    interrupted.value = props.manager.operations.value.find(
      (entry) =>
        !before.has(entry.operationId) &&
        entry.kind === kind &&
        playlistPending(entry.phase) &&
        (kind === 'create' ? entry.label === name.value.trim() : entry.playlistId === id),
    );
  }
  return value;
}
async function collected(value: PlaylistMutationView | undefined) {
  if (!alive || !value || !props.manager.saved(value.phase)) return;
  if (value.kind === 'create' && value.playlistId) {
    // A confirmed creation is retained if adding the songs later fails. Never
    // create a second playlist when retrying the second half of this flow.
    created.value = value.playlistId;
    await collect(value.playlistId);
  } else emit('close');
}
async function collect(id: string) {
  if (!alive || busy.value || pending.value) return;
  selected.value = id;
  await collected(
    await perform(
      () =>
        props.manager.addTracks(
          id,
          props.tracks.map((track) => track.id),
        ),
      'tracks',
      id,
    ),
  );
}
async function submit() {
  if (busy.value || pending.value || (!created.value && invalid.value)) return;
  if (created.value) await collect(created.value);
  else
    await collected(
      await perform(
        () =>
          props.manager.mutate({
            kind: 'create',
            name: name.value.trim(),
            private: privateList.value,
          }),
        'create',
      ),
    );
}
async function reconcile() {
  if (!receipt.value || busy.value) return;
  await collected(await props.manager.reconcile(receipt.value.operationId));
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-collect-dialog"
    aria-labelledby="collect-title"
    :aria-busy="busy"
    @cancel.prevent="close"
  >
    <header>
      <button
        v-if="creating"
        class="icon-button collect-back"
        aria-label="返回收藏歌单列表"
        :disabled="busy || pending"
        @click="back"
      >
        <Icon name="back" />
      </button>
      <h2 id="collect-title">{{ creating ? '创建新歌单' : '收藏到歌单' }}</h2>
      <button
        class="icon-button collect-close"
        aria-label="关闭歌单操作"
        :disabled="busy"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <p class="sr-only">
      {{ tracks.length === 1 ? `收藏${tracks[0]?.name}` : `收藏${tracks.length}首歌曲` }}
    </p>
    <p v-if="selectionSummary" class="collect-selection-summary" role="status">
      播放列表共 {{ selectionSummary.total }} 项，将收藏 {{ tracks.length }} 首歌曲。<template
        v-if="selectionSummary.unsupported"
        >{{ selectionSummary.unsupported }} 项声音或未匹配音乐无法收藏到歌单。</template
      ><template v-if="selectionSummary.duplicates"
        >{{ selectionSummary.duplicates }} 项重复歌曲已合并。</template
      >
    </p>
    <div v-if="error && !pending" class="collect-feedback error-state" role="alert">
      {{ error }}
    </div>
    <div
      v-if="receipt && !manager.saved(receipt.phase)"
      class="collect-feedback mutation-result"
      :data-phase="receipt.phase"
      role="status"
    >
      {{ receipt.message }}
      <button v-if="pending" class="text-button" :aria-disabled="busy" @click="reconcile">
        核对结果
      </button>
    </div>
    <Transition name="collect-step" mode="out-in" @after-enter="entered">
      <form v-if="creating" key="create" class="collect-create" @submit.prevent="submit">
        <fieldset :disabled="busy || pending || !!created">
          <label
            >歌单名称<input
              ref="nameInput"
              v-model="name"
              aria-label="歌单名称"
              aria-describedby="collect-name-count"
              :aria-invalid="!!name && invalid"
              placeholder="输入歌单标题"
              maxlength="80"
            />
            <small
              id="collect-name-count"
              :class="{ 'form-warning': playlistLength(name.trim()) > 40 }"
            >
              {{ playlistLength(name.trim()) }} / 40 · 中文计 2 个字符
            </small></label
          >
          <label class="private-checkbox"
            ><input
              v-model="privateList"
              type="checkbox"
              aria-label="设为私密歌单"
            />设为私密歌单</label
          >
          <p v-if="!privateList" class="muted">公开歌单可被其他用户查看和收藏。</p>
          <p v-if="name && invalid" class="form-warning">
            名称不能包含 @ 或 #，并请保持在字数上限内。
          </p>
        </fieldset>
        <p v-if="created" class="muted" role="status">歌单已创建，继续将歌曲收藏到这个歌单。</p>
        <footer>
          <button
            type="submit"
            class="primary"
            :disabled="!created && invalid"
            :aria-disabled="busy || pending"
          >
            {{ busy ? '正在收藏…' : created ? '继续收藏' : '创建并收藏' }}
          </button>
        </footer>
      </form>
      <div v-else key="list" class="collect-body">
        <div v-if="loadError || loading" class="collect-load" role="status">
          <span>{{ loadError || '正在刷新歌单…' }}</span>
          <button v-if="loadError" class="text-button" :aria-disabled="loading" @click="reload">
            {{ loading ? '正在重试…' : '重试' }}
          </button>
        </div>
        <PlaylistCollectList
          :playlists="own"
          :disabled="busy || pending"
          :selected-id="selected"
          :scroll-top="scrollTop"
          :loading="loading || !!loadError"
          @scroll="scrollTop = $event"
          @select="collect"
          @create="startCreate"
        />
      </div>
    </Transition>
    <p v-if="busy" class="collect-status" role="status">正在收藏，请稍候…</p>
  </dialog>
</template>
