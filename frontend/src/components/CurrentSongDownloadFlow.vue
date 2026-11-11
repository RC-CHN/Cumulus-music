<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts';
import {
  currentSongDownloadPicker,
  currentSongDownloadDecisionNotice,
  type CurrentSongDownloadPresentation,
  type DownloadPickerRequest,
} from '../services/currentSongDownload';
import type { DownloadOptions } from '../services/downloads';
import { downloadAdmissionSummary } from '../services/downloadFeedback';
import type { SearchPurchasedQuality } from '../services/search';
import ModalTransition from './ModalTransition.vue';
import SearchGroupDownloadPicker from './SearchGroupDownloadPicker.vue';
import VoiceSongPurchasedQuality from './VoiceSongPurchasedQuality.vue';

const props = defineProps<{
  target?: CurrentSongDownloadPresentation;
  options: (quality: SearchPurchasedQuality) => DownloadOptions;
}>();
const emit = defineEmits<{
  close: [target: CurrentSongDownloadPresentation];
  notice: [
    target: CurrentSongDownloadPresentation,
    message: string,
    action?: 'settings' | 'manage',
  ];
  refresh: [];
  expired: [];
}>();
const presented = shallowRef<CurrentSongDownloadPresentation>();
const picker = shallowRef<{ selection: string; request: DownloadPickerRequest }>();
const busy = ref(false);
const visible = computed(() => !!props.target?.scope.live());
let cancelSelection: string | undefined;
let submitted = false;
watch(
  () => props.target,
  (value, previous) => {
    if (previous && value !== previous && !submitted)
      previous.scope.cancel(cancelSelection ?? previous.offer);
    if (value) presented.value = value;
    cancelSelection = undefined;
    submitted = false;
    busy.value = false;
    picker.value = undefined;
  },
  { flush: 'sync' },
);

onBeforeUnmount(() => {
  if (props.target && !submitted) props.target.scope.cancel(cancelSelection ?? props.target.offer);
});

const current = (target: CurrentSongDownloadPresentation) =>
  props.target === target && target.scope.live();
const restoreFocus = computed(() => presented.value?.scope.live() === true);
const focus = () => presented.value?.opener ?? null;
function fallbackFocus() {
  return restoreFocus.value
    ? document.querySelector<HTMLElement>('.play-toggle:not(:disabled)')
    : null;
}
async function openGuide() {
  const target = props.target;
  if (target && current(target) && !picker.value && !busy.value)
    await target.scope.openGuide(target.offer);
}
function close() {
  const target = props.target;
  if (target) emit('close', target);
}
function report(
  target: CurrentSongDownloadPresentation,
  message: string,
  action?: 'settings' | 'manage',
) {
  if (!current(target)) return;
  emit('notice', target, message, action);
  close();
}
function directory(target: CurrentSongDownloadPresentation) {
  report(target, '请先设置下载目录，再重新选择下载。', 'settings');
}
function unknown(target: CurrentSongDownloadPresentation) {
  emit('refresh');
  report(target, '无法确认下载任务是否已受理，请到下载管理核对。', 'manage');
}
function accepted(count: number, addFailed: boolean) {
  const target = props.target;
  if (!target || !current(target)) return;
  submitted = true;
  emit('refresh');
  report(
    target,
    (addFailed ? '未能添加到歌单。' : '') +
      (count ? `已添加 ${count} 首到下载列表` : '未添加新任务，请到下载管理核对。'),
    'manage',
  );
}
function pickerUnknown() {
  const target = props.target;
  if (!target || !current(target)) return;
  submitted = true;
  unknown(target);
}
async function confirm(quality: SearchPurchasedQuality) {
  const target = props.target;
  if (!target || !current(target) || busy.value || !target.choices.includes(quality)) return;
  busy.value = true;
  try {
    const reply = await target.scope.call({ kind: 'select', offer: target.offer, quality });
    if (!current(target)) return;
    if (reply?.kind !== 'selection' || reply.quality !== quality) {
      report(target, '未能核实当前歌曲的下载选项，请重新打开。');
      return;
    }
    const { selection, decision } = reply;
    if (!selection || (decision !== 'ready' && decision !== 'playlist_required')) {
      emit('refresh');
      report(target, currentSongDownloadDecisionNotice(decision));
      return;
    }
    cancelSelection = selection;
    const options = props.options(quality);
    if (decision === 'playlist_required') {
      picker.value = {
        selection,
        request: currentSongDownloadPicker(target.scope, selection, options),
      };
      return;
    }
    submitted = true;
    const outcome = await target.scope.call({
      kind: 'start',
      selection,
      continuation: null,
      options,
    });
    if (!current(target)) return;
    if (outcome?.kind === 'directory_required') {
      submitted = false;
      directory(target);
    } else if (outcome?.kind === 'started' && outcome.result.epoch === target.scope.owner.epoch) {
      const result = outcome.result;
      emit('refresh');
      report(
        target,
        downloadAdmissionSummary(result) ??
          (result.accepted.length
            ? `已添加 ${result.accepted.length} 首到下载列表`
            : result.failed[0]?.message || '未添加新任务，请到下载管理核对。'),
        'manage',
      );
    } else unknown(target);
  } catch (error) {
    if (!current(target)) return;
    const issue = failure(error);
    if (['session_expired', 'profile_changed'].includes(issue.code)) {
      close();
      emit('expired');
    } else if (submitted) unknown(target);
    else report(target, issue.message);
  } finally {
    if (current(target)) busy.value = false;
  }
}
</script>

<template>
  <ModalTransition
    :return-focus="focus"
    :fallback-focus="fallbackFocus"
    :restore-focus="restoreFocus"
  >
    <SearchGroupDownloadPicker
      v-if="target && visible && picker"
      :key="picker.selection"
      :epoch="target.scope.owner.epoch"
      :receipt="picker.selection"
      :request="picker.request"
      :live="target.scope.live"
      parent-owns-cancellation
      @close="close"
      @accepted="accepted"
      @directory="target && directory(target)"
      @unknown="pickerUnknown"
    />
    <VoiceSongPurchasedQuality
      v-else-if="target && visible"
      :key="target.offer"
      :choices="target.choices"
      :open-guide="openGuide"
      :busy="busy"
      @close="close"
      @confirm="confirm"
    />
  </ModalTransition>
</template>
