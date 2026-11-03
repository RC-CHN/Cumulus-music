<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { accountDate, type LoginDevice } from '../../services/accountDetail';
import Icon from '../Icon.vue';
const props = defineProps<{ device: LoginDevice; canRemove?: boolean }>();
const emit = defineEmits<{ close: []; remove: [] }>();
const dialog = ref<HTMLDialogElement>();
const fields = computed(() => [
  { label: '设备名称', text: props.device.name },
  { label: '登录地点', text: props.device.location },
  { label: '登录方式', text: props.device.loginType },
  { label: '最近活跃', text: accountDate(props.device.lastActiveTime, true) },
]);
onMounted(() => dialog.value?.showModal());
onBeforeUnmount(() => {
  if (dialog.value) {
    dialog.value.inert = true;
    dialog.value.hidden = true;
  }
});
</script>
<template>
  <dialog
    ref="dialog"
    class="login-device-detail"
    aria-labelledby="login-device-detail-title"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 id="login-device-detail-title">登录设备详情</h2>
      <button type="button" class="icon-button" aria-label="关闭设备详情" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <dl>
      <div v-for="field in fields" :key="field.label">
        <dt>{{ field.label }}</dt>
        <dd>{{ field.text || '未提供' }}</dd>
      </div>
    </dl>
    <footer v-if="!device.current && canRemove">
      <button
        type="button"
        class="primary"
        data-testid="login-device-remove-open"
        @click="emit('remove')"
      >
        移除该设备
      </button>
    </footer>
  </dialog>
</template>
