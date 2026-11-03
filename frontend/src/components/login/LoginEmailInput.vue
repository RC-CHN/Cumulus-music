<script setup lang="ts">
import { computed, ref, useId } from 'vue';
import { emailSuggestions } from '../../services/loginPresentation';
const props = defineProps<{ modelValue: string; disabled: boolean }>();
const emit = defineEmits<{ 'update:modelValue': [value: string]; chosen: [] }>();
const focused = ref(false),
  active = ref(0),
  dismissed = ref(false);
const id = useId();
const suggestions = computed(() =>
  focused.value && !dismissed.value ? emailSuggestions(props.modelValue) : [],
);
function input(event: Event) {
  dismissed.value = false;
  active.value = 0;
  emit('update:modelValue', (event.target as HTMLInputElement).value);
}
function choose(value: string) {
  emit('update:modelValue', value);
  dismissed.value = true;
  active.value = 0;
  emit('chosen');
}
function key(event: KeyboardEvent) {
  if (event.isComposing || event.keyCode === 229) return;
  if (event.key === 'Escape' && suggestions.value.length) {
    event.stopPropagation();
    event.preventDefault();
    dismissed.value = true;
    active.value = 0;
  } else if (['ArrowUp', 'ArrowDown'].includes(event.key) && suggestions.value.length) {
    event.preventDefault();
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    active.value = Math.max(0, Math.min(suggestions.value.length - 1, active.value + delta));
  } else if (event.key === 'Enter' && suggestions.value[active.value]) {
    event.preventDefault();
    if (!event.repeat) choose(suggestions.value[active.value]!);
  }
}
</script>
<template>
  <div class="login-email-input">
    <label :for="id"><span class="sr-only">邮箱</span></label>
    <input
      :id="id"
      :value="modelValue"
      aria-label="登录邮箱"
      type="email"
      placeholder="请输入邮箱"
      autocomplete="username"
      maxlength="254"
      required
      :disabled="disabled"
      role="combobox"
      aria-autocomplete="list"
      :aria-expanded="suggestions.length > 0"
      :aria-controls="suggestions.length ? `${id}-suggestions` : undefined"
      :aria-activedescendant="suggestions[active] ? `${id}-suggestion-${active}` : undefined"
      @input="input"
      @focus="focused = true"
      @blur="focused = false"
      @keydown="key"
    />
    <div
      v-if="suggestions.length"
      :id="`${id}-suggestions`"
      role="listbox"
      aria-label="邮箱建议"
      class="login-email-suggestions"
    >
      <div
        v-for="(value, index) in suggestions"
        :id="`${id}-suggestion-${index}`"
        :key="value"
        role="option"
        :aria-selected="active === index"
        :class="{ active: active === index }"
        @pointerdown.prevent
        @click="choose(value)"
      >
        {{ value }}
      </div>
    </div>
  </div>
</template>
