<script setup lang="ts">
import MenuTransition from './MenuTransition.vue';
import { computed, nextTick, onBeforeUnmount, ref, shallowRef } from 'vue';
import type { TrackView, PlaylistView } from '../services/contracts';
import type { PlayerModel } from '../composables/usePlayer';
import type { MenuItem } from '../services/menu';
import { songLink, type TrackMenuRequest } from '../services/trackMenu';
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
}>();
const client = useCore(),
  menu = ref<InstanceType<typeof ActionMenu>>();
const request = shallowRef<TrackMenuRequest>(),
  playlist = shallowRef<PlaylistView>();
const shared = shallowRef<TrackView>(),
  message = ref(''),
  failed = ref(false),
  copying = ref(false),
  retrying = ref(false);
let returnFocus: HTMLElement | undefined;
let sequence = 0,
  timer: ReturnType<typeof setTimeout> | undefined;
const items = computed<MenuItem[]>(() => {
  const track = request.value?.track;
  if (!track) return [];
  const available = !track.missing && !track.local;
  const link = songLink(track);
  return [
    {
      key: 'play',
      label: '播放',
      icon: 'play',
      disabled: !available || !props.player.ready.value || props.player.busy.value,
    },
    {
      key: 'next',
      label: '下一首播放',
      icon: 'next',
      disabled:
        !available ||
        !props.player.ready.value ||
        props.player.busy.value ||
        props.player.view.value?.fm,
      hint: props.player.view.value?.fm ? '私人漫游期间请先切换到普通播放队列' : undefined,
    },
    ...(!track.local ? [{ key: 'comments', label: '查看评论', icon: 'comment' }] : []),
    { key: 'main-divider', label: '', separator: true },
    { key: 'collect', label: '收藏', icon: 'star', disabled: !available },
    { key: 'download', label: '下载', icon: 'download', disabled: !available },
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
  shared.value = undefined;
  clear();
}
onBeforeUnmount(close);
async function show(input: TrackMenuRequest, source?: PlaylistView) {
  clear();
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
  await menu.value?.showAt(
    pointer ? event.clientX : (box?.left ?? 8),
    pointer ? event.clientY : (box?.bottom ?? 8),
    trigger,
  );
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
  const link = request.value && songLink(request.value.track);
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
  const input = request.value;
  if (!input || items.value.find((i) => i.key === key)?.disabled) return;
  const { track } = input;
  if (key === 'play') input.play();
  else if (key === 'collect') emit('collect', track);
  else if (key === 'download') emit('download', track);
  else if (key === 'comments') emit('comments', track);
  else if (key === 'remove' && playlist.value) emit('remove', track, playlist.value);
  else if (key === 'manage' && playlist.value) emit('manage', track, playlist.value);
  else if (key === 'share') shared.value = track;
  else if (key === 'copy') await copy();
  else if (key === 'next') {
    const view = props.player.view.value;
    if (!view) return;
    const attempt = sequence;
    const next = await props.player.action({
      kind: 'next_play',
      id: track.id,
      quality: view.qualityPreferences.quality,
      expected_queue_revision: view.queueRevision,
    });
    if (attempt === sequence && next)
      feedback(
        view.track?.id === track.id
          ? '这首歌已是当前歌曲'
          : view.queueLen || view.track
            ? '已设为下一首播放'
            : '已开始播放',
      );
  }
}
defineExpose({ show, close });
</script>
<template>
  <ActionMenu
    ref="menu"
    context-only
    variant="song"
    label="歌曲操作"
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
