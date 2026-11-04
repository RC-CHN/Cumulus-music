<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import type { CatalogResource } from '../services/catalog';
import type { TrackView } from '../services/contracts';
import type { MenuItem } from '../services/menu';
import type { CurrentCatalogTrack } from '../services/player';
import { songLink } from '../services/trackMenu';
import ActionMenu from './ActionMenu.vue';
import ModalTransition from './ModalTransition.vue';
import TrackShareDialog from './TrackShareDialog.vue';

const props = defineProps<{
  selection?: CurrentCatalogTrack;
  accountEpoch: string;
  effectsAvailable: boolean;
}>();
const emit = defineEmits<{ catalog: [resource: CatalogResource]; effects: [] }>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const sharing = shallowRef<TrackView>();
let opened: string | undefined;
const selection = computed(() =>
  props.selection?.epoch === props.accountEpoch ? props.selection : undefined,
);
const key = computed(() => {
  const current = selection.value;
  return current ? JSON.stringify([current.epoch, current.playId, current.track.id]) : undefined;
});
watch(
  key,
  () => {
    opened = undefined;
    menu.value?.close(menu.value.containsFocus());
  },
  { flush: 'sync' },
);
watch(
  () => props.accountEpoch,
  () => (sharing.value = undefined),
);

const firstArtist = computed(() => selection.value?.track.artistRefs?.[0]);
const items = computed<MenuItem[]>(() => {
  const track = selection.value?.track;
  if (!track) return [];
  return [
    { key: 'share', label: '分享', icon: 'share', disabled: !songLink(track) },
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
  opened = undefined;
  const { track } = target;
  if (action === 'share' && songLink(track)) sharing.value = track;
  else if (action === 'artist' && firstArtist.value?.id)
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
    @opened="opened = key"
    @select="select"
  >
    <template #item="{ item }"
      ><span class="player-track-action-label" :title="item.label">{{ item.label }}</span></template
    >
  </ActionMenu>
  <ModalTransition
    ><TrackShareDialog v-if="sharing" :track="sharing" @close="sharing = undefined"
  /></ModalTransition>
</template>
<style scoped>
.player-track-action-label {
  min-width: 0;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
