<script setup lang="ts">
import { computed } from 'vue';
import ActionMenu from './ActionMenu.vue';
import type { MenuItem } from '../services/menu';
import type { RankingPeriod } from '../services/rankings';
const props = defineProps<{ period: RankingPeriod; year: number }>();
const emit = defineEmits<{ period: [period: RankingPeriod]; year: [year: number] }>();
const currentYear = new Date().getFullYear();
const years = computed<MenuItem[]>(() =>
  Array.from({ length: Math.max(1, currentYear - 2018 + 1) }, (_, i) => ({
    key: String(2018 + i),
    label: `${2018 + i}年榜`,
    checked: props.year === 2018 + i,
  })),
);
</script>
<template>
  <div class="rank-periods" aria-label="榜单周期">
    <button :aria-pressed="period === 'daily'" @click="emit('period', 'daily')">日榜</button>
    <button :aria-pressed="period === 'week'" @click="emit('period', 'week')">周榜</button>
    <span class="rank-year" :class="{ active: period === 'year' }">
      <ActionMenu
        label="榜单年份"
        :trigger-text="`${year}年榜`"
        :trigger-pressed="period === 'year'"
        :items="years"
        variant="year"
        focus-checked
        @opened="emit('period', 'year')"
        @select="emit('year', Number($event))"
      />
    </span>
    <button :aria-pressed="period === 'total'" @click="emit('period', 'total')">总榜</button>
  </div>
</template>
<style scoped>
.rank-periods {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  padding: 16px 0;
}
.rank-periods > button,
.rank-year :deep(.menu-trigger) {
  width: auto;
  height: auto;
  padding: 7px 16px;
  border: 0.5px solid var(--line);
  background: var(--surface-alt);
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 18px;
  font-weight: 500;
  border-radius: 20px;
}
.rank-periods > [aria-pressed='true'],
.rank-year.active :deep(.menu-trigger) {
  color: var(--accent);
  background: var(--accent-soft);
  border-color: var(--accent-soft);
}
.rank-year :deep(.menu-trigger)::after {
  content: '';
  width: 6px;
  height: 6px;
  border-right: 1px solid currentColor;
  border-bottom: 1px solid currentColor;
  margin-left: 6px;
  transform: rotate(45deg) translateY(-2px);
  transition: transform 160ms ease;
}
.rank-year :deep([aria-expanded='true'])::after {
  transform: rotate(225deg) translate(-1px, -1px);
}
</style>
