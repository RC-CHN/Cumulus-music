<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { groupLoginDevices, type LoginDevice } from '../../services/accountDetail';
import { useAccountRead } from '../../composables/useAccountRead';
import { useCore } from '../../services/core';
import ModalTransition from '../ModalTransition.vue';
import AccountReadLayout from './AccountReadLayout.vue';
import LoginDeviceGroup from './LoginDeviceGroup.vue';
import LoginDeviceDetail from './LoginDeviceDetail.vue';
const props = defineProps<{ account: AccountView }>();
const emit = defineEmits<{ back: []; expired: [] }>();
const core = useCore();
const owner = () => (props.account.status === 'authenticated' ? props.account.epoch : null);
const { value, state, error, refresh } = useAccountRead(
  owner,
  (epoch) => core.accountLoginDevices(epoch),
  (view) => !view.devices.length,
  () => emit('expired'),
);
const groups = computed(() => groupLoginDevices(value.value?.devices ?? []));
const detail = shallowRef<LoginDevice>();
const detailOrigin = ref<HTMLElement | null>(null);
function showDetail(device: LoginDevice, event: MouseEvent) {
  if (!owner() || !value.value?.devices.includes(device)) return;
  detailOrigin.value = event.currentTarget as HTMLElement;
  detail.value = device;
}
const returnDetailFocus = () => detailOrigin.value;
watch(
  owner,
  () => {
    detailOrigin.value = null;
    detail.value = undefined;
  },
  { flush: 'sync' },
);
</script>
<template>
  <AccountReadLayout
    kind="devices"
    :epoch="account.epoch"
    title="登录设备管理"
    subtitle="查看最近登录过的设备。"
    :state="state"
    :error="error"
    :refresh="refresh"
    @back="emit('back')"
  >
    <dl v-if="value?.limits" class="login-device-limit" aria-label="设备额度">
      <div>
        <dt>当前数量</dt>
        <dd>{{ value.limits.currentNum ?? '未提供' }}</dd>
      </div>
      <div>
        <dt>设备额度</dt>
        <dd>{{ value.limits.deviceLimit ?? '未提供' }}</dd>
      </div>
    </dl>
    <LoginDeviceGroup
      kind="signed-in"
      title="已登录的设备"
      empty-label="暂无已登录的设备"
      :devices="groups.signedIn"
      @detail="showDetail"
    />
    <LoginDeviceGroup
      kind="signed-out"
      title="退出登录的设备"
      empty-label="暂无退出的设备"
      :devices="groups.signedOut"
      @detail="showDetail"
    />
  </AccountReadLayout>
  <ModalTransition :return-focus="returnDetailFocus">
    <LoginDeviceDetail v-if="detail" :device="detail" @close="detail = undefined" />
  </ModalTransition>
</template>
