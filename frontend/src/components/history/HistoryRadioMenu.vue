<script lang="ts">
import type { HistoryRadioKind } from '../../services/history';

export interface HistoryRadioMenuTarget {
  kind: HistoryRadioKind;
  revision: string;
  id: string;
  name: string;
  epoch: string;
  keyword: string;
  unavailable: boolean;
  anchor: HTMLElement;
}
</script>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import { useCore } from '../../services/core';
import type { MenuItem } from '../../services/menu';
import { podcastPublicLink } from '../../services/podcast';
import ActionMenu from '../ActionMenu.vue';
import ModalTransition from '../ModalTransition.vue';
import TrackShareDialog from '../TrackShareDialog.vue';

const props = defineProps<{
  signedIn: boolean;
  canPlay: boolean;
  canRemove: boolean;
  canCollect: (target: HistoryRadioMenuTarget) => boolean;
  live: (target: HistoryRadioMenuTarget) => boolean;
  fallbackFocus: () => HTMLElement | null;
}>();
const emit = defineEmits<{
  choose: [action: 'open' | 'play' | 'next' | 'collect' | 'remove', target: HistoryRadioMenuTarget];
  login: [];
}>();
const core = useCore();
const menu = ref<InstanceType<typeof ActionMenu>>();
const target = shallowRef<HistoryRadioMenuTarget>();
const sharing = ref(false);
const copying = ref(false);
const notice = ref('');
const failed = ref(false);
const current = () => !!target.value && props.live(target.value) && target.value.anchor.isConnected;
const items = computed<MenuItem[]>(() => {
  const row = target.value;
  if (!row || !current()) return [];
  const playable = props.canPlay && props.signedIn && !row.unavailable;
  return [
    { key: 'open', label: '查看播客', icon: 'check' },
    { key: 'play', label: '播放', icon: 'play', disabled: !playable },
    { key: 'next', label: '下一首播放', icon: 'next', disabled: !playable },
    { key: 'line', label: '', separator: true },
    ...(props.canCollect(row) ? [{ key: 'collect', label: '收藏...', icon: 'collect' }] : []),
    { key: 'share', label: '分享...', icon: 'share', disabled: !podcastPublicLink(row.id) },
    { key: 'copy', label: '复制链接', icon: 'copy', disabled: !podcastPublicLink(row.id) },
    { key: 'remove', label: '从列表中删除', disabled: !props.canRemove },
  ];
});
function invalidate() {
  menu.value?.close();
  target.value = undefined;
  sharing.value = false;
  copying.value = false;
  notice.value = '';
  failed.value = false;
}
watch(
  current,
  (live) => {
    if (!live) invalidate();
  },
  { flush: 'sync' },
);
onBeforeUnmount(invalidate);
async function show(row: HistoryRadioMenuTarget, point?: { x: number; y: number }) {
  if (!props.live(row) || !row.anchor.isConnected) return;
  const open = row.anchor.getAttribute('aria-expanded') === 'true';
  invalidate();
  if (open && !point) return;
  target.value = row;
  await nextTick();
  if (target.value !== row || !current()) return;
  if (point) await menu.value?.showAt(point.x, point.y, row.anchor);
  else await menu.value?.showBelow(row.anchor);
}
async function copy() {
  const row = target.value;
  const link = row && podcastPublicLink(row.id);
  if (!row || !link || !current() || copying.value) return;
  copying.value = true;
  notice.value = '';
  failed.value = false;
  try {
    await core.copyText(link);
    if (target.value === row && current()) notice.value = '链接已复制';
  } catch {
    if (target.value === row && current()) {
      notice.value = '复制失败，请重试';
      failed.value = true;
    }
  } finally {
    if (target.value === row) copying.value = false;
  }
}
function select(action: string) {
  const row = target.value;
  if (!row || !current()) return;
  if (action === 'share') {
    if (props.signedIn) sharing.value = true;
    else emit('login');
  } else if (action === 'copy') void copy();
  else if (
    action === 'open' ||
    action === 'play' ||
    action === 'next' ||
    action === 'collect' ||
    action === 'remove'
  )
    emit('choose', action, row);
}
defineExpose({ show });
</script>

<template>
  <ActionMenu
    ref="menu"
    context-only
    label="播放记录操作"
    variant="song"
    :items="items"
    :disabled="!current()"
    @select="select"
  />
  <Teleport to="body">
    <ModalTransition :return-focus="() => target?.anchor ?? null" :fallback-focus="fallbackFocus">
      <TrackShareDialog
        v-if="sharing && target"
        :podcast="target"
        :can-copy="current"
        @close="sharing = false"
      />
    </ModalTransition>
  </Teleport>
  <p
    v-if="notice"
    class="history-outcome history-radio-menu-feedback"
    :role="failed ? 'alert' : 'status'"
  >
    {{ notice }}
    <button v-if="failed" class="secondary" :disabled="copying" @click="copy">重试复制</button>
  </p>
</template>
