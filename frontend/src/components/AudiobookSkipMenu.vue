<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import type { AudiobookSkipContext } from '../services/audiobookSkip';
import ActionMenu from './ActionMenu.vue';
import AudiobookSkipDialog from './AudiobookSkipDialog.vue';
import ModalTransition from './ModalTransition.vue';

const props = defineProps<{
  epoch?: string;
  context?: AudiobookSkipContext;
  enabled?: boolean;
  disabled?: boolean;
}>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const target = shallowRef<{ epoch: string; context: AudiobookSkipContext }>();
const key = computed(() =>
  props.context && props.epoch ? JSON.stringify([props.epoch, props.context]) : undefined,
);
const footer = computed(() => props.context?.kind === 'player');
const items = computed(() => [
  {
    key: 'skip',
    icon: 'next',
    label: footer.value ? '跳过片头片尾' : `跳过片头尾${props.enabled ? '（已开启）' : ''}`,
  },
]);
let opened: string | undefined;
watch(
  key,
  () => {
    opened = undefined;
    target.value = undefined;
    menu.value?.close(menu.value.containsFocus());
  },
  { flush: 'sync' },
);
function choose() {
  if (!props.context || !props.epoch || !opened || key.value !== opened) return;
  target.value = { epoch: props.epoch, context: props.context };
  opened = undefined;
}
</script>

<template>
  <ActionMenu
    v-if="context && epoch"
    ref="menu"
    class="audiobook-skip-menu"
    variant="song"
    :label="footer ? '当前声音更多操作' : '播单更多操作'"
    :items="items"
    :disabled="disabled"
    @opened="opened = key"
    @select="choose"
  />
  <Teleport to="body">
    <ModalTransition>
      <AudiobookSkipDialog v-if="target" v-bind="target" @close="target = undefined" />
    </ModalTransition>
  </Teleport>
</template>
