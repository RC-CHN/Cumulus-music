<script setup lang="ts">
import { computed } from 'vue';
import type { TrackView } from '../services/contracts';
import { artworkAt } from '../services/artwork';
import { useCoverImage } from '../composables/useCoverImage';
import Icon from './Icon.vue';

const props = defineProps<{
  track: TrackView;
  index: number;
  selected: boolean;
  focused: boolean;
  playable: boolean;
  copyrightBlocked: boolean;
  liked: boolean;
  likeBusy: boolean;
}>();
const emit = defineEmits<{
  select: [gesture: 'down' | 'up' | 'activate', event: MouseEvent | KeyboardEvent];
  focus: [];
  play: [gesture: 'cover' | 'double_click'];
  more: [event: MouseEvent | KeyboardEvent];
  like: [];
  artist: [id: string];
}>();
const credits = computed(() =>
  props.track.artistRefs?.length
    ? props.track.artistRefs
    : props.track.artists.map((name) => ({ name, id: null })),
);
const title = computed(() => props.track.name || '未知歌曲');
const cover = useCoverImage(() => ({
  owner: String(props.index),
  source: props.track.coverUrl ? artworkAt(props.track.coverUrl, 120) : undefined,
}));
function rowSurface(event: Event) {
  return event.target instanceof Element && !event.target.closest('button, a');
}
function down(event: MouseEvent) {
  if (!rowSurface(event)) return;
  emit('select', 'down', event);
  if (event.button === 0) (event.currentTarget as HTMLElement).focus({ preventScroll: true });
}
function up(event: MouseEvent) {
  if (rowSurface(event)) emit('select', 'up', event);
}
function clickRow(event: MouseEvent) {
  // Programmatic/assistive click may have no pointer down/up. Real pointer
  // clicks already selected above; reprocessing click would undo modifiers.
  if (event.detail !== 0 || !rowSurface(event)) return;
  emit('select', 'activate', event);
  (event.currentTarget as HTMLElement).focus({ preventScroll: true });
}
function doubleClick(event: MouseEvent) {
  if (props.playable && rowSurface(event)) emit('play', 'double_click');
}
function more(event: MouseEvent | KeyboardEvent) {
  const row = event.currentTarget;
  if (row instanceof HTMLElement && !row.contains(document.activeElement))
    row.focus({ preventScroll: true });
  emit('more', event);
}
function key(event: KeyboardEvent) {
  if (event.currentTarget !== event.target) return;
  if ((event.shiftKey && event.key === 'F10') || event.key === 'ContextMenu') {
    event.preventDefault();
    more(event);
  } else if (event.key === ' ' || event.key === 'Enter') {
    event.preventDefault();
    if (event.key === 'Enter' && props.playable) emit('play', 'double_click');
    else emit('select', 'activate', event);
  }
}
</script>

<template>
  <div
    class="voice-song-row"
    :class="{ selected }"
    data-voice-song-row
    :data-row-index="index"
    :data-track-id="track.id"
    :data-copyright-blocked="copyrightBlocked || undefined"
    role="row"
    :aria-rowindex="index + 1"
    :aria-selected="selected"
    aria-haspopup="menu"
    :tabindex="focused ? 0 : -1"
    @focus.self="emit('focus')"
    @mousedown="down"
    @mouseup="up"
    @click="clickRow"
    @dblclick="doubleClick"
    @contextmenu.prevent.stop="more"
    @keydown="key"
  >
    <div class="voice-song-cell" role="gridcell">
      <button
        type="button"
        class="voice-song-cover"
        data-voice-song-cover
        :aria-label="`播放 ${title}`"
        :title="!playable ? '当前无法在线播放' : undefined"
        :disabled="!playable"
        @click.stop="emit('play', 'cover')"
        @dblclick.stop
      >
        <img
          v-if="cover.showImage.value"
          v-show="cover.loaded.value"
          :key="cover.generation.value"
          :src="cover.source.value!"
          :data-cover-generation="cover.generation.value"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
          @load="cover.load"
          @error="cover.error"
        />
        <Icon v-if="!cover.loaded.value" name="music" />
        <span class="voice-song-cover-play" aria-hidden="true"><Icon name="play" /></span>
      </button>
      <div class="voice-song-description">
        <span class="voice-song-title" :title="title">{{ title }}</span>
        <span v-if="credits.length" class="voice-song-credits">
          <template v-for="(artist, i) in credits" :key="i">
            <span v-if="i"> / </span>
            <button
              v-if="artist.id"
              type="button"
              :title="artist.name"
              @click.stop="emit('artist', artist.id)"
              @dblclick.stop
            >
              {{ artist.name }}
            </button>
            <span v-else>{{ artist.name }}</span>
          </template>
        </span>
      </div>
      <button
        type="button"
        class="icon-button voice-song-heart"
        data-voice-song-heart
        :aria-label="`${liked ? '取消喜欢' : '喜欢'} ${title}`"
        :aria-pressed="liked"
        :disabled="likeBusy"
        @click.stop="emit('like')"
        @dblclick.stop
      >
        <Icon name="heart" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.voice-song-row {
  height: 60px;
  box-sizing: border-box;
}
.voice-song-row:hover,
.voice-song-row:focus-within,
.voice-song-row.selected {
  background: color-mix(in srgb, var(--text) 9%, transparent);
}
.voice-song-cell {
  height: 100%;
  padding-inline: 20px;
  display: grid;
  grid-template-columns: 48px minmax(0, 1fr) 32px;
  gap: 10px;
  align-items: center;
}
.voice-song-cover {
  width: 48px;
  height: 48px;
  display: grid;
  place-items: center;
  position: relative;
  border-radius: 8px;
  overflow: hidden;
  background: color-mix(in srgb, var(--text) 8%, transparent);
  color: var(--text-secondary);
  padding: 0;
}
.voice-song-cover img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.voice-song-cover-play {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  background: rgb(0 0 0 / 45%);
  color: white;
  opacity: 0;
}
.voice-song-cover:not(:disabled):is(:hover, :focus-visible) .voice-song-cover-play {
  opacity: 1;
}
.voice-song-description {
  display: grid;
  gap: 4px;
  min-width: 0;
}
.voice-song-title,
.voice-song-credits {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.voice-song-title {
  font-size: 16px;
  font-weight: 500;
}
.voice-song-credits,
.voice-song-credits button {
  font-size: 13px;
  color: var(--text-secondary);
}
.voice-song-credits button:hover {
  color: var(--text);
}
.voice-song-heart {
  opacity: 0;
  justify-self: center;
}
.voice-song-row:is(:hover, :focus-within) .voice-song-heart {
  opacity: 1;
}
.voice-song-heart[aria-pressed='true'] {
  color: var(--accent);
}
.voice-song-heart[aria-pressed='true'] :deep(path) {
  fill: currentColor;
}
</style>
