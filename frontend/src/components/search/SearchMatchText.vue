<script setup lang="ts">
import { computed } from 'vue';
import { splitSearchMatch, type SearchTextPart } from '../../services/searchMatch';
const props = defineProps<
  | { text: string; keyword: string; parts?: never }
  | { text?: string; parts: readonly SearchTextPart[]; keyword?: never }
>();
const parts = computed(
  () => props.parts ?? splitSearchMatch(props.text ?? '', props.keyword ?? ''),
);
</script>

<template>
  <template v-for="(part, index) in parts" :key="index">
    <mark v-if="part.match" class="search-match-text">{{ part.text }}</mark>
    <template v-else>{{ part.text }}</template>
  </template>
</template>

<style scoped>
.search-match-text {
  color: var(--search-match-color, var(--link));
  background: transparent;
}
@media (forced-colors: active) {
  .search-match-text {
    color: inherit;
    text-decoration: underline;
  }
}
</style>
