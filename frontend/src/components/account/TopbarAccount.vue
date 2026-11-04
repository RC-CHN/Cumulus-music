<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import Icon from '../Icon.vue';

const props = defineProps<{ account: AccountView; disabled: boolean }>();
const emit = defineEmits<{ profile: []; accounts: []; login: [] }>();
const signedIn = computed(() => props.account.status === 'authenticated');
const profile = computed(() => signedIn.value && !!props.account.profileShortcut);
const label = computed(() =>
  signedIn.value ? (props.account.nickname ?? '我的账号') : '点击登录',
);
const avatarFailed = ref(false);
watch([() => props.account.epoch, () => props.account.avatarUrl], () => {
  avatarFailed.value = false;
});
const showAvatar = computed(() => props.account.avatarUrl && !avatarFailed.value);
</script>

<template>
  <div class="account-control">
    <button
      v-if="profile"
      class="account-avatar-button"
      aria-label="打开我的主页"
      title="打开我的主页"
      type="button"
      :disabled="disabled"
      @click="emit('profile')"
    >
      <img
        v-if="showAvatar"
        :src="account.avatarUrl!"
        alt=""
        referrerpolicy="no-referrer"
        @error="avatarFailed = true"
      />
      <span v-else class="avatar"><Icon name="user" /></span>
    </button>
    <button
      class="account-button"
      :class="{ 'account-nickname': profile }"
      type="button"
      :title="label"
      :disabled="disabled"
      @click="signedIn ? emit('accounts') : emit('login')"
    >
      <template v-if="!profile">
        <img
          v-if="showAvatar"
          :src="account.avatarUrl!"
          alt=""
          referrerpolicy="no-referrer"
          @error="avatarFailed = true"
        />
        <span v-else class="avatar"><Icon name="user" /></span>
      </template>
      <span>{{ label }}</span>
      <Icon v-if="profile" name="down" class="account-unfold" />
    </button>
  </div>
</template>
