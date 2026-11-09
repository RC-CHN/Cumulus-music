<script setup lang="ts">
import { computed } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';

const props = defineProps<{
  player: PlayerModel;
  disabled?: boolean;
  inHeader?: boolean;
  canToggle?: () => boolean;
}>();
const emit = defineEmits<{ change: [] }>();
const { view, equalizerDraft } = props.player;
const { settings, draft, pending, discarding, blocked, toggleMaster } = equalizerDraft;
const shownSettings = computed(() =>
  props.inHeader ? view.value?.equalizer.settings : settings.value,
);
const busy = computed(
  () =>
    props.disabled ||
    (props.inHeader && !!(draft.value || pending.value || discarding.value || blocked.value)),
);
function change() {
  if (busy.value || !shownSettings.value || props.canToggle?.() === false) return;
  emit('change');
  toggleMaster();
}
</script>

<template>
  <button
    type="button"
    class="local-equalizer-master"
    role="switch"
    :aria-label="inHeader ? '本机均衡器与增强音效' : '启用均衡器和增强音效'"
    :aria-checked="!!shownSettings && !shownSettings.master_bypassed"
    :aria-disabled="inHeader && busy ? true : undefined"
    :disabled="!inHeader && busy"
    @click="change"
  >
    <span aria-hidden="true"></span>
    {{
      inHeader
        ? '本机均衡器'
        : shownSettings?.master_bypassed
          ? '均衡器与增强音效已暂停'
          : '均衡器与增强音效'
    }}
  </button>
</template>

<style scoped>
.local-equalizer-master {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 30px;
  border-radius: 6px;
  color: var(--text-secondary);
  font-size: 12px;
  white-space: nowrap;
}
.local-equalizer-master[aria-disabled='true'] {
  opacity: 0.65;
}
.local-equalizer-master span {
  position: relative;
  flex: none;
  width: 30px;
  height: 18px;
  border-radius: 12px;
  background: var(--line-strong);
}
.local-equalizer-master span::after {
  content: '';
  position: absolute;
  inset: 2px auto auto 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--surface);
  transition: transform 0.15s;
}
.local-equalizer-master[aria-checked='true'] span {
  background: var(--accent-fill);
}
.local-equalizer-master[aria-checked='true'] span::after {
  transform: translateX(12px);
}
@media (prefers-reduced-motion: reduce) {
  .local-equalizer-master span::after {
    transition: none;
  }
}
</style>
