<script setup lang="ts">
import MenuTransition from './MenuTransition.vue';
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import { failure, type TrackView, type PlaylistView } from '../services/contracts';
import type { PlayerModel } from '../composables/usePlayer';
import type { MenuItem } from '../services/menu';
import {
  queueSongMenuItems,
  songLink,
  type QueueMenuAction,
  type QueueTrackMenuRequest,
  type TrackMenuRequest,
} from '../services/trackMenu';
import { useCore } from '../services/core';
import ActionMenu from './ActionMenu.vue';
import ModalTransition from './ModalTransition.vue';
import TrackShareDialog from './TrackShareDialog.vue';
const props = defineProps<{ player: PlayerModel }>();
const emit = defineEmits<{
  collect: [track: TrackView];
  download: [track: TrackView];
  comments: [track: TrackView];
  remove: [track: TrackView, playlist: PlaylistView];
  manage: [track: TrackView, playlist: PlaylistView];
  queueAction: [key: QueueMenuAction, input: QueueTrackMenuRequest];
}>();
const client = useCore(),
  menu = ref<InstanceType<typeof ActionMenu>>();
const request = shallowRef<TrackMenuRequest>(),
  playlist = shallowRef<PlaylistView>();
const queueRequest = shallowRef<QueueTrackMenuRequest>();
const shared = shallowRef<TrackView>(),
  message = ref(''),
  failed = ref(false),
  copying = ref(false),
  retrying = ref(false);
let returnFocus: HTMLElement | undefined;
let sequence = 0,
  timer: ReturnType<typeof setTimeout> | undefined;
