<script setup lang="ts">
import Icon from '../Icon.vue';
defineProps<{ methods: string[]; busy: boolean }>();
defineEmits<{ select: [method: string] }>();
const labels: Record<string, [string, string]> = {
  DEVICE_CONFIRM: ['原设备确认', 'phoneConfirm'],
  DEVICE_SCAN: ['原设备扫码验证', 'scan'],
  SECURITY_EMAIL: ['安全邮箱', 'mail'],
  UP_SMS: ['短信验证', 'comment'],
  BINDING_CELLPHONE: ['绑定手机号验证', 'phoneConfirm'],
  WEICHAT_AUTH: ['绑定的微信授权', 'comment'],
};
</script>
<template>
  <div class="verification-choice">
    <p>你正在一台新设备登录</p>
    <p class="muted">为确保是本人操作，请选择任意一种方式进行验证</p>
    <div class="verification-methods" aria-label="安全验证方式">
      <button
        v-for="method in methods.filter((m) => labels[m])"
        :key="method"
        :data-verification-method="method"
        :disabled="busy"
        @click="$emit('select', method)"
      >
        <Icon :name="labels[method][1]" /><span>{{ labels[method][0] }}</span
        ><span aria-hidden="true">›</span>
      </button>
    </div>
  </div>
</template>
