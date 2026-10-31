<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { usePlaybackMode } from '../composables/usePlaybackMode';
import { modeIcon, modeLabel } from '../services/playbackMode';
import Icon from './Icon.vue';
import MenuTransition from './MenuTransition.vue';
const props = defineProps<{ player: PlayerModel; togetherActive: boolean }>();
const model = usePlaybackMode(props.player, () => props.togetherActive);
const trigger = ref<HTMLButtonElement>(),
  dismissed = ref(false);
const label = computed(() => modeLabel(model.value.value, model.displayFamily.value));
watch(model.issue, () => {
  dismissed.value = false;
});
function retryMode() {
  // Restore the owner before MenuTransition makes the outgoing surface inert.
  trigger.value?.focus({ preventScroll: true });
  model.retry();
}
async function resumeMusic() {
  const epoch = props.player.view.value?.epoch;
  trigger.value?.focus({ preventScroll: true });
  const active = document.activeElement;
  const result = await props.player.action({ kind: 'play' });
  if (result && epoch === props.player.view.value?.epoch) {
    dismissed.value = true;
    if (document.activeElement === active || document.activeElement === document.body)
      trigger.value?.focus({ preventScroll: true });
  }
}
function escape(event: KeyboardEvent) {
  if (event.key === 'Escape' && model.issue.value && !dismissed.value) {
    event.preventDefault();
    event.stopPropagation();
    dismissed.value = true;
    trigger.value?.focus({ preventScroll: true });
  }
}
function enter(event: FocusEvent) {
  if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node))
    dismissed.value = false;
}
</script>
<template>
  <span
    class="mode-control"
    :data-family="model.family.value"
    :data-proposed-mode="model.value.value"
    @keydown="escape"
    @focusin="enter"
  >
    <button
      ref="trigger"
      class="icon-button"
      :aria-label="label"
      :title="model.pending.value ? `正在切换为${label}` : label"
      :aria-busy="model.pending.value"
      :disabled="!player.ready.value"
      @click="model.cycle"
    >
      <Icon :name="modeIcon(model.value.value, model.displayFamily.value)" />
    </button>
    <MenuTransition>
      <span v-if="model.issue.value && !dismissed" class="mode-feedback">
        <span role="alert">{{ model.issue.value.message }}</span>
        <button
          v-if="model.issue.value.code === 'intelligence_audio_ownership'"
          class="text-button"
          :disabled="player.busy.value"
          @click="resumeMusic"
        >
          恢复音乐
        </button>
        <button v-else-if="model.canRetry.value" class="text-button" @click="retryMode">
          重试切换
        </button>
      </span>
    </MenuTransition>
    <span class="sr-only" role="status">{{
      model.pending.value ? `正在切换为${label}` : label
    }}</span>
  </span>
</template>
