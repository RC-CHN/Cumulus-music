<script setup lang="ts">
import type { LikeRecoveryModel } from '../composables/useLikeRecovery';
defineProps<{ model: LikeRecoveryModel }>();
</script>
<template>
  <aside
    v-if="model.view.value.total || model.error.value"
    class="like-recovery-notice notice"
    role="status"
  >
    <span>{{
      model.view.value.total
        ? `${model.view.value.total} 首歌曲的喜欢操作待核对`
        : '喜欢状态核对遇到问题，请查看详情。'
    }}</span>
    <button class="text-button" @click="model.open.value = true">查看并核对</button>
  </aside>
</template>
<style scoped>
.like-recovery-notice {
  flex: 0 0 auto;
  margin: 0 0 10px;
}
.like-recovery-notice span {
  min-width: 0;
  overflow-wrap: anywhere;
}
.like-recovery-notice button {
  flex: 0 0 auto;
  white-space: nowrap;
}
</style>
