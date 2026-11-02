<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref } from 'vue';
const props = withDefaults(
  defineProps<{ disabled: boolean; label?: string; groupLabel?: string }>(),
  { label: '确认码', groupLabel: '原设备确认码' },
);
const emit = defineEmits<{ complete: [value: string]; edit: [] }>();
const root = ref<HTMLElement>();
const digits = ref(Array<string>(6).fill(''));
let completed = false;
function clear() {
  digits.value = Array<string>(6).fill('');
  // Also clear DOM retained by a leaving transition before Vue's next patch.
  root.value?.querySelectorAll('input').forEach((input) => (input.value = ''));
  completed = false;
}
function focus(index = 0) {
  const input = root.value?.querySelectorAll('input')[index];
  if (input?.isConnected && !input.closest('[inert]') && !input.disabled)
    input.focus({ preventScroll: true });
}
function publish() {
  if (props.disabled || completed || !digits.value.every((d) => /^[0-9]$/.test(d))) return;
  const answer = digits.value.join('');
  completed = true;
  digits.value = Array<string>(6).fill('');
  root.value?.querySelectorAll('input').forEach((input) => (input.value = ''));
  emit('complete', answer);
}
function input(event: Event, index: number) {
  if (props.disabled || (event as InputEvent).isComposing) return;
  const target = event.target as HTMLInputElement;
  const value = target.value;
  completed = false;
  if (/^[0-9]{2,6}$/.test(value)) {
    // One-time-code autofill can replace an entire code despite maxlength=1.
    digits.value = [...value.padEnd(6, ' ')].map((c) => (c === ' ' ? '' : c));
  } else {
    digits.value[index] = /^[0-9]$/.test(value) ? value : '';
    target.value = digits.value[index];
  }
  emit('edit');
  if (digits.value.every(Boolean)) publish();
  else if (value)
    void nextTick(() => focus(Math.min(value.length > 1 ? value.length : index + 1, 5)));
}
function keydown(event: KeyboardEvent, index: number) {
  if (props.disabled) return;
  if (event.key === 'Backspace') {
    event.preventDefault();
    const target = digits.value[index] || index === 0 ? index : index - 1;
    digits.value[target] = '';
    completed = false;
    emit('edit');
    void nextTick(() => focus(target));
  } else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
    event.preventDefault();
    focus(Math.max(0, Math.min(5, index + (event.key === 'ArrowLeft' ? -1 : 1))));
  }
}
function paste(event: ClipboardEvent) {
  event.preventDefault();
  if (props.disabled) return;
  const value = event.clipboardData?.getData('text').replace(/\s/g, '') ?? '';
  if (!/^[0-9]{1,6}$/.test(value)) return;
  completed = false;
  digits.value = [...value.padEnd(6, ' ')].map((c) => (c === ' ' ? '' : c));
  emit('edit');
  if (value.length === 6) publish();
  else void nextTick(() => focus(value.length));
}
defineExpose({ clear, focus });
onBeforeUnmount(clear);
</script>
<template>
  <div ref="root" class="login-pin" role="group" :aria-label="groupLabel">
    <input
      v-for="(_, index) in digits"
      :key="index"
      :value="digits[index]"
      :aria-label="`${label}第 ${index + 1} 位`"
      :data-pin-index="index"
      type="text"
      :autocomplete="index === 0 ? 'one-time-code' : 'off'"
      inputmode="numeric"
      maxlength="1"
      pattern="[0-9]"
      :disabled="disabled"
      @input="input($event, index)"
      @compositionend="input($event, index)"
      @keydown="keydown($event, index)"
      @paste="paste"
    />
  </div>
</template>
