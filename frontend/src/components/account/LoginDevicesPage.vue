<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { groupLoginDevices, type LoginDevice } from '../../services/accountDetail';
import { useLoginDeviceRemoval } from '../../composables/useLoginDeviceRemoval';
import { useAccountRead } from '../../composables/useAccountRead';
import { useCore } from '../../services/core';
import ModalTransition from '../ModalTransition.vue';
import AccountReadLayout from './AccountReadLayout.vue';
import LoginDeviceGroup from './LoginDeviceGroup.vue';
import LoginDeviceDetail from './LoginDeviceDetail.vue';
import LoginDeviceRemovalDialog from './LoginDeviceRemovalDialog.vue';
import AccountBindingQrDialog from './AccountBindingQrDialog.vue';
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
const layout = ref<InstanceType<typeof AccountReadLayout>>();
const detail = shallowRef<LoginDevice>();
const detailOrigin = ref<HTMLElement | null>(null);
const panel = ref<'detail' | 'confirm' | 'operation' | 'binding'>();
const result = ref('');
const removal = useLoginDeviceRemoval(
  core,
  owner,
  () => {
    result.value = '上次核对时，目标设备已不在列表中。';
    void refresh();
  },
  () => emit('expired'),
  () => void refresh(),
);
const { canBegin, pending, readRequired, busy: removalBusy } = removal;
const canRemove = computed(
  () => canBegin.value && value.value?.devices.some((device) => device.current),
);
function showDetail(device: LoginDevice, event: MouseEvent) {
  if (!owner() || !value.value?.devices.includes(device)) return;
  detailOrigin.value = event.currentTarget as HTMLElement;
  detail.value = device;
  panel.value = 'detail';
}
function confirmRemoval() {
  if (canRemove.value && detail.value && !detail.value.current) panel.value = 'confirm';
}
function beginRemoval() {
  const device = detail.value;
  if (!device || device.current || !canRemove.value || !value.value?.devices.includes(device))
    return;
  result.value = '';
  panel.value = 'operation';
  void removal.command({ kind: 'begin', device_id: device.id });
}
function resumeRemoval(event: MouseEvent) {
  detail.value = undefined;
  detailOrigin.value = event.currentTarget as HTMLElement;
  panel.value = 'operation';
}
function closePanel() {
  if (panel.value === 'confirm') {
    panel.value = 'detail';
    return;
  }
  if (panel.value === 'binding') {
    panel.value = 'operation';
    return;
  }
  const phase = removal.view.value?.phase;
  if (phase === 'cancelled' || phase === 'not_sent' || phase === 'needs_binding') void refresh();
  panel.value = undefined;
  detail.value = undefined;
}
const returnDetailFocus = () => detailOrigin.value;
const fallbackFocus = () => layout.value?.focusAnchor() ?? null;
watch(
  owner,
  () => {
    detailOrigin.value = null;
    detail.value = undefined;
    panel.value = undefined;
    result.value = '';
  },
  { flush: 'sync' },
);
</script>
<template>
  <AccountReadLayout
    ref="layout"
    kind="devices"
    :epoch="account.epoch"
    title="登录设备管理"
    subtitle="管理最近登录过的设备，移除后设备再次登录需安全验证。"
    :state="state"
    :error="error"
    :refresh="refresh"
    @back="emit('back')"
  >
    <template #before>
      <div v-if="pending || readRequired || result" class="login-device-removal-banner">
        <p v-if="result" role="status" data-testid="login-device-removal-result">{{ result }}</p>
        <p v-else-if="pending || readRequired">有一项设备移除需要处理。</p>
        <button
          v-if="pending || readRequired"
          type="button"
          class="secondary"
          data-testid="login-device-removal-resume"
          :disabled="removalBusy"
          @click="resumeRemoval"
        >
          {{ pending ? '查看移除进度' : '读取移除进度' }}
        </button>
      </div>
    </template>
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
  <ModalTransition :return-focus="returnDetailFocus" :fallback-focus="fallbackFocus">
    <LoginDeviceDetail
      v-if="panel === 'detail' && detail"
      :device="detail"
      :can-remove="canRemove"
      @close="closePanel"
      @remove="confirmRemoval"
    />
    <LoginDeviceRemovalDialog
      v-else-if="panel === 'confirm' || panel === 'operation'"
      :epoch="account.epoch"
      :model="removal"
      :confirm="panel === 'confirm'"
      :device-id="detail?.id"
      :device-name="detail?.name ?? undefined"
      @begin="beginRemoval"
      @close="closePanel"
      @binding="panel = 'binding'"
    />
    <AccountBindingQrDialog
      v-else-if="panel === 'binding'"
      :epoch="account.epoch"
      @close="closePanel"
    />
  </ModalTransition>
</template>
