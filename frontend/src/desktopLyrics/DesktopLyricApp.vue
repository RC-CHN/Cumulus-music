<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef } from 'vue';
import { createDesktopLyricClient } from '../adapters/desktopLyrics';
import type { DesktopLyricAction, DesktopLyricView } from '../services/desktopLyrics';
import { failure } from '../services/contracts';
import Icon from '../components/Icon.vue';
import LyricRow from './LyricRow.vue';
import './desktopLyrics.css';
import { fontStack } from '../services/fonts';
import { useLocalShortcuts } from '../composables/useLocalShortcuts';
import { createWindowShortcutClient } from '../adapters/windowShortcuts';
const generation = location.hash.split('=')[1]!;
const client = createDesktopLyricClient(generation);
const view = shallowRef<DesktopLyricView>(),
  error = ref(''),
  busy = ref(false);
const shortcuts = useLocalShortcuts((value) => {
  error.value = value.message;
}, createWindowShortcutClient());
let disposed = false,
  reading = false,
  timer: ReturnType<typeof setTimeout> | undefined;
const appearance = computed(() => view.value?.preferences.current?.appearance);
const vertical = computed(() => appearance.value?.composition === 'vertical');
const rows = computed(() => view.value?.lyrics?.frame.lines ?? []);
const color = (index: number) => {
  const c = view.value?.colors[index] ?? [238, 238, 238, 255];
  return `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${c[3]! / 255})`;
};
const style = computed(() => ({
  '--lyric-size': `${appearance.value?.fontSize ?? 36}px`,
  '--lyric-unplayed': `linear-gradient(${color(0)},${color(1)})`,
  '--lyric-played': `linear-gradient(${color(2)},${color(3)})`,
  '--lyric-outline-unplayed': color(4),
  '--lyric-outline-played': color(5),
  '--lyric-stroke': appearance.value?.outline ? '1px' : '0px',
  fontFamily: fontStack(view.value?.fontFamilies ?? []),
  fontWeight: appearance.value?.bold ? 700 : 400,
}));
const empty = computed(() =>
  view.value?.state === 'loading'
    ? '正在加载歌词…'
    : view.value?.state === 'failed'
      ? '歌词暂时无法加载'
      : view.value?.media.item?.kind === 'video'
        ? '视频播放中'
        : view.value?.lyrics?.frame.displayType === 'pure'
          ? '纯音乐，请欣赏'
          : view.value?.media.item
            ? '这首歌暂无歌词'
            : 'Cumulus · 让音乐陪伴你',
);
async function refresh() {
  if (disposed || reading) return;
  reading = true;
  try {
    const next = await client.read();
    if (!disposed && next.generation === generation) {
      view.value = next;
      shortcuts.account(next.media.epoch);
    }
  } catch (e) {
    if (!disposed) {
      view.value = undefined;
      if (failure(e).code !== 'stale') error.value = failure(e).message;
    }
  } finally {
    reading = false;
  }
}
async function poll() {
  await refresh();
  if (!disposed) timer = setTimeout(() => void poll(), 120);
}
async function perform(action: DesktopLyricAction) {
  if (busy.value || !view.value || view.value.locked) return;
  const ticket = view.value.ticket;
  busy.value = true;
  error.value = '';
  try {
    await client.action(ticket, action);
  } catch (e) {
    if (!disposed) error.value = failure(e).message;
  } finally {
    if (!disposed) {
      busy.value = false;
      await refresh();
    }
  }
}
async function drag(event: MouseEvent) {
  if (event.button || view.value?.locked || (event.target as HTMLElement).closest('button')) return;
  try {
    await client.drag();
  } catch {
    error.value = '暂时无法移动歌词窗口';
  }
}
async function resize(edge: 'East' | 'South' | 'SouthEast') {
  if (view.value?.locked) return;
  try {
    await client.resize(edge);
  } catch {
    error.value = '暂时无法调整歌词窗口大小';
  }
}
onMounted(async () => {
  window.addEventListener('keydown', key);
  try {
    await client.ready();
    await refresh();
    if (!disposed && view.value) {
      try {
        await shortcuts.connect(view.value.media.epoch);
      } catch {
        error.value = '快捷键暂不可用，仍可使用歌词控制按钮';
      }
    }
    if (!disposed) void poll();
  } catch {
    error.value = '歌词窗口未能打开，请从设置重试';
  }
});
onUnmounted(() => {
  disposed = true;
  clearTimeout(timer);
  shortcuts.dispose();
  window.removeEventListener('keydown', key);
});
function key(event: KeyboardEvent) {
  if (view.value && !view.value.locked)
    shortcuts.handle(event, busy.value, view.value.media.selection);
}
</script>
<template>
  <main
    class="desktop-lyric-surface"
    :class="{ locked: view?.locked, vertical }"
    :style="style"
    aria-label="桌面歌词"
    @mousedown="drag"
    @keydown.esc="perform('close')"
  >
    <div v-if="!view?.locked" class="desktop-lyric-toolbar" aria-label="桌面歌词控制">
      <button
        class="lyric-details"
        :disabled="busy || !view?.media.item"
        title="打开播放详情"
        @click="perform('details')"
      >
        {{ view?.media.item?.title || 'Cumulus' }}
      </button>
      <div class="lyric-transport">
        <button
          aria-label="上一首"
          title="上一首"
          :disabled="busy || !view?.media.canPrevious"
          @click="perform('previous')"
        >
          <Icon name="previous" />
        </button>
        <button
          :aria-label="view?.media.playing ? '暂停' : '播放'"
          :title="view?.media.playing ? '暂停' : '播放'"
          :disabled="busy || !view?.media.item"
          @click="perform('toggle')"
        >
          <Icon :name="view?.media.playing ? 'pause' : 'play'" />
        </button>
        <button
          aria-label="下一首"
          title="下一首"
          :disabled="busy || !view?.media.canNext"
          @click="perform('next')"
        >
          <Icon name="next" />
        </button>
      </div>
      <div class="lyric-offset-controls">
        <button
          aria-label="歌词延后半秒"
          title="歌词延后半秒"
          :disabled="busy || !view?.lyrics"
          @click="perform('offset_back')"
        >
          −</button
        ><span>{{ (view?.lyrics?.frame.offset ?? 0).toFixed(1) }}s</span
        ><button
          aria-label="歌词提前半秒"
          title="歌词提前半秒"
          :disabled="busy || !view?.lyrics"
          @click="perform('offset_forward')"
        >
          +
        </button>
      </div>
      <button
        aria-label="桌面歌词设置"
        title="桌面歌词设置"
        :disabled="busy || !view"
        @click="perform('settings')"
      >
        <Icon name="settings" />
      </button>
      <button
        aria-label="锁定桌面歌词"
        title="锁定歌词；可从设置或托盘解锁"
        :disabled="busy || !view"
        @click="perform('lock')"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <rect x="5" y="10" width="14" height="10" rx="2" />
          <path d="M8 10V7a4 4 0 0 1 8 0v3m-4 4v2" />
        </svg>
      </button>
      <button
        aria-label="关闭桌面歌词"
        title="关闭桌面歌词"
        :disabled="busy"
        @click="
          client.close().catch(() => {
            error = '关闭窗口失败，请从设置重试';
          })
        "
      >
        <Icon name="close" />
      </button>
    </div>
    <div class="desktop-lyric-lines" :class="{ vertical }">
      <LyricRow
        v-for="(line, index) in rows"
        :key="`${view?.media.epoch}:${view?.lyrics?.playId}:${line.index}`"
        :line="line"
        :vertical="vertical"
        :align="view?.alignments[index] ?? 'center'"
      />
      <p v-if="!rows.length" class="desktop-lyric-empty">{{ empty }}</p>
    </div>
    <div
      v-if="
        !view?.locked &&
        (error ||
          view?.error ||
          view?.state === 'failed' ||
          view?.lyrics?.info.fetchFailed ||
          view?.lyrics?.info.networkFailed)
      "
      class="desktop-lyric-error"
      role="status"
    >
      {{ error || view?.error || (rows.length ? '正在显示缓存歌词' : '歌词加载未完成')
      }}<button :disabled="busy || !view" @click="perform('retry')">重试</button>
    </div>
    <template v-if="!view?.locked"
      ><div
        v-for="edge in ['East', 'South', 'SouthEast'] as const"
        :key="edge"
        class="lyric-resize"
        :class="edge"
        aria-hidden="true"
        @mousedown.stop.prevent="resize(edge)"
    /></template>
  </main>
</template>
