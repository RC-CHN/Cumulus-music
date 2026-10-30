<script setup lang="ts">
import type { TrackMenuRequest } from '../services/trackMenu';
import type { TrackView } from '../services/contracts';
import { artworkAt } from '../services/artwork';
import Icon from './Icon.vue';
const props = defineProps<{
  track: TrackView;
  index: number;
  liked: boolean;
  busy: boolean;
  playable?: boolean;
  currentId?: string;
  navigable?: boolean;
}>();
const emit = defineEmits<{
  like: [track: TrackView];
  play: [index: number];
  more: [request: TrackMenuRequest];
  artist: [id: string];
  album: [id: string];
}>();
function more(event: MouseEvent | KeyboardEvent) {
  emit('more', { track: props.track, event, play: () => emit('play', props.index) });
}
function duration(ms: number) {
  const seconds = Math.floor(ms / 1000);
  return `${Math.floor(seconds / 60)
    .toString()
    .padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`;
}
</script>
<template>
  <div
    class="track-columns track-row"
    :class="{
      unavailable: track.missing,
      current: !track.missing && !track.local && track.id === currentId,
      striped: index % 2 === 0,
    }"
    @dblclick="!track.missing && playable && emit('play', index)"
    @contextmenu.prevent.stop="more"
    @keydown.shift.f10.prevent.stop="more"
    role="row"
    :aria-rowindex="index + 2"
    :data-track-id="track.id"
  >
    <span class="track-index" role="cell"
      ><button
        class="track-play icon-button"
        :aria-label="`播放 ${track.name}`"
        :disabled="!playable || track.missing"
        @click.stop="emit('play', index)"
        @dblclick.stop
      >
        <span>{{ (index + 1).toString().padStart(2, '0') }}</span
        ><Icon
          :name="!track.missing && !track.local && track.id === currentId ? 'volume' : 'play'"
        /></button
    ></span>
    <div class="track-title" role="cell">
      <img
        v-if="track.coverUrl"
        :src="artworkAt(track.coverUrl, 120)"
        alt=""
        loading="lazy"
        decoding="async"
        referrerpolicy="no-referrer"
      /><span v-else class="track-art"><Icon name="music" /></span>
      <div>
        <div class="ellipsis" :data-tooltip="track.name">
          {{ track.name }} <span v-if="track.fee === 1" class="vip-tag">VIP</span>
        </div>
        <small
          v-if="navigable && track.artistRefs?.length"
          class="ellipsis track-credit-links"
          :data-tooltip="track.artistRefs.map((artist) => artist.name).join(' / ')"
          ><template v-for="(artist, i) in track.artistRefs" :key="i"
            ><span v-if="i"> / </span
            ><button v-if="artist.id" @click.stop="emit('artist', artist.id)" @dblclick.stop>
              {{ artist.name }}</button
            ><span v-else>{{ artist.name }}</span></template
          ></small
        >
        <small v-else class="ellipsis" :data-tooltip="track.artists.join(' / ')">{{
          track.artists.join(' / ') || (track.local ? '本地文件' : '未知歌手')
        }}</small>
      </div>
    </div>
    <span class="track-album ellipsis" role="cell" :data-tooltip="track.album"
      ><button
        v-if="navigable && track.albumId"
        class="track-album-link ellipsis"
        @click.stop="emit('album', track.albumId)"
        @dblclick.stop
      >
        {{ track.album }}</button
      ><template v-else>{{ track.album }}</template></span
    >
    <span role="cell"
      ><button
        class="icon-button like-button"
        :class="{ liked: liked }"
        :aria-label="`${liked ? '取消喜欢' : '喜欢'} ${track.name}`"
        :aria-pressed="liked"
        :disabled="track.missing || busy"
        @click.stop="emit('like', track)"
        @dblclick.stop
      >
        <Icon name="heart" /></button
    ></span>
    <span class="track-time-actions" role="cell"
      ><span class="track-duration">{{ track.missing ? '—' : duration(track.durationMs) }}</span
      ><button
        class="icon-button track-more"
        :aria-label="`更多操作 ${track.name}`"
        aria-haspopup="menu"
        @click.stop="more"
        @dblclick.stop
      >
        <Icon name="more" /></button
    ></span>
  </div>
</template>
