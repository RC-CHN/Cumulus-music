<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { TrackView } from '../../services/contracts';
import type { SearchTrackHint } from '../../services/search';
import { artworkAt } from '../../services/artwork';
import Icon from '../Icon.vue';
import SearchMatchText from './SearchMatchText.vue';
const props = defineProps<{
  track: TrackView;
  mvId?: string;
  hint?: SearchTrackHint;
  explicit?: boolean;
  matchKeyword: string;
  playable: boolean;
  current: boolean;
  playing: boolean;
  liked: boolean;
  busyLike: boolean;
}>();
const emit = defineEmits<{
  play: [];
  like: [];
  more: [event: MouseEvent | KeyboardEvent];
  download: [];
  artist: [id: string];
  mv: [id: string];
}>();
const hintText = computed(() =>
  props.hint?.kind === 'lyric'
    ? props.hint.parts.map((part) => part.text).join('')
    : props.hint?.text,
);
const failedCover = ref(false);
watch(
  () => props.track.coverUrl,
  () => {
    failedCover.value = false;
  },
);
</script>
<template>
  <article
    class="search-track-card"
    :class="{ current, unavailable: track.missing }"
    :data-result-key="`song:${track.id}`"
    @dblclick="playable && !track.missing && emit('play')"
    @contextmenu.prevent="emit('more', $event)"
    @keydown.shift.f10.prevent.stop="emit('more', $event)"
  >
    <button
      class="search-track-cover"
      :aria-label="`${current && playing ? '暂停' : '播放'} ${track.name}`"
      :disabled="!playable || track.missing"
      @click.stop="emit('play')"
      @dblclick.stop
    >
      <img
        v-if="track.coverUrl && !failedCover"
        :src="artworkAt(track.coverUrl, 160)"
        alt=""
        loading="lazy"
        decoding="async"
        referrerpolicy="no-referrer"
        @error="failedCover = true"
      />
      <Icon v-else name="music" />
      <span class="search-track-mask"><Icon :name="current && playing ? 'pause' : 'play'" /></span>
    </button>
    <div class="search-track-copy">
      <p class="search-track-title">
        <span class="ellipsis" :data-tooltip="track.name">
          <SearchMatchText :text="track.name" :keyword="matchKeyword" />
        </span>
        <span v-if="explicit" class="search-track-explicit" role="img" aria-label="含成人内容"
          >E</span
        >
      </p>
      <div class="search-track-credit">
        <span v-if="track.fee === 1" class="vip-tag">VIP</span>
        <button
          v-if="mvId"
          class="search-track-mv"
          :aria-label="`播放 MV ${track.name}`"
          @click.stop="emit('mv', mvId)"
          @dblclick.stop
        >
          MV <Icon name="play" />
        </button>
        <span class="ellipsis" :data-tooltip="track.artists.join(' / ')">
          <template v-if="track.artistRefs?.length">
            <template v-for="(artist, i) in track.artistRefs" :key="i">
              <span v-if="i"> / </span
              ><button v-if="artist.id" @click.stop="emit('artist', artist.id)" @dblclick.stop>
                <SearchMatchText :text="artist.name" :keyword="matchKeyword" />
              </button>
              <span v-else><SearchMatchText :text="artist.name" :keyword="matchKeyword" /></span>
            </template>
          </template>
          <SearchMatchText
            v-else-if="track.artists.length"
            :text="track.artists.join(' / ')"
            :keyword="matchKeyword"
          />
          <template v-else>未知歌手</template>
        </span>
      </div>
      <p v-if="hint" class="search-track-hint ellipsis" :data-tooltip="hintText">
        <template v-if="hint.kind === 'lyric'">
          <Icon name="list" />
          <template v-for="(part, i) in hint.parts" :key="i">
            <mark v-if="part.highlight" class="search-track-hint-match">{{ part.text }}</mark
            ><template v-else>{{ part.text }}</template>
          </template>
        </template>
        <SearchMatchText v-else :text="hint.text" :keyword="matchKeyword" />
      </p>
    </div>
    <div class="search-track-actions">
      <button
        class="icon-button"
        :aria-label="`下载 ${track.name}`"
        :disabled="track.missing"
        @click.stop="emit('download')"
        @dblclick.stop
      >
        <Icon name="download" />
      </button>
      <button
        class="icon-button like-button"
        :class="{ liked }"
        :aria-label="`${liked ? '取消喜欢' : '喜欢'} ${track.name}`"
        :aria-pressed="liked"
        :disabled="track.missing || busyLike"
        @click.stop="emit('like')"
        @dblclick.stop
      >
        <Icon name="heart" />
      </button>
      <button
        class="icon-button"
        :aria-label="`更多操作 ${track.name}`"
        aria-haspopup="menu"
        @click.stop="emit('more', $event)"
        @dblclick.stop
      >
        <Icon name="more" />
      </button>
    </div>
  </article>
</template>
