<script setup lang="ts">
import type { useMvSocial } from '../composables/useMvSocial';
import { videoTime } from '../services/mv';
const props = defineProps<{ model: ReturnType<typeof useMvSocial> }>();
const emit = defineEmits<{ open: [id: string] }>();
const { recommendations, recommendationLoading, recommendationError } = props.model;
</script>
<template>
  <section class="mv-recommendations" aria-label="相似MV">
    <div class="page-heading">
      <h2>相似 MV</h2>
      <button
        class="text-button"
        :disabled="recommendationLoading"
        @click="model.loadRecommendations"
      >
        刷新推荐
      </button>
    </div>
    <p v-if="recommendationError" class="form-warning" role="alert">{{ recommendationError }}</p>
    <p v-if="recommendations?.cached" class="form-warning">当前显示缓存推荐。</p>
    <p v-if="recommendationLoading && !recommendations" class="muted" role="status">
      正在读取推荐…
    </p>
    <p v-else-if="recommendations && !recommendations.items.length" class="muted">暂无相似 MV</p>
    <div v-if="recommendations" class="mv-grid">
      <button
        v-for="card in recommendations.items"
        :key="card.id"
        class="mv-card"
        :data-recommended-mv="card.id"
        :aria-label="`播放相似MV ${card.title}`"
        @click="emit('open', card.id)"
      >
        <span class="mv-cover"
          ><img
            v-if="card.coverUrl"
            :src="card.coverUrl"
            alt=""
            loading="lazy"
            referrerpolicy="no-referrer"
          /><span class="mv-duration">{{ videoTime(card.durationMs / 1000) }}</span
          ><span class="mv-hover-play">▶</span></span
        >
        <span class="mv-card-title">{{ card.title }}</span
        ><small>{{ card.artists.join(' / ') }}</small>
      </button>
    </div>
  </section>
</template>
