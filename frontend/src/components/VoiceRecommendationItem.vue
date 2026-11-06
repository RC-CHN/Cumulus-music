<script setup lang="ts">
import { computed } from 'vue';
import Icon from './Icon.vue';
import { useCoverImage } from '../composables/useCoverImage';
import { playlistTableCount } from '../services/playlistHeader';
import type { VoiceRecommendationCard } from '../services/voiceRecommendations';

const props = defineProps<{ card: VoiceRecommendationCard; epoch: string }>();
const emit = defineEmits<{ open: [radioId: string] }>();
const cover = useCoverImage(() => ({ owner: props.epoch, source: props.card.coverUrl }));
const count = computed(() => {
  const value = props.card.playCount;
  if (typeof value === 'string') return value ? { text: value, label: `播放次数：${value}` } : null;
  return value > 0 ? playlistTableCount(value, false) : null;
});
</script>

<template>
  <button
    type="button"
    class="voice-recommendation-item"
    data-voice-recommendation-card
    :data-radio-id="card.radioId"
    :aria-label="card.name || '查看播单'"
    @click="emit('open', card.radioId)"
  >
    <span class="voice-recommendation-art">
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
      <Icon v-if="!cover.loaded.value" name="headphones" />
      <span v-if="count" class="voice-recommendation-count" :title="count.label">
        <Icon name="play" />{{ count.text }}
      </span>
    </span>
    <span class="voice-recommendation-name" :title="card.name">{{ card.name }}</span>
  </button>
</template>

<style scoped>
.voice-recommendation-item {
  display: grid;
  align-content: start;
  gap: 8px;
  min-width: 0;
  padding: 0;
  text-align: start;
  border-radius: 10px;
}
.voice-recommendation-art {
  position: relative;
  display: grid;
  place-items: center;
  width: 100%;
  aspect-ratio: 1;
  overflow: hidden;
  border-radius: 10px;
  background: var(--surface-alt);
  color: var(--muted);
}
.voice-recommendation-art > .icon {
  width: 24%;
  height: 24%;
}
.voice-recommendation-art img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.voice-recommendation-count {
  position: absolute;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 3px;
  inset: 0 0 auto;
  padding: 7px 10px 20px;
  background: linear-gradient(rgb(0 0 0 / 48%), transparent);
  color: white;
  font-size: clamp(11px, 1.15vw, 15px);
  font-weight: 700;
  pointer-events: none;
}
.voice-recommendation-count .icon {
  width: 12px;
  height: 12px;
}
.voice-recommendation-name {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  min-height: 36px;
  overflow: hidden;
  overflow-wrap: anywhere;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 18px;
}
.voice-recommendation-item:hover .voice-recommendation-name,
.voice-recommendation-item:focus-visible .voice-recommendation-name {
  color: var(--text);
}
</style>
