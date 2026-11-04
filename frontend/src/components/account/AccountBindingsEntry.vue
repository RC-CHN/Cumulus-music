<script setup lang="ts">
import { ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { useCore } from '../../services/core';
import { useAccountRead } from '../../composables/useAccountRead';
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
    <button
      v-else
      type="button"
      class="secondary"
      data-testid="account-bindings-open"
      :disabled="loading"
      @click="emit('open', $event)"
    >
      绑定账号 <span aria-hidden="true">›</span>
    </button>
    <AccountBindingQrDialog
      v-if="qr"
      :epoch="account.epoch"
      purpose="password"
      @close="qr = false"
    />
  </template>
</template>
