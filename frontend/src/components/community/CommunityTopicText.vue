<script setup lang="ts">
import { computed } from 'vue';
import { topicParts } from '../../services/communityTopic';

const props = defineProps<{ value: string | null; source: string }>();
const emit = defineEmits<{ topic: [name: string, opener: string] }>();
const parts = computed(() => topicParts(props.value));
</script>

<template>
  <template v-for="(part, index) in parts" :key="index">
    <button
      v-if="part.topic"
      type="button"
      class="community-topic-link"
      :data-topic-opener="`${source}:${index}`"
      @click="emit('topic', part.topic, `${source}:${index}`)"
    >
      {{ part.text }}
    </button>
    <template v-else>{{ part.text }}</template>
  </template>
</template>

<style scoped>
.community-topic-link {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent);
  font: inherit;
  text-align: start;
  white-space: inherit;
  overflow-wrap: anywhere;
}
.community-topic-link:hover {
  text-decoration: underline;
}
</style>
