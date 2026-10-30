<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import ActionMenu from './ActionMenu.vue';
const props = defineProps<{ player: PlayerModel }>();
const menu = ref<InstanceType<typeof ActionMenu>>();
// The first six rows match the recovered VoicePlaySpeed.tsx. Retain the extra
// engine speeds already supported by Cumulus in a separate group.
const speeds = [2, 1.5, 1.25, 1, 0.75, 0.5, 0.25, 3, 4];
const current = computed(() => props.player.view.value?.speed ?? 1);
const items = computed(() =>
  speeds.flatMap((value, index) => [
    ...(index === 6 ? [{ key: 'extended', label: '', separator: true }] : []),
    {
      key: String(value),
      label: `${value}×`,
      checked: current.value === value,
      disabled: props.player.busy.value,
    },
  ]),
);
watch(
  () => props.player.view.value?.epoch,
  () => menu.value?.close(),
  { flush: 'sync' },
);
function choose(key: string) {
  const value = Number(key);
  if (!props.player.busy.value && speeds.includes(value))
    void props.player.action({ kind: 'speed', value });
}
</script>
<template>
  <ActionMenu
    ref="menu"
    class="speed-control"
    label="播放速度"
    :trigger-text="current === 1 ? '倍速' : `${current}×`"
    :disabled="!player.ready.value"
    :items="items"
    variant="speed"
    focus-checked
    @select="choose"
  />
</template>
