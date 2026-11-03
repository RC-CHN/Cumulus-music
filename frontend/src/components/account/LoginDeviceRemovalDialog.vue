<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { LoginDeviceRemovalModel } from '../../composables/useLoginDeviceRemoval';
import { useControlFocus } from '../../composables/useControlFocus';
import { validDeviceRemovalCode } from '../../services/loginDeviceRemoval';
import Icon from '../Icon.vue';
import '../../styles/login-device-removal.css';

const props = defineProps<{
  epoch: string;
  model: LoginDeviceRemovalModel;
  confirm: boolean;
  deviceId?: string;
  deviceName?: string;
}>();
const emit = defineEmits<{ begin: []; close: []; binding: [] }>();
const { view, busy, seconds, error, readRequired, canBegin } = props.model;
const dialog = ref<HTMLDialogElement>();
const heading = ref<HTMLElement>();
const input = ref<HTMLInputElement>();
const code = ref('');
const codeError = ref(false);
const phase = computed(() => {
  if (props.confirm) return 'confirm';
  if (readRequired.value) return 'read_required';
  if (props.deviceId && view.value?.id !== props.deviceId)
    return busy.value ? 'working' : 'not_sent';
  return view.value?.phase ?? (busy.value ? 'working' : 'not_sent');
});
const sms = computed(
  () =>
    phase.value === 'needs_sms' ||
    phase.value === 'sms_ready' ||
    phase.value === 'sms_send_uncertain',
);
const canSubmit = computed(
  () => phase.value === 'sms_ready' || phase.value === 'sms_send_uncertain',
);
const title = computed(() =>
  sms.value ? '安全验证' : phase.value === 'recovery_required' ? '核对设备移除结果' : '移除设备',
);
const focus = useControlFocus(
  () => props.epoch,
  () => heading.value,
);
const tail = computed(() =>
  /^[0-9]{4}$/.test(view.value?.phoneTail ?? '') ? view.value?.phoneTail : undefined,
);
function wipe() {
  code.value = '';
  codeError.value = false;
  if (input.value) input.value.value = '';
}
function close() {
  if (busy.value) return;
  wipe();
  emit('close');
}
function operation(kind: 'send_sms' | 'reconcile' | 'cancel') {
  const id = view.value?.id;
  if (!id || busy.value || readRequired.value) return;
  if (kind === 'send_sms' || kind === 'cancel') wipe();
  return props.model.command({ kind, operation_id: id });
}
async function submit(event: SubmitEvent) {
  const id = view.value?.id;
  if (!id || busy.value || !canSubmit.value) return;
  if (!validDeviceRemovalCode(code.value)) {
    codeError.value = true;
    input.value?.focus();
    return;
  }
  const submitted = code.value;
  wipe();
  const restore = focus.capture(event.submitter);
  await props.model.command({ kind: 'submit_sms', operation_id: id, code: submitted });
  await nextTick();
  restore();
  if (
    view.value?.smsNotAccepted &&
    dialog.value?.open &&
    (document.activeElement === heading.value || document.activeElement === document.body)
  )
    input.value?.focus({ preventScroll: true });
}
watch([() => props.epoch, () => view.value?.id, sms], wipe, { flush: 'sync' });
watch(phase, async (next, previous) => {
  if (next === previous) return;
  await nextTick();
  if (!dialog.value?.open) return;
  if (document.activeElement === document.body || !dialog.value.contains(document.activeElement))
    heading.value?.focus({ preventScroll: true });
});
onMounted(() => {
  dialog.value?.showModal();
  heading.value?.focus({ preventScroll: true });
});
onBeforeUnmount(() => {
  wipe();
  dialog.value?.close();
});
</script>

