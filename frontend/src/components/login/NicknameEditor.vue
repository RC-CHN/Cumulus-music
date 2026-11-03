<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId } from 'vue';
import { nicknameFormatIssue } from '../../services/nickname';
import { nicknameError } from '../../services/phoneReuse';
import Icon from '../Icon.vue';
const props = defineProps<{
  modelValue: string;
  disabled: boolean;
  busy: boolean;
  error: string;
  allowDefault: boolean;
  submitBlocked?: boolean;
  nameUnavailable?: boolean;
  errorMuted?: boolean;
}>();
const emit = defineEmits<{
  'update:modelValue': [value: string];
  random: [];
  submit: [useDefault: boolean];
}>();
const input = ref<HTMLInputElement>();
const feedbackId = useId();
const spin = ref(0);
let alive = true;
const formatIssue = computed(() => nicknameFormatIssue(props.modelValue));
const feedback = computed(() => props.error || nicknameError(formatIssue.value));
const blocked = computed(
  () => props.disabled || props.submitBlocked || props.nameUnavailable || !!formatIssue.value,
);
function submit(useDefault = false) {
  if (props.disabled || props.submitBlocked || (useDefault ? !props.allowDefault : blocked.value))
    return;
  emit('submit', useDefault);
}
async function clear() {
  if (props.disabled) return;
  emit('update:modelValue', '');
  await nextTick();
  if (alive && input.value?.isConnected && !input.value.closest('[inert]'))
    input.value.focus({ preventScroll: true });
}
function random() {
  if (props.disabled) return;
  spin.value++;
  emit('random');
}
function clearDom() {
  if (input.value) input.value.value = '';
}
defineExpose({ clearDom });
onBeforeUnmount(() => {
  alive = false;
  clearDom();
});
</script>
<template>
  <form class="nickname-editor" novalidate @submit.prevent="submit()">
    <div class="reuse-name-field">
      <input
        ref="input"
        :value="modelValue"
        aria-label="注册昵称"
        :aria-describedby="feedbackId"
        :aria-invalid="!!formatIssue || nameUnavailable || undefined"
        autocomplete="off"
        maxlength="64"
        placeholder="请输入昵称"
        :disabled="disabled"
        @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
      />
      <button
        v-if="modelValue"
        type="button"
        class="nickname-clear"
        aria-label="清空昵称"
        :disabled="disabled"
        @click="clear"
      >
        <Icon name="close" />
      </button>
      <button
        type="button"
        class="nickname-random"
        aria-label="换一个推荐昵称"
        :disabled="disabled"
        @click="random"
      >
        <Icon :key="spin" name="refresh" :class="{ 'nickname-random-spinning': spin > 0 }" />
        <span>随机</span>
      </button>
    </div>
    <div :id="feedbackId" class="nickname-editor-feedback">
      <p
        v-if="feedback"
        class="verification-error"
        :class="{
          'nickname-feedback-muted': errorMuted || (!error && formatIssue === 'too_short'),
        }"
        role="alert"
      >
        {{ feedback }}
      </p>
    </div>
    <button type="submit" class="primary login-submit reuse-primary" :disabled="blocked">
      {{ busy ? '正在处理…' : '完成注册' }}
    </button>
    <button
      v-if="allowDefault"
      type="button"
      class="reuse-default"
      :disabled="disabled || submitBlocked"
      @click="submit(true)"
    >
      以默认昵称注册
    </button>
    <slot />
  </form>
</template>
