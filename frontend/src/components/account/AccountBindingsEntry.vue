<script setup lang="ts">
import { ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { useCore } from '../../services/core';
import { useAccountRead } from '../../composables/useAccountRead';
import { bindingMethodName } from '../../services/accountBindings';
import AccountBindingProviderMark from './AccountBindingProviderMark.vue';
import AccountBindingQrDialog from './AccountBindingQrDialog.vue';

const props = defineProps<{ account: AccountView }>();
const emit = defineEmits<{ open: [event: MouseEvent]; expired: [] }>();
const core = useCore();
const owner = () => (props.account.status === 'authenticated' ? props.account.epoch : null);
const { value, loading } = useAccountRead(
  owner,
  (epoch) => core.accountBindings(epoch),
  undefined,
  () => emit('expired'),
);
const qr = ref(false);
const socialMethods = ['wechat', 'qq', 'weibo'] as const;
const bindingState = (method: (typeof socialMethods)[number]) =>
  !value.value
    ? 'unknown'
    : value.value.bindings.some((row) => row.provider === method)
      ? 'bound'
      : 'unbound';
const bindingStateLabel = { unknown: '绑定状态未确认', bound: '已绑定', unbound: '未绑定' };
watch(owner, () => (qr.value = false), { flush: 'sync' });
</script>

<template>
  <template v-if="owner()">
    <button
      v-if="value?.foreignRegisteredPhone"
      type="button"
      class="secondary"
      data-testid="account-bindings-foreign-password"
      @click="qr = true"
    >
      修改密码
    </button>
    <div v-else class="settings-binding-methods" aria-label="账号绑定">
      <button
        v-for="method in socialMethods"
        :key="method"
        type="button"
        class="settings-binding-provider"
        :data-binding-provider="method"
        :data-binding-state="bindingState(method)"
        :aria-label="`${bindingMethodName[method]}：${bindingStateLabel[bindingState(method)]}，查看绑定账号`"
        :disabled="loading"
        @click="emit('open', $event)"
      >
        <AccountBindingProviderMark :method="method" :bound="bindingState(method) === 'bound'" />
      </button>
      <button
        type="button"
        class="text-button settings-binding-open"
        data-testid="account-bindings-open"
        :disabled="loading"
        @click="emit('open', $event)"
      >
        绑定账号 <span aria-hidden="true">›</span>
      </button>
    </div>
    <AccountBindingQrDialog
      v-if="qr"
      :epoch="account.epoch"
      purpose="password"
      @close="qr = false"
    />
  </template>
</template>

<style scoped>
.settings-binding-methods {
  display: inline-flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.settings-binding-provider {
  display: inline-flex;
  padding: 0;
  border-radius: 50%;
}
.settings-binding-provider:hover:not(:disabled) {
  background: var(--surface-hover);
}
.settings-binding-open {
  min-height: 40px;
  padding-inline: 6px;
}
</style>
