<script setup lang="ts">
import { ref, watch } from 'vue';
import Icon from '../Icon.vue';
const props = defineProps<{ nickname: string | null; avatarUrl: string | null }>();
const failed = ref(false);
watch(
  () => props.avatarUrl,
  () => {
    failed.value = false;
  },
);
</script>
<template>
  <div class="verification-identity">
    <img
      v-if="avatarUrl && !failed"
      :src="avatarUrl"
      alt=""
      referrerpolicy="no-referrer"
      @error="failed = true"
    />
    <span v-else class="verification-identity-placeholder" aria-hidden="true"
      ><Icon name="user"
    /></span>
    <div>
      <p class="verification-identity-title">用户昵称</p>
      <p class="verification-identity-name" :title="nickname ?? ''">
        {{ nickname || '待验证账号' }}
      </p>
    </div>
  </div>
</template>
