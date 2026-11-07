<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue';
import type { DownloadTask } from '../../services/downloads';
import type { TrackView } from '../../services/contracts';
import {
  downloadCommentCountCandidate,
  downloadMenuItems,
  downloadMenuShareTrack,
  downloadShareRestriction,
  type DownloadFileAction,
} from '../../services/downloadMenu';
import { songLink } from '../../services/trackMenu';
import { useCore } from '../../services/core';
import ActionMenu from '../ActionMenu.vue';
import ModalTransition from '../ModalTransition.vue';
import TrackShareDialog from '../TrackShareDialog.vue';
const props = defineProps<{
  epoch: string;
  tasks: DownloadTask[];
  locked: boolean;
  playable: boolean;
  queueable: boolean;
}>();
const emit = defineEmits<{
  play: [task: DownloadTask];
  next: [task: DownloadTask];
  collect: [task: DownloadTask];
  comments: [task: DownloadTask];
  open: [task: DownloadTask];
  file: [task: DownloadTask, action?: DownloadFileAction];
  cancel: [task: DownloadTask];
  control: [task: DownloadTask];
}>();
const menu = ref<InstanceType<typeof ActionMenu>>(),
  id = ref(''),
  shared = shallowRef<{ taskId: string; epoch: string; track: TrackView }>(),
  commentCount = ref<number>();
const task = computed(() => props.tasks.find((t) => t.id === id.value));
const items = computed(() => {
  const { locked, playable, queueable } = props;
  return task.value
    ? downloadMenuItems(task.value, {
        locked,
        playable,
        queueable,
        commentCount: commentCount.value,
      })
    : [];
});
const client = useCore(),
  message = ref(''),
  failed = ref(false),
  copying = ref(false),
  sharing = ref(false),
  retrying = ref(false),
  feedbackAction = ref<'copy' | 'share'>('copy');
let origin: HTMLElement | undefined,
  host: HTMLElement | undefined,
  version = 0,
  preparingComment = false;
