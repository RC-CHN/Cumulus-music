<script setup lang="ts">
import { onBeforeUnmount, ref, useId, watch } from 'vue';
import Icon from './Icon.vue';
import VoiceRecommendationItem from './VoiceRecommendationItem.vue';
import { useMotion } from '../composables/useMotion';
import type { VoiceRecommendationsPage } from '../services/voiceRecommendations';

const props = defineProps<{ page: VoiceRecommendationsPage; epoch: string; paging: boolean }>();
const emit = defineEmits<{ next: []; open: [page: VoiceRecommendationsPage, radioId: string] }>();
const label = useId();
const icon = ref<HTMLElement>();
const motion = useMotion();
let spin: Animation | undefined;
function next() {
  if (props.paging) return;
  emit('next');
  if (motion.reduced.value) return;
  spin?.cancel();
  spin = icon.value?.animate?.([{ transform: 'rotate(0deg)' }, { transform: 'rotate(360deg)' }], {
    duration: 500,
    easing: 'ease-out',
  });
}
watch(motion.reduced, (reduced) => {
  if (reduced) spin?.cancel();
});
onBeforeUnmount(() => spin?.cancel());
</script>

<template>
  <section
    class="voice-recommendations"
    data-voice-recommendations
    data-non-selectable
    :data-page="page.page"
    :data-page-count="page.pageCount"
    :aria-labelledby="label"
  >
    <h2>
      <button
        type="button"
        class="voice-recommendations-next"
        data-voice-recommendations-next
        :aria-disabled="paging || undefined"
        title="换一批"
        @click="next"
      >
        <span ref="icon" class="voice-recommendations-icon" aria-hidden="true">
          <Icon name="refresh" />
        </span>
        <span :id="label">听了该声音的人也在听</span>
      </button>
    </h2>
    <span class="sr-only" role="status">第 {{ page.page }} 页，共 {{ page.pageCount }} 页</span>
    <div class="voice-recommendations-grid">
      <VoiceRecommendationItem
        v-for="(card, index) in page.cards"
        :key="`${page.page}:${index}:${card.radioId}`"
        :card="card"
        :epoch="epoch"
        @open="emit('open', page, $event)"
      />
    </div>
  </section>
</template>

<style scoped>
.voice-recommendations {
  display: grid;
  gap: 20px;
  padding: 20px clamp(12px, 2vw, 24px);
  border-radius: 12px;
  background: color-mix(in srgb, var(--text) 7%, transparent);
}
.voice-recommendations h2 {
  margin: 0;
  font-size: inherit;
}
.voice-recommendations-next {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0;
  text-align: start;
  color: var(--text);
  font-size: clamp(16px, 1.7vw, 20px);
  font-weight: 600;
  line-height: 24px;
  letter-spacing: 0.5px;
}
.voice-recommendations-icon {
  display: inline-grid;
  flex: none;
  place-items: center;
  width: 24px;
  height: 24px;
  color: var(--text-secondary);
}
.voice-recommendations-icon .icon {
  width: 100%;
  height: 100%;
}
.voice-recommendations-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 15px clamp(8px, 2vw, 24px);
}
</style>
