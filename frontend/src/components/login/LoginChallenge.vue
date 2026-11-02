<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import VerificationMethods from './VerificationMethods.vue';
import DeviceVerification from './DeviceVerification.vue';
import SmsVerification from './SmsVerification.vue';
import EmailVerification from './EmailVerification.vue';
import VerificationIdentity from './VerificationIdentity.vue';
const props = defineProps<{ model: LoginModel }>();
const emit = defineEmits<{
  qr: [];
  device: [value: 'confirm' | 'scan' | 'sms' | 'email' | null];
}>();
const { view, busy } = props.model;
const method = ref(''),
  answer = ref('');
const root = ref<HTMLElement>();
const devicePanel = ref<{ clearSecrets: () => void }>();
const smsPanel = ref<{ clearSecrets: () => void }>();
const emailPanel = ref<{ clearSecrets: () => void }>();
const returning = ref(false);
const blockedNotice = ref('');
const selected = computed(() =>
  view.value?.challenge.methods.includes(method.value) ? method.value : '',
);
watch(selected, (value) =>
  emit(
    'device',
    value === 'DEVICE_SCAN'
      ? 'scan'
      : value === 'DEVICE_CONFIRM'
        ? 'confirm'
        : value === 'UP_SMS'
          ? 'sms'
          : value === 'SECURITY_EMAIL'
            ? 'email'
            : null,
  ),
);
function clearSecrets() {
  answer.value = '';
  devicePanel.value?.clearSecrets();
  smsPanel.value?.clearSecrets();
  emailPanel.value?.clearSecrets();
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
  if (value === 'UP_SMS')
    await props.model.run({
      kind: 'sms_start',
      direction: view.value?.challenge.countryCode === '86' ? 'uplink' : 'downlink',
    });
  if (value === 'SECURITY_EMAIL') await props.model.run({ kind: 'security_mail_send' });
  await nextTick();
  if (root.value?.isConnected && !root.value.closest('[inert]'))
    root.value
      .querySelector<HTMLElement>('input:not(:disabled),button:not(:disabled)')
      ?.focus({ preventScroll: true });
}
async function back() {
  if (returning.value) return;
  if (
    busy.value &&
    !(
      (selected.value.startsWith('DEVICE_') && view.value?.device) ||
      (selected.value === 'UP_SMS' && view.value?.sms) ||
      (selected.value === 'SECURITY_EMAIL' && view.value?.email)
    )
  )
    return;
  returning.value = true;
  try {
    clearSecrets();
    const device = view.value?.device;
    if (selected.value.startsWith('DEVICE_') && device) await props.model.stopDevice(device.id);
    if (selected.value === 'UP_SMS' && view.value?.sms)
      await props.model.stopSms(view.value.sms.id);
    if (selected.value === 'SECURITY_EMAIL' && view.value?.email)
      await props.model.stopEmail(view.value.email.id);
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
        view &&
        (view.status === 'two_factor' ||
          (selected.startsWith('DEVICE_') && view.device) ||
          (selected === 'UP_SMS' && view.sms) ||
          (selected === 'SECURITY_EMAIL' && view.email))
      "
    >
      <VerificationIdentity
        v-if="selected === 'SECURITY_EMAIL'"
        :nickname="view.challenge.nickname"
        :avatar-url="view.challenge.avatarUrl"
      />
      <template v-else>
        <img
          v-if="view?.challenge.avatarUrl"
          :src="view.challenge.avatarUrl"
          class="login-avatar"
          alt=""
          referrerpolicy="no-referrer"
        />
        <p v-if="view.challenge.nickname">{{ view.challenge.nickname }}</p>
        <p v-if="view.challenge.phoneMask && selected !== 'UP_SMS'" class="muted">
          {{ view.challenge.phoneMask }}
        </p>
      </template>
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
        <SmsVerification v-else-if="selected === 'UP_SMS'" ref="smsPanel" :model="model" />
        <EmailVerification
          v-else-if="selected === 'SECURITY_EMAIL'"
          ref="emailPanel"
          :model="model"
          @back="back"
        />
        <div v-else-if="selected === 'BINDING_CELLPHONE'" class="verification-handoff">
          <p>如需通过绑定手机号验证，请前往手机端操作。</p>
          <p class="muted">也可以返回选择其他验证方式。</p>
        </div>
        <p v-else-if="selected === 'WEICHAT_AUTH'" class="muted">
          当前客户端暂不支持微信验证，请选择其他验证方式。
        </p>
        <button
          v-if="selected && !(selected === 'SECURITY_EMAIL' && view.email?.phase === 'send_limit')"
          class="verification-back"
          :disabled="
            returning ||
            (busy &&
              !(
                (selected.startsWith('DEVICE_') && view.device) ||
                (selected === 'UP_SMS' && view.sms) ||
                (selected === 'SECURITY_EMAIL' && view.email)
              ))
          "
          @click="back"
        >
          {{
            selected === 'SECURITY_EMAIL' &&
            view.email &&
            ['code_required', 'verifying', 'send_unconfirmed'].includes(view.email.phase)
              ? '收不到邮箱验证码？尝试其他验证方式'
              : '选择其他验证方式'
          }}
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
