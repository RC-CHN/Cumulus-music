<script setup lang="ts">
import { computed, ref, watch, onUnmounted } from 'vue';
import { useVirtualRows } from '../../composables/useVirtualRows';
import type { AlbumCard, ArtistCard, ConcertCard, CatalogResource } from '../../services/catalog';
import type { PlayerOrigin } from '../../services/player';
import type { AlbumCardRequest } from '../../services/catalogCards';
import type { ArtistCardRequest } from '../../services/artistCards';
import AlbumItem from './AlbumCard.vue';
import ArtistItem from './ArtistCard.vue';
const props = defineProps<{
  albums?: AlbumCard[];
  artists?: ArtistCard[];
  concerts?: ConcertCard[];
  playable?: boolean;
  queueable?: boolean;
  origin?: PlayerOrigin | null;
  playing?: boolean;
  saved?: boolean;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  playAlbum: [album: AlbumCard];
  albumAction: [request: AlbumCardRequest];
  playArtist: [artist: ArtistCard];
  artistAction: [request: ArtistCardRequest];
}>();
const albums = computed(() => props.albums ?? []);
const artists = computed(() => props.artists ?? []);
const concerts = computed(() => props.concerts ?? []);
const columns = ref(3);
const availableWidth = ref(540);
const rowHeight = computed(() => {
  const width = (availableWidth.value - (columns.value - 1) * 16) / columns.value;
  return albums.value.length ? width + 80 : artists.value.length ? width + 74 : 260;
});
const total = computed(() => albums.value.length + artists.value.length + concerts.value.length);
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
const windowConcerts = computed(() =>
  concerts.value.slice(start.value * columns.value, end.value * columns.value),
);
function date(value: string | number | null) {
  return typeof value === 'number' ? new Date(value).toLocaleDateString() : (value ?? '');
}
</script>
<template>
  <div
    v-if="total"
    ref="scroller"
    class="catalog-card-scroll"
    :data-catalog-total="total"
    @scroll.passive="onScroll"
  >
    <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
    <div
      class="catalog-grid"
      :style="{
        gridTemplateColumns: `repeat(${columns},minmax(0,1fr))`,
        gridAutoRows: `${rowHeight - 16}px`,
      }"
    >
      <AlbumItem
        v-for="(album, i) in windowAlbums"
        :key="`album:${i}:${album.id}`"
        :album="album"
        :playable="!!playable"
        :queueable="!!queueable"
        :current="origin?.kind === 'album' && origin.id === album.id"
        :playing="!!playing"
        :saved="!!saved"
        @open="emit('open', $event)"
        @play="emit('playAlbum', $event)"
        @action="emit('albumAction', $event)"
      />
      <ArtistItem
        v-for="(artist, i) in windowArtists"
        :key="`artist:${i}:${artist.id}`"
        :artist="artist"
        :playable="!!playable"
        :current="origin?.kind === 'artist' && origin.id === artist.id"
        :playing="!!playing"
        @open="emit('open', $event)"
        @play="emit('playArtist', $event)"
        @action="emit('artistAction', $event)"
      />
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
