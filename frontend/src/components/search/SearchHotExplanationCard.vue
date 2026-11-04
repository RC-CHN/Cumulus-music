<script setup lang="ts">
import type { SearchHotExplanation, SearchHotTarget } from '../../services/search';
import Icon from '../Icon.vue';
import { ref, watch } from 'vue';

const props = defineProps<{ explanation: SearchHotExplanation }>();
const emit = defineEmits<{ open: [target: SearchHotTarget] }>();
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
        @click="emit('open', explanation.action.target)"
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
.hot-marker {
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
