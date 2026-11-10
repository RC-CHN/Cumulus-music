<script setup lang="ts">
defineProps<{
  checked: boolean;
  label: string;
  busy?: boolean;
  keepFocus?: boolean;
}>();
const emit = defineEmits<{ toggle: [event: MouseEvent] }>();
</script>

<template>
  <button
    type="button"
    class="effect-master-switch"
    role="switch"
    :aria-label="label"
    :aria-checked="checked"
    :aria-disabled="keepFocus && busy ? true : undefined"
    :disabled="!keepFocus && busy"
    @click="emit('toggle', $event)"
  >
    <span aria-hidden="true"></span>
    <slot />
  </button>
</template>

<style scoped>
.effect-master-switch {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 30px;
  border-radius: 6px;
  color: var(--text-secondary);
  font-size: 12px;
  white-space: nowrap;
}
.effect-master-switch[aria-disabled='true'] {
  opacity: 0.65;
}
.effect-master-switch span {
  position: relative;
  flex: none;
  width: 30px;
  height: 18px;
  border-radius: 12px;
  background: var(--line-strong);
}
.effect-master-switch span::after {
  content: '';
  position: absolute;
  inset: 2px auto auto 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--surface);
  transition: transform 0.15s;
}
.effect-master-switch[aria-checked='true'] span {
  background: var(--accent-fill);
}
.effect-master-switch[aria-checked='true'] span::after {
  transform: translateX(12px);
}
:global([data-motion='reduced'] .effect-master-switch span::after) {
  transition: none;
}
@media (prefers-reduced-motion: reduce) {
  .effect-master-switch span::after {
    transition: none;
  }
}
</style>
