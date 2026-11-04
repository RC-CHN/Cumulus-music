<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import ActionMenu from './ActionMenu.vue';
import { failure } from '../services/contracts';
const props = defineProps<{ player: PlayerModel }>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const pending = ref(false);
const issue = ref('');
let generation = 0;
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
      disabled: pending.value,
    },
  ]),
);
watch(
  [
    () => props.player.view.value?.epoch,
    () => props.player.view.value?.speedScope,
    props.player.ready,
  ],
  () => {
    generation++;
    pending.value = false;
    issue.value = '';
    menu.value?.close();
  },
  { flush: 'sync' },
);
onBeforeUnmount(() => generation++);
async function choose(key: string) {
  const value = Number(key);
  if (!props.player.ready.value || pending.value || !speeds.includes(value)) return;
  const owner = ++generation;
  pending.value = true;
  issue.value = '';
  try {
    await props.player.configureSpeed(value);
  } catch (error) {
    const result = failure(error);
    if (owner === generation && result.code !== 'stale') issue.value = result.message;
  } finally {
    if (owner === generation) pending.value = false;
  }
}
</script>
<template>
  <span
    class="speed-control"
    :data-speed-save-state="pending ? 'pending' : issue ? 'failed' : 'idle'"
  >
    <ActionMenu
      ref="menu"
      label="播放速度"
      :trigger-text="current === 1 ? '倍速' : `${current}×`"
      :disabled="!player.ready.value"
      :items="items"
      variant="speed"
      focus-checked
      @select="choose"
    >
      <template #header>
        <p v-if="issue" class="speed-save-error">未能更改播放速度：{{ issue }}</p>
      </template>
    </ActionMenu>
    <span v-if="issue" class="speed-error-marker" aria-hidden="true">!</span>
    <span class="sr-only" role="status">{{ issue ? `未能更改播放速度：${issue}` : '' }}</span>
  </span>
</template>

<style scoped>
.speed-control {
  position: relative;
}
.speed-error-marker {
  position: absolute;
  top: -3px;
  right: 1px;
  color: var(--accent);
  font-weight: 700;
  pointer-events: none;
}
.speed-save-error {
  margin: 4px 10px;
  overflow-wrap: anywhere;
}
</style>
