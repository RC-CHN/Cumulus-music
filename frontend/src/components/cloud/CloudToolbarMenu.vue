<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { CloudModel } from '../../composables/useCloud';
import type { PlayerModel } from '../../composables/usePlayer';
import { useCloudToolbar, type CloudToolbarAction } from '../../composables/useCloudToolbar';
import type { TrackView } from '../../services/contracts';
import { useCore } from '../../services/core';
import ActionMenu from '../ActionMenu.vue';
const props = defineProps<{ model: CloudModel; player: PlayerModel; disabled: boolean }>();
const emit = defineEmits<{
  batch: [];
  download: [tracks: TrackView[], revision: string];
  expired: [];
}>();
const root = ref<HTMLElement>(),
  menu = ref<InstanceType<typeof ActionMenu>>();
let preparedQueue: ReturnType<PlayerModel['prepareCloudQueue']> | undefined;
const source = computed(() => {
  const revision = props.model.page.value?.revision;
  return revision && props.model.query.value.tab === 'uploaded'
    ? { epoch: props.model.accountEpoch.value, revision, query: { ...props.model.query.value } }
    : undefined;
});
const state = useCloudToolbar(
  useCore(),
  () => source.value,
  async (action, data, query) => {
    if (props.disabled || document.querySelector('dialog[open]')) return false;
    if (action === 'download') {
      emit(
        'download',
        data.tracks.map((row) => row.track),
        data.revision,
      );
      return true;
    }
    return !!(await preparedQueue?.(
      data.tracks.map((row) => row.track.id),
      data.revision,
      query,
    ));
  },
  () => emit('expired'),
);
watch(
  () => JSON.stringify(source.value),
  () => menu.value?.close(),
);
const items = computed(() => {
  const locked = props.disabled || state.busy.value || !source.value,
    empty = !props.model.page.value?.total;
  return [
    {
      key: 'queue',
      label: '添加全部至播放列表',
      icon: 'list',
      disabled:
        locked ||
        empty ||
        !props.player.ready.value ||
        props.player.busy.value ||
        props.player.queueBusy.value ||
        !!props.player.view.value?.fm,
      hint: props.player.view.value?.fm ? '请先退出私人漫游再添加到普通播放列表' : undefined,
    },
    { key: 'batch', label: '批量操作', icon: 'grid', disabled: locked },
    { key: 'download', label: '下载全部', icon: 'download', disabled: locked || empty },
  ];
});
let last: CloudToolbarAction = 'queue';
async function run(action: CloudToolbarAction) {
  last = action;
  if (action === 'queue') preparedQueue = props.player.prepareCloudQueue();
  const owner = props.model.accountEpoch.value;
  root.value?.focus({ preventScroll: true });
  await state.run(action);
  await nextTick();
  if (owner === props.model.accountEpoch.value && document.activeElement === root.value)
    root.value
      ?.querySelector<HTMLElement>('.menu-trigger:not(:disabled)')
      ?.focus({ preventScroll: true });
}
function choose(key: string) {
  if (!items.value.some((i) => i.key === key && !i.disabled)) return;
  if (key === 'batch') emit('batch');
  else void run(key as CloudToolbarAction);
}
async function refresh() {
  const owner = props.model.accountEpoch.value;
  root.value?.focus({ preventScroll: true });
  state.clear();
  await props.model.refresh(true);
  await returnFocus(owner);
}
async function returnFocus(owner: string) {
  await nextTick();
  if (owner === props.model.accountEpoch.value && document.activeElement === root.value)
    root.value
      ?.querySelector<HTMLElement>('.menu-trigger:not(:disabled)')
      ?.focus({ preventScroll: true });
}
function dismiss() {
  const owner = props.model.accountEpoch.value;
  root.value?.focus({ preventScroll: true });
  state.clear();
  void returnFocus(owner);
}
defineExpose({ ...state, clear: dismiss, retry: () => run(last), refresh });
</script>
<template>
  <span
    ref="root"
    class="cloud-toolbar-more"
    data-cloud-toolbar-more
    tabindex="-1"
    :aria-busy="state.busy.value"
  >
    <ActionMenu
      ref="menu"
      label="云盘更多操作"
      variant="song"
      :disabled="disabled || state.busy.value || !source || !model.page.value?.uploadedCount"
      :items="items"
      @select="choose"
    />
  </span>
</template>
