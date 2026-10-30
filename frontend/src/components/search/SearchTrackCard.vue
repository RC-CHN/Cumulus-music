<script setup lang="ts">
import { ref, watch } from 'vue';
import type { TrackView } from '../../services/contracts';
import { artworkAt } from '../../services/artwork';
import Icon from '../Icon.vue';
const props = defineProps<{
  track: TrackView;
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
}>();
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
      <p class="search-track-title ellipsis" :data-tooltip="track.name">{{ track.name }}</p>
      <div class="search-track-credit">
        <span v-if="track.fee === 1" class="vip-tag">VIP</span>
        <span class="ellipsis" :data-tooltip="track.artists.join(' / ')">
          <template v-if="track.artistRefs?.length">
            <template v-for="(artist, i) in track.artistRefs" :key="i">
              <span v-if="i"> / </span
              ><button v-if="artist.id" @click.stop="emit('artist', artist.id)" @dblclick.stop>
                {{ artist.name }}</button
              ><span v-else>{{ artist.name }}</span>
            </template>
          </template>
          <template v-else>{{ track.artists.join(' / ') || '未知歌手' }}</template>
        </span>
      </div>
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
