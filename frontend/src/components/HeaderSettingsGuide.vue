<script setup lang="ts">
import { nextTick, onUnmounted, ref, watch } from 'vue';
import { useMotion } from '../composables/useMotion';
import type { HeaderGuideKind } from '../composables/useHeaderSettingsGuide';
import Icon from './Icon.vue';

const props = defineProps<{ kind?: HeaderGuideKind; disabled: boolean; settingsOpen: boolean }>();
const emit = defineEmits<{ settings: []; configure: []; dismiss: [] }>();
const host = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const bubble = ref<HTMLElement>();
const motion = useMotion();

function place() {
  if (!props.kind || !host.value || !trigger.value || !bubble.value) return;
  const anchor = trigger.value.getBoundingClientRect();
  const width = bubble.value.offsetWidth;
  const center = anchor.left + anchor.width / 2;
  const left = Math.max(8, Math.min(center - width / 2, innerWidth - 8 - width));
  bubble.value.style.setProperty(
    '--guide-left',
    `${left - host.value.getBoundingClientRect().left}px`,
  );
  bubble.value.style.setProperty('--guide-arrow', `${center - left}px`);
}

watch(
  () => props.kind,
  async (kind, _, onCleanup) => {
    if (!kind) return;
    let current = true;
    let resize: ResizeObserver | undefined;
    onCleanup(() => {
      current = false;
      resize?.disconnect();
      window.removeEventListener('resize', place);
    });
    await nextTick();
    if (!current || !bubble.value) return;
    place();
    resize = new ResizeObserver(place);
    resize.observe(bubble.value);
    window.addEventListener('resize', place);
  },
);

function dismiss() {
  emit('dismiss');
  trigger.value?.focus({ preventScroll: true });
}
function leave(element: Element) {
  (element as HTMLElement).inert = true;
}
function enter(element: Element) {
  (element as HTMLElement).inert = false;
}
onUnmounted(() => window.removeEventListener('resize', place));
</script>

<template>
  <div ref="host" class="header-playback-guide-host">
    <button
      ref="trigger"
      type="button"
      class="icon-button settings-button"
      :disabled="disabled"
      aria-label="打开设置"
      title="设置"
      :aria-pressed="settingsOpen"
      @click="emit('settings')"
    >
      <Icon name="settings" />
    </button>
    <Transition
      name="header-playback-guide"
      :css="!motion.reduced.value"
      @before-enter="enter"
      @before-leave="leave"
    >
      <aside
        v-if="kind"
        ref="bubble"
        class="header-playback-guide"
        :data-guide="kind"
        :aria-label="kind === 'system_media' ? '系统媒体控制提示' : '播放设置提示'"
        @keydown.escape.stop.prevent="dismiss"
      >
        <span class="header-playback-guide-text">{{
          kind === 'system_media'
            ? '已支持SMTC，将播放信息同步到系统菜单栏'
            : '功能上新，音乐播放中保持亮屏不休眠'
        }}</span>
        <button type="button" class="header-playback-guide-action" @click="emit('configure')">
          去设置
        </button>
        <button
          type="button"
          class="header-playback-guide-close"
          aria-label="关闭播放设置提示"
          @click="dismiss"
        >
          <Icon name="close" />
        </button>
      </aside>
    </Transition>
  </div>
</template>

<style scoped>
.header-playback-guide-host {
  position: relative;
  display: flex;
  flex-shrink: 0;
}
.header-playback-guide {
  position: absolute;
  z-index: 999;
  top: calc(100% + 8px);
  left: var(--guide-left, 0px);
  transform-origin: var(--guide-arrow, 18px) 0;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 6px;
  width: max-content;
  max-width: calc(100vw - 16px);
  padding: 10px 14px;
  background: var(--surface-raised);
  border-radius: 10px;
  box-shadow:
    0 1px 6px #0002,
    0 1px 2px #0002;
}
.header-playback-guide::before {
  position: absolute;
  top: -4px;
  left: calc(var(--guide-arrow, 18px) - 4px);
  width: 8px;
  height: 8px;
  transform: rotate(45deg);
  background: inherit;
  content: '';
}
.header-playback-guide-text {
  color: var(--text);
  font-size: 13px;
  font-weight: 400;
  line-height: 20px;
  overflow-wrap: anywhere;
}
.header-playback-guide-action,
.header-playback-guide-close {
  flex-shrink: 0;
  height: 20px;
  padding: 0;
  border: 0;
}
.header-playback-guide-action {
  width: 70px;
  border-radius: 13px;
  background: var(--accent-fill);
  color: var(--on-accent);
  font-size: 12px;
}
.header-playback-guide-action:hover {
  opacity: 0.9;
}
.header-playback-guide-close {
  display: grid;
  place-items: center;
  width: 20px;
  color: var(--muted);
}
.header-playback-guide-close:hover {
  color: var(--text);
}
.header-playback-guide-close .icon {
  width: 12px;
  height: 12px;
}
.header-playback-guide-enter-active,
.header-playback-guide-leave-active {
  transition:
    transform 300ms cubic-bezier(0.45, 0.6, 0.58, 0.97),
    opacity 300ms cubic-bezier(0.45, 0.6, 0.58, 0.97);
}
.header-playback-guide-leave-active {
  transition-duration: 200ms;
  pointer-events: none;
}
.header-playback-guide-enter-from,
.header-playback-guide-leave-to {
  transform: scale(0.8);
  opacity: 0;
}
</style>
