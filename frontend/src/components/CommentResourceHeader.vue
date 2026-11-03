<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useCore } from '../services/core';
import type { CommentResource } from '../services/comments';
import { resourceKey } from '../services/comments';
import {
  currentCommentResource,
  type CommentResourceCard,
  type CommentResourceDestination,
} from '../services/commentResource';
import type { PlayerView } from '../services/player';
import { useCommentResource } from '../composables/useCommentResource';
import { usePlaybackTriggerFocus } from '../composables/usePlaybackTriggerFocus';
import { useCommentReadFocus } from '../composables/useCommentReadFocus';
import Icon from './Icon.vue';
const props = defineProps<{
  epoch: string;
  resource: CommentResource;
  title: string;
  playable: boolean;
  player?: PlayerView | null;
}>();
const emit = defineEmits<{
  play: [card: CommentResourceCard];
  open: [destination: CommentResourceDestination];
  expired: [];
}>();
const model = useCommentResource(useCore(), () => emit('expired'));
const root = ref<HTMLElement>(),
  failed = ref(false);
watch(
  () => [props.epoch, resourceKey(props.resource)],
  () => void model.connect(props.epoch, props.resource),
  { immediate: true, flush: 'sync' },
);
onUnmounted(model.disconnect);
const card = computed(() => model.view.value?.card);
const name = computed(() =>
  card.value?.kind === 'track'
    ? card.value.track.name
    : card.value?.kind === 'voice'
      ? card.value.episode.name
      : card.value?.kind === 'mv'
        ? card.value.title
        : props.title,
);
const cover = computed(() =>
  card.value?.kind === 'track'
    ? card.value.track.coverUrl
    : card.value?.kind === 'voice'
      ? card.value.episode.coverUrl
      : card.value?.coverUrl,
);
watch(cover, () => {
  failed.value = false;
});
const artists = computed(() =>
  card.value?.kind === 'track'
    ? (card.value.track.artistRefs ?? card.value.track.artists.map((name) => ({ id: null, name })))
    : card.value?.kind === 'mv'
      ? card.value.artists
      : [],
);
const active = computed(() => !!card.value && currentCommentResource(card.value, props.player));
const playing = computed(
  () => active.value && ['playing', 'buffering'].includes(props.player?.state ?? ''),
);
const available = computed(() => card.value?.kind === 'mv' || (!!card.value && props.playable));
const focus = usePlaybackTriggerFocus(
  () => available.value,
  root,
  () => !root.value?.closest('.page-leave-active'),
);
function play() {
  if (!card.value || !available.value) return;
  const captured = card.value;
  if (captured.kind === 'mv') emit('open', { kind: 'mv', id: captured.id });
  else focus.start(() => emit('play', captured));
}
const retry = useCommentReadFocus(
  root,
  () => `${props.epoch}:${resourceKey(props.resource)}`,
  () => resourceKey(props.resource),
);
</script>
<template>
  <section
    ref="root"
    class="comment-resource-header"
    aria-label="评论对应的资源"
    :aria-busy="model.loading.value"
    :data-resource-kind="resource.kind"
    :data-resource-id="resource.id"
  >
    <div class="comment-resource-card">
      <div
        class="comment-resource-cover"
        :class="{ 'comment-resource-video': card?.kind === 'mv' }"
      >
        <img
          v-if="cover && !failed"
          :src="cover"
          alt=""
          decoding="async"
          referrerpolicy="no-referrer"
          @error="failed = true"
        />
        <Icon v-else :name="card?.kind === 'mv' ? 'video' : 'music'" />
        <button
          v-if="card"
          class="comment-resource-play"
          :class="{ 'is-current': active }"
          :aria-label="`${playing ? '暂停' : '播放'}：${name}`"
          :disabled="!available"
          @click="play"
        >
          <Icon :name="playing ? 'pause' : 'play'" />
        </button>
      </div>
      <div class="comment-resource-copy">
        <div class="comment-resource-title">
          <span v-if="card" class="comment-resource-type">{{
            card.kind === 'track' ? '单曲' : card.kind === 'voice' ? '声音' : 'MV'
          }}</span
          ><span tabindex="-1" data-comment-resource-name :title="name">{{ name }}</span>
        </div>
        <div v-if="card" class="comment-resource-info">
          <span v-if="card.kind === 'track' && card.track.album"
            >专辑：<button
              v-if="card.track.albumId"
              class="text-button"
              @click="emit('open', { kind: 'album', id: card.track.albumId })"
            >
              {{ card.track.album }}</button
            ><span v-else>{{ card.track.album }}</span></span
          >
          <template v-if="card.kind === 'voice'"
            ><span v-if="card.episode.radioName"
              >播单：<button
                v-if="card.episode.radioId"
                class="text-button"
                @click="emit('open', { kind: 'radio', id: card.episode.radioId })"
              >
                {{ card.episode.radioName }}</button
              ><span v-else>{{ card.episode.radioName }}</span></span
            ><span v-if="card.episode.creator"
              >主播：<button
                v-if="card.creatorAccountId"
                class="text-button"
                type="button"
                @click="emit('open', { kind: 'user', id: card.creatorAccountId })"
              >
                {{ card.episode.creator }}</button
              ><span v-else>{{ card.episode.creator }}</span></span
            ></template
          >
          <span v-else-if="artists.length"
            >歌手：<template v-for="(artist, index) in artists" :key="index"
              ><span v-if="index"> / </span
              ><button
                v-if="artist.id"
                class="text-button"
                @click="emit('open', { kind: 'artist', id: artist.id })"
              >
                {{ artist.name }}</button
              ><span v-else>{{ artist.name }}</span></template
            ></span
          >
        </div>
        <p v-if="model.loading.value" class="muted" role="status">正在读取资源信息…</p>
      </div>
    </div>
    <p v-if="model.error.value" class="comment-resource-error" role="alert">
      资源信息读取失败：{{ model.error.value
      }}<button
        class="text-button"
        :disabled="model.loading.value"
        @click="retry($event, model.read)"
      >
        重试资源信息
      </button>
    </p>
    <p v-if="model.view.value?.cached" class="form-warning">当前显示缓存资源信息。</p>
  </section>
