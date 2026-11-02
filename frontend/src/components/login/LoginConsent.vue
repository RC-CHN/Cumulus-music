<script setup lang="ts">
import { ref } from 'vue';
import type { LoginDocument } from '../../services/loginPresentation';
defineProps<{ modelValue: boolean; invalid: boolean; disabled: boolean; opening: boolean }>();
const emit = defineEmits<{
  'update:modelValue': [value: boolean];
  open: [document: LoginDocument];
}>();
const checkbox = ref<HTMLInputElement>();
defineExpose({ focus: () => checkbox.value?.focus({ preventScroll: true }) });
</script>
<template>
  <div class="login-consent" :data-consent-required="invalid">
    <p v-if="invalid" class="login-consent-tip" role="alert">请阅读并勾选</p>
    <label
      ><input
        ref="checkbox"
        :checked="modelValue"
        type="checkbox"
        aria-label="同意登录协议"
        :disabled="disabled"
        @change="emit('update:modelValue', ($event.target as HTMLInputElement).checked)"
      />同意</label
    >
    <div class="login-legal-links">
      <button type="button" :disabled="opening" @click="emit('open', 'service')">
        《服务条款》
      </button>
      <button type="button" :disabled="opening" @click="emit('open', 'privacy')">
        《隐私政策》
      </button>
      <button type="button" :disabled="opening" @click="emit('open', 'children')">
        《儿童隐私政策》
      </button>
    </div>
  </div>
</template>
