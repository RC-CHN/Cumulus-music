<script setup lang="ts">
import { computed } from 'vue';
import { commentTextParts } from '../services/commentEmoji';
const props = defineProps<{ text: string }>();
const parts = computed(() => commentTextParts(props.text));
</script>

<template>
  <template v-for="(part, index) in parts" :key="index"
    ><template v-if="part.kind === 'text'">{{ part.text }}</template
    ><span
      v-else
      class="comment-inline-emoji"
      role="img"
      :aria-label="part.label"
      :title="part.label"
      >{{ part.preview }}</span
    ></template
  >
</template>

<style scoped>
.comment-inline-emoji {
  white-space: nowrap;
}
</style>
