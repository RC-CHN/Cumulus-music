<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { CloudModel } from '../../composables/useCloud';
import type { PlayerModel } from '../../composables/usePlayer';
import { useCloudResource } from '../../composables/useCloudResource';
import type { CloudTrack } from '../../services/cloud';
import type { TrackView } from '../../services/contracts';
import { failure } from '../../services/contracts';
import {
  cloudMenuItems,
  cloudMenuTarget,
  cloudShareTrack,
  type CloudMenuTarget,
} from '../../services/cloudMenu';
import { useCore } from '../../services/core';
import ActionMenu from '../ActionMenu.vue';
import ModalTransition from '../ModalTransition.vue';
import TrackShareDialog from '../TrackShareDialog.vue';
const props = defineProps<{
  model: CloudModel;
  player: PlayerModel;
  rows: CloudTrack[];
  selected: Set<string>;
  disabled: boolean;
}>();
const emit = defineEmits<{
  collect: [target: CloudMenuTarget, origin: HTMLElement | undefined];
  remove: [target: CloudMenuTarget, origin: HTMLElement | undefined];
  expired: [];
  dialog: [open: boolean];
}>();
const client = useCore(),
  state = useCloudResource(
    client,
    () => props.model.accountEpoch.value,
    () => props.model.page.value?.revision,
    () => emit('expired'),
  );
const menu = ref<InstanceType<typeof ActionMenu>>(),
  shared = shallowRef<TrackView>(),
  acting = ref(false),
  message = ref(''),
  failed = ref(false);
let origin: HTMLElement | undefined,
  host: HTMLElement | undefined,
  owner = '',
  operation = 0;
const items = computed(() => {
  const target = state.target.value,
    value = state.resource.value;
  if (!target) return [];
  if (state.error.value)
    return [
      { key: 'issue', label: state.error.value, disabled: true },
      {
        key: 'retry',
        label: state.needsRefresh.value ? '刷新云盘列表' : '重新读取操作',
        icon: 'refresh',
        disabled: state.loading.value,
        keepOpen: !state.needsRefresh.value,
      },
    ];
  if (state.loading.value || !value)
    return [{ key: 'loading', label: '正在核对歌曲操作…', disabled: true }];
  return cloudMenuItems(target, value, {
    locked: props.disabled || acting.value || !state.active(),
    playable: props.player.ready.value && !props.player.busy.value,
    queueable:
      props.player.ready.value &&
      !props.player.busy.value &&
      !props.player.queueBusy.value &&
      !props.player.view.value?.fm,
    fm: !!props.player.view.value?.fm,
  });
});
watch(shared, (value) => emit('dialog', !!value));
watch(state.target, (value) => {
  if (!value) close();
});
function close() {
  operation++;
  menu.value?.close();
  shared.value = undefined;
  state.clear();
  message.value = '';
  acting.value = false;
}
onBeforeUnmount(close);
async function show(event: MouseEvent | KeyboardEvent, row: CloudTrack) {
  if (
    event instanceof KeyboardEvent &&
    !(event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  if (props.disabled) return;
  const revision = props.model.page.value?.revision;
  if (!revision) return;
  const target = cloudMenuTarget(
    props.rows,
    props.selected,
    row.track.id,
    props.model.accountEpoch.value,
    revision,
    props.model.query.value,
  );
  if (!target) return;
  close();
  state.set(target);
  owner = target.epoch;
  const tr = (event.currentTarget as HTMLElement).closest<HTMLElement>('[data-cloud-id]');
  origin =
    (event.target as HTMLElement).closest<HTMLElement>('button,input') ??
    tr?.querySelector<HTMLElement>('.cloud-more') ??
    undefined;
  host =
    tr
      ?.closest<HTMLElement>('.cloud-page')
      ?.querySelector<HTMLElement>('[aria-label="搜索云盘歌曲"]') ?? undefined;
  origin?.focus({ preventScroll: true });
  const r = origin?.getBoundingClientRect();
  const pointer =
    event instanceof MouseEvent &&
    event.type === 'contextmenu' &&
    (event.clientX !== 0 || event.clientY !== 0);
  await nextTick();
  if (!state.active()) return;
  await menu.value?.showAt(
    pointer ? event.clientX : (r?.left ?? 8),
    pointer ? event.clientY : (r?.bottom ?? 8),
    origin,
  );
  if (state.active()) void state.read();
}
async function refresh() {
  close();
  await props.model.refresh(true);
}
function feedback(text: string, error = false) {
  message.value = text;
  failed.value = error;
}
async function choose(key: string) {
  if (key === 'retry') {
    if (state.needsRefresh.value) await refresh();
    else await state.read();
    return;
  }
  const target = state.target.value,
    value = state.resource.value;
  if (
    !state.active() ||
    !target ||
    !value ||
    !items.value.some((i) => i.key === key && !i.disabled)
  )
    return;
  if (key === 'collect') {
    emit('collect', target, origin);
    return;
  }
  if (key === 'remove') {
    emit('remove', target, origin);
    return;
  }
  if (key === 'share') {
    shared.value = cloudShareTrack(value);
    return;
  }
  const attempt = ++operation;
  acting.value = true;
  failed.value = false;
  try {
    if (key === 'open') {
      await client.openCloudDirectory(target.epoch, target.clickedId, target.revision);
      if (attempt === operation && state.active()) feedback('已请求打开文件所在目录');
    } else {
      const quality = props.player.view.value?.qualityPreferences.quality ?? 'standard';
      const result =
        key === 'next'
          ? await props.player.queueCloud(
              target.tracks.map((t) => t.track.id),
              target.revision,
              target.query,
            )
          : await props.player.selectForPage(
              target.selection
                ? {
                    kind: 'cloud_batch',
                    ids: target.tracks.map((t) => t.track.id),
                    revision: target.revision,
                    quality,
                    query: target.query,
                  }
                : {
                    kind: 'cloud',
                    query: target.query,
                    revision: target.revision,
                    start_id: target.clickedId,
                    quality,
                  },
            );
      if (attempt === operation && state.active())
        feedback(
          result
            ? key === 'next'
              ? '播放列表已更新'
              : '正在播放云盘歌曲'
            : '本次播放操作已更新或取消',
        );
    }
  } catch (e) {
    if (attempt === operation && state.active()) {
      const issue = failure(e);
      feedback(issue.message, true);
      if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
    }
  } finally {
    if (attempt === operation) acting.value = false;
  }
}
function dismiss() {
  message.value = '';
  if (origin?.isConnected) origin.focus({ preventScroll: true });
  else host?.focus({ preventScroll: true });
}
const returnOrigin = () => origin ?? null,
  returnHost = () => host ?? null;
defineExpose({ show, close });
</script>
<template>
  <ActionMenu
    ref="menu"
    context-only
    variant="song"
    label="云盘歌曲操作"
    :items="items"
    @select="choose"
  />
  <ModalTransition
    :restore-focus="owner === model.accountEpoch.value"
    :return-focus="returnOrigin"
    :fallback-focus="returnHost"
  >
    <TrackShareDialog v-if="shared" :track="shared" @close="shared = undefined" />
  </ModalTransition>
  <div v-if="message" class="track-action-feedback" :class="{ failed }" role="status">
    {{ message
    }}<button v-if="failed" class="text-button" :disabled="acting" @click="refresh">
      刷新云盘
    </button>
    <button class="text-button" @click="dismiss">关闭</button>
  </div>
</template>
