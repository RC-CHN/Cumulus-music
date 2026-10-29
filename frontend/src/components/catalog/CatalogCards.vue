<script setup lang="ts">
import { computed, ref, watch, onUnmounted } from 'vue';
import { useVirtualRows } from '../../composables/useVirtualRows';
import type {
  AlbumCard,
  ArtistCard,
  CatalogVideoCard,
  ConcertCard,
  CatalogResource,
} from '../../services/catalog';
import Icon from '../Icon.vue';
const props = defineProps<{
  albums?: AlbumCard[];
  artists?: ArtistCard[];
  videos?: CatalogVideoCard[];
  concerts?: ConcertCard[];
}>();
const emit = defineEmits<{ open: [resource: CatalogResource]; mv: [id: string] }>();
const albums = computed(() => props.albums ?? []);
const artists = computed(() => props.artists ?? []);
const videos = computed(() => props.videos ?? []);
const concerts = computed(() => props.concerts ?? []);
const columns = ref(3);
const availableWidth = ref(540);
const rowHeight = computed(() =>
  albums.value.length
    ? (availableWidth.value - (columns.value - 1) * 16) / columns.value + 80
    : 260,
);
const total = computed(
  () => albums.value.length + artists.value.length + videos.value.length + concerts.value.length,
);
const {
  container: scroller,
  start,
  end,
  onScroll,
} = useVirtualRows(
  () => Math.ceil(total.value / columns.value),
  () => rowHeight.value,
  2,
);
let observer: ResizeObserver | undefined;
watch(
  scroller,
  (element) => {
    observer?.disconnect();
    if (element) {
      observer = new ResizeObserver((entries) => {
        availableWidth.value = entries[0]?.contentRect.width ?? 540;
        columns.value = Math.max(1, Math.floor((availableWidth.value + 16) / 196));
      });
      observer.observe(element);
    }
  },
  { flush: 'post' },
);
onUnmounted(() => observer?.disconnect());
const windowAlbums = computed(() =>
  albums.value.slice(start.value * columns.value, end.value * columns.value),
);
const windowArtists = computed(() =>
  artists.value.slice(start.value * columns.value, end.value * columns.value),
);
const windowVideos = computed(() =>
  videos.value.slice(start.value * columns.value, end.value * columns.value),
);
const windowConcerts = computed(() =>
  concerts.value.slice(start.value * columns.value, end.value * columns.value),
);
function date(value: string | number | null) {
  return typeof value === 'number' ? new Date(value).toLocaleDateString() : (value ?? '');
}
</script>
<template>
  <div v-if="total" ref="scroller" class="catalog-card-scroll" @scroll.passive="onScroll">
    <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
    <div
      class="catalog-grid"
      :style="{
        gridTemplateColumns: `repeat(${columns},minmax(0,1fr))`,
        gridAutoRows: `${rowHeight - 16}px`,
      }"
    >
      <button
        v-for="(album, i) in windowAlbums"
        :key="`album:${i}:${album.id}`"
        class="catalog-card"
        @click="emit('open', { kind: 'album', id: album.id })"
      >
        <img
          v-if="album.coverUrl"
          :src="album.coverUrl"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
        /><span v-else class="catalog-card-placeholder"><Icon name="music" /></span
        ><strong :title="album.name">{{ album.name }}</strong
        ><small>{{ date(album.publishedAt) }} · {{ album.trackCount }} 首</small>
      </button>
      <button
        v-for="(artist, i) in windowArtists"
        :key="`artist:${i}:${artist.id}`"
        class="catalog-card artist"
        @click="emit('open', { kind: 'artist', id: artist.id })"
      >
        <img
          v-if="artist.coverUrl"
          :src="artist.coverUrl"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
        /><span v-else class="catalog-card-placeholder"><Icon name="user" /></span
        ><strong :title="artist.name">{{ artist.name }}</strong>
      </button>
      <article
        v-for="(video, i) in windowVideos"
        :key="`video:${i}:${video.id}`"
        class="catalog-video-card"
      >
        <button
          :disabled="!video.mvId"
          :aria-label="`播放 MV ${video.title}`"
          @click="video.mvId && emit('mv', video.mvId)"
        >
          <img
            v-if="video.coverUrl"
            :src="video.coverUrl"
            alt=""
            loading="lazy"
            decoding="async"
            referrerpolicy="no-referrer"
          /><span v-else class="catalog-card-placeholder"><Icon name="video" /></span
          ><span v-if="video.mvId" class="catalog-video-play"><Icon name="play" /></span></button
        ><strong :title="video.title">{{ video.title }}</strong
        ><small :title="video.artists.map((a) => a.name).join(' / ')">{{
          video.artists.map((a) => a.name).join(' / ')
        }}</small
        ><small v-if="!video.mvId">{{
          video.kind === 'video' ? '普通视频播放暂未接入' : '暂不支持此视频类型'
        }}</small>
      </article>
      <article
        v-for="(concert, i) in windowConcerts"
        :key="`concert:${i}:${concert.id}`"
        class="catalog-concert-card"
      >
        <img
          v-if="concert.coverUrl"
          :src="concert.coverUrl"
          alt=""
          loading="lazy"
          referrerpolicy="no-referrer"
        />
        <div>
          <strong>{{ concert.title }}</strong>
          <p>
            {{ date(concert.start)
            }}<span v-if="concert.end && concert.end !== concert.start">
              – {{ date(concert.end) }}</span
            >
          </p>
          <p>{{ concert.address }}</p>
          <p v-if="concert.price">¥ {{ concert.price }}</p>
        </div>
      </article>
    </div>
    <div
      :style="{ height: `${(Math.ceil(total / columns) - end) * rowHeight}px` }"
      aria-hidden="true"
    />
  </div>
  <p v-if="!total" class="empty-state">暂无相关内容</p>
</template>
