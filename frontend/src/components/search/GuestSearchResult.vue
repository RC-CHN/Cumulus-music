<script setup lang="ts">
import { computed } from 'vue';
import { artworkAt } from '../../services/artwork';
import { searchCardKey } from '../../services/searchCards';
import type { SearchItem } from '../../services/search';
import type { MenuItem } from '../../services/menu';
import Icon from '../Icon.vue';
import ActionMenu from '../ActionMenu.vue';
import SearchMatchText from './SearchMatchText.vue';

const props = defineProps<{
  item: SearchItem;
  keyword: string;
  songPublicLinkAvailable?: boolean;
}>();
const emit = defineEmits<{ search: [word: string]; songLinkAction: [action: 'share' | 'copy'] }>();
const songLinkItems: MenuItem[] = [
  { key: 'share', label: '分享', icon: 'share' },
  { key: 'copy', label: '复制链接', icon: 'copy' },
];
const display = computed<[string, string, string | null]>(() => {
  const item = props.item;
  switch (item.kind) {
    case 'song':
      return [item.track.name, item.track.artists.join(' / '), item.track.coverUrl];
    case 'playlist':
      return [item.playlist.name, item.playlist.creator, item.playlist.coverUrl];
    case 'artist':
      return [item.artist.name, item.artist.aliases.join(' / '), item.artist.coverUrl];
    case 'album':
      return [
        item.album.name,
        item.album.artists.map(({ name }) => name).join(' / '),
        item.album.coverUrl,
      ];
    case 'voice':
      return [item.voice.name, item.voice.creator, item.voice.coverUrl];
    case 'podcast':
      return [item.podcast.name, item.podcast.creator, item.podcast.coverUrl];
    case 'mv':
      return [item.mv.title, item.mv.artists.map(({ name }) => name).join(' / '), item.mv.coverUrl];
    case 'user':
      return [item.user.name, item.user.signature, item.user.avatarUrl];
    case 'related':
      return [item.keyword, '', null];
  }
});
const cover = computed(() => (display.value[2] ? artworkAt(display.value[2], 120) : null));
</script>

<template>
  <article class="guest-search-result" :data-result-key="searchCardKey(item)">
    <button
      v-if="item.kind === 'related'"
      class="text-button"
      type="button"
      @click="emit('search', item.keyword)"
    >
      <SearchMatchText :text="item.keyword" :keyword="keyword" />
    </button>
    <template v-else>
      <img v-if="cover" :src="cover" alt="" loading="lazy" />
      <span v-else class="guest-search-cover"><Icon name="music" /></span>
      <div class="guest-search-copy">
        <b><SearchMatchText :text="display[0] ?? ''" :keyword="keyword" /></b>
        <span v-if="display[1]">{{ display[1] }}</span>
        <small v-if="item.kind === 'song' && item.excerpt.length">
          {{ item.excerpt.join(' / ') }}
        </small>
      </div>
      <ActionMenu
        v-if="songPublicLinkAvailable && item.kind === 'song'"
        class="guest-search-more"
        :label="`更多：${item.track.name}`"
        catalog-key="guest-search-song-share"
        variant="song"
        :items="songLinkItems"
        @select="
          (key) => {
            if (key === 'share' || key === 'copy') emit('songLinkAction', key);
          }
        "
      />
    </template>
  </article>
</template>

<style scoped>
.guest-search-result {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  min-height: 64px;
}
.guest-search-result img,
.guest-search-cover {
  flex: 0 0 48px;
  display: grid;
  place-items: center;
  width: 48px;
  height: 48px;
  object-fit: cover;
  border-radius: 7px;
  background: var(--art-bg);
}
.guest-search-copy {
  display: grid;
  gap: 4px;
  min-width: 0;
}
.guest-search-copy > * {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.guest-search-more {
  margin-inline-start: auto;
  /* WebKitGTK's overlay scrollbar can intercept input inside the panel edge. */
  margin-inline-end: 20px;
}
.guest-search-copy span,
.guest-search-copy small {
  color: var(--muted);
}
</style>
