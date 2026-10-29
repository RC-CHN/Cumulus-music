<script setup lang="ts">
import { onUnmounted, watch } from 'vue';
import { createLyricSettingsClient } from '../adapters/desktopLyrics';
import { useDesktopLyricSettings } from '../composables/useDesktopLyricSettings';
const props = defineProps<{ epoch: string }>();
const model = useDesktopLyricSettings(createLyricSettingsClient());
watch(
  () => props.epoch,
  (epoch) => void model.connect(epoch),
  { immediate: true },
);
onUnmounted(model.dispose);
async function toggle() {
  await model.read();
  if (model.preferences.value)
    await model.change({ kind: 'enabled', value: !model.preferences.value.enabled });
}
</script>
<template>
  <span class="desktop-lyric-entry"
    ><button
      class="text-button"
      :class="{ active: model.preferences.value?.enabled }"
      :aria-pressed="model.preferences.value?.enabled ?? false"
      :disabled="!model.canChange.value"
      @click="toggle"
    >
      桌面歌词</button
    ><span v-if="model.error.value" class="desktop-lyric-entry-error" role="alert">{{
      model.error.value
    }}</span></span
  >
</template>
<style scoped>
.desktop-lyric-entry {
  position: relative;
  margin-left: auto;
  margin-right: 16px;
}
.desktop-lyric-entry .active {
  color: var(--accent);
}
.desktop-lyric-entry-error {
  position: absolute;
  top: 100%;
  right: 0;
  width: 220px;
  padding: 10px;
  border-radius: 8px;
  background: var(--surface);
  color: var(--accent);
  font-size: 12px;
}
</style>
