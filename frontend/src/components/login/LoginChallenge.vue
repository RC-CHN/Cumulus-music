<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import VerificationMethods from './VerificationMethods.vue';
import DeviceVerification from './DeviceVerification.vue';
const props = defineProps<{ model: LoginModel }>();
const emit = defineEmits<{ qr: []; device: [value: 'confirm' | 'scan' | null] }>();
const { view, busy, seconds } = props.model;
const method = ref(''),
  code = ref(''),
  answer = ref('');
const root = ref<HTMLElement>();
const devicePanel = ref<{ clearSecrets: () => void }>();
const returning = ref(false);
const blockedNotice = ref('');
const known: Record<string, string> = {
  SECURITY_EMAIL: '安全邮箱',
  UP_SMS: '短信验证码',
  BINDING_CELLPHONE: '绑定手机号验证',
  DEVICE_CONFIRM: '原设备确认',
  DEVICE_SCAN: '原设备扫码',
  WEICHAT_AUTH: '微信授权',
};
const selected = computed(() =>
  view.value?.challenge.methods.includes(method.value) ? method.value : '',
);
watch(selected, (value) =>
  emit('device', value === 'DEVICE_SCAN' ? 'scan' : value === 'DEVICE_CONFIRM' ? 'confirm' : null),
);
function clearSecrets() {
  code.value = '';
  answer.value = '';
  devicePanel.value?.clearSecrets();
}
defineExpose({ clearSecrets });
async function choose(value: string) {
  if (busy.value) return;
  blockedNotice.value = '';
  clearSecrets();
  method.value = value;
  if (value === 'DEVICE_CONFIRM' || value === 'DEVICE_SCAN')
    await props.model.run({
      kind: 'device_start',
      method: value === 'DEVICE_CONFIRM' ? 'confirm' : 'scan',
    });
  await nextTick();
  if (root.value?.isConnected && !root.value.closest('[inert]'))
    root.value
      .querySelector<HTMLElement>('input:not(:disabled),button:not(:disabled)')
      ?.focus({ preventScroll: true });
}
async function back() {
  if (returning.value) return;
  if (busy.value && (!selected.value.startsWith('DEVICE_') || !view.value?.device)) return;
  returning.value = true;
  try {
    clearSecrets();
    const device = view.value?.device;
    if (selected.value.startsWith('DEVICE_') && device) await props.model.stopDevice(device.id);
    const previous = method.value;
    method.value = '';
    await nextTick();
    if (root.value?.isConnected && !root.value.closest('[inert]')) {
      const target = root.value.querySelector<HTMLElement>(
        `[data-verification-method="${previous}"]`,
      );
      target?.scrollIntoView({ block: 'nearest' });
      target?.focus({ preventScroll: true });
    }
  } finally {
    returning.value = false;
  }
}
async function verify() {
  const otp = code.value;
  code.value = '';
  if (selected.value === 'SECURITY_EMAIL')
    await props.model.run({ kind: 'security_mail_verify', code: otp });
  else if (selected.value === 'UP_SMS')
    await props.model.run({ kind: 'verification_sms_verify', code: otp });
}
watch(
  () => [view.value?.device?.phase, busy.value],
  () => {
    if (
      view.value?.device?.phase === 'blocked' &&
      !busy.value &&
      selected.value.startsWith('DEVICE_') &&
      !returning.value
    ) {
      blockedNotice.value = '原设备已拒绝登录请求，请次日再试或尝试其他验证方式';
      void back();
    }
  },
);
</script>
<template>
  <section ref="root" class="login-challenge" aria-label="登录验证">
    <template
      v-if="
        view && (view.status === 'two_factor' || (selected.startsWith('DEVICE_') && view.device))
      "
    >
      <img
        v-if="view?.challenge.avatarUrl"
        :src="view.challenge.avatarUrl"
        class="login-avatar"
        alt=""
        referrerpolicy="no-referrer"
      />
      <p v-if="view.challenge.nickname">{{ view.challenge.nickname }}</p>
      <p v-if="view.challenge.phoneMask" class="muted">{{ view.challenge.phoneMask }}</p>
      <button
        v-if="!view.challenge.methods.length"
        class="primary"
        :disabled="busy"
        @click="model.run({ kind: 'verification_methods' })"
      >
        获取验证方式
      </button>
      <template v-else>
        <p v-if="!selected && blockedNotice" class="verification-blocked" role="alert">
          {{ blockedNotice }}
        </p>
        <p
          v-if="!selected && !blockedNotice && view.device?.phase === 'stop_unconfirmed'"
          class="muted"
          role="status"
        >
          已停止本机轮询，尚未确认远端取消结果。
        </p>
        <VerificationMethods
          v-if="!selected"
          :methods="view.challenge.methods"
          :busy="busy"
          @select="choose"
        />
        <DeviceVerification
          v-else-if="selected.startsWith('DEVICE_')"
          ref="devicePanel"
          :model="model"
          :kind="selected === 'DEVICE_SCAN' ? 'scan' : 'confirm'"
        />
        <div v-else-if="selected === 'BINDING_CELLPHONE'" class="verification-handoff">
          <p>如需通过绑定手机号验证，请前往手机端操作。</p>
          <p class="muted">也可以返回选择其他验证方式。</p>
        </div>
        <p v-else-if="selected === 'WEICHAT_AUTH'" class="muted">
          当前客户端暂不支持微信验证，请选择其他验证方式。
        </p>
        <form
          v-else-if="['SECURITY_EMAIL', 'UP_SMS'].includes(selected)"
          class="credential-form"
          @submit.prevent="verify"
        >
          <p>{{ known[selected] }}</p>
          <label
            >验证码<input
              v-model="code"
              aria-label="安全验证码"
              inputmode="numeric"
              autocomplete="one-time-code"
              pattern="[0-9]{4,8}"
              maxlength="8"
              required
              :disabled="busy"
          /></label>
          <button
            type="button"
            :disabled="busy || seconds > 0"
            @click="
              model.run({
                kind:
                  selected === 'SECURITY_EMAIL' ? 'security_mail_send' : 'verification_sms_send',
              })
            "
          >
            {{ seconds ? `${seconds} 秒后重发` : '发送验证码' }}
          </button>
          <button type="submit" class="primary" :disabled="busy">确认验证</button>
        </form>
        <button
          v-if="selected"
          class="verification-back"
          :disabled="returning || (busy && (!selected.startsWith('DEVICE_') || !view.device))"
          @click="back"
        >
          选择其他验证方式
        </button>
      </template>
    </template>
    <template v-else-if="view?.status === 'security_question'">
      <button
        v-if="!view.challenge.question"
        class="primary"
        :disabled="busy"
        @click="model.run({ kind: 'question_read' })"
      >
        读取安全问题
      </button>
      <form
        v-else
        class="login-questions"
        @submit.prevent="model.run({ kind: 'question_answer', answer })"
      >
        <p>{{ view.challenge.question }}</p>
        <label v-for="item in view.challenge.answers" :key="item.value"
          ><input
            v-model="answer"
            type="radio"
            name="login-answer"
            :value="item.value"
            :disabled="busy"
          /><img v-if="item.picture" :src="item.picture" alt="" referrerpolicy="no-referrer" /><span
            >{{ item.title }}<small v-if="item.subtitle">{{ item.subtitle }}</small></span
          ></label
        >
        <button class="primary" type="submit" :disabled="busy || !answer">确认答案</button>
      </form>
    </template>
    <template v-else-if="view?.status === 'phone_reuse'">
      <button
        v-if="!view.challenge.reuseConfirmation"
        :disabled="busy"
        class="primary"
        @click="model.run({ kind: 'reuse_read' })"
      >
        查看关联账号
      </button>
      <template v-else
        ><img
          v-if="view.challenge.avatarUrl"
          class="login-avatar"
          :src="view.challenge.avatarUrl"
          alt=""
          referrerpolicy="no-referrer"
        />
        <p>{{ view.challenge.nickname }}</p>
        <button class="primary" :disabled="busy" @click="model.run({ kind: 'reuse_confirm' })">
          这是我的账号，继续验证
        </button></template
      >
    </template>
    <button
      v-else-if="view?.status === 'captcha' && view.captchaAvailable"
      class="primary"
      :disabled="busy"
      @click="model.captcha()"
    >
      {{ busy ? '请在验证窗口中完成操作…' : '打开安全验证' }}
    </button>
    <button
      v-else-if="view?.status === 'error'"
      class="primary"
      :disabled="busy"
      @click="model.run({ kind: 'reconcile' })"
    >
      核对登录状态
    </button>
    <button
      v-else-if="
        view &&
        ['captcha', 'verification', 'security_qr', 'risk_notice', 'upgrade_required'].includes(
          view.status,
        )
      "
      :disabled="busy"
      @click="emit('qr')"
    >
      改用扫码登录
    </button>
  </section>
</template>
