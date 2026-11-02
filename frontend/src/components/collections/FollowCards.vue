<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { FollowCard } from '../../services/collections';
import { useVirtualRows } from '../../composables/useVirtualRows';
import FollowArtistCard from './FollowArtistCard.vue';
import type { ArtistCardRequest } from '../../services/artistCards';
const props = defineProps<{ entries: FollowCard[] }>();
const emit = defineEmits<{ artist: [id: string]; action: [request: ArtistCardRequest] }>();
const columns = ref(2);
const { container, start, end, onScroll } = useVirtualRows(
  () => Math.ceil(props.entries.length / columns.value),
  96,
  2,
);
const visible = computed(() =>
  props.entries.slice(start.value * columns.value, end.value * columns.value),
);
let observer: ResizeObserver | undefined;
watch(
  container,
  (element) => {
    observer?.disconnect();
    if (element) {
      observer = new ResizeObserver(([entry]) => {
        columns.value = (entry?.contentRect.width ?? 0) >= 650 ? 2 : 1;
      });
      observer.observe(element);
    }
  },
  { flush: 'post' },
);
onUnmounted(() => observer?.disconnect());
</script>
<template>
  <div ref="container" class="collection-follow-scroll" @scroll.passive="onScroll">
    <div :style="{ height: `${start * 96}px` }" aria-hidden="true" />
    <div
      class="collection-follow-grid"
      :style="{ gridTemplateColumns: `repeat(${columns},minmax(0,1fr))` }"
    >
      <FollowArtistCard
        v-for="(entry, index) in visible"
        :key="
          entry.artist
            ? `artist:${entry.artist.id}`
            : `user:${entry.user?.id ?? start * columns + index}`
        "
        :entry="entry"
        @artist="emit('artist', $event)"
        @action="emit('action', $event)"
      />
    </div>
    <div
      :style="{ height: `${(Math.ceil(entries.length / columns) - end) * 96}px` }"
      aria-hidden="true"
    />
  </div>
</template>
