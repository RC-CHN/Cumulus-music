<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { qualities, type Quality } from '../services/player';
import type { MenuItem } from '../services/menu';
import ActionMenu from './ActionMenu.vue';

const props = defineProps<{ player: PlayerModel }>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const view = computed(() => props.player.view.value);
const damaged = computed(
  () => !!view.value?.qualityPreferences.damaged && view.value.qualityPreferences.appliesToCurrent,
);
const label = (value?: string | null) =>
  qualities.find((q) => q.value === value)?.label ?? (value ? '未知音质' : '未播放');
const actualLabel = computed(() =>
  view.value?.actualQuality
    ? label(view.value.actualQuality)
    : props.player.loaded.value
      ? '未知音质'
      : '未播放',
);
const order: Quality[] = [
  'vivid',
  'sky',
  'jymaster',
  'jyeffect',
  'hires',
  'lossless',
  'exhigh',
  'standard',
  'higher',
  'dolby',
  '64aac',
];
const descriptions: Record<Quality, string> = {
  vivid: 'Audio Vivid · 最高 7.1 声道',
  sky: '最高 5.1 声道',
  jymaster: '最高 192 kHz / 24 bit',
  jyeffect: '最高 96 kHz / 24 bit',
  hires: '最高 192 kHz / 24 bit',
  lossless: '最高 48 kHz / 16 bit',
  exhigh: '320 kbps',
  standard: '128 kbps',
  higher: '192 kbps · 兼容音质',
  dolby: '杜比音源 · 兼容音质',
  '64aac': '64 kbps · 兼容音质',
};
let context: { epoch: string; playId: string; revision: string } | undefined;
function capture() {
  const current = view.value;
  context =
    current?.playId &&
    (current.qualityPreferences.revision || !current.qualityPreferences.appliesToCurrent)
      ? {
          epoch: current.epoch,
          playId: current.playId,
          revision: current.qualityPreferences.revision ?? 'podcast',
        }
      : undefined;
}
const items = computed<MenuItem[]>(() => {
  if (damaged.value)
    return [{ key: 'repair', label: '重置音质偏好', disabled: props.player.busy.value }];
  return order.flatMap((value, index) => [
    ...(index === 8 ? [{ key: 'compatibility', label: '', separator: true }] : []),
    {
      key: value,
      label: label(value),
      checked: view.value?.actualQuality === value,
      disabled:
        !view.value?.canChangeQuality || !props.player.loaded.value || props.player.busy.value,
    },
  ]);
});
function choose(key: string) {
  if (props.player.busy.value) return;
  if (key === 'repair') {
    void props.player.action({ kind: 'repair_quality' });
    return;
  }
  const captured = context;
  if (!captured || captured.epoch !== view.value?.epoch || !order.includes(key as Quality)) return;
  void props.player.action({
    kind: 'quality_for',
    level: key as Quality,
    play_id: captured.playId,
    expected_revision: captured.revision,
  });
}
watch(
  [
    () => view.value?.epoch,
    () => view.value?.playId,
    () => view.value?.qualityPreferences.revision,
  ],
  () => menu.value?.close(),
);
</script>
<template>
  <span v-if="!view?.track?.local" class="quality-control">
    <ActionMenu
      ref="menu"
      label="音质"
      :trigger-text="actualLabel"
      :disabled="!player.ready.value || (!view?.canChangeQuality && !damaged)"
      :items="items"
      variant="quality"
      focus-checked
      @opened="capture"
      @select="choose"
    >
      <template #header>
        <div class="quality-menu-heading">
          <strong>歌曲音质</strong>
          <p v-if="damaged" class="quality-warning">音质偏好无法读取，重置后可重新选择。</p>
          <template v-else>
            <p>
              正在播放：{{ actualLabel
              }}<span v-if="view?.requestedQuality !== view?.actualQuality">
                · 请求：{{ label(view?.requestedQuality) }}</span
              >
            </p>
            <p v-if="view?.qualityPreferences.appliesToCurrent">
              后续播放偏好：{{ label(view?.qualityPreferences.quality) }}
            </p>
            <p v-else>当前为播客音质，歌曲偏好不受影响</p>
          </template>
        </div>
      </template>
      <template #item="{ item }">
        <span class="quality-menu-option"
          ><span>{{ item.label }}</span>
          <small v-if="item.key !== 'repair'">{{ descriptions[item.key as Quality] }}</small>
        </span>
      </template>
      <template #footer><p class="quality-menu-note">可用音质以歌曲和账号权益为准</p></template>
    </ActionMenu>
  </span>
</template>
