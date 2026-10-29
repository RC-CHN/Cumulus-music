<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
import Icon from '../components/Icon.vue';
import { createMiniClient } from '../adapters/mini';
import type { MiniAction, MiniQueuePage, MiniView } from '../services/mini';
import type { PlaybackMode } from '../services/player';
import './mini.css';

const client = createMiniClient(location.hash.slice('#mini='.length));
const view = ref<MiniView | null>(null);
const media = computed(() => view.value?.media);
const busy = ref(false);
const error = ref('');
const expanded = ref(false);
const queue = ref<MiniQueuePage | null>(null);
const queueTicket = ref('');
const queueError = ref('');
const confirmingClear = ref(false);
let disposed = false;
let reading = false;
let timer: ReturnType<typeof setTimeout> | undefined;
let queueRequest = 0;
const modes: PlaybackMode[] = ['order_loop', 'single_loop', 'random', 'sequential'];
const modeNames = {
  order_loop: '列表循环',
  single_loop: '单曲循环',
  random: '随机播放',
  sequential: '顺序播放',
};
const modeIcons = {
  order_loop: 'repeat',
  single_loop: 'repeatOne',
  random: 'shuffle',
  sequential: 'list',
};
const liked = computed(() => media.value?.like?.observed === true);
const likeRefresh = computed(
  () => media.value?.like?.pending || media.value?.like?.observed === null,
);
const likeLabel = computed(() =>
  likeRefresh.value ? '刷新喜欢状态' : liked.value ? '取消喜欢' : '喜欢',
);
function message(value: unknown) {
  const code = (value as { code?: string })?.code;
  if (code === 'stale' || code === 'snapshot_changed') return '播放状态已更新，请再操作一次';
  return (value as { message?: string })?.message || '操作未完成，请重试';
}
function time(seconds: number | null | undefined) {
  const value = Math.max(0, Math.floor(seconds || 0));
  return `${Math.floor(value / 60)}:${String(value % 60).padStart(2, '0')}`;
}
async function refreshQueue(offset = queue.value?.offset || 0) {
  if (!view.value || !expanded.value) return;
  const ticket = view.value.ticket;
  const request = ++queueRequest;
  confirmingClear.value = false;
  queue.value = null;
  queueError.value = '';
  try {
    const page = await client.queue(ticket, offset);
    if (disposed || request !== queueRequest || view.value.ticket !== ticket) return;
    if (offset >= page.total && offset > 0) {
      await refreshQueue(Math.max(0, Math.floor((page.total - 1) / 30) * 30));
      return;
    }
    queue.value = page;
    queueTicket.value = ticket;
  } catch (e) {
    if (!disposed && request === queueRequest) queueError.value = message(e);
  }
}
async function refresh() {
  if (reading || disposed) return;
  reading = true;
  try {
    const value = await client.read();
    if (disposed) return;
    const changed = view.value?.ticket !== value.ticket;
    view.value = value;
    if (changed && expanded.value) {
      if (value.media.item?.kind === 'music') void refreshQueue();
      else {
        expanded.value = false;
        queue.value = null;
        ++queueRequest;
        void client.panel(false).catch(() => {});
      }
    }
  } finally {
    reading = false;
  }
}
async function poll() {
  try {
    await refresh();
  } catch (e) {
    if (!disposed) error.value = message(e);
  }
  if (!disposed) timer = setTimeout(() => void poll(), 350);
}
async function perform(action: MiniAction, ticket = view.value?.ticket) {
  if (busy.value || !ticket) return;
  busy.value = true;
  error.value = '';
  try {
    await client.action(ticket, action);
  } catch (e) {
    error.value = message(e);
  } finally {
    busy.value = false;
    await refresh().catch(() => {});
  }
}
async function pin() {
  if (!view.value || busy.value) return;
  const saved = view.value;
  busy.value = true;
  error.value = '';
  try {
    await client.preferences(
      saved.ticket,
      saved.preferences.damaged
        ? { kind: 'repair' }
        : {
            kind: 'change',
            expected_revision: saved.preferences.revision!,
            always_on_top: !saved.preferences.alwaysOnTop,
          },
    );
  } catch (e) {
    error.value = message(e);
  } finally {
    busy.value = false;
    await refresh().catch(() => {});
  }
}
async function panel() {
  const next = !expanded.value;
  try {
    await client.panel(next);
    expanded.value = next;
    if (next) await refreshQueue(0);
    else {
      ++queueRequest;
      queue.value = null;
    }
  } catch (e) {
    error.value = message(e);
  }
}
async function windowAction(kind: 'returnMain' | 'close' | 'drag') {
  try {
    await client[kind]();
  } catch {
    error.value = '窗口操作未完成，请重试';
  }
}
function key(event: KeyboardEvent) {
  if (event.repeat || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    void windowAction('returnMain');
    return;
  }
  if (event.target instanceof HTMLElement && event.target.closest('input,button,[contenteditable]'))
    return;
  if (event.code === 'Space') {
    event.preventDefault();
    void perform({ kind: 'toggle' });
  }
}
onMounted(async () => {
  window.addEventListener('keydown', key);
  try {
    await refresh();
    await nextTick();
    await client.ready();
    void poll();
  } catch (e) {
    error.value = message(e);
  }
});
onUnmounted(() => {
  disposed = true;
  ++queueRequest;
  clearTimeout(timer);
  window.removeEventListener('keydown', key);
});
</script>

