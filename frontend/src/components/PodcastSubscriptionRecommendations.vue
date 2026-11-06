<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { PodcastSubscriptionRecommendation } from '../services/podcast';
import Icon from './Icon.vue';

const props = defineProps<{ cards: PodcastSubscriptionRecommendation[]; disabled: boolean }>();
const emit = defineEmits<{ open: [id: string] }>();
const viewport = ref<HTMLElement>();
const width = ref(0),
  start = ref(0);
const gap = 20;
// GridCarousel uses miniNum=4 (span 3), capped at six cards.
const columns = computed(() => Math.max(1, Math.min(6, Math.floor((width.value + gap) / 198))));
const maxStart = computed(() => Math.max(0, props.cards.length - columns.value));
const cell = computed(() => Math.max(0, (width.value - gap * (columns.value - 1)) / columns.value));
const count = new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 });
let resize: ResizeObserver | undefined;
onMounted(() => {
  if (!viewport.value) return;
  width.value = viewport.value.clientWidth;
  resize = new ResizeObserver(([entry]) => {
    width.value = entry.contentRect.width;
  });
  resize.observe(viewport.value);
});
onBeforeUnmount(() => resize?.disconnect());
watch(columns, () => (start.value = 0));
watch(maxStart, (limit) => (start.value = Math.min(start.value, limit)));
function move(direction: -1 | 1) {
  start.value = Math.max(0, Math.min(maxStart.value, start.value + direction * columns.value));
}
</script>

<template>
  <section
    class="subscription-recommendations"
    aria-label="为你推荐"
    data-podcast-subscription-recommendations
  >
    <h2>为你推荐</h2>
    <div ref="viewport" class="recommendation-viewport">
      <div
        class="recommendation-rail"
        :style="{
          gridAutoColumns: cell + 'px',
          transform: 'translateX(' + -(cell + gap) * start + 'px)',
        }"
      >
        <button
          v-for="(card, index) in cards"
          :key="card.id"
          type="button"
          class="recommendation-card"
          :data-recommended-radio-id="card.id"
          :inert="index < start || index >= start + columns"
          :disabled="disabled || card.unavailable"
          @click="emit('open', card.id)"
        >
          <span class="recommendation-cover">
            <img
              v-if="card.coverUrl"
              :src="card.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer"
            />
            <Icon v-else name="music" />
            <span v-if="card.playCount !== null && card.playCount > 0" class="recommendation-count">
              <Icon name="headphones" />{{ count.format(card.playCount) }}
            </span>
          </span>
          <span class="recommendation-name" :title="card.name">{{ card.name }}</span>
        </button>
      </div>
    </div>
    <template v-if="maxStart > 0">
      <button
        class="recommendation-arrow recommendation-previous"
        type="button"
        aria-label="上一组推荐播客"
        :aria-disabled="start === 0"
        @click="move(-1)"
      >
        <Icon name="back" />
      </button>
      <button
        class="recommendation-arrow recommendation-next"
        type="button"
        aria-label="下一组推荐播客"
        :aria-disabled="start === maxStart"
        @click="move(1)"
      >
        <Icon name="back" />
      </button>
    </template>
  </section>
</template>

<style scoped>
.subscription-recommendations {
  position: relative;
  padding-top: 39px;
  margin-bottom: 20px;
  min-width: 0;
}
.subscription-recommendations h2 {
  font-size: 22px;
  line-height: 1.4;
  margin: 0 0 13px;
}
.recommendation-viewport {
  overflow: hidden;
}
.recommendation-rail {
  display: grid;
  grid-auto-flow: column;
  gap: 20px;
  transition: transform 0.6s ease;
}
.recommendation-card {
  min-width: 0;
  border: 0;
  border-radius: 6px;
  overflow: hidden;
  padding: 0;
  background: var(--surface-alt);
  color: inherit;
  text-align: left;
}
.recommendation-card:focus-visible {
  outline-offset: -2px;
}
.recommendation-cover {
  position: relative;
  display: grid;
  width: 100%;
  aspect-ratio: 1;
  place-items: center;
  background: #713948;
  color: white;
}
.recommendation-cover > img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.recommendation-cover > .icon {
  width: 35%;
  height: 35%;
}
.recommendation-count {
  position: absolute;
  top: 0;
  inset-inline: 0;
  display: flex;
  gap: 4px;
  align-items: center;
  justify-content: flex-end;
  padding: 8px 10px;
  font-size: 12px;
  background: linear-gradient(#0007, transparent);
}
.recommendation-count .icon {
  width: 12px;
  height: 12px;
}
.recommendation-name {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  min-height: 48px;
  padding: 6px 12px;
  overflow-wrap: anywhere;
}
.recommendation-arrow {
  position: absolute;
  top: calc(50% + 35px);
  width: 26px;
  height: 32px;
  border: 0;
  border-radius: 6px;
  color: inherit;
  background: var(--sidebar-bg);
  transform: translateY(-50%);
}
.recommendation-arrow[aria-disabled='true'] {
  opacity: 0.45;
  cursor: default;
}
.recommendation-previous {
  left: 0;
}
.recommendation-next {
  right: 0;
}
.recommendation-next .icon {
  transform: rotate(180deg);
}
@media (prefers-reduced-motion: reduce) {
  .recommendation-rail {
    transition: none;
  }
}
</style>
