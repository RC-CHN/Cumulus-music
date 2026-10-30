<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { DownloadTask } from '../../services/downloads';
import { downloadMenuItems, type DownloadFileAction } from '../../services/downloadMenu';
import { songLink } from '../../services/trackMenu';
import { useCore } from '../../services/core';
import ActionMenu from '../ActionMenu.vue';
import ModalTransition from '../ModalTransition.vue';
import TrackShareDialog from '../TrackShareDialog.vue';
const props = defineProps<{
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
  shared = shallowRef<DownloadTask>();
const task = computed(() => props.tasks.find((t) => t.id === id.value));
const items = computed(() => (task.value ? downloadMenuItems(task.value, props) : []));
const client = useCore(),
  message = ref(''),
  failed = ref(false),
  copying = ref(false),
  retrying = ref(false);
let origin: HTMLElement | undefined,
  host: HTMLElement | undefined,
  version = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
function close() {
  version++;
  clearTimeout(timer);
  menu.value?.close();
  shared.value = undefined;
  message.value = '';
  copying.value = false;
  failed.value = false;
  retrying.value = false;
}
function dismiss() {
  close();
  if (origin?.isConnected) {
    origin.closest<HTMLElement>('[data-download-task]')?.focus({ preventScroll: true });
    origin.focus({ preventScroll: true });
  } else host?.focus({ preventScroll: true });
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
onBeforeUnmount(close);
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
  const row = (event.currentTarget as HTMLElement).closest<HTMLElement>('[data-download-task]');
  origin = (event.target as HTMLElement).closest<HTMLElement>('button') ?? row ?? undefined;
  host = row?.closest<HTMLElement>('.download-scroll') ?? undefined;
  if (!origin) return;
  origin.focus({ preventScroll: true });
  const r = origin.getBoundingClientRect();
  const pointer = event instanceof MouseEvent && event.type === 'contextmenu' && event.button === 2;
  await nextTick();
  if (!origin.isConnected) return;
  await menu.value?.showAt(
    pointer ? event.clientX : r.left,
    pointer ? event.clientY : r.bottom,
    origin,
  );
}
async function copy() {
  const link = task.value && songLink(task.value.track);
  if (!link || copying.value) return;
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
  else if (key === 'share') shared.value = current;
  else if (key === 'copy') void copy();
}
function scroll() {
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
  <ModalTransition
    ><TrackShareDialog v-if="shared" :track="shared.track" @close="shared = undefined"
  /></ModalTransition>
  <div v-if="message" class="track-action-feedback" :class="{ failed }" role="status">
    {{ message }}
    <button v-if="failed || retrying" class="text-button" :aria-disabled="copying" @click="copy">
      {{ copying ? '正在复制…' : '重试' }}
    </button>
    <button class="text-button" @click="dismiss">关闭</button>
  </div>
</template>
