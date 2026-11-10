<script setup lang="ts">
import MenuTransition from './MenuTransition.vue';
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import { failure, type TrackView, type PlaylistView } from '../services/contracts';
import type { PlayerModel } from '../composables/usePlayer';
import type { MenuItem } from '../services/menu';
import {
  queueFileMenuItems,
  queueSongMenuItems,
  queueVoiceMenuItems,
  songLink,
  type QueueMenuAction,
  type QueueMenuRequest,
  type TrackMenuRequest,
} from '../services/trackMenu';
import { useCore } from '../services/core';
import { podcastProgramPublicLink, type PodcastProgramShare } from '../services/podcast';
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
  queueAction: [key: QueueMenuAction, input: QueueMenuRequest];
}>();
const client = useCore(),
  menu = ref<InstanceType<typeof ActionMenu>>();
const request = shallowRef<TrackMenuRequest>(),
  playlist = shallowRef<PlaylistView>();
const queueRequest = shallowRef<QueueMenuRequest>();
const searchGroupQueue = shallowRef<{ playId: string; revision: string }>();
function currentSearchGroupQueue() {
  const current = props.player.view.value;
  const target = searchGroupQueue.value;
  if (
    target &&
    current?.playId === target.playId &&
    current.queueRevision === target.revision &&
    (current.state === 'playing' || current.state === 'paused') &&
    !current.fm &&
    !current.intelligence &&
    current.mode !== 'random'
  )
    return target;
}
const sharedVoice = shallowRef<{ program: PodcastProgramShare; live: () => boolean }>();
const voiceShareRestore = ref(false);
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
    const group = queue.tracks.length > 1;
    const playing =
      props.player.playing.value && props.player.view.value?.entryId === queue.info.entryId;
    const locked =
      !props.player.ready.value || props.player.busy.value || props.player.queueBusy.value;
    if (queue.kind === 'voice') return queueVoiceMenuItems(queue.info, group, playing, locked);
    if (queue.kind === 'local' || queue.kind === 'downloaded')
      return queueFileMenuItems(queue, playing, locked);
    return queueSongMenuItems(queue.info, group, playing, locked).map((item) =>
      item.key === 'collect' ? { ...item, disabled: queue.collectionLocked() } : item,
    );
  }
  const input = request.value;
  const track = input?.track;
  if (!track || (input?.episode && !input.episode.live())) return [];
  const nextDisabled =
    !props.player.ready.value ||
    props.player.busy.value ||
    (!!input.queueNext && props.player.queueBusy.value) ||
    props.player.view.value?.fm;
  const nextHint = props.player.view.value?.fm ? '私人漫游期间请先切换到普通播放队列' : undefined;
  if (input.removeLocalMember)
    return [
      {
        key: 'play',
        label: '播放',
        icon: 'play',
        disabled: track.missing || !props.player.ready.value || props.player.busy.value,
      },
      ...(input.queueNext
        ? [
            {
              key: 'next',
              label: '下一首播放',
              icon: 'next',
              disabled: track.missing || nextDisabled,
              hint: nextHint,
            },
          ]
        : []),
      { key: 'remove_local_member', label: '从歌单中移除', icon: 'trash' },
    ];
  const searchGroup = input.searchGroupNext;
  if (searchGroup) {
    if (!searchGroup.live()) return [];
    const target = currentSearchGroupQueue();
    return [
      {
        key: 'next',
        label: '下一首播放',
        icon: 'next',
        disabled:
          !searchGroup.action ||
          searchGroup.allCopyrightBlocked ||
          !target ||
          !props.player.ready.value ||
          props.player.busy.value ||
          props.player.queueBusy.value,
        hint: searchGroup.allCopyrightBlocked
          ? '所选歌曲均不可播放'
          : !searchGroup.action
            ? '所选歌曲中有未核实播放权限的条目'
            : !target
              ? '仅支持当前普通播放队列'
              : undefined,
      },
    ];
  }
  const selection = input.episode?.selection;
  const groupDownload = selection && input.episode?.downloadGroup;
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
      disabled: !canPlay || nextDisabled,
      hint: nextHint,
    },
    ...(!track.local && !selection
      ? [{ key: 'comments', label: '查看评论', icon: 'comment' }]
      : []),
    ...(!selection
      ? [
          { key: 'main-divider', label: '', separator: true },
          { key: 'collect', label: '收藏', icon: 'star', disabled: !available },
        ]
      : input.episode?.collectGroup
        ? [{ key: 'collect', label: '收藏', icon: 'star' }]
        : []),
    ...(!selection && (!input.episode || input.episode.download)
      ? [
          {
            key: 'download',
            label: '下载',
            icon: 'download',
            disabled: !available || !!input.episode?.copyrightBlocked,
          },
        ]
      : groupDownload && groupDownload.available()
        ? [{ key: 'download', label: '下载', icon: 'download', disabled: groupDownload.busy() }]
        : []),
    ...(available && link
      ? [
          { key: 'share', label: '分享', icon: 'share' },
          { key: 'copy', label: '复制链接', icon: 'copy' },
        ]
      : []),
    ...(playlist.value?.owned && !playlist.value.favorite && !track.local && !input.mixedLocal
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
  closeVoiceShare(false);
  clear();
}
watch(
  () => queueRequest.value?.live(),
  (live) => {
    if (live === false) close();
  },
);
watch(
  () => request.value?.episode?.live() ?? request.value?.searchGroupNext?.live(),
  (live) => {
    if (live === false) close();
  },
);
onBeforeUnmount(close);
async function show(input: TrackMenuRequest, source?: PlaylistView) {
  clear();
  queueRequest.value = undefined;
  request.value = input;
  const view = props.player.view.value;
  searchGroupQueue.value =
    input.searchGroupNext && view?.playId
      ? { playId: view.playId, revision: view.queueRevision }
      : undefined;
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
  if (
    request.value !== input ||
    (input.episode && !input.episode.live()) ||
    (input.searchGroupNext && !input.searchGroupNext.live())
  )
    return;
  await menu.value?.showAt(
    pointer ? event.clientX : (box?.left ?? 8),
    pointer ? event.clientY : (box?.bottom ?? 8),
    trigger,
  );
}
async function showQueue(input: QueueMenuRequest) {
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
  const link =
    queue?.kind === 'voice'
      ? podcastProgramPublicLink(queue.info.voiceId)
      : track && songLink(track);
  if (!link || copying.value) return;
  const attempt = sequence;
  retrying.value = !!document.activeElement?.closest('.track-action-feedback');
  copying.value = true;
  message.value = '正在复制链接…';
  failed.value = false;
  try {
    await client.copyText(link);
    if (attempt === sequence && (!queue || queue.live())) {
      const retryFocused = document.activeElement?.closest('.track-action-feedback');
      feedback('链接已复制');
      if (retryFocused && returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    }
  } catch {
    if (attempt === sequence && (!queue || queue.live())) feedback('复制链接失败，请重试', true);
  } finally {
    if (attempt === sequence) {
      copying.value = false;
      retrying.value = false;
    }
  }
}
function closeVoiceShare(restore: boolean) {
  voiceShareRestore.value = restore;
  sharedVoice.value = undefined;
}
function voiceShareReturn() {
  return returnFocus ?? null;
}
async function select(key: string) {
  const queue = queueRequest.value;
  if (queue) {
    const action = items.value.find((item) => item.key === key);
    if (!queue.live() || !action || action.disabled || action.separator) return;
    if (key === 'play') queue.play();
    else if (key === 'collect' && queue.kind !== 'voice') queue.collect();
    else if (key === 'remove') queue.remove();
    else if (key === 'copy') await copy();
    else if (key === 'share') {
      if (queue.kind === 'voice') {
        sharedVoice.value = {
          program: {
            id: queue.info.voiceId,
            name: queue.info.track.name,
            creator: queue.info.creator?.name || queue.info.track.artists.join(' / '),
          },
          live: () => queueRequest.value === queue && queue.live(),
        };
      } else shared.value = queue.info.track;
    } else emit('queueAction', key as QueueMenuAction, queue);
    return;
  }
  const input = request.value;
  const item = items.value.find((i) => i.key === key);
  if (
    !input ||
    !item ||
    item.disabled ||
    item.separator ||
    (input.episode && !input.episode.live()) ||
    (input.searchGroupNext && !input.searchGroupNext.live())
  )
    return;
  const { track } = input;
  if (key === 'play') input.play();
  else if (key === 'remove_local_member') input.removeLocalMember?.(returnFocus);
  else if (key === 'collect') {
    if (input.episode?.selection) input.episode.collectGroup?.();
    else emit('collect', track);
  } else if (key === 'download') {
    if (input.episode?.selection) {
      const group = input.episode.downloadGroup;
      if (group?.available() && !group.busy()) group.start();
    } else if (input.episode) input.episode.download?.();
    else emit('download', track);
  } else if (key === 'comments') {
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
      const group = input.searchGroupNext;
      if (group) {
        const target = currentSearchGroupQueue();
        if (!target || !group.action) return;
        const wire = ({ receipt, trackId }: (typeof group.action.rows)[number]) => ({
          receipt,
          track_id: trackId,
        });
        const next = await props.player.action({
          kind: 'search_group_next_play',
          anchor: wire(group.action.anchor),
          rows: group.action.rows.map(wire),
          quality: view.qualityPreferences.quality,
          expected_play_id: target.playId,
          expected_queue_revision: target.revision,
        });
        if (attempt === sequence && next && group.live()) feedback('已将所选歌曲设为下一首播放');
        return;
      }
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
    :label="
      queueRequest?.kind === 'voice' ||
      (queueRequest?.kind === 'downloaded' && queueRequest.downloadKind === 'voice')
        ? '播放列表声音操作'
        : queueRequest
          ? '播放列表歌曲操作'
          : '歌曲操作'
    "
    :items="items"
    @select="select"
  />
  <ModalTransition
    ><TrackShareDialog v-if="shared" :track="shared" @close="shared = undefined"
  /></ModalTransition>
  <ModalTransition :return-focus="voiceShareReturn" :restore-focus="voiceShareRestore">
    <TrackShareDialog
      v-if="sharedVoice"
      :program="sharedVoice.program"
      :can-copy="sharedVoice.live"
      @close="closeVoiceShare(sharedVoice.live())"
    />
  </ModalTransition>
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
