<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import {
  failure,
  type AccountView,
  type PlaylistView,
  type TrackView,
} from '../../services/contracts';
import { playlistDownloadTracks, playlistLink } from '../../services/playlistMenu';
import type { PlaylistManager } from '../../composables/usePlaylists';
import type { DownloadModel } from '../../composables/useDownloads';
import type { PlayerModel } from '../../composables/usePlayer';
import { useCore } from '../../services/core';
import Icon from '../Icon.vue';
import ModalTransition from '../ModalTransition.vue';
import TrackShareDialog from '../TrackShareDialog.vue';
import DownloadDialog from '../DownloadDialog.vue';
import PlaylistMutationDialog from './PlaylistMutationDialog.vue';
import '../../styles/playlist-menu.css';

const props = defineProps<{
  account: AccountView;
  manager: PlaylistManager;
  downloads: DownloadModel;
  player: PlayerModel;
}>();
const emit = defineEmits<{ expired: []; started: [] }>();
const client = useCore();
const shared = shallowRef<PlaylistView>();
const mutation = shallowRef<{ playlist: PlaylistView; subscribe: boolean }>();
const download = shallowRef<PlaylistView>();
const tracks = shallowRef<TrackView[]>();
const readDialog = ref<HTMLDialogElement>();
const reading = ref(false),
  readError = ref(''),
  progress = ref('');
const copied = shallowRef<PlaylistView>();
const queued = shallowRef<PlaylistView>();
const adding = ref(false);
const copying = ref(false),
  retrying = ref(false),
  message = ref(''),
  failed = ref(false);
const downloadOpen = computed(() => !!download.value);
const busy = computed(() => reading.value || copying.value || adding.value);
let returnFocus: HTMLElement | null = null;
let alive = true,
  sequence = 0,
  timer: ReturnType<typeof setTimeout> | undefined;
watch(readDialog, (dialog) => dialog?.showModal());
onBeforeUnmount(() => {
  alive = false;
  sequence++;
  clearTimeout(timer);
});
const fallback = () =>
  document.querySelector<HTMLElement>('.playlist-library-page .page-heading button') ??
  document.querySelector<HTMLElement>('.sidebar-navigation [aria-current="page"]') ??
  document.querySelector<HTMLElement>('.sidebar-scroll');
