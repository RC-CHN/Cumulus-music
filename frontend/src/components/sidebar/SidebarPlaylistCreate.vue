<script lang="ts">
const hintKey = 'cumulus.sidebar.external-import-hint-seen.v1';
let shownInProcess = false;
function hintWasShown() {
  try {
    return shownInProcess || localStorage.getItem(hintKey) === '1';
  } catch {
    return shownInProcess;
  }
}
function rememberHint() {
  shownInProcess = true;
  try {
    localStorage.setItem(hintKey, '1');
  } catch {
    // A disabled preference store still suppresses repeat prompts this run.
  }
}
</script>

<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';
import type { MenuItem } from '../../services/menu';
import { modalDepth } from '../../services/modal';

const props = defineProps<{ importEnabled: boolean; signedIn: boolean }>();
const emit = defineEmits<{ create: []; importPlaylist: [trigger: HTMLElement] }>();
const host = ref<HTMLElement>();
const visible = ref(false);
const hint = ref(false);
const hintAbove = ref(false);
const menuOpen = ref(false);
let observer: IntersectionObserver | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;
const items: MenuItem[] = [
  { key: 'create', label: '创建歌单', icon: 'plus' },
  { key: 'import', label: '歌单导入', icon: 'download' },
];

function stop() {
  clearTimeout(timer);
  timer = undefined;
  hint.value = false;
}
function schedule() {
  stop();
  if (
    !props.importEnabled ||
    !props.signedIn ||
    !visible.value ||
    document.hidden ||
    modalDepth.value ||
    menuOpen.value ||
    hintWasShown()
  )
    return;
  timer = setTimeout(() => {
    if (hintWasShown() || document.hidden) return;
    const anchor = host.value;
    const viewport = anchor?.closest('.sidebar-scroll');
    if (!anchor?.isConnected || !viewport) return;
    const anchorRect = anchor.getBoundingClientRect();
    const viewportRect = viewport.getBoundingClientRect();
    hintAbove.value = viewportRect.bottom - anchorRect.bottom < 40;
    hint.value = true;
    rememberHint();
    timer = setTimeout(stop, 5000);
  }, 2000);
}
function outside(event: PointerEvent) {
  if (hint.value && !host.value?.contains(event.target as Node)) stop();
}
function select(key: string) {
  if (key === 'create') emit('create');
  else if (key === 'import' && props.importEnabled) {
    const trigger = host.value?.querySelector<HTMLElement>('.menu-trigger');
    if (trigger) emit('importPlaylist', trigger);
  }
}
watch([() => props.importEnabled, () => props.signedIn, visible, menuOpen, modalDepth], schedule);
watch(
  () => props.importEnabled,
  async () => {
    if (!menuOpen.value && !host.value?.contains(document.activeElement)) return;
    await nextTick();
    if (!modalDepth.value) host.value?.querySelector('button')?.focus({ preventScroll: true });
  },
);
onMounted(() => {
  if (typeof IntersectionObserver !== 'undefined' && host.value) {
    observer = new IntersectionObserver(
      ([entry]) => (visible.value = !!entry?.isIntersecting && entry.intersectionRatio >= 0.5),
      { threshold: [0, 0.5] },
    );
    observer.observe(host.value);
  } else visible.value = true;
  document.addEventListener('pointerdown', outside, true);
  document.addEventListener('visibilitychange', schedule);
});
onUnmounted(() => {
  observer?.disconnect();
  document.removeEventListener('pointerdown', outside, true);
  document.removeEventListener('visibilitychange', schedule);
  stop();
});
</script>

<template>
  <span ref="host" class="sidebar-playlist-create">
    <ActionMenu
      v-if="importEnabled"
      label="新建歌单"
      icon="plus"
      class="sidebar-create-menu"
      :items="items"
      @opened="menuOpen = true"
      @closed="menuOpen = false"
      @select="select"
    />
    <button
      v-else
      type="button"
      class="sidebar-create"
      aria-label="新建歌单"
      data-tooltip="新建歌单"
      @click="emit('create')"
    >
      <Icon name="plus" />
    </button>
    <span v-if="hint" class="sidebar-import-hint" :class="{ above: hintAbove }" role="status">
      支持导入歌单啦
    </span>
  </span>
</template>