<template>
  <main class="mini-shell" :class="{ expanded }" aria-label="迷你播放器">
    <header class="mini-header">
      <span class="mini-brand" @mousedown.left="windowAction('drag')"
        >Cumulus <span>迷你播放器</span></span
      >
      <button
        :class="{ selected: view?.preferences.alwaysOnTop }"
        :disabled="busy || !view"
        :title="view?.preferences.damaged ? '修复置顶设置' : '始终置顶'"
        :aria-label="view?.preferences.damaged ? '修复置顶设置' : '始终置顶'"
        :aria-pressed="view?.preferences.alwaysOnTop"
        @click="pin"
      >
        <svg viewBox="0 0 24 24" class="icon" fill="none" stroke="currentColor" stroke-width="1.6">
          <path d="m9 3 6 0-1 6 4 4v2H6v-2l4-4-1-6m3 12v7" />
        </svg>
      </button>
      <button title="返回主窗口 · Esc" aria-label="返回主窗口" @click="windowAction('returnMain')">
        <Icon name="monitor" />
      </button>
      <button
        class="mini-close"
        title="关闭窗口"
        aria-label="关闭窗口"
        @click="windowAction('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <section class="mini-now">
      <button
        class="mini-cover"
        title="打开主窗口"
        aria-label="打开主窗口"
        @click="windowAction('returnMain')"
      >
        <img v-if="media?.item?.artwork" :src="media.item.artwork" alt="" /><Icon
          v-else
          :name="media?.item?.kind === 'video' ? 'video' : 'music'"
        />
      </button>
      <div class="mini-track">
        <h1 :title="media?.item?.title">{{ media?.item?.title || '从主窗口选一首音乐' }}</h1>
        <p :title="media?.item?.artists.join(' / ')">
          {{ media?.item?.artists.join(' / ') || '让音乐陪着你' }}
        </p>
        <div class="mini-timeline">
          <span>{{ time(media?.position) }}</span
          ><input
            aria-label="播放进度"
            type="range"
            :min="media?.seekStart || 0"
            :max="media?.seekEnd ?? media?.duration ?? 0"
            step="0.1"
            :value="media?.position || 0"
            :disabled="busy || !media?.canSeek"
            @change="
              perform({ kind: 'seek', seconds: Number(($event.target as HTMLInputElement).value) })
            "
          /><span>{{ time(media?.duration) }}</span>
        </div>
      </div>
    </section>
    <nav class="mini-transport" aria-label="播放控制">
      <button
        :disabled="busy || !media?.mode"
        :title="media?.mode ? modeNames[media.mode] : '当前模式由内容控制'"
        :aria-label="media?.mode ? modeNames[media.mode] : '播放模式不可用'"
        @click="
          media?.mode &&
          perform({ kind: 'mode', mode: modes[(modes.indexOf(media.mode) + 1) % modes.length]! })
        "
      >
        <Icon :name="media?.mode ? modeIcons[media.mode] : 'repeat'" />
      </button>
      <button
        :disabled="busy || !media?.canPrevious"
        title="上一首"
        aria-label="上一首"
        @click="perform({ kind: 'previous' })"
      >
        <Icon name="previous" />
      </button>
      <button
        class="mini-play"
        :disabled="busy || !media?.item"
        :aria-label="media?.playing ? '暂停' : '播放'"
        :title="media?.playing ? '暂停' : '播放'"
        @click="perform({ kind: 'toggle' })"
      >
        <Icon :name="media?.playing ? 'pause' : 'play'" />
      </button>
      <button
        :disabled="busy || !media?.canNext"
        title="下一首"
        aria-label="下一首"
        @click="perform({ kind: 'next' })"
      >
        <Icon name="next" />
      </button>
      <button
        :disabled="busy || !media?.item"
        title="停止"
        aria-label="停止"
        @click="perform({ kind: 'stop' })"
      >
        <Icon name="stop" />
      </button>
      <button
        :disabled="busy || !media?.like"
        :class="{ liked }"
        :title="likeLabel"
        :aria-label="likeLabel"
        @click="perform({ kind: likeRefresh ? 'refresh_like' : 'like' })"
      >
        <Icon :name="likeRefresh ? 'refresh' : 'heart'" />
      </button>
      <button
        :class="{ selected: expanded }"
        :disabled="media?.item?.kind !== 'music'"
        :aria-expanded="expanded"
        title="播放队列"
        aria-label="播放队列"
        @click="panel"
      >
        <Icon name="list" />
      </button>
    </nav>
    <footer class="mini-footer">
      <span
        class="mini-message"
        :title="error || media?.error || ''"
        :role="error || media?.error ? 'alert' : undefined"
        >{{
          error ||
          media?.error ||
          (media?.state === 'loading' ? '正在加载…' : media?.item?.album || '云间音乐')
        }}</span
      ><Icon :name="media?.volume === 0 ? 'muted' : 'volume'" /><input
        aria-label="音量"
        type="range"
        min="0"
        max="100"
        :value="media?.volume ?? 100"
        :disabled="busy || !media?.item"
        @change="
          perform({ kind: 'volume', value: Number(($event.target as HTMLInputElement).value) })
        "
      />
    </footer>
    <section v-if="expanded" class="mini-queue" aria-label="播放队列列表">
      <header>
        <strong
          >播放队列 <small v-if="queue">{{ queue.total }} 首</small></strong
        ><button
          v-if="queue?.editable && queue.total"
          :disabled="busy"
          @click="confirmingClear = !confirmingClear"
        >
          清空
        </button>
      </header>
      <div v-if="confirmingClear" class="mini-confirm" role="alert">
        清空当前播放队列？<button @click="confirmingClear = false">取消</button
        ><button :disabled="busy" @click="perform({ kind: 'clear' }, queueTicket)">确认清空</button>
      </div>
      <div v-if="queueError" class="mini-queue-empty" role="alert">
        {{ queueError }}<button @click="refreshQueue()">重新加载</button>
      </div>
      <div v-else-if="!queue" class="mini-queue-empty">正在读取队列…</div>
      <ol v-else class="mini-queue-rows">
        <li
          v-for="entry in queue.tracks"
          :key="entry.entryId"
          :class="{ current: queue.current === entry.entryId }"
        >
          <button
            class="mini-queue-track"
            :disabled="busy || !queue.editable"
            :title="`${entry.track.name} · ${entry.track.artists.join(' / ')}`"
            @click="perform({ kind: 'select', entry_id: entry.entryId }, queueTicket)"
          >
            <Icon :name="queue.current === entry.entryId ? 'volume' : 'music'" /><span
              >{{ entry.track.name }}<small>{{ entry.track.artists.join(' / ') }}</small></span
            >
          </button>
          <button
            v-if="queue.editable"
            :disabled="busy"
            :aria-label="`从队列移除 ${entry.track.name}`"
            title="从队列移除"
            @click="perform({ kind: 'remove', entry_id: entry.entryId }, queueTicket)"
          >
            <Icon name="close" />
          </button>
        </li>
      </ol>
      <footer v-if="queue && queue.total > 30" class="mini-pages">
        <button :disabled="queue.offset === 0" @click="refreshQueue(queue!.offset - 30)">
          上一页</button
        ><span>{{ Math.floor(queue.offset / 30) + 1 }} / {{ Math.ceil(queue.total / 30) }}</span
        ><button
          :disabled="queue.offset + 30 >= queue.total"
          @click="refreshQueue(queue!.offset + 30)"
        >
          下一页
        </button>
      </footer>
      <p v-if="queue && !queue.editable" class="mini-queue-note">漫游队列由当前频道管理</p>
    </section>
  </main>
</template>
