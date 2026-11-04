<script setup lang="ts">
import SearchMatchText from './SearchMatchText.vue';

defineProps<{
  artists: { id: string | null; name: string }[];
  keyword: string;
}>();
const emit = defineEmits<{ artist: [id: string] }>();
</script>

<template>
  <template v-for="(artist, index) in artists" :key="index">
    <span v-if="index" class="search-card-credit-separator" aria-hidden="true">/</span>
    <button
      v-if="artist.id"
      type="button"
      class="search-card-artist"
      :data-result-action="`artist:${artist.id}:${index}`"
      :aria-label="`查看歌手 ${artist.name}`"
      :data-tooltip="artist.name"
      @click.stop="emit('artist', artist.id)"
      @dblclick.stop
    >
      <SearchMatchText :text="artist.name" :keyword="keyword" />
    </button>
    <span v-else class="search-card-artist-text">
      <SearchMatchText :text="artist.name" :keyword="keyword" />
    </span>
  </template>
</template>
