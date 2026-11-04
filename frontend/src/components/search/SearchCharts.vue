<script setup lang="ts">
import Icon from '../Icon.vue';
import type { SearchChart, SearchChannel, SearchChartDestination } from '../../services/search';
defineProps<{
  charts: SearchChart[];
  playable: boolean;
  pending: Record<string, boolean>;
  errors: Record<string, string>;
}>();
const emit = defineEmits<{
  select: [keyword: string, channel: SearchChannel | undefined, display: string];
  play: [keywords: string[]];
  radio: [id: string];
  discover: [target: SearchChartDestination];
  retry: [id: string];
}>();
</script>
<template>
  <section v-for="chart in charts" :key="chart.id" class="search-chart">
    <div class="search-chart-title">
      <h3>{{ chart.name }}</h3>
      <button
        v-if="
          chart.needPlay &&
          chart.items.length &&
          !pending['chart:' + chart.id] &&
          !errors['chart:' + chart.id]
        "
        class="search-chart-play"
        :aria-label="'播放' + chart.name"
        :disabled="!playable || !chart.playKeywords?.length"
        @click="chart.playKeywords?.length && emit('play', chart.playKeywords)"
      >
        <Icon name="play" />播放
      </button>
      <button
        v-if="
          chart.more &&
          chart.items.length &&
          !pending['chart:' + chart.id] &&
          !errors['chart:' + chart.id]
        "
        class="search-chart-play"
        data-search-choice
        :aria-label="`${chart.name}：${chart.more.label}`"
        @click="emit('discover', chart.more.target)"
      >
        {{ chart.more.label }}<Icon name="forward" />
      </button>
    </div>
    <p v-if="pending['chart:' + chart.id]" class="search-muted" role="status">正在加载榜单…</p>
    <div v-else-if="errors['chart:' + chart.id]" class="search-section-error" role="alert">
      <span>{{ errors['chart:' + chart.id] }}</span
      ><button @click="emit('retry', chart.id)">重新加载榜单</button>
    </div>
    <p v-else-if="!chart.items.length" class="search-muted">暂无榜单内容</p>
    <div v-else class="search-chart-items">
      <button
        v-for="(word, i) in chart.items.slice(0, 9)"
        :key="i"
        data-search-choice
        :title="chart.audio && !word.radioId ? `${word.label} · 暂不可打开` : word.label"
        :disabled="chart.audio && !word.radioId"
        @click="
          chart.audio
            ? word.radioId && emit('radio', word.radioId)
            : emit('select', word.keyword, word.channel, word.label)
        "
      >
        <span class="search-rank" :class="{ leading: i < 3 }">{{ i + 1 }}</span
        ><span class="search-word-label">{{ word.label }}</span
        ><small v-if="chart.audio && !word.radioId" class="search-word-tag">暂不可打开</small>
        <small v-else-if="word.tag" class="search-word-tag">{{ word.tag }}</small>
      </button>
    </div>
  </section>
</template>

<style scoped>
.search-chart-title {
  display: flex;
  align-items: center;
  gap: 6px;
}
.search-chart-play {
  display: inline-flex;
  align-items: center;
  flex: none;
  gap: 4px;
  height: 22px;
  padding: 0 8px;
  border: 1px solid var(--line);
  border-radius: 999px;
  font-size: 12px;
}
.search-chart-play:hover:not(:disabled) {
  background: var(--surface-hover);
}
.search-chart-play svg {
  width: 16px;
  height: 16px;
}
</style>
