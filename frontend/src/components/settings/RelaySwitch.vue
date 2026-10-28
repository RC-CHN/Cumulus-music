<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { RelayModel } from '../../composables/useRelay';
import { relaySettingPending, type RelaySettingOperation } from '../../services/relay';
import { useControlFocus } from '../../composables/useControlFocus';
import ModalTransition from '../ModalTransition.vue';
import SettingsConfirm from './SettingsConfirm.vue';
const props = defineProps<{
  model: RelayModel;
  epoch: string;
  fallbackFocus: () => HTMLElement | null | undefined;
}>();
const root = ref<HTMLElement>();
const fallbackFocus = () =>
  root.value?.querySelector<HTMLInputElement>('input:not(:disabled)') ??
  root.value?.querySelector<HTMLButtonElement>('[data-setting-reconcile]:not(:disabled)') ??
  props.fallbackFocus() ??
  null;
const controlFocus = useControlFocus(() => props.epoch, fallbackFocus);
const state = computed(() => props.model.page.value?.settings);
const pending = computed(() => state.value?.operations.filter(relaySettingPending) ?? []);
const latest = computed(() => state.value?.operations.find((op) => !relaySettingPending(op)));
const confirmation = ref<{ id: string; revision: string }>();
watch(
  () => props.model.page.value,
  (value) => {
    if (!value) confirmation.value = undefined;
  },
);
function toggle(event: Event) {
  const input = event.currentTarget as HTMLInputElement,
    next = input.checked;
  input.checked = state.value?.enabled === true;
  void controlFocus.run(event, () => props.model.changeSetting(next));
}
function outcome(op: RelaySettingOperation) {
  if (op.phase === 'confirmed' || op.phase === 'unchanged')
    return op.enabled ? '已确认开启跨端续播' : '已确认关闭跨端续播';
  if (op.phase === 'accepted_current_state') return '已保留当前设备设置';
  if (op.phase === 'cancelled') return '已取消尚未开始的修改';
  if (op.phase === 'rejected') return '设置修改被服务端拒绝';
  return '尚未确认设备设置，请先核对结果';
}
async function confirm() {
  const value = confirmation.value;
  if (value && (await props.model.acceptSetting(value.id, value.revision)))
    confirmation.value = undefined;
}
</script>
<template>
  <div v-if="state" ref="root" class="relay-switch">
    <label class="settings-checkbox">
      <input
        type="checkbox"
        aria-label="开启跨端续播"
        :checked="state.enabled === true"
        :disabled="!model.canChangeSetting.value"
        @change="toggle"
      />
      <span>开启跨端续播</span>
    </label>
    <p v-if="state.enabled === null" class="settings-note">
      服务端未返回当前设备开关，暂时无法修改。
    </p>
    <div v-if="model.settingUnknown.value" class="relay-recovery">
      <p>尚未收到设置修改结果，请先核对。</p>
      <button
        class="secondary"
        data-setting-reconcile
        :disabled="model.busy.value || model.loading.value"
        @click="controlFocus.run($event, model.cancelUnknownSetting)"
      >
        核对设置结果
      </button>
    </div>
    <div
      v-for="operation in pending"
      :key="operation.id"
      class="relay-recovery relay-setting-pending"
    >
      <p role="status">{{ outcome(operation) }}</p>
      <p v-if="operation.lastReadAt" class="settings-note">
        最近核对：{{
          operation.observed === null
            ? '服务端未返回开关'
            : operation.observed
              ? '已开启'
              : '已关闭'
        }}
      </p>
      <div class="relay-recovery-actions">
        <button
          class="secondary"
          data-setting-reconcile
          :disabled="model.busy.value || model.loading.value"
          @click="controlFocus.run($event, () => model.reconcileSetting(operation.id))"
        >
          核对设备开关
        </button>
        <button
          v-if="operation.readRevision"
          class="secondary"
          :disabled="model.busy.value || model.loading.value"
          @click="confirmation = { id: operation.id, revision: operation.readRevision }"
        >
          保留当前设置
        </button>
      </div>
    </div>
    <p v-if="latest" class="settings-note relay-setting-result" role="status">
      {{ outcome(latest) }}
    </p>
    <ModalTransition :fallback-focus="fallbackFocus">
      <SettingsConfirm
        v-if="confirmation"
        title="保留当前设备设置"
        description="以刚刚读取的设备开关为准，结束这次未确认的修改。此操作不会再次提交开关。"
        :busy="model.busy.value"
        :error="model.error.value"
        @close="confirmation = undefined"
        @confirm="confirm"
      />
    </ModalTransition>
  </div>
</template>
