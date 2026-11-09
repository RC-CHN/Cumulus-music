<script setup lang="ts">
import { computed } from 'vue';
import type { SearchSongTableHint } from '../../services/search';
import Icon from '../Icon.vue';
import SearchMatchText from './SearchMatchText.vue';

const props = defineProps<{ hint: SearchSongTableHint }>();
const alias = computed(() => props.hint.aliasParts?.map((part) => part.text).join(''));
const lyric = computed(() => props.hint.lyricParts?.map((part) => part.text).join(''));
const lyricMarks = computed(() =>
  props.hint.lyricParts?.map((part) => ({ text: part.text, match: part.highlight })),
);
</script>

<template>
  <div class="song-title-hint" data-search-title-hint>
    <div v-if="hint.aliasParts" class="ellipsis" data-search-title-alias :data-tooltip="alias">
      <SearchMatchText :text="alias ?? ''" :parts="hint.aliasParts" />
    </div>
    <div v-if="lyricMarks" class="ellipsis" data-search-title-lyric :data-tooltip="lyric">
      <Icon name="music" aria-hidden="true" />
      <SearchMatchText :text="lyric ?? ''" :parts="lyricMarks" />
    </div>
  </div>
</template>

<style scoped>
.song-title-hint {
  min-width: 0;
  color: var(--muted);
  font-size: 11px;
  line-height: 15px;
}
.song-title-hint .icon {
  width: 12px;
  height: 12px;
  margin-right: 4px;
  color: var(--link);
  vertical-align: -2px;
}
</style>
