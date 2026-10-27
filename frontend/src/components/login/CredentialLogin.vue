<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useCore } from '../../services/core';
import { useLogin } from '../../composables/useLogin';
import { loginMessage, type LoginMode } from '../../services/auth';
import LoginChallenge from './LoginChallenge.vue';
const props = defineProps<{ epoch: string; mode: Exclude<LoginMode, 'saved_account'> }>();
const emit = defineEmits<{
  authorized: [];
  mode: [value: Exclude<LoginMode, 'saved_account'>];
  qr: [];
}>();
const model = useLogin(useCore(), () => emit('authorized'));
const { view, busy, error, seconds } = model;
const phone = ref(''),
  country = ref('86'),
  email = ref(''),
  password = ref(''),
  code = ref('');
const remember = ref(true);
const initial = computed(
  () => !!view.value && ['ready', 'code_sent', 'rejected'].includes(view.value.status),
);
const disabled = computed(() => busy.value || !view.value || !initial.value);
async function start() {
  password.value = '';
  code.value = '';
  await model.start(props.epoch, props.mode);
}
watch(
  () => props.mode,
  () => {
    void start();
  },
);
onMounted(() => {
  void start();
});
onUnmounted(() => model.dispose());
async function submit() {
  if (disabled.value) return;
  const secret = password.value,
    otp = code.value;
  password.value = '';
  code.value = '';
  if (view.value?.status === 'rejected') await model.start(props.epoch, props.mode);
  if (props.mode === 'email')
    await model.run({
      kind: 'email',
      address: email.value.trim(),
      password: secret,
      remember: remember.value,
    });
  else if (props.mode === 'phone_password')
    await model.run({
      kind: 'phone_password',
      phone: phone.value.trim(),
      country_code: country.value,
      password: secret,
      remember: remember.value,
    });
  else
    await model.run({
      kind: 'phone_sms',
      phone: phone.value.trim(),
      country_code: country.value,
      code: otp,
      remember: remember.value,
    });
}
</script>
<template>
  <form v-if="initial || !view" class="credential-form" @submit.prevent="submit">
    <label v-if="mode === 'email'"
      >邮箱<input
        v-model="email"
        aria-label="登录邮箱"
        type="email"
        autocomplete="username"
        maxlength="254"
        required
        :disabled="disabled"
    /></label>
    <div v-else class="login-phone">
      <label class="country"
        >区号<input
          v-model="country"
          aria-label="国际区号"
          inputmode="numeric"
          pattern="[0-9]{1,4}"
          maxlength="4"
          required
          :disabled="disabled"
      /></label>
      <label
        >手机号<input
          v-model="phone"
          aria-label="登录手机号"
          type="tel"
          autocomplete="tel-national"
          pattern="[0-9]{5,15}"
          maxlength="15"
          required
          :disabled="disabled"
      /></label>
    </div>
    <label v-if="mode !== 'phone_sms'"
      >密码<input
        v-model="password"
        aria-label="登录密码"
        type="password"
        autocomplete="current-password"
        maxlength="1024"
        required
        :disabled="disabled"
    /></label>
    <label v-else
      >验证码<span class="login-code"
        ><input
          v-model="code"
          aria-label="短信验证码"
          inputmode="numeric"
          autocomplete="one-time-code"
          pattern="[0-9]{4,8}"
          maxlength="8"
          required
          :disabled="disabled"
        /><button
          type="button"
          :disabled="
            disabled || seconds > 0 || !/^[0-9]{5,15}$/.test(phone) || !/^[0-9]{1,4}$/.test(country)
          "
          @click="model.run({ kind: 'send_sms', phone, country_code: country })"
        >
          {{ seconds ? `${seconds} 秒后重发` : '获取验证码' }}
        </button></span
      ></label
    >
    <div class="login-options">
      <label><input v-model="remember" type="checkbox" :disabled="busy" />自动登录</label
      ><button
        v-if="mode !== 'email'"
        type="button"
        :disabled="busy"
        @click="emit('mode', mode === 'phone_sms' ? 'phone_password' : 'phone_sms')"
      >
        {{ mode === 'phone_sms' ? '密码登录' : '验证码登录' }}
      </button>
    </div>
    <button class="primary login-submit" type="submit" :disabled="disabled">
      {{ busy ? '正在登录…' : '登录' }}
    </button>
  </form>
  <LoginChallenge v-else :model="model" @qr="emit('qr')" />
  <p class="login-status" role="status" :data-login-status="view?.status">
    {{ error || loginMessage(view) }}
  </p>
  <button
    v-if="
      !busy &&
      (!view || ['error', 'rejected', 'rate_limited', 'account_disabled'].includes(view.status))
    "
    class="login-restart"
    @click="start"
  >
    重新开始登录
  </button>
</template>
