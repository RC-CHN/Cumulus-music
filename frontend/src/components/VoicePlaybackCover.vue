<script setup lang="ts">
import { computed } from 'vue';
import { useCoverImage } from '../composables/useCoverImage';
import { formatVoicePlayCount } from '../services/voiceDetail';
import Icon from './Icon.vue';

const props = defineProps<{
  epoch: string;
  coverUrl?: string;
  playCount?: number | string;
}>();
const cover = useCoverImage(() => ({ owner: props.epoch, source: props.coverUrl }));
const label = computed(() => formatVoicePlayCount(props.playCount));
</script>

<template>
  <div class="voice-playback-cover" data-voice-cover :data-cover-state="cover.state.value">
    <img
      v-if="cover.showImage.value"
      v-show="cover.loaded.value"
      :key="cover.generation.value"
      :src="cover.source.value!"
      :data-cover-generation="cover.generation.value"
      alt="节目封面"
      referrerpolicy="no-referrer"
      @load="cover.load"
      @error="cover.error"
    />
    <Icon v-if="!cover.loaded.value" name="music" data-cover-placeholder />
    <div v-if="label !== null" class="voice-playback-count-mask">
      <span class="voice-playback-count">
        <Icon name="headphones" />
        <span class="sr-only">播放量：</span>
        <span class="voice-playback-count-label" data-voice-play-count>{{ label }}</span>
      </span>
    </div>
  </div>
</template>

<style scoped>
.voice-playback-cover {
  position: relative;
  /* M53: 400px at the layout's base scale (440px + 476px by 592px after gutters).
     Cap by the space in the current player column. */
  width: max(
    0px,
    min(
      100%,
      calc((min(100vw, 1920px) - 140px) * 0.4366812227),
      calc((min(100vh, 1920px) - 160px) * 0.6756756757)
    )
  );
  aspect-ratio: 1;
  flex: none;
  border-radius: 8px;
  overflow: hidden;
  background: var(--surface-alt);
  display: grid;
  place-items: center;
}
.voice-playback-cover img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.voice-playback-cover > .icon {
  width: min(25%, 80px);
  height: auto;
  color: var(--muted);
}
.voice-playback-count-mask {
  position: absolute;
  inset: 0 0 auto;
  height: 110px;
  max-height: 100%;
  background: linear-gradient(180deg, rgb(0 0 0 / 66%), transparent);
  pointer-events: none;
}
.voice-playback-count {
  position: absolute;
  top: 23px;
  right: 20px;
  display: flex;
  align-items: center;
  gap: 4px;
  max-width: calc(100% - 40px);
  color: white;
  font-size: 22px;
  font-weight: 700;
  line-height: 1;
  text-shadow: 0 1px 3px rgb(0 0 0 / 80%);
}
.voice-playback-count .icon {
  width: 20px;
  height: 20px;
  flex: none;
}
.voice-playback-count-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
