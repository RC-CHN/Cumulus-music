<script setup lang="ts">
defineProps<{ more: boolean; loading: boolean; error: string }>();
const emit = defineEmits<{ load: [] }>();
</script>
<template>
  <div v-if="more || error" class="download-continuation" role="status">
    <template v-if="loading"><span class="spinner" />正在加载更多…</template>
    <template v-else-if="error"
      ><span>{{ error }}</span
      ><button class="text-button" @click="emit('load')">重试加载</button></template
    >
    <button v-else class="text-button" @click="emit('load')">加载更多</button>
  </div>
</template>
