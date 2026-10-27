<script setup lang="ts">
import { computed, ref } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
const props = defineProps<{ model: LoginModel }>();
const emit = defineEmits<{ qr: [] }>();
const { view, busy, seconds } = props.model;
const method = ref(''),
  code = ref(''),
  answer = ref('');
const known: Record<string, string> = {
  SECURITY_EMAIL: '安全邮箱',
  UP_SMS: '短信验证码',
  BINDING_CELLPHONE: '绑定手机号验证',
  DEVICE_CONFIRM: '原设备确认',
  DEVICE_SCAN: '原设备扫码',
  WEICHAT_AUTH: '微信授权',
};
const available = computed(
  () => view.value?.challenge.methods.filter((m) => ['SECURITY_EMAIL', 'UP_SMS'].includes(m)) ?? [],
);
const selected = computed(() =>
  available.value.includes(method.value) ? method.value : (available.value[0] ?? ''),
);
async function verify() {
  const otp = code.value;
  code.value = '';
  if (selected.value === 'SECURITY_EMAIL')
    await props.model.run({ kind: 'security_mail_verify', code: otp });
  else if (selected.value === 'UP_SMS')
    await props.model.run({ kind: 'verification_sms_verify', code: otp });
}
</script>
<template>
  <section class="login-challenge" aria-label="登录验证">
    <template v-if="view?.status === 'two_factor'">
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
        <label v-if="available.length"
          >验证方式<select
            :value="selected"
            :disabled="busy"
            aria-label="安全验证方式"
            @change="method = ($event.target as HTMLSelectElement).value"
          >
            <option v-for="m in available" :key="m" :value="m">{{ known[m] }}</option>
          </select></label
        >
        <p
          v-for="m in view.challenge.methods.filter((m) => !available.includes(m))"
          :key="m"
          class="muted"
        >
          {{
            m === 'BINDING_CELLPHONE'
              ? '当前账号需要绑定手机号，请前往官方 App 操作。'
              : `${known[m]}尚未接入，请使用官方 App 完成验证。`
          }}
        </p>
        <form v-if="selected" class="credential-form" @submit.prevent="verify">
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
