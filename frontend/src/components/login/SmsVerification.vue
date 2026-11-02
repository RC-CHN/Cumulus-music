<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
const props = defineProps<{ model: LoginModel }>();
const { busy, seconds } = props.model;
const countdown = computed(
  () =>
    `${String(Math.floor(seconds.value / 60)).padStart(2, '0')}:${String(seconds.value % 60).padStart(2, '0')}`,
);
const sms = computed(() => props.model.view.value?.sms);
const root = ref<HTMLElement>();
const input = ref<HTMLInputElement>();
const code = ref(''),
  checking = ref(false),
  changing = ref(false),
  edited = ref(false);
let alive = true;
function clearSecrets() {
  code.value = '';
  if (input.value) input.value.value = '';
}
defineExpose({ clearSecrets });
onBeforeUnmount(() => {
  alive = false;
  clearSecrets();
});
watch(
  () => sms.value?.id,
  () => {
    clearSecrets();
    edited.value = false;
  },
);
const recovery = computed(() => sms.value?.phase === 'recovery_required');
const failed = computed(() => ['failed', 'timed_out'].includes(sms.value?.phase ?? ''));
const loading = computed(() => !sms.value || sms.value.phase === 'loading');
const canAnswer = computed(() => !!sms.value && ['ready', 'code_sent'].includes(sms.value.phase));
async function focus() {
  await nextTick();
  if (alive && root.value?.isConnected && !root.value.closest('[inert]'))
    (
      input.value ??
      root.value.querySelector<HTMLElement>('button:not(:disabled)') ??
      root.value
    ).focus({ preventScroll: true });
}
async function check() {
  if (!sms.value || busy.value || !canAnswer.value) return;
  checking.value = true;
  root.value?.focus({ preventScroll: true });
  await props.model.run({ kind: 'sms_check', verification_id: sms.value.id });
  checking.value = false;
  if (alive) await focus();
}
async function refresh() {
  if (!sms.value || busy.value || changing.value) return;
  changing.value = true;
  root.value?.focus({ preventScroll: true });
  await props.model.run({ kind: 'sms_refresh', verification_id: sms.value.id });
  changing.value = false;
  if (alive) await focus();
}
async function downlink() {
  if (!sms.value || changing.value) return;
  changing.value = true;
  await props.model.stopSms(sms.value.id);
  if (alive) await props.model.run({ kind: 'sms_start', direction: 'downlink' });
  changing.value = false;
  if (alive) await focus();
}
async function verify() {
  if (busy.value || !canAnswer.value || !/^[0-9]{4,8}$/.test(code.value)) return;
  const answer = code.value;
  edited.value = false;
  clearSecrets();
  root.value?.focus({ preventScroll: true });
  await props.model.run({ kind: 'verification_sms_verify', code: answer });
  if (alive) await focus();
}
async function send() {
  if (busy.value || seconds.value > 0 || !canAnswer.value) return;
  edited.value = false;
  clearSecrets();
  root.value?.focus({ preventScroll: true });
  await props.model.run({ kind: 'verification_sms_send' });
  if (alive) await focus();
}
</script>
<template>
  <section
    ref="root"
    class="sms-verification"
    :data-sms-direction="sms?.direction"
    :data-sms-phase="sms?.phase"
    :aria-busy="busy || changing"
    tabindex="-1"
    aria-label="短信安全验证"
  >
    <p v-if="loading || changing" class="sms-loading" role="status">正在获取短信验证信息…</p>
    <template v-else-if="sms?.direction === 'uplink'">
      <template v-if="failed">
        <p class="sms-instructions" role="alert">短信验证不成功</p>
        <p class="sms-tips">
          手机使用双卡的用户请确认选择了正确的号码发送短信，如果依旧不成功，请选择其他的验证方式。
        </p>
        <button type="button" class="primary login-submit" :disabled="busy" @click="refresh">
          重新发送
        </button>
        <button type="button" class="sms-alternative" :disabled="changing" @click="downlink">
          无法发送？获取短信验证码
        </button>
      </template>
      <template v-else-if="recovery">
        <p class="sms-tips" role="status">{{ model.error.value || '短信验证结果未确认' }}</p>
        <button
          type="button"
          class="primary login-submit"
          :disabled="busy"
          @click="model.run({ kind: 'reconcile' })"
        >
          核对登录状态
        </button>
        <button type="button" class="sms-secondary" :disabled="busy" @click="refresh">
          重新获取短信信息
        </button>
      </template>
      <template v-else>
        <div class="sms-instructions">
          <p>
            请使用该账号绑定手机号 <span>+{{ sms?.countryCode }} {{ sms?.phoneMask }}</span>
          </p>
          <p>
            发送短信 <strong>{{ sms?.uploadText }}</strong> 到
            <strong>{{ sms?.uploadNumber }}</strong>
          </p>
        </div>
        <ol class="sms-tips">
          <li>确保短信已成功发送后点击下方【已发送】按钮</li>
          <li>手机使用双卡的用户请确认已选择正确的号码发送短信</li>
        </ol>
        <button
          type="button"
          class="primary login-submit"
          :class="{ 'sms-checking': checking }"
          :disabled="busy || !canAnswer"
          @click="check"
        >
          {{ checking ? '验证中…' : '已发送，立即登录' }}
        </button>
      </template>
    </template>
    <template v-else>
      <p class="sms-down-title">请输入短信验证码</p>
      <div class="sms-phone" aria-label="待验证手机号">
        <span v-if="sms?.countryCode">+{{ sms.countryCode }}</span
        ><span>{{ sms?.phoneMask || '账号绑定手机号' }}</span>
      </div>
      <form class="sms-code-form" @submit.prevent="verify">
        <div class="login-code">
          <input
            ref="input"
            v-model="code"
            aria-label="短信安全验证码"
            placeholder="请输入验证码"
            autocomplete="one-time-code"
            inputmode="numeric"
            pattern="[0-9]{4,8}"
            maxlength="8"
            required
            :disabled="busy || !canAnswer"
            @input="edited = true"
          />
          <button type="button" :disabled="busy || !canAnswer || seconds > 0" @click="send">
            {{ seconds ? countdown : '获取验证码' }}
          </button>
        </div>
        <p class="verification-error" role="alert">
          {{
            !busy && !edited && sms?.answerRejected
              ? '验证码错误或已失效，请重新输入'
              : !busy && !edited && sms?.code !== null && sms?.phase === 'ready'
                ? '验证码发送未成功，请稍后重试'
                : ''
          }}
        </p>
        <p v-if="recovery" class="sms-tips" role="status">
          结果未确认，请先核对登录状态，避免重复提交。
        </p>
        <button
          v-if="recovery"
          type="button"
          class="primary login-submit"
          :disabled="busy"
          @click="model.run({ kind: 'reconcile' })"
        >
          核对登录状态
        </button>
        <button
          v-else
          type="submit"
          class="primary login-submit"
          :disabled="busy || !canAnswer || !/^[0-9]{4,8}$/.test(code)"
        >
          确认验证
        </button>
      </form>
    </template>
  </section>
</template>
