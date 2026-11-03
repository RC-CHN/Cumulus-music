<script setup lang="ts">
import { computed, ref } from 'vue';
import type { AccountView } from '../services/contracts';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
const props = defineProps<{ account: AccountView; error: string }>();
const unverified = computed(() => props.account.status === 'unknown' && !props.account.loginNotice);
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
        (unverified
          ? '暂时无法确认登录状态'
          : account.status === 'expired'
            ? '登录已失效'
            : '让喜欢的音乐，陪在身边')
      }}
    </h1>
    <p aria-live="polite">
      {{
        account.loginNotice?.message ||
        error ||
        (account.status === 'expired'
          ? '请重新登录后继续管理歌单、查看推荐和使用账号资源。'
          : unverified
            ? '请重新连接以核对当前账号，也可以选择登录。'
            : '登录网易云音乐，找回你的歌单与每日推荐')
      }}
    </p>
    <button v-if="unverified" type="button" class="primary" @click="$emit('retry')">
      重新连接
    </button>
    <button type="button" :class="unverified ? 'text-button' : 'primary'" @click="$emit('login')">
      {{ account.loginNotice || account.status === 'expired' ? '重新登录' : '登录账号' }}
    </button>
    <button
      v-if="error && !account.loginNotice && !unverified"
      type="button"
      class="text-button"
      @click="$emit('retry')"
    >
      重新连接
    </button>
  </div>
</template>
