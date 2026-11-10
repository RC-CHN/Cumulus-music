<script setup lang="ts">
import { computed } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import EffectMasterSwitch from './EffectMasterSwitch.vue';

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
  <EffectMasterSwitch
    class="local-equalizer-master"
    :label="inHeader ? '本机均衡器与增强音效' : '启用均衡器和增强音效'"
    :checked="!!shownSettings && !shownSettings.master_bypassed"
    :busy="busy"
    :keep-focus="inHeader"
    @toggle="change"
  >
    {{
      inHeader
        ? '本机均衡器'
        : shownSettings?.master_bypassed
          ? '均衡器与增强音效已暂停'
          : '均衡器与增强音效'
    }}
  </EffectMasterSwitch>
</template>
