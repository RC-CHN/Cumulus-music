<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import type { AccountReadState } from '../../composables/useAccountRead';
import { useControlFocus } from '../../composables/useControlFocus';
import Icon from '../Icon.vue';
import '../../styles/account-detail.css';
const props = defineProps<{
  kind: 'profile' | 'devices';
  epoch: string;
  title: string;
  subtitle: string;
  state: AccountReadState;
  error: string;
  refresh: () => Promise<void>;
}>();
const emit = defineEmits<{ back: [] }>();
const heading = ref<HTMLElement>();
const root = ref<HTMLElement>();
const focus = useControlFocus(
  () => props.epoch,
  () => heading.value,
);
defineExpose({ focusAnchor: () => heading.value ?? null });
onMounted(async () => {
  await nextTick();
  const current = document.activeElement;
  if (
    heading.value?.isConnected &&
    !document.querySelector('dialog[open]') &&
    (current === document.body || !current?.getClientRects().length)
  )
    heading.value.focus({ preventScroll: true });
});
onBeforeUnmount(() => {
  if (root.value) {
    root.value.inert = true;
    root.value.hidden = true;
  }
});
</script>
<template>
  <section
    ref="root"
    class="account-read-page"
    :class="kind === 'profile' ? 'account-profile-page' : 'login-devices-page'"
    :data-account-read-state="state"
    :aria-busy="state === 'loading'"
    aria-labelledby="account-read-title"
  >
    <header class="account-read-heading">
      <button type="button" class="icon-button" aria-label="返回设置" @click="emit('back')">
        <Icon name="back" />
      </button>
      <div>
        <h1 id="account-read-title" ref="heading" tabindex="-1">{{ title }}</h1>
        <p>{{ subtitle }}</p>
      </div>
      <button
        v-if="state !== 'signed-out'"
        type="button"
        class="account-read-retry"
        :data-testid="`account-${kind}-retry`"
        :aria-label="`重新读取${kind === 'profile' ? '个人资料' : '登录设备'}`"
        :disabled="state === 'loading'"
        @click="focus.run($event, refresh)"
      >
        <Icon name="refresh" />
        {{ state === 'error' ? '重试' : '刷新' }}
      </button>
    </header>
    <div class="account-read-scroll" tabindex="0" :aria-label="title">
      <slot name="before" />
      <p v-if="state === 'loading'" class="account-read-status" role="status">
        <span class="spinner" aria-hidden="true" />正在读取{{
          kind === 'profile' ? '个人资料' : '登录设备'
        }}…
      </p>
      <p v-else-if="state === 'error'" class="account-read-status account-read-error" role="alert">
        {{ error }}
      </p>
      <p v-else-if="state === 'signed-out'" class="account-read-status">
        登录后可查看{{ title }}。
      </p>
      <slot v-else />
    </div>
  </section>
</template>
