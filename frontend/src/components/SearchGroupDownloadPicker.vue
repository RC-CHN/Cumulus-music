<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId } from 'vue';
import { failure, type PlaylistView } from '../services/contracts';
import { useCore } from '../services/core';
import { playlistLength } from '../services/playlists';
import type {
  SearchGroupDownloadCommand,
  SearchGroupDownloadPlaylistResult,
  SearchGroupDownloadSort,
} from '../services/search';
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
const nameCount = useId();
const busy = ref(false);
const error = ref('');
const pending = ref(false);
const rows = ref<PlaylistView[]>([]);
const likedReceipt = ref<string | null>(null);
const canCreate = ref(false);
const createForm = ref(false);
const creation = ref<'none' | 'checking' | 'ready' | 'rejected'>('none');
const name = ref('');
const privatePlaylist = ref(false);
const nameInput = ref<HTMLInputElement>();
const validName = computed(() => {
  const value = name.value.trim();
  return !!value && playlistLength(value) <= 40 && !/[@#]/u.test(value);
});
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
    canCreate.value = response.canCreate;
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
  canCreate.value = false;
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

function openCreate() {
  if (!live() || busy.value || pending.value || !canCreate.value) return;
  name.value = '';
  privatePlaylist.value = false;
  error.value = '';
  createForm.value = true;
  void nextTick(() => nameInput.value?.focus());
}
function back() {
  if (busy.value || !createForm.value) return;
  createForm.value = false;
  error.value = '';
  void nextTick(() =>
    dialog.value?.querySelector<HTMLElement>('[aria-label="创建新歌单"]')?.focus(),
  );
}
function dismiss() {
  if (createForm.value) back();
  else close();
}

async function afterPlaylist(result: SearchGroupDownloadPlaylistResult) {
  if (!live() || result.epoch !== props.epoch || result.receipt !== props.receipt) return;
  if (!result.continuation) {
    error.value = '加入歌单的结果尚未确认。请核对后再继续。';
    return;
  }
  if (result.likedIds.length) emit('liked', result.epoch, result.likedIds);
  try {
    const reply = await core.searchGroupDownload(props.epoch, {
      kind: 'start_from_playlist',
      receipt: props.receipt,
      continuation: result.continuation,
    });
    if (!live()) return;
    if (
      reply.kind === 'accepted' &&
      reply.result.epoch === props.epoch &&
      reply.result.operationId === props.receipt
    )
      emit('accepted', reply.result.accepted.length, result.addFailed);
    else if (reply.kind === 'directory_required') emit('directory');
  } catch {
    if (live()) emit('unknown');
  }
}

async function submitCreation(action: 'create' | 'readback' | 'add') {
  if (
    !live() ||
    busy.value ||
    (action === 'create' && (!createForm.value || !validName.value || !canCreate.value)) ||
    (action === 'add' && creation.value !== 'ready') ||
    (action === 'readback' && creation.value !== 'checking')
  )
    return;
  const receipt = props.receipt;
  const command: SearchGroupDownloadCommand =
    action === 'create'
      ? {
          kind: 'create_playlist',
          receipt,
          name: name.value.trim(),
          privatePlaylist: privatePlaylist.value,
        }
      : action === 'readback'
        ? { kind: 'creation_status', receipt, reconcile: true }
        : { kind: 'add_created', receipt };
  createForm.value = false;
  pending.value = busy.value = true;
  creation.value = 'checking';
  error.value = '';
  try {
    const reply = await core.searchGroupDownload(props.epoch, command);
    if (
      !live() ||
      reply.kind !== 'creation' ||
      reply.result.epoch !== props.epoch ||
      reply.result.receipt !== receipt
    )
      return;
    const result = reply.result;
    if (result.add) {
      if (!result.add.continuation && (result.add.addFailed || result.add.phase === 'rejected')) {
        creation.value = 'rejected';
        error.value = '加入歌单未成功，未开始下载。';
      } else await afterPlaylist(result.add);
    } else if (result.canAdd) {
      creation.value = 'ready';
      error.value = '已核对新歌单。请确认加入歌曲后继续下载。';
    } else if (result.createPhase === 'rejected') {
      creation.value = 'rejected';
      error.value = '创建歌单未成功，没有加入或下载歌曲。';
    } else if (result.createPhase === 'confirmed' && !result.playlistId) {
      creation.value = 'rejected';
      error.value = '已核对，但原请求没有返回可核实的歌单 ID。没有加入或下载歌曲。';
    } else error.value = '创建歌单的结果尚未确认。请核对后再继续。';
  } catch (issue) {
    if (live()) {
      const value = failure(issue);
      error.value = value.message;
      if (action === 'create' && value.code === 'download_create_not_started') {
        pending.value = false;
        creation.value = 'none';
        createForm.value = true;
      }
    }
  } finally {
    busy.value = false;
    if (live())
      void nextTick(() => {
        if (createForm.value) nameInput.value?.focus();
        else
          dialog.value
            ?.querySelector<HTMLElement>(
              creation.value === 'ready'
                ? '[data-search-group-creation-add]'
                : creation.value === 'rejected'
                  ? '.collect-close'
                  : '[data-search-group-creation-reconcile]',
            )
            ?.focus();
      });
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
    if (response.kind === 'playlist') await afterPlaylist(response.result);
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
    @cancel.prevent="dismiss"
  >
    <header>
      <button
        v-if="createForm"
        type="button"
        class="icon-button collect-back"
        aria-label="返回下载歌单"
        @click="back"
      >
        <Icon name="back" />
      </button>
      <h2 :id="title">{{ createForm ? '创建新歌单' : '下载到歌单' }}</h2>
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
    <div v-if="!pending && !createForm" class="collect-body">
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
        :include-create="canCreate"
        :system-liked="likedReceipt !== null"
        :disabled="busy"
        :scroll-top="scrollTop"
        selected-id=""
        :loading="busy || !!error || cursor !== null"
        @scroll="scrollTop = $event"
        @near-end="!error && read()"
        @select="(id) => submit({ kind: 'regular', id })"
        @liked="likedReceipt && submit({ kind: 'liked', receipt: likedReceipt })"
        @create="openCreate"
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
    <form v-else-if="createForm" class="collect-create" @submit.prevent="submitCreation('create')">
      <label>
        歌单名称<input
          ref="nameInput"
          v-model="name"
          aria-label="歌单名称"
          :aria-describedby="nameCount"
          :aria-invalid="!!name && !validName"
          placeholder="输入歌单标题"
          maxlength="80"
        />
        <small :id="nameCount">{{ playlistLength(name.trim()) }} / 40</small>
      </label>
      <label class="private-checkbox"
        ><input v-model="privatePlaylist" type="checkbox" />设为私密歌单</label
      >
      <p v-if="!privatePlaylist" class="muted">公开歌单可被其他用户查看和收藏。</p>
      <p v-if="error || (name && !validName)" class="form-warning" role="status">
        {{ error || '名称不能包含 @ 或 #，并请保持在字数上限内。' }}
      </p>
      <footer>
        <button class="primary" type="submit" :disabled="!validName || busy">创建</button>
      </footer>
    </form>
    <div v-else-if="creation !== 'none'" class="collect-feedback collect-create-feedback">
      <p role="status">{{ error || '正在处理新歌单…' }}</p>
      <button
        v-if="creation === 'checking'"
        type="button"
        class="secondary"
        :disabled="busy"
        data-search-group-creation-reconcile
        @click="submitCreation('readback')"
      >
        核对结果
      </button>
      <button
        v-else-if="creation === 'ready'"
        type="button"
        class="primary"
        :disabled="busy"
        data-search-group-creation-add
        @click="submitCreation('add')"
      >
        加入已创建的歌单
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
