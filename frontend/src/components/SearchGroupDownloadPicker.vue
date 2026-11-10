<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId } from 'vue';
import { failure, type PlaylistView } from '../services/contracts';
import { useCore } from '../services/core';
import type { SearchGroupDownloadReply, SearchGroupDownloadSort } from '../services/search';
import Icon from './Icon.vue';
import PlaylistCollectList from './PlaylistCollectList.vue';
import '../styles/playlist-collect.css';

const props = defineProps<{ epoch: string; receipt: string; live: () => boolean }>();
const emit = defineEmits<{
  close: [];
  accepted: [count: number, addFailed: boolean];
  directory: [];
  unknown: [];
  liked: [epoch: string, ids: string[]];
}>();
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const title = useId();
const busy = ref(false);
const error = ref('');
const pending = ref(false);
const rows = ref<PlaylistView[]>([]);
const likedReceipt = ref<string | null>(null);
const cursor = ref<number | null>(0);
const preference = 'cumulus.playlist-collect-sort.v1';
const options = [
  { value: 'default', label: '默认排序' },
  { value: 'recently', label: '常用优先' },
] as const;
function savedSort(): SearchGroupDownloadSort {
  try {
    return localStorage.getItem(preference) === 'recently' ? 'recently' : 'default';
  } catch {
    return 'default';
  }
}
const sort = ref<SearchGroupDownloadSort>(savedSort());
const scrollTop = ref(0);
let alive = true;
const live = () => alive && props.live();
const canLoad = computed(() => !pending.value && cursor.value !== null && !busy.value);
onMounted(() => {
  dialog.value?.showModal();
  void read();
});
onBeforeUnmount(() => {
  alive = false;
});

async function read() {
  if (!live() || !canLoad.value) return;
  const page = cursor.value!;
  busy.value = true;
  error.value = '';
  try {
    const response = await core.searchGroupDownload(props.epoch, {
      kind: 'destinations',
      receipt: props.receipt,
      sort: sort.value,
      page,
    });
    if (!live() || response.kind !== 'destinations' || response.page.epoch !== props.epoch) return;
    likedReceipt.value = response.systemLikedReceipt;
    const seen = new Set(rows.value.map((row) => row.id));
    for (const row of response.page.value.playlists) {
      if (row.owned && !row.favorite && !seen.has(row.id)) {
        seen.add(row.id);
        rows.value.push(row);
      }
    }
    const next = response.page.value.nextPage;
    cursor.value = next !== null && next > page ? next : null;
  } catch (issue) {
    if (live()) error.value = failure(issue).message;
  } finally {
    busy.value = false;
  }
}
function selectSort(value: SearchGroupDownloadSort) {
  if (!live() || pending.value || busy.value || value === sort.value) return;
  sort.value = value;
  rows.value = [];
  likedReceipt.value = null;
  cursor.value = 0;
  scrollTop.value = 0;
  try {
    localStorage.setItem(preference, value);
  } catch {
    /* Per visit choice remains. */
  }
  void read();
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
function close() {
  if (busy.value) return;
  if (!pending.value)
    void core
      .searchGroupDownload(props.epoch, { kind: 'cancel', receipt: props.receipt })
      .catch(() => {});
  emit('close');
}

async function afterPlaylist(response: SearchGroupDownloadReply) {
  if (
    !live() ||
    response.kind !== 'playlist' ||
    response.result.epoch !== props.epoch ||
    response.result.receipt !== props.receipt
  )
    return;
  if (!response.result.continuation) {
    error.value = '加入歌单的结果尚未确认。请核对后再继续。';
    return;
  }
  if (response.result.likedIds.length)
    emit('liked', response.result.epoch, response.result.likedIds);
  try {
    const result = await core.searchGroupDownload(props.epoch, {
      kind: 'start_from_playlist',
      receipt: props.receipt,
      continuation: response.result.continuation,
    });
    if (!live()) return;
    if (
      result.kind === 'accepted' &&
      result.result.epoch === props.epoch &&
      result.result.operationId === props.receipt
    )
      emit('accepted', result.result.accepted.length, response.result.addFailed);
    else if (result.kind === 'directory_required') emit('directory');
  } catch {
    if (live()) emit('unknown');
  }
}
type Choice = { kind: 'regular'; id: string } | { kind: 'liked'; receipt: string };
async function submit(choice?: Choice) {
  if (
    !live() ||
    busy.value ||
    (choice &&
      (pending.value ||
        (choice.kind === 'regular'
          ? !rows.value.some((row) => row.id === choice.id)
          : likedReceipt.value !== choice.receipt)))
  )
    return;
  pending.value = true;
  busy.value = true;
  error.value = '';
  try {
    const response = await core.searchGroupDownload(
      props.epoch,
      !choice
        ? { kind: 'reconcile', receipt: props.receipt }
        : choice.kind === 'regular'
          ? { kind: 'choose', receipt: props.receipt, playlistId: choice.id }
          : {
              kind: 'choose_liked',
              receipt: props.receipt,
              destinationReceipt: choice.receipt,
            },
    );
    await afterPlaylist(response);
  } catch (issue) {
    if (live()) error.value = failure(issue).message;
  } finally {
    busy.value = false;
    if (live() && pending.value)
      void nextTick(() =>
        dialog.value?.querySelector<HTMLElement>('[data-search-group-reconcile]')?.focus(),
      );
  }
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-collect-dialog"
    :aria-labelledby="title"
    :aria-busy="busy"
    data-search-group-download-picker
    @cancel.prevent="close"
  >
    <header>
      <h2 :id="title">下载到歌单</h2>
      <button
        type="button"
        class="icon-button collect-close"
        aria-label="关闭歌曲下载"
        :disabled="busy"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div v-if="!pending" class="collect-body">
      <div class="collect-sort-tabs" role="tablist" aria-label="下载歌单排序" @keydown="sortKeys">
        <button
          v-for="option in options"
          :key="option.value"
          :data-collect-sort="option.value"
          type="button"
          role="tab"
          :aria-selected="option.value === sort"
          :tabindex="option.value === sort ? 0 : -1"
          :disabled="busy"
          @click="selectSort(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
      <p v-if="error || busy" class="collect-load" role="status">{{ error || '正在读取歌单…' }}</p>
      <PlaylistCollectList
        :key="sort"
        purpose="download"
        :playlists="rows"
        :include-create="false"
        :system-liked="likedReceipt !== null"
        :disabled="busy"
        :scroll-top="scrollTop"
        selected-id=""
        :loading="busy || !!error || cursor !== null"
        @scroll="scrollTop = $event"
        @near-end="!error && read()"
        @select="(id) => submit({ kind: 'regular', id })"
        @liked="likedReceipt && submit({ kind: 'liked', receipt: likedReceipt })"
      />
      <button
        v-if="cursor !== null && (rows.length || error)"
        type="button"
        class="text-button collect-more"
        :disabled="busy"
        @click="read"
      >
        {{ error ? '重试读取' : '加载更多歌单' }}
      </button>
    </div>
    <div v-else class="collect-feedback">
      <p role="status">{{ error || '正在处理下载到歌单…' }}</p>
      <button
        type="button"
        class="secondary"
        :disabled="busy"
        data-search-group-reconcile
        @click="submit()"
      >
        核对结果
      </button>
    </div>
  </dialog>
</template>