const items = computed<MenuItem[]>(() => {
  const queue = queueRequest.value;
  if (queue) {
    if (!queue.live()) return [];
    return queueSongMenuItems(
      queue.info,
      queue.tracks.length > 1,
      props.player.playing.value && props.player.view.value?.entryId === queue.info.entryId,
      !props.player.ready.value || props.player.busy.value || props.player.queueBusy.value,
    ).map((item) =>
      item.key === 'collect' ? { ...item, disabled: queue.collectionLocked() } : item,
    );
  }
  const input = request.value;
  const track = input?.track;
  if (!track || (input?.episode && !input.episode.live())) return [];
  const selection = input.episode?.selection;
  const available = !track.missing && !track.local;
  const canPlay = selection
    ? !selection.allCopyrightBlocked
    : available && !input.playUnavailable && !input.episode?.copyrightBlocked;
  const link = !selection && !input.episode?.copyrightBlocked && songLink(track);
  return [
    {
      key: 'play',
      label: '播放',
      icon: 'play',
      disabled: !canPlay || !props.player.ready.value || props.player.busy.value,
    },
    {
      key: 'next',
      label: '下一首播放',
      icon: 'next',
      disabled:
        !canPlay ||
        !props.player.ready.value ||
        props.player.busy.value ||
        (!!request.value?.queueNext && props.player.queueBusy.value) ||
        props.player.view.value?.fm,
      hint: props.player.view.value?.fm ? '私人漫游期间请先切换到普通播放队列' : undefined,
    },
    ...(!track.local && !selection
      ? [{ key: 'comments', label: '查看评论', icon: 'comment' }]
      : []),
    ...(!selection
      ? [
          { key: 'main-divider', label: '', separator: true },
          { key: 'collect', label: '收藏', icon: 'star', disabled: !available },
        ]
      : []),
    ...(!input?.episode
      ? [{ key: 'download', label: '下载', icon: 'download', disabled: !available }]
      : []),
    ...(available && link
      ? [
          { key: 'share', label: '分享', icon: 'share' },
          { key: 'copy', label: '复制链接', icon: 'copy' },
        ]
      : []),
    ...(playlist.value?.owned && !playlist.value.favorite
      ? [
          { key: 'edit-divider', label: '', separator: true },
          { key: 'remove', label: '从歌单中删除', icon: 'trash' },
          { key: 'manage', label: '调整歌曲顺序…', icon: 'list' },
        ]
      : []),
  ];
});
function clear() {
  sequence++;
  clearTimeout(timer);
  message.value = '';
  failed.value = false;
  copying.value = false;
  retrying.value = false;
}
function dismiss() {
  clear();
  if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
}
function close() {
  menu.value?.close();
  request.value = undefined;
  queueRequest.value = undefined;
  shared.value = undefined;
  clear();
}
watch(
  () => queueRequest.value?.live(),
  (live) => {
    if (live === false) close();
  },
);
watch(
  () => request.value?.episode?.live(),
  (live) => {
    if (live === false) close();
  },
);
onBeforeUnmount(close);
async function show(input: TrackMenuRequest, source?: PlaylistView) {
  clear();
  queueRequest.value = undefined;
  request.value = input;
  playlist.value = source;
  const event = input.event;
  const scope = event.currentTarget instanceof HTMLElement ? event.currentTarget : undefined;
  const focused =
    document.activeElement instanceof HTMLElement && scope?.contains(document.activeElement)
      ? document.activeElement
      : undefined;
  const trigger =
    scope?.closest('button') ??
    focused ??
    scope?.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]') ??
    scope;
  returnFocus = trigger;
  const box = trigger?.getBoundingClientRect();
  const pointer =
    event instanceof MouseEvent &&
    event.type === 'contextmenu' &&
    (event.clientX !== 0 || event.clientY !== 0);
  await nextTick();
  if (request.value !== input || (input.episode && !input.episode.live())) return;
  await menu.value?.showAt(
    pointer ? event.clientX : (box?.left ?? 8),
    pointer ? event.clientY : (box?.bottom ?? 8),
    trigger,
  );
}
async function showQueue(input: QueueTrackMenuRequest) {
  clear();
  menu.value?.close();
  request.value = undefined;
  playlist.value = undefined;
  queueRequest.value = input;
  returnFocus = input.anchor;
  const attempt = sequence;
  await nextTick();
  if (attempt !== sequence || !input.live()) return;
  if (input.point) await menu.value?.showAt(input.point.x, input.point.y, input.anchor);
  else await menu.value?.showBelow(input.anchor);
}
function feedback(text: string, error = false) {
  message.value = text;
  failed.value = error;
  clearTimeout(timer);
  if (!error)
    timer = setTimeout(() => {
      message.value = '';
    }, 3000);
}
async function copy() {
  const queue = queueRequest.value;
  if (queue && !queue.live()) return;
  const episode = request.value?.episode;
  if (episode && (!episode.live() || episode.selection || episode.copyrightBlocked)) return;
  const track = queue?.info.track ?? request.value?.track;
  const link = track && songLink(track);
  if (!link || copying.value) return;
  const attempt = sequence;
  retrying.value = !!document.activeElement?.closest('.track-action-feedback');
  copying.value = true;
  message.value = '正在复制链接…';
  failed.value = false;
  try {
    await client.copyText(link);
    if (attempt === sequence) {
      const retryFocused = document.activeElement?.closest('.track-action-feedback');
      feedback('链接已复制');
      if (retryFocused && returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    }
  } catch {
    if (attempt === sequence) feedback('复制链接失败，请重试', true);
  } finally {
    if (attempt === sequence) {
      copying.value = false;
      retrying.value = false;
    }
  }
}
async function select(key: string) {
  const queue = queueRequest.value;
  if (queue) {
    const action = items.value.find((item) => item.key === key);
    if (!queue.live() || !action || action.disabled || action.separator) return;
    if (key === 'play') queue.play();
    else if (key === 'collect') queue.collect();
    else if (key === 'remove') queue.remove();
    else if (key === 'copy') await copy();
    else if (key === 'share') shared.value = queue.info.track;
    else emit('queueAction', key as QueueMenuAction, queue);
    return;
  }
  const input = request.value;
  const item = items.value.find((i) => i.key === key);
  if (
    !input ||
    !item ||
    item.disabled ||
    item.separator ||
    (input.episode && !input.episode.live())
  )
    return;
  const { track } = input;
  if (key === 'play') input.play();
  else if (key === 'collect') emit('collect', track);
  else if (key === 'download') emit('download', track);
  else if (key === 'comments') {
    if (input.episode) input.episode.comments();
    else emit('comments', track);
  } else if (key === 'remove' && playlist.value) emit('remove', track, playlist.value);
  else if (key === 'manage' && playlist.value) emit('manage', track, playlist.value);
  else if (key === 'share') shared.value = track;
  else if (key === 'copy') await copy();
  else if (key === 'next') {
    const view = props.player.view.value;
    if (!view) return;
    const attempt = sequence;
    try {
      const next = input.queueNext
        ? await input.queueNext()
        : await props.player.action({
            kind: input.nextOrigin === 'search' ? 'search_next_play' : 'next_play',
            id: track.id,
            quality: view.qualityPreferences.quality,
            expected_queue_revision: view.queueRevision,
          });
      if (attempt === sequence && next)
        feedback(
          !input.episode && view.track?.id === track.id
            ? '这首歌已是当前歌曲'
            : view.queueLen || view.track
              ? '已设为下一首播放'
              : '已开始播放',
        );
    } catch (issue) {
      if (attempt === sequence) feedback(failure(issue).message);
    }
  }
}
defineExpose({ show, showQueue, close });
</script>
<template>
  <ActionMenu
    ref="menu"
    context-only
    variant="song"
    :label="queueRequest ? '播放列表歌曲操作' : '歌曲操作'"
    :items="items"
    @select="select"
  />
  <ModalTransition
    ><TrackShareDialog v-if="shared" :track="shared" @close="shared = undefined"
  /></ModalTransition>
  <MenuTransition
    ><div v-if="message" class="track-action-feedback" :class="{ failed }" role="status">
      {{ message
      }}<button
        v-if="failed || retrying"
        class="text-button"
        :aria-disabled="copying"
        @click="copy"
      >
        {{ copying ? '正在复制…' : '重试' }}
      </button>
      <button v-if="failed" class="text-button" @click="dismiss">关闭</button>
    </div></MenuTransition
  >
</template>
