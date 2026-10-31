<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { usePlaybackQuality } from '../composables/usePlaybackQuality';
import { useCore } from '../services/core';
import type { Quality } from '../services/player';
import type { MenuItem } from '../services/menu';
import {
  qualityOrder,
  qualityDescriptions,
  qualityLabel,
  qualityBadge,
  qualitySize,
  qualityUnavailable,
  qualityDuration,
} from '../services/playbackQuality';
import ActionMenu from './ActionMenu.vue';
import QualityTrialDialog from './QualityTrialDialog.vue';
import ModalTransition from './ModalTransition.vue';
const props = defineProps<{ player: PlayerModel }>();
const emit = defineEmits<{ member: [] }>();
const model = usePlaybackQuality(props.player, useCore());
const now = ref(Date.now());
let clock: ReturnType<typeof setInterval> | undefined;
watch(model.opened, (open) => {
  if (clock !== undefined) clearInterval(clock);
  clock = undefined;
  now.value = Date.now();
  if (open) clock = setInterval(() => (now.value = Date.now()), 1000);
});
onUnmounted(() => {
  if (clock !== undefined) clearInterval(clock);
});
const menu = ref<InstanceType<typeof ActionMenu>>();
const view = computed(() => props.player.view.value);
const damaged = computed(
  () => !!view.value?.qualityPreferences.damaged && view.value.qualityPreferences.appliesToCurrent,
);
const actualLabel = computed(() =>
  view.value?.actualQuality
    ? qualityLabel(view.value.actualQuality)
    : props.player.loaded.value
      ? '未知音质'
      : '未播放',
);
const choice = (key: string) => model.details.value?.choices.find((c) => c.quality === key);
const trial = (key: string) => model.details.value?.trials.find((t) => t.right === key);
function remaining(key: string) {
  const info = choice(key);
  if (info?.trialPhase !== 'active' || info.remainingMs == null) return '';
  const ms = Math.max(0, info.remainingMs - Math.max(0, now.value - model.receivedAt.value));
  return ms > 0 ? `剩余 ${qualityDuration(ms)}` : '正在核对试用权益';
}
const pending = (key: string) =>
  !!trial(key) && ['sending', 'uncertain', 'acknowledged'].includes(trial(key)!.phase);
const items = computed<MenuItem[]>(() => {
  if (damaged.value)
    return [{ key: 'repair', label: '重置音质偏好', disabled: props.player.busy.value }];
  const main: MenuItem[] = [],
    compatibility: MenuItem[] = [];
  qualityOrder.forEach((value, index) => {
    const detail = choice(value),
      recover = pending(value);
    if (detail?.availability === 'unavailable' && view.value?.actualQuality !== value && !recover)
      return;
    const item = {
      key: value,
      label: qualityLabel(value),
      checked: view.value?.actualQuality === value,
      disabled:
        !view.value?.canChangeQuality ||
        !props.player.loaded.value ||
        props.player.busy.value ||
        (!recover && qualityUnavailable(detail)),
      hint: detail?.access === 'trial_unsupported' ? '当前试听歌曲不支持此音质' : undefined,
    };
    (index >= 8 ? compatibility : main).push(item);
  });
  return [
    ...main,
    ...(compatibility.length
      ? [{ key: 'compatibility', label: '', separator: true }, ...compatibility]
      : []),
  ];
});
const note = computed(() =>
  model.loading.value
    ? '正在读取歌曲音质与账号权益…'
    : model.readError.value
      ? '音质信息暂未读取成功'
      : model.details.value?.trialsDamaged
        ? '试用记录无法读取，其他音质仍可选择'
        : model.partial.value
          ? '部分音质信息暂未确认'
          : view.value?.qualityPreferences.appliesToCurrent
            ? '可用音质以当前歌曲和账号权益为准'
            : '播客音质由节目授权决定',
);
function choose(key: string) {
  if (key === 'repair') {
    if (!props.player.busy.value) void props.player.action({ kind: 'repair_quality' });
    return;
  }
  if (qualityOrder.includes(key as Quality)) model.choose(key as Quality);
}
let afterDialog: { action: 'refresh' | 'member'; epoch: string | undefined } | undefined;
function leave(action?: 'refresh' | 'member') {
  afterDialog = action ? { action, epoch: view.value?.epoch } : undefined;
  model.closePrompt();
}
function dialogLeft() {
  const action = afterDialog;
  afterDialog = undefined;
  if (!action || action.epoch !== view.value?.epoch || !props.player.ready.value) return;
  if (action.action === 'member') emit('member');
  else void menu.value?.show();
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
      @opened="model.show"
      @closed="model.hide"
      @select="choose"
    >
      <template #header
        ><div class="quality-menu-heading">
          <strong>歌曲音质</strong>
          <p v-if="damaged" class="quality-warning">音质偏好无法读取，重置后可重新选择。</p>
          <template v-else
            ><p>
              正在播放：{{ actualLabel
              }}<span v-if="view?.requestedQuality !== view?.actualQuality">
                · 请求：{{ qualityLabel(view?.requestedQuality) }}</span
              >
            </p>
            <p v-if="view?.qualityPreferences.appliesToCurrent">
              后续播放偏好：{{ qualityLabel(view.qualityPreferences.quality) }}
            </p>
            <p v-else>当前为播客音质，歌曲偏好不受影响</p></template
          >
        </div></template
      >
      <template #item="{ item }">
        <span class="quality-menu-option"
          ><span>{{ item.label }}</span
          ><small v-if="item.key !== 'repair'"
            >{{ qualitySize(choice(item.key)?.sizeBytes)
            }}{{ qualitySize(choice(item.key)?.sizeBytes) ? ' · ' : ''
            }}{{ qualityDescriptions[item.key as Quality] }}</small
          ><small v-if="remaining(item.key)" class="quality-trial-remaining">{{
            remaining(item.key)
          }}</small></span
        >
        <span
          v-if="qualityBadge(choice(item.key), trial(item.key))"
          class="quality-menu-access"
          :class="{
            'quality-trial-badge':
              pending(item.key) ||
              ['trial_eligible', 'trial_active'].includes(choice(item.key)?.access ?? ''),
          }"
          >{{ qualityBadge(choice(item.key), trial(item.key)) }}</span
        >
      </template>
      <template #footer
        ><div class="quality-menu-note" :aria-busy="model.loading.value">
          <span role="status">{{ note }}</span
          ><button
            v-if="view?.qualityPreferences.appliesToCurrent && !damaged"
            class="text-button"
            role="menuitem"
            :aria-disabled="model.loading.value"
            @click="!model.loading.value && model.refresh()"
          >
            刷新音质
          </button>
        </div></template
      >
    </ActionMenu>
  </span>
  <ModalTransition @left="dialogLeft">
    <QualityTrialDialog
      v-if="model.prompt.value"
      :target="model.prompt.value"
      :model="model"
      @close="leave()"
      @refresh="leave('refresh')"
      @member="leave('member')"
    />
  </ModalTransition>
</template>
