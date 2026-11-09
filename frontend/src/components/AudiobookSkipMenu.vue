<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, watch } from 'vue';
import type { AudiobookSkipContext } from '../services/audiobookSkip';
import { useCore } from '../services/core';
import type { MenuItem } from '../services/menu';
import type {
  PlayerPodcastResumeTarget,
  PlayerPodcastResumeView,
} from '../services/playerPodcastResume';
import ActionMenu from './ActionMenu.vue';
import AudiobookSkipDialog from './AudiobookSkipDialog.vue';
import ModalTransition from './ModalTransition.vue';
import PlayerPodcastResumeDialog from './PlayerPodcastResumeDialog.vue';

const props = defineProps<{
  epoch?: string;
  context?: AudiobookSkipContext;
  enabled?: boolean;
  disabled?: boolean;
  resumeTarget?: PlayerPodcastResumeTarget;
  fallbackFocus?: () => HTMLElement | null;
}>();
const emit = defineEmits<{ resumeConfirmed: [epoch: string, radioId: string, enabled: boolean] }>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const target = shallowRef<{ epoch: string; context: AudiobookSkipContext }>();
const client = useCore();
const resume = shallowRef<PlayerPodcastResumeView>();
const resumeDialog = shallowRef<{
  target: PlayerPodcastResumeTarget;
  initial: PlayerPodcastResumeView;
}>();
let resumeRead = 0;
async function readResume() {
  const visit = ++resumeRead;
  const candidate = props.resumeTarget;
  if (!candidate || resumeDialog.value) return;
  try {
    const result = await client.playerPodcastResume(
      candidate.epoch,
      candidate.playId,
      candidate.entryId,
      { kind: 'view' },
    );
    if (
      visit === resumeRead &&
      candidate === props.resumeTarget &&
      result.epoch === candidate.epoch
    ) {
      resume.value = result.value ?? undefined;
      return resume.value;
    }
  } catch {
    if (visit === resumeRead && candidate === props.resumeTarget) resume.value = undefined;
  }
}
watch(
  () => props.resumeTarget,
  () => {
    resume.value = undefined;
    resumeDialog.value = undefined;
    void readResume();
  },
  { flush: 'sync', immediate: true },
);
const key = computed(() =>
  props.epoch && (props.context || props.resumeTarget)
    ? JSON.stringify([props.epoch, props.context, props.resumeTarget])
    : undefined,
);
const footer = computed(() => !!props.resumeTarget || props.context?.kind === 'player');
const items = computed<MenuItem[]>(() => {
  const choices: MenuItem[] = [];
  if (props.context)
    choices.push({
      key: 'skip',
      icon: 'next',
      label: footer.value ? '跳过片头片尾' : `跳过片头尾${props.enabled ? '（已开启）' : ''}`,
    });
  if (resume.value)
    choices.push({
      key: 'resume',
      icon: 'repeat',
      label: `当前播客断点续播 (${resume.value.enabled ? '开' : '关'})`,
    });
  return choices;
});
watch(
  () => items.value.length > 0 && !!props.epoch,
  async (visible) => {
    if (visible || !props.fallbackFocus) return;
    const menuRoot = menu.value?.$el;
    const owned =
      menu.value?.containsFocus() ||
      (menuRoot instanceof HTMLElement && menuRoot.contains(document.activeElement));
    if (!owned) return;
    await nextTick();
    // Don't steal a focus move made while the old trigger was being removed.
    const active = document.activeElement;
    if (active === document.body || !active?.isConnected)
      props.fallbackFocus()?.focus({ preventScroll: true });
  },
  { flush: 'sync' },
);
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
function menuOpened() {
  opened = key.value;
  if (props.resumeTarget) void readResume();
}
async function choose(action: string) {
  if (!props.epoch || !opened || key.value !== opened) return;
  opened = undefined;
  if (action === 'skip' && props.context) {
    target.value = { epoch: props.epoch, context: props.context };
  } else if (action === 'resume' && props.resumeTarget && resume.value) {
    const scope = props.resumeTarget;
    const owner = document.activeElement;
    const initial = await readResume();
    if (
      initial &&
      scope === props.resumeTarget &&
      !opened &&
      !target.value &&
      document.activeElement === owner
    )
      resumeDialog.value = { target: scope, initial };
  }
}
function resumeConfirmed(radioId: string, enabled: boolean) {
  const scope = resumeDialog.value?.target;
  if (scope && scope === props.resumeTarget) emit('resumeConfirmed', scope.epoch, radioId, enabled);
}
function resumeClosed() {
  resumeDialog.value = undefined;
  void readResume();
}
</script>

<template>
  <ActionMenu
    v-if="items.length && epoch"
    ref="menu"
    class="audiobook-skip-menu"
    variant="song"
    :label="footer ? '当前声音更多操作' : '播单更多操作'"
    :items="items"
    :disabled="disabled"
    @opened="menuOpened"
    @select="choose"
  />
  <Teleport to="body">
    <ModalTransition>
      <AudiobookSkipDialog v-if="target" v-bind="target" @close="target = undefined" />
    </ModalTransition>
    <ModalTransition :fallback-focus="fallbackFocus">
      <PlayerPodcastResumeDialog
        v-if="resumeDialog"
        v-bind="resumeDialog"
        @close="resumeClosed"
        @confirmed="resumeConfirmed"
      />
    </ModalTransition>
  </Teleport>
</template>
