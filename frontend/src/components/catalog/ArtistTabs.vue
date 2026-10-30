<script setup lang="ts">
import { computed } from 'vue';
import type { ArtistCard, ArtistTab } from '../../services/catalog';
const props = defineProps<{
  artist: ArtistCard;
  selected: ArtistTab;
  onlineCount?: number;
  idPrefix: string;
}>();
const emit = defineEmits<{ choose: [tab: ArtistTab] }>();
const active = computed(() => (props.selected === 'all' ? 'songs' : props.selected));
const items = computed<{ key: ArtistTab; label: string; count?: number }[]>(() => [
  { key: 'songs', label: '歌曲' },
  { key: 'albums', label: '专辑', count: props.artist.albumCount },
  ...(props.artist.mvCount !== 0 || props.selected === 'videos'
    ? [
        {
          key: 'videos' as const,
          label: props.artist.mvCount == null ? '视频' : 'MV',
          count: props.artist.mvCount ?? undefined,
        },
      ]
    : []),
  { key: 'intro', label: '歌手详情' },
  { key: 'similar', label: '相似歌手' },
  ...((props.onlineCount ?? 0) > 0 || props.selected === 'concerts'
    ? [{ key: 'concerts' as const, label: '演出', count: props.onlineCount }]
    : []),
]);
function move(event: KeyboardEvent, index: number) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const target =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? items.value.length - 1
        : (index + (event.key === 'ArrowRight' ? 1 : -1) + items.value.length) % items.value.length;
  const tab = items.value[target].key;
  emit('choose', tab);
  document.getElementById(`${props.idPrefix}-${tab}`)?.focus();
}
</script>
<template>
  <nav class="catalog-tabs artist-tabs" role="tablist" aria-label="歌手内容">
    <button
      v-for="(item, index) in items"
      :id="`${idPrefix}-${item.key}`"
      :key="item.key"
      :data-catalog-tab="item.key"
      role="tab"
      :aria-controls="`${idPrefix}-panel`"
      :aria-selected="active === item.key"
      :tabindex="active === item.key ? 0 : -1"
      @click="emit('choose', item.key)"
      @keydown="move($event, index)"
    >
      {{ item.label }}<small v-if="item.count != null">{{ item.count }}</small>
    </button>
  </nav>
</template>
