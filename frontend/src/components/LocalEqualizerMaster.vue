<script setup lang="ts">
import { computed } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import EffectMasterSwitch from './EffectMasterSwitch.vue';

const props = defineProps<{
  player: PlayerModel;
  disabled?: boolean;
  inHeader?: boolean;
  canToggle?: () => boolean;
  spatialKey?: string;
  spatialBusy?: boolean;
}>();
const emit = defineEmits<{ change: []; spatialToggle: [renderedKey: string] }>();
const { view, equalizerDraft } = props.player;
const { settings, draft, pending, discarding, blocked, toggleMaster } = equalizerDraft;
const shownSettings = computed(() =>
  props.inHeader ? view.value?.equalizer.settings : settings.value,
);
const busy = computed(
  () =>
    props.disabled ||
    props.spatialBusy ||
    (props.inHeader && !!(draft.value || pending.value || discarding.value || blocked.value)),
);
function change(event: MouseEvent) {
  if (busy.value || !shownSettings.value || props.canToggle?.() === false) return;
  const renderedKey = (event.currentTarget as HTMLButtonElement).dataset.spatialMasterKey;
  if (renderedKey !== undefined || props.spatialKey !== undefined) {
    if (renderedKey && renderedKey === props.spatialKey) emit('spatialToggle', renderedKey);
    return;
  }
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
    :data-spatial-master-key="spatialKey"
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
