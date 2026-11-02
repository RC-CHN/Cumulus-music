<script setup lang="ts">
import { ref } from 'vue';
import type { AccountView } from '../services/contracts';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
defineProps<{ account: AccountView; error: string }>();
defineEmits<{ login: []; retry: [] }>();
const root = ref<HTMLElement>();
usePageEntryFocus(
  root,
  () => true,
  () => !document.querySelector('dialog[open],[aria-modal="true"]'),
);
</script>
<template>
  <div ref="root" class="empty-state welcome">
    <div class="welcome-note">♪</div>
    <h1 tabindex="-1">
      {{
        account.loginNotice?.title ||
        (account.status === 'expired' ? '登录已失效' : '让喜欢的音乐，陪在身边')
      }}
    </h1>
    <p aria-live="polite">
      {{
        account.loginNotice?.message ||
        error ||
        (account.status === 'expired'
          ? '请重新登录后继续管理歌单、查看推荐和使用账号资源。'
          : '登录网易云音乐，找回你的歌单与每日推荐')
      }}
    </p>
    <button class="primary" @click="$emit('login')">
      {{ account.loginNotice || account.status === 'expired' ? '重新登录' : '登录账号' }}
    </button>
    <button v-if="error && !account.loginNotice" class="text-button" @click="$emit('retry')">
      重新连接
    </button>
  </div>
</template>
