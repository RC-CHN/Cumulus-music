<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts';
import { useCore } from '../services/core';
import type { DailyNotification, DailyNotificationAction } from '../services/dailyNotification';
import ActionMenu from './ActionMenu.vue';
import Icon from './Icon.vue';
import MenuTransition from './MenuTransition.vue';

const props = defineProps<{ epoch: string }>();
const emit = defineEmits<{
  settings: [epoch: string];
  details: [epoch: string];
  played: [];
  issue: [message: string];
}>();
const client = useCore();
const notice = shallowRef<DailyNotification>();
const card = ref<HTMLElement>();
const settled = ref(false);
let mounted = false;
let unlisten: (() => void) | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;
let generation = 0;
let focusOrigin: HTMLElement | undefined;
const retired = new Set<string>();
const items = [
  { key: 'snooze', label: '7天内不再推送' },
  { key: 'disable', label: '关闭桌面通知' },
  { key: 'separator', label: '', separator: true },
  { key: 'settings', label: '管理桌面通知' },
];

function hide() {
  clearTimeout(timer);
  notice.value = undefined;
  settled.value = false;
}

async function accept(view: DailyNotification | null) {
  if (!mounted) return;
  if (!view) {
    hide();
    return;
  }
  if (
    view.epoch !== props.epoch ||
    retired.has(view.receipt) ||
    notice.value?.receipt === view.receipt ||
    view.remainingMs <= 0
  )
    return;
  hide();
  const origin = document.activeElement;
  focusOrigin = origin instanceof HTMLElement ? origin : undefined;
  notice.value = view;
  timer = setTimeout(() => void run('close'), view.remainingMs);
  await nextTick();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  if (notice.value !== view || props.epoch !== view.epoch || !card.value?.getClientRects().length)
    return;
  try {
    if (
      !view.shown &&
      (await client.dailyNotificationCommand(view.epoch, view.receipt, 'shown')) !== 'shown'
    )
      return;
    if (notice.value === view && props.epoch === view.epoch) settled.value = true;
  } catch {
    if (notice.value === view) hide();
  }
}

async function reload() {
  const ticket = ++generation;
  const owner = props.epoch;
  try {
    const view = await client.dailyNotification(owner);
    if (ticket === generation && props.epoch === owner) await accept(view);
  } catch {
    // An optional native prompt never surfaces a service error at startup.
  }
}

async function run(action: Exclude<DailyNotificationAction, 'shown'>) {
  const view = notice.value;
  if (!view || (action !== 'close' && !settled.value)) return;
  const restore = card.value?.contains(document.activeElement);
  retired.add(view.receipt);
  hide();
  if (restore && focusOrigin?.isConnected) focusOrigin.focus({ preventScroll: true });
  try {
    const result = await client.dailyNotificationCommand(view.epoch, view.receipt, action);
    if (!mounted || view.epoch !== props.epoch) return;
    if (result === 'settings') emit('settings', view.epoch);
    else if (result === 'details') emit('details', view.epoch);
    else if (result === 'played') emit('played');
  } catch (error) {
    if (mounted && props.epoch === view.epoch && action !== 'close')
      emit('issue', failure(error).message);
  }
}

function more(key: string) {
  if (key === 'snooze' || key === 'disable' || key === 'settings') void run(key);
}

watch(
  () => props.epoch,
  () => {
    hide();
    retired.clear();
    if (unlisten) void reload();
  },
  { flush: 'sync' },
);
onMounted(async () => {
  mounted = true;
  try {
    const stop = await client.onDailyNotification((view) => {
      generation++;
      void accept(view);
    });
    if (!mounted) {
      stop();
      return;
    }
    unlisten = stop;
    await reload();
  } catch {
    // There is no remote or JS fallback for a missing native host.
  }
});
onUnmounted(() => {
  mounted = false;
  generation++;
  unlisten?.();
  hide();
});
</script>

<template>
  <MenuTransition>
    <section
      v-if="notice"
      ref="card"
      class="daily-notification"
      data-daily-notification
      :aria-busy="!settled"
      aria-label="每日推荐桌面通知"
      @keydown.esc.stop.prevent="run('close')"
    >
      <header>
        <span class="daily-notification-date" aria-hidden="true">{{ notice.day }}</span>
        <div role="status">
          <h2>每日推荐</h2>
          <p v-if="notice.description">{{ notice.description }}</p>
        </div>
        <ActionMenu label="更多通知操作" :items="items" :disabled="!settled" @select="more" />
        <button class="icon-button" aria-label="关闭每日推荐通知" @click="run('close')">
          <Icon name="close" />
        </button>
      </header>
      <ol>
        <li v-for="(song, index) in notice.songs" :key="index">
          <img
            v-if="song.coverUrl"
            :src="song.coverUrl"
            alt=""
            @error="($event.target as HTMLImageElement).hidden = true"
          />
          <span>{{ song.name }}</span>
          <small>{{ song.artists }}</small>
        </li>
      </ol>
      <footer>
        <button class="secondary" :disabled="!settled" @click="run('details')">查看详情</button>
        <button class="primary" :disabled="!settled" @click="run('play')">一键播放</button>
      </footer>
    </section>
  </MenuTransition>
</template>

<style scoped>
.daily-notification {
  position: fixed;
  inset-inline-end: 24px;
  inset-block-end: 100px;
  z-index: 12;
  inline-size: min(360px, calc(100vw - 48px));
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--surface-raised);
  box-shadow: 0 8px 26px #0003;
}
header,
footer,
li {
  display: flex;
  align-items: center;
  gap: 8px;
}
header > div:first-of-type,
li > span {
  flex: 1;
  min-width: 0;
}
header h2 {
  margin: 0;
  font-size: 18px;
}
header p,
li small {
  color: var(--text-secondary);
}
header p {
  margin: 2px 0 0;
  font-size: 12px;
}
.daily-notification-date {
  color: var(--accent);
  font-size: 26px;
  font-weight: 700;
}
ol {
  list-style: none;
  margin: 12px 0;
  padding: 0;
}
li {
  min-height: 28px;
}
li img {
  width: 24px;
  height: 24px;
  object-fit: cover;
  border-radius: 4px;
}
li > span,
li small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
li small {
  max-inline-size: 34%;
}
footer {
  justify-content: flex-end;
}
</style>
