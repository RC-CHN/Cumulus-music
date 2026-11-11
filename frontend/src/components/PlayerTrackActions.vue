<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { CatalogResource } from '../services/catalog';
import type { MenuItem } from '../services/menu';
import type { SongSpeedMoreTarget } from '../services/songSpeedMore';
import type { CurrentCatalogTrack, PlayerSongShareTarget } from '../services/player';
import { useCore } from '../services/core';
import { useSongShare } from '../composables/useSearchSongShare';
import ActionMenu from './ActionMenu.vue';
import ModalTransition from './ModalTransition.vue';
import HistoryRowShareDialog from './history/HistoryRowShareDialog.vue';

const props = defineProps<{
  selection?: CurrentCatalogTrack;
  entryId?: string;
  shareTarget?: PlayerSongShareTarget;
  accountEpoch: string;
  effectsAvailable: boolean;
  songSpeed?: SongSpeedMoreTarget;
}>();
const emit = defineEmits<{
  catalog: [resource: CatalogResource];
  effects: [];
  songSpeed: [anchor: HTMLElement];
  login: [];
  expired: [];
}>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const core = useCore();
const sharing = useSongShare<PlayerSongShareTarget>(
  {
    prepare: (epoch, target) => core.playerSongSharePrepare(epoch, target),
    choose: (epoch, target, offer, choice) =>
      core.playerSongShareChoose(epoch, target, offer, choice),
    cancel: (epoch, target, offer) => core.playerSongShareCancel(epoch, target, offer),
  },
  {
    expired: () => emit('expired'),
    loginRequired: () => emit('login'),
    staleMessage: '当前歌曲已更新，请重新打开菜单。',
  },
);
onBeforeUnmount(sharing.reset);
watch(sharing.feedback, (message, _, onCleanup) => {
  if (!message) return;
  const timer = setTimeout(sharing.clearFeedback, 5000);
  onCleanup(() => clearTimeout(timer));
});
let opened: string | undefined;
let openedSpeed: string | undefined;
const selection = computed(() =>
  props.selection?.epoch === props.accountEpoch ? props.selection : undefined,
);
const key = computed(() => {
  const current = selection.value;
  return current
    ? JSON.stringify([current.epoch, current.playId, props.entryId, current.track.id])
    : undefined;
});
watch(
  key,
  () => {
    opened = undefined;
    openedSpeed = undefined;
    sharing.reset();
    menu.value?.close(menu.value.containsFocus());
  },
  { flush: 'sync' },
);

watch(
  () => props.songSpeed?.key,
  (next) => {
    if (!opened || next === openedSpeed) return;
    opened = undefined;
    openedSpeed = undefined;
    menu.value?.close(menu.value.containsFocus());
  },
  { flush: 'sync' },
);
const firstArtist = computed(() => selection.value?.track.artistRefs?.[0]);
const items = computed<MenuItem[]>(() => {
  const track = selection.value?.track;
  if (!track) return [];
  return [
    ...(props.shareTarget
      ? [{ key: 'share', label: '分享', icon: 'share', disabled: sharing.busy.value }]
      : []),
    ...(props.songSpeed
      ? [
          { key: 'song-speed', label: props.songSpeed.label, icon: 'speed' },
          { key: 'song-speed-divider', label: '', separator: true },
        ]
      : []),
    {
      key: 'artist',
      label: `歌手：${firstArtist.value?.name || track.artists[0] || '未知'}`,
      icon: 'user',
      disabled: !firstArtist.value?.id,
    },
    {
      key: 'album',
      label: `专辑：${track.album || '未知'}`,
      icon: 'album',
      disabled: !track.albumId,
    },
    { key: 'effects', label: '音效', icon: 'settings', disabled: !props.effectsAvailable },
  ];
});
function select(action: string) {
  const target = selection.value;
  if (!target || key.value !== opened) return;
  const shown = opened;
  const speed = openedSpeed;
  opened = undefined;
  openedSpeed = undefined;
  const { track } = target;
  if (action === 'song-speed') {
    const anchor = menu.value?.triggerElement();
    if (speed && speed === props.songSpeed?.key && anchor) emit('songSpeed', anchor);
    return;
  }
  if (action === 'share') {
    const bound = props.shareTarget;
    const anchor = menu.value?.triggerElement();
    if (!bound || !anchor || sharing.busy.value) return;
    void sharing.open(
      {
        epoch: target.epoch,
        target: bound,
        live: () =>
          key.value === shown &&
          props.shareTarget?.playId === bound.playId &&
          props.shareTarget?.entryId === bound.entryId,
        fallbackFocus: () => menu.value?.triggerElement() ?? null,
      },
      anchor,
    );
    return;
  }
  sharing.reset();
  if (action === 'artist' && firstArtist.value?.id)
    emit('catalog', { kind: 'artist', id: firstArtist.value.id });
  else if (action === 'album' && track.albumId)
    emit('catalog', { kind: 'album', id: track.albumId });
  else if (action === 'effects' && props.effectsAvailable) emit('effects');
}
</script>

<template>
  <ActionMenu
    v-if="selection"
    ref="menu"
    class="player-track-actions"
    variant="song"
    label="当前歌曲更多操作"
    :items="items"
    @opened="
      opened = key;
      openedSpeed = songSpeed?.key;
      sharing.clearFeedback();
    "
    @select="select"
  >
    <template #item="{ item }"
      ><span class="player-track-action-label" :title="item.label">{{ item.label }}</span></template
    >
  </ActionMenu>
  <Teleport to="body">
    <ModalTransition
      :return-focus="sharing.returnFocus"
      :fallback-focus="sharing.fallbackFocus"
      :restore-focus="sharing.restoreFocus.value"
      @left="sharing.keepFocusVisible"
    >
      <HistoryRowShareDialog
        v-if="sharing.dialog.value"
        :key="sharing.dialog.value.stage"
        data-player-song-share-dialog
        :modules="sharing.dialog.value.stage === 'qr' ? sharing.dialog.value.modules : undefined"
        @choose="sharing.choose"
        @close="sharing.close()"
      />
    </ModalTransition>
    <p
      v-if="sharing.busy.value || sharing.feedback.value"
      class="track-action-feedback player-song-share-status"
      :class="{ failed: sharing.feedback.value?.error }"
      :role="sharing.feedback.value?.error ? 'alert' : 'status'"
      data-player-song-share-status
    >
      {{ sharing.feedback.value?.text ?? '正在核对分享权限…' }}
    </p>
  </Teleport>
</template>
<style scoped>
.player-track-action-label {
  min-width: 0;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.player-song-share-status {
  pointer-events: none;
}
</style>