const target = () => returnFocus;
function restoreFocus() {
  (returnFocus?.isConnected ? returnFocus : fallback())?.focus({ preventScroll: true });
}
function reset() {
  sequence++;
  clearTimeout(timer);
  message.value = '';
  failed.value = false;
  copying.value = false;
  retrying.value = false;
  adding.value = false;
  queued.value = undefined;
}
function show(list: PlaylistView, action: string) {
  reset();
  returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  if (action === 'share' && playlistLink(list)) shared.value = list;
  else if (action === 'copy' && playlistLink(list)) {
    copied.value = list;
    void copy();
  } else if (action === 'remove' || action === 'subscribe')
    mutation.value = { playlist: list, subscribe: action === 'subscribe' };
  else if (action === 'download') {
    download.value = list;
    void read();
  } else if (action === 'next' || action === 'queue') {
    queued.value = list;
    void addToQueue();
  }
}
async function addToQueue() {
  if (
    !queued.value ||
    adding.value ||
    !props.player.ready.value ||
    props.player.busy.value ||
    props.player.queueBusy.value
  )
    return;
  const current = sequence,
    epoch = props.account.epoch,
    list = queued.value;
  adding.value = true;
  retrying.value = !!document.activeElement?.closest('.playlist-action-feedback');
  failed.value = false;
  message.value = '正在添加歌单歌曲…';
  try {
    const result = await props.player.queuePlaylist(list.id);
    if (!alive || sequence !== current || props.account.epoch !== epoch) return;
    message.value = result
      ? `已添加至播放列表${list.trackCount > 10000 ? '（播放列表最多保留 10000 首）' : ''}`
      : '播放操作已更新，本次添加已取消';
    if (retrying.value) restoreFocus();
    timer = setTimeout(() => {
      message.value = '';
    }, 3500);
  } catch (value) {
    if (!alive || sequence !== current || props.account.epoch !== epoch) return;
    const issue = failure(value);
    message.value = issue.code === 'stale' ? '播放操作已更新，本次添加已取消' : issue.message;
    failed.value = true;
    if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
  } finally {
    if (sequence === current) {
      adding.value = false;
      retrying.value = false;
    }
  }
}
function retry() {
  if (queued.value) void addToQueue();
  else void copy();
}
function dismiss() {
  reset();
  restoreFocus();
}
async function copy() {
  const link = copied.value && playlistLink(copied.value);
  if (!link || copying.value) return;
  const current = sequence;
  copying.value = true;
  retrying.value = !!document.activeElement?.closest('.playlist-action-feedback');
  failed.value = false;
  message.value = '正在复制链接…';
  try {
    await client.copyText(link);
    if (!alive || current !== sequence) return;
    message.value = '链接已复制';
    if (retrying.value) restoreFocus();
    timer = setTimeout(() => {
      message.value = '';
    }, 3000);
  } catch {
    if (!alive || current !== sequence) return;
    message.value = '复制链接失败，请重试';
    failed.value = true;
  } finally {
    if (current === sequence) {
      copying.value = false;
      retrying.value = false;
    }
  }
}
function closeDownload() {
  sequence++;
  download.value = undefined;
  readError.value = '';
  tracks.value = undefined;
  reading.value = false;
}
async function read() {
  if (reading.value || !download.value) return;
  const current = ++sequence,
    epoch = props.account.epoch;
  const active = () =>
    alive &&
    sequence === current &&
    props.account.epoch === epoch &&
    props.account.status === 'authenticated';
  reading.value = true;
  progress.value = '正在读取歌单歌曲…';
  try {
    const value = await playlistDownloadTracks(
      client,
      epoch,
      download.value.id,
      active,
      (count, total) => {
        progress.value = `正在读取歌单歌曲：${count} / ${total}`;
      },
    );
    if (!active()) return;
    if (!value.length) {
      readError.value = '歌单中没有可下载的在线歌曲。';
      return;
    }
    tracks.value = value;
    readError.value = '';
  } catch (value) {
    if (!alive || current !== sequence) return;
    const issue = failure(value);
    readError.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
  } finally {
    if (current === sequence) reading.value = false;
  }
}
defineExpose({ show, busy, downloadOpen });
</script>
<template>
  <ModalTransition :return-focus="target" :fallback-focus="fallback">
    <TrackShareDialog v-if="shared" :playlist="shared" @close="shared = undefined" />
    <PlaylistMutationDialog
      v-else-if="mutation"
      :playlist="mutation.playlist"
      :subscribe="mutation.subscribe"
      :manager="manager"
      @close="mutation = undefined"
    />
    <DownloadDialog
      v-else-if="download && tracks"
      :model="downloads"
      :target="{ kind: 'start', tracks }"
      @close="closeDownload"
      @started="emit('started')"
    />
    <dialog
      v-else-if="download"
      ref="readDialog"
      class="playlist-dialog playlist-download-read"
      aria-labelledby="playlist-download-title"
      :aria-busy="reading"
      @cancel.prevent="closeDownload"
    >
      <header>
        <h2 id="playlist-download-title">下载歌单</h2>
        <button class="icon-button" aria-label="关闭歌单下载" @click="closeDownload">
          <Icon name="close" />
        </button>
      </header>
      <div class="playlist-mutation-body">
        <strong>{{ download.name }}</strong>
        <p v-if="reading" role="status" class="muted">{{ progress }}</p>
        <p v-if="readError" role="alert" class="playlist-operation-error">{{ readError }}</p>
      </div>
      <footer>
        <button class="download-button" @click="closeDownload">取消</button
        ><button v-if="readError" class="primary" :aria-disabled="reading" @click="read">
          {{ reading ? '正在读取…' : '重新读取' }}
        </button>
      </footer>
    </dialog>
  </ModalTransition>
  <Transition name="menu"
    ><div
      v-if="message"
      class="track-action-feedback playlist-action-feedback"
      :class="{ failed }"
      role="status"
      :aria-busy="copying || adding"
    >
      {{ message
      }}<button
        v-if="failed || retrying"
        class="text-button"
        :aria-disabled="copying || adding || (!!queued && player.busy.value)"
        @click="retry"
      >
        {{ copying || adding ? '正在重试…' : '重试' }}</button
      ><button v-if="failed" class="text-button" @click="dismiss">关闭</button>
    </div></Transition
  >
</template>