<template>
  <dialog
    ref="dialog"
    class="login-device-removal-dialog"
    :data-removal-phase="phase"
    :aria-busy="busy"
    aria-labelledby="login-device-removal-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="login-device-removal-title" ref="heading" tabindex="-1">{{ title }}</h2>
      <button
        type="button"
        class="icon-button"
        aria-label="关闭设备移除"
        data-testid="login-device-removal-close"
        :disabled="busy"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="login-device-removal-body">
      <template v-if="phase === 'confirm'">
        <p v-if="deviceName" class="login-device-removal-target">{{ deviceName }}</p>
        <p>移除设备后，该设备再次登录需要重新安全验证。</p>
      </template>
      <form v-else-if="sms" id="login-device-removal-sms" @submit.prevent="submit">
        <p class="login-device-sms-title">请完成手机短信验证</p>
        <div class="login-device-sms-input">
          <input
            ref="input"
            :value="code"
            type="text"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="8"
            aria-label="短信验证码"
            placeholder="请输入验证码"
            aria-describedby="login-device-sms-hint"
            :aria-invalid="codeError"
            :disabled="busy || !canSubmit"
            data-testid="login-device-removal-code"
            @input="
              code = ($event.target as HTMLInputElement).value;
              codeError = false;
            "
          />
          <button
            type="button"
            data-testid="login-device-removal-send"
            :disabled="busy || seconds > 0"
            @click="focus.run($event, () => operation('send_sms'))"
          >
            {{ seconds > 0 ? `${seconds}s` : phase === 'needs_sms' ? '获取验证码' : '重新获取' }}
          </button>
        </div>
        <p
          id="login-device-sms-hint"
          class="login-device-sms-hint"
          :role="codeError || view?.smsNotAccepted ? 'alert' : 'status'"
        >
          <template v-if="codeError">请输入 4 至 8 位数字验证码。</template>
          <template v-else-if="view?.smsNotAccepted">服务未接受这次验证码，请核对后重试。</template>
          <template v-else-if="phase === 'sms_send_uncertain'">
            未确认短信是否已发送。如果已收到验证码，可以填写后继续。
          </template>
          <template v-else-if="phase === 'sms_ready'">
            {{ tail ? `验证码已发送至尾号 ${tail} 的手机。` : '验证码已发送至绑定手机。' }}
          </template>
          <template v-else>{{
            tail ? `验证码将发送至尾号 ${tail} 的手机。` : '验证码将发送至绑定手机。'
          }}</template>
        </p>
      </form>
      <p v-else-if="phase === 'needs_binding'">请先绑定手机号，再移除设备。</p>
      <template v-else-if="phase === 'recovery_required'">
        <p>尚未确认设备是否已移除。可以重新读取设备列表核对结果。</p>
        <p v-if="view?.observedPresent === true" role="status">
          设备仍出现在最新列表中，暂不能确认移除。
        </p>
      </template>
      <p v-else-if="phase === 'observed_absent'" role="status">目标设备已不在最新设备列表中。</p>
      <p v-else-if="phase === 'cancelled'" role="status">已取消本次短信验证。</p>
      <p v-else-if="phase === 'not_sent'">本次移除未提交。</p>
      <p v-else-if="phase === 'read_required'">请先读取本次移除进度。</p>
      <p v-if="busy" class="login-device-removal-progress" role="status">正在处理，请稍候…</p>
      <p v-if="error" class="login-device-removal-error" role="alert">{{ error }}</p>
    </div>
    <footer>
      <template v-if="phase === 'confirm'">
        <button type="button" class="secondary" :disabled="busy" @click="close">取消</button>
        <button
          type="button"
          class="primary"
          data-testid="login-device-remove-confirm"
          :disabled="!canBegin"
          @click="emit('begin')"
        >
          确认
        </button>
      </template>
      <template v-else-if="sms">
        <button
          type="button"
          class="secondary"
          data-testid="login-device-removal-cancel"
          :disabled="busy"
          @click="focus.run($event, () => operation('cancel'))"
        >
          取消验证
        </button>
        <button
          type="submit"
          form="login-device-removal-sms"
          class="primary"
          data-testid="login-device-removal-submit"
          :disabled="busy || !canSubmit || !validDeviceRemovalCode(code)"
        >
          下一步
        </button>
      </template>
      <template v-else>
        <button type="button" class="secondary" :disabled="busy" @click="close">
          {{ phase === 'recovery_required' || phase === 'read_required' ? '稍后处理' : '关闭' }}
        </button>
        <button
          v-if="phase === 'needs_binding'"
          type="button"
          class="primary"
          data-testid="login-device-removal-binding"
          :disabled="busy"
          @click="emit('binding')"
        >
          扫码设置
        </button>
        <button
          v-else-if="phase === 'recovery_required'"
          type="button"
          class="primary"
          data-testid="login-device-removal-reconcile"
          :disabled="busy"
          @click="focus.run($event, () => operation('reconcile'))"
        >
          核对设备列表
        </button>
        <button
          v-else-if="phase === 'read_required'"
          type="button"
          class="primary"
          data-testid="login-device-removal-reload"
          :disabled="busy"
          @click="focus.run($event, () => model.read())"
        >
          重新读取
        </button>
      </template>
    </footer>
  </dialog>
</template>
