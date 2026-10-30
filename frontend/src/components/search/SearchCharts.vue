<script setup lang="ts">
import type { SearchChart } from '../../services/search';
defineProps<{
  charts: SearchChart[];
  pending: Record<string, boolean>;
  errors: Record<string, string>;
}>();
const emit = defineEmits<{ select: [keyword: string]; retry: [id: string] }>();
</script>
<template>
  <section v-for="chart in charts" :key="chart.id" class="search-chart">
    <h3>{{ chart.name }}</h3>
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
        :title="word.label"
        @click="emit('select', word.keyword)"
      >
        <span class="search-rank" :class="{ leading: i < 3 }">{{ i + 1 }}</span
        ><span class="search-word-label">{{ word.label }}</span
        ><small v-if="word.tag" class="search-word-tag">{{ word.tag }}</small>
      </button>
    </div>
  </section>
</template>