</template>
<style scoped>
.comment-resource-header {
  min-height: 72px;
  position: relative;
  margin-bottom: 12px;
}
.comment-resource-card {
  display: flex;
  align-items: center;
  gap: 16px;
  min-height: 72px;
}
.comment-resource-cover {
  position: relative;
  width: 64px;
  height: 64px;
  border-radius: 8px;
  overflow: hidden;
  flex: none;
  background: var(--surface-alt);
  display: grid;
  place-items: center;
}
.comment-resource-video {
  width: 114px;
}
.comment-resource-cover > img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.comment-resource-cover > .icon {
  width: 28px;
  height: 28px;
  color: var(--muted);
}
.comment-resource-play {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  background: #0006;
  color: white;
  opacity: 0;
  transition: opacity 140ms;
}
.comment-resource-cover:hover .comment-resource-play,
.comment-resource-play:focus-visible,
.comment-resource-play.is-current {
  opacity: 1;
}
.comment-resource-play:focus-visible {
  outline-offset: -3px;
}
.comment-resource-copy {
  min-width: 0;
  flex: 1;
}
.comment-resource-title {
  display: flex;
  align-items: baseline;
  gap: 4px;
  font-size: 16px;
  font-weight: 600;
  line-height: 22px;
}
.comment-resource-title > [data-comment-resource-name] {
  min-width: 0;
  overflow-wrap: anywhere;
  user-select: text;
}
.comment-resource-title > [data-comment-resource-name]:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
  border-radius: 2px;
}
.comment-resource-type {
  color: var(--accent);
  border: 1px solid currentColor;
  border-radius: 2px;
  padding: 0 3px;
  line-height: 16px;
  font-size: 11px;
  font-weight: 400;
  flex: none;
}
.comment-resource-info {
  margin-top: 4px;
  display: flex;
  flex-wrap: wrap;
  gap: 4px 20px;
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 18px;
  overflow-wrap: anywhere;
}
.comment-resource-info .text-button {
  font-size: inherit;
  line-height: inherit;
  padding: 0;
  color: inherit;
  text-align: left;
}
.comment-resource-info .text-button:hover {
  color: var(--link);
}
.comment-resource-info button:focus-visible {
  outline-offset: 0;
}
.comment-resource-error {
  color: var(--danger);
  font-size: 13px;
  overflow-wrap: anywhere;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
@media (prefers-reduced-motion: reduce) {
  .comment-resource-play {
    transition: none;
  }
}
:global(:root[data-motion='reduced']) .comment-resource-play {
  transition: none;
}
.comment-resource-header:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
  border-radius: 8px;
}
</style>
