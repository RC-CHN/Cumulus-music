<script setup lang="ts">
import type {
  SearchHotExplanation,
  SearchHotPlaybackTarget,
  SearchHotTarget,
} from '../../services/search';
import Icon from '../Icon.vue';
import { ref, watch } from 'vue';

const props = defineProps<{
  explanation: SearchHotExplanation;
  playable: boolean;
  detailReady: boolean;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [target: SearchHotTarget, anchor: HTMLButtonElement];
  play: [target: SearchHotPlaybackTarget, anchor: HTMLButtonElement];
}>();
const imageFailed = ref(false);
watch(
  () => props.explanation.pictureUrl,
  () => {
    imageFailed.value = false;
  },
);
</script>

<template>
  <article class="hot-explanation" data-result-key="hot-explanation" aria-label="热点说明">
    <div class="hot-cover">
      <img
        v-if="explanation.pictureUrl && !imageFailed"
        :src="explanation.pictureUrl"
        alt=""
        referrerpolicy="no-referrer"
        @error="imageFailed = true"
      />
      <Icon v-else name="info" />
      <button
        v-if="explanation.playTarget"
        type="button"
        class="hot-cover-play"
        data-result-action="play"
        :class="{ playing }"
        :aria-label="`${playing ? '暂停' : '播放'}${explanation.title ?? '热点'}`"
        :disabled="!playable"
        @click.stop="
          emit('play', explanation.playTarget, $event.currentTarget as HTMLButtonElement)
        "
      >
        <Icon :name="playing ? 'pause' : 'play'" />
      </button>
      <span v-if="explanation.pictureCornerMarker" class="hot-marker">{{
        explanation.pictureCornerMarker
      }}</span>
    </div>
    <div class="hot-description">
      <h2>{{ explanation.title }}</h2>
      <p>{{ explanation.text }}</p>
    </div>
    <div v-if="explanation.hotValue || explanation.action" class="hot-controls">
      <span
        v-if="explanation.hotValue"
        class="hot-value"
        :aria-label="`热度 ${explanation.hotValue}`"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path
            d="M13 2c1 5-3 6-3 10-2-1-2-3-2-4-3 3-4 5-4 8a8 8 0 0 0 16 0c0-4-3-8-7-14Z"
            fill="currentColor"
          />
        </svg>
        {{ explanation.hotValue }}
      </span>
      <button
        v-if="explanation.action"
        type="button"
        class="hot-action"
        data-result-action="open"
        :disabled="
          (explanation.action.target.kind === 'song' ||
            explanation.action.target.kind === 'voice') &&
          !detailReady
        "
        @click="emit('open', explanation.action.target, $event.currentTarget as HTMLButtonElement)"
      >
        {{ explanation.action.label }}
      </button>
    </div>
  </article>
</template>

<style scoped>
.hot-explanation {
  display: grid;
  grid-template-columns: 64px minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  min-height: 84px;
  padding: 10px 24px 10px 10px;
  margin: 8px 0 0;
  border-radius: 10px;
  background: var(--surface-alt);
  box-sizing: border-box;
}
.hot-cover {
  position: relative;
  display: grid;
  place-items: center;
  overflow: hidden;
  width: 64px;
  height: 64px;
  border-radius: 8px;
  background: var(--art-bg);
  color: var(--art-color);
}
.hot-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.hot-cover > .icon {
  width: 24px;
  height: 24px;
}
.hot-cover-play {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  border: 0;
  background: #0008;
  color: white;
  opacity: 0;
}
.hot-cover-play:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.hot-cover:hover .hot-cover-play:not(:disabled),
.hot-cover-play:focus-visible,
.hot-cover-play.playing {
  opacity: 1;
}
.hot-cover:has(.hot-cover-play:is(:focus-visible, .playing)) > .icon,
.hot-cover:hover:has(.hot-cover-play:not(:disabled)) > .icon {
  visibility: hidden;
}
.hot-cover-play .icon {
  width: 28px;
  height: 28px;
}
.hot-marker {
  pointer-events: none;
  position: absolute;
  left: 4px;
  right: 4px;
  bottom: 4px;
  justify-self: start;
  max-width: calc(100% - 8px);
  padding: 0 3px;
  overflow: hidden;
  border-radius: 2px;
  background: #000a;
  color: white;
  font-size: 9px;
  line-height: 14px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.hot-description h2 {
  overflow: hidden;
  margin: 0 0 4px;
  font-size: 18px;
  line-height: 22px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.hot-description p {
  display: -webkit-box;
  margin: 0;
  overflow: hidden;
  color: var(--muted);
  font-size: 13px;
  line-height: 18px;
  overflow-wrap: anywhere;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}
.hot-controls,
.hot-value {
  display: flex;
  align-items: center;
  gap: 10px;
}
.hot-value {
  gap: 4px;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
.hot-value svg {
  width: 16px;
  height: 16px;
}
.hot-action {
  max-width: 144px;
  overflow: hidden;
  padding: 4px 18px;
  min-height: 34px;
  border: 1px solid currentColor;
  border-radius: 20px;
  background: transparent;
  color: var(--accent);
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}
.hot-action:hover {
  background: var(--accent-soft);
}
@media (max-width: 820px) {
  .hot-explanation {
    padding-right: 12px;
    gap: 10px;
  }
  .hot-controls {
    flex-direction: column;
    gap: 4px;
  }
  .hot-action {
    max-width: 106px;
    padding-inline: 10px;
  }
}
</style>