let timer: ReturnType<typeof setTimeout> | undefined;
function close() {
  version++;
  preparingComment = false;
  commentCount.value = undefined;
  clearTimeout(timer);
  menu.value?.close();
  shared.value = undefined;
  message.value = '';
  copying.value = false;
  sharing.value = false;
  failed.value = false;
  retrying.value = false;
}
function restoreOrigin() {
  if (origin?.isConnected) {
    origin.closest<HTMLElement>('[data-download-task]')?.focus({ preventScroll: true });
    origin.focus({ preventScroll: true });
  } else host?.focus({ preventScroll: true });
}
function dismiss() {
  close();
  restoreOrigin();
}
watch(task, (value) => {
  if (!value) {
    const owned =
      document.activeElement?.closest('.action-menu')?.getAttribute('aria-label') ===
      '下载资源操作';
    close();
    if (owned) host?.focus({ preventScroll: true });
  }
});
function abandonPreparation() {
  if (!preparingComment) return;
  preparingComment = false;
  version++;
}
function abandonForOtherTarget(event: Event) {
  if (!(event.target instanceof Node) || !origin?.contains(event.target)) abandonPreparation();
}
watch(() => props.epoch, close, { flush: 'sync' });
onMounted(() => {
  // The event that opens the menu reached capture before preparation started.
  // A subsequent gesture belongs to the user, even inside the same row.
  document.addEventListener('pointerdown', abandonPreparation, true);
  document.addEventListener('focusin', abandonForOtherTarget, true);
  document.addEventListener('keydown', abandonPreparation, true);
  window.addEventListener('blur', abandonPreparation);
  window.addEventListener('resize', abandonPreparation);
});
onBeforeUnmount(() => {
  close();
  document.removeEventListener('pointerdown', abandonPreparation, true);
  document.removeEventListener('focusin', abandonForOtherTarget, true);
  document.removeEventListener('keydown', abandonPreparation, true);
  window.removeEventListener('blur', abandonPreparation);
  window.removeEventListener('resize', abandonPreparation);
});
async function show(event: MouseEvent | KeyboardEvent, selected: DownloadTask) {
  if (
    event instanceof KeyboardEvent &&
    !(event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  close();
  id.value = selected.id;
  const row =
    event.currentTarget instanceof Element
      ? event.currentTarget.closest<HTMLElement>('[data-download-task]')
      : null;
  if (!row) return;
  const button = event.target instanceof Element ? event.target.closest('button') : null;
  const rowButton = button && row.contains(button) && !button.disabled ? button : null;
  // Keyboard access can target a row action hidden until the row receives
  // focus. Reveal it first; fall back to the row if it cannot receive focus.
  if (document.activeElement !== rowButton) row.focus({ preventScroll: true });
  rowButton?.focus({ preventScroll: true });
  const anchor = rowButton && document.activeElement === rowButton ? rowButton : row;
  origin = anchor;
  host = row.closest<HTMLElement>('.download-scroll') ?? undefined;
  const ticket = version;
  const epoch = props.epoch;
  const live = () =>
    ticket === version &&
    epoch === props.epoch &&
    anchor.isConnected &&
    task.value?.id === selected.id;
  const r = anchor.getBoundingClientRect();
  const pointer = event instanceof MouseEvent && event.type === 'contextmenu' && event.button === 2;
  if (epoch && !props.locked && downloadCommentCountCandidate(selected)) {
    preparingComment = true;
    try {
      const response = await client.downloadSongCommentCount(epoch, selected.id);
      const current = task.value;
      if (
        !live() ||
        response.epoch !== epoch ||
        !current ||
        !downloadCommentCountCandidate(current)
      )
        return;
      commentCount.value = response.value ?? undefined;
    } catch {
      // Native reports a failed count as null only if the task still belongs
      // to this account and is present. A rejected call cannot open a menu.
      return;
    } finally {
      if (ticket === version) preparingComment = false;
    }
  }
  await nextTick();
  if (!live()) return;
  await menu.value?.showAt(
    pointer ? event.clientX : r.left,
    pointer ? event.clientY : r.bottom,
    anchor,
  );
}
async function copy() {
  const track = task.value && downloadMenuShareTrack(task.value);
  const link = track && songLink(track);
  if (!link || copying.value || sharing.value || props.locked) return;
  feedbackAction.value = 'copy';
  const ticket = version;
  retrying.value = !!document.activeElement?.closest('.track-action-feedback');
  copying.value = true;
  failed.value = false;
  message.value = '正在复制链接…';
  try {
    await client.copyText(link);
    if (ticket === version) {
      message.value = '链接已复制';
      if (retrying.value && document.activeElement?.closest('.track-action-feedback'))
        origin?.focus({ preventScroll: true });
      timer = setTimeout(() => {
        if (!document.activeElement?.closest('.track-action-feedback')) message.value = '';
      }, 3000);
    }
  } catch {
    if (ticket === version) {
      message.value = '复制链接失败，请重试';
      failed.value = true;
    }
  } finally {
    if (ticket === version) {
      copying.value = false;
      retrying.value = false;
    }
  }
}
type ShareTarget = NonNullable<typeof shared.value>;
function sameShareTarget(target: ShareTarget) {
  const current = task.value;
  return (
    !props.locked &&
    props.epoch === target.epoch &&
    current?.id === target.taskId &&
    downloadMenuShareTrack(current)?.id === target.track.id
  );
}
async function share() {
  const current = task.value;
  const track = current && downloadMenuShareTrack(current);
  if (!current || !track || !props.epoch || props.locked || sharing.value || copying.value) return;
  const target = { taskId: current.id, track, epoch: props.epoch };
  const ticket = version;
  feedbackAction.value = 'share';
  sharing.value = true;
  retrying.value = !!document.activeElement?.closest('.track-action-feedback');
  failed.value = false;
  message.value = '正在查询分享权限…';
  try {
    const response = await client.downloadSongShareAccess(target.epoch, target.taskId);
    if (ticket !== version) return;
    if (response.epoch !== target.epoch || !sameShareTarget(target)) {
      message.value = '下载记录已变化，请重新选择歌曲';
      return;
    }
    const restriction = downloadShareRestriction(response.value);
    if (restriction) {
      message.value = restriction;
      return;
    }
    if (retrying.value && document.activeElement?.closest('.track-action-feedback'))
      restoreOrigin();
    message.value = '';
    shared.value = target;
  } catch {
    if (ticket !== version) return;
    failed.value = sameShareTarget(target);
    message.value = failed.value ? '无法确认分享权限，请重试' : '下载记录已变化，请重新选择歌曲';
  } finally {
    if (ticket === version) {
      sharing.value = false;
      retrying.value = false;
    }
  }
}
async function verifySharedCopy() {
  const target = shared.value;
  if (!target || !sameShareTarget(target)) return '下载记录已变化，请重新选择歌曲';
  try {
    const response = await client.downloadSongShareAccess(target.epoch, target.taskId);
    if (shared.value !== target || response.epoch !== target.epoch || !sameShareTarget(target))
      return '下载记录已变化，请重新选择歌曲';
    return downloadShareRestriction(response.value);
  } catch {
    return '无法确认分享权限，请重试';
  }
}
function retryFeedback() {
  if (feedbackAction.value === 'share') void share();
  else void copy();
}
function choose(key: string) {
  const current = task.value;
  if (!current || !items.value.some((i) => i.key === key && !i.disabled)) return;
  if (key === 'play') emit('play', current);
  else if (key === 'next') emit('next', current);
  else if (key === 'collect') emit('collect', current);
  else if (key === 'comments') emit('comments', current);
  else if (key === 'open') emit('open', current);
  else if (key === 'control') emit('control', current);
  else if (key === 'cancel') emit('cancel', current);
  else if (key === 'manage') emit('file', current);
  else if (['rename', 'move', 'delete', 'forget'].includes(key))
    emit('file', current, key as DownloadFileAction);
  else if (key === 'share') void share();
  else if (key === 'copy') void copy();
}
function scroll() {
  abandonPreparation();
  if (
    document.activeElement?.closest('.action-menu')?.getAttribute('aria-label') === '下载资源操作'
  ) {
    const row = origin?.closest<HTMLElement>('[data-download-task]');
    const container = host;
    const r = row?.getBoundingClientRect(),
      b = container?.getBoundingClientRect();
    (r && b && r.top >= b.top && r.bottom <= b.bottom ? origin : container)?.focus({
      preventScroll: true,
    });
  }
  menu.value?.close();
}
defineExpose({ show, scroll, close });
</script>
<template>
  <ActionMenu
    ref="menu"
    context-only
    variant="song"
    label="下载资源操作"
    :items="items"
    @select="choose"
  />
  <ModalTransition>
    <TrackShareDialog
      v-if="shared"
      :track="shared.track"
      :before-copy="verifySharedCopy"
      @close="shared = undefined"
    />
  </ModalTransition>
  <div v-if="message" class="track-action-feedback" :class="{ failed }" role="status">
    {{ message }}
    <button
      v-if="failed || retrying"
      class="text-button"
      :aria-disabled="copying || sharing"
      @click="retryFeedback"
    >
      {{ copying || sharing ? '正在重试…' : '重试' }}
    </button>
    <button class="text-button" @click="dismiss">关闭</button>
  </div>
</template>
