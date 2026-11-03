<script setup lang="ts">
import BoundedPanelTransition from '../components/BoundedPanelTransition.vue';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import Icon from '../components/Icon.vue';
import { useLocalShortcuts } from '../composables/useLocalShortcuts';
import { useMiniModes } from '../composables/useMiniModes';
import { useMiniFeedback } from '../composables/useMiniFeedback';
import { useMiniPin } from '../composables/useMiniPin';
import { useMiniWindowMenu } from '../composables/useMiniWindowMenu';
import MiniWindowMenu from './MiniWindowMenu.vue';
import MiniFeedbackPanel from './MiniFeedbackPanel.vue';
import ModalTransition from '../components/ModalTransition.vue';
import { createMiniClient, createMiniShortcutClient } from '../adapters/mini';
import type { MiniAction, MiniQueuePage, MiniView, MiniLyricMenuAction } from '../services/mini';
import type { ModeTarget } from '../services/playbackMode';
import { modeLabel, modeIcon } from '../services/playbackMode';
import { artistLabel } from '../services/artistLabel';
import './mini.css';

const client = createMiniClient(location.hash.slice('#mini='.length));
const view = ref<MiniView | null>(null);
const media = computed(() => view.value?.media);
const busy = ref(false);
const error = ref('');
const shortcuts = useLocalShortcuts((e) => {
  error.value = e.message;
}, createMiniShortcutClient());
const expanded = ref(false);
const queue = ref<MiniQueuePage | null>(null);
const queueTicket = ref('');
const queueError = ref('');
const confirmingClear = ref(false);
let disposed = false;
let reading: Promise<MiniView | undefined> | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;
let queueRequest = 0;
const modes = useMiniModes(view, client, () => refresh(true));
const feedback = useMiniFeedback(
  view,
  client,
  () => refresh(true),
  () => busy.value || modes.busy.value || modes.mode.pending.value,
);
const feedbackTrigger = ref<HTMLButtonElement>();
const feedbackNavigationError = ref('');
const pinState = useMiniPin(view, client, () => refresh(true), busy);
const pinTrigger = ref<HTMLButtonElement>();
const windowMenu = ref<InstanceType<typeof MiniWindowMenu>>();
const menuModel = useMiniWindowMenu(view, client, busy, () => refresh(true));
const reportingEpoch = ref<string>();
const reportOpening = computed(
  () => !!reportingEpoch.value && reportingEpoch.value === media.value?.epoch,
);
const menuProgress = computed(() =>
  reportOpening.value ? '正在打开歌词报错…' : menuModel.working.value ? '正在更新桌面歌词…' : '',
);
const lyricMenuIssue = computed(() =>
  menuModel.issue.value ? `桌面歌词：${menuModel.issue.value}` : '',
);
let menuFocus: { epoch: string; control: HTMLElement; surface: HTMLElement } | undefined;
async function restoreMenuFocus() {
  if (
    busy.value ||
    reportOpening.value ||
    menuModel.working.value ||
    modes.busy.value ||
    modes.mode.pending.value ||
    !menuFocus
  )
    return;
  const saved = menuFocus;
  menuFocus = undefined;
  await nextTick();
  if (
    !disposed &&
    media.value?.epoch === saved.epoch &&
    document.activeElement === saved.surface &&
    saved.control.isConnected &&
    !(saved.control as HTMLButtonElement).disabled
  )
    saved.control.focus({ preventScroll: true });
}
watch(
  [busy, menuModel.working, reportOpening, modes.busy, modes.mode.pending],
  () => void restoreMenuFocus(),
);
async function windowMenuSelect(key: string) {
  const control = document.activeElement,
    surface = control?.closest<HTMLElement>('.mini-shell');
  if (view.value && control instanceof HTMLElement && surface) {
    menuFocus = { epoch: view.value.media.epoch, control, surface };
    surface.focus({ preventScroll: true });
  }
  if (key === 'pin') await pinState.apply();
  else if (key === 'return') await windowAction('returnMain');
  else if (key === 'exit') {
    try {
      await client.exit();
    } catch (e) {
      error.value = message(e);
    }
  } else if (key === 'lyric:report') {
    const shown = view.value;
    if (!shown || reportOpening.value) return;
    reportingEpoch.value = shown.media.epoch;
    try {
      await client.reportLyrics(shown.ticket);
    } catch (e) {
      if (!disposed && media.value?.epoch === shown.media.epoch) error.value = message(e);
    } finally {
      if (reportingEpoch.value === shown.media.epoch) reportingEpoch.value = undefined;
    }
  } else if (key.startsWith('mode:')) modes.mode.select(key.slice(5) as ModeTarget);
  else if (key.startsWith('lyric:')) await menuModel.action(key.slice(6) as MiniLyricMenuAction);
  await restoreMenuFocus();
}
function windowContext(event: MouseEvent | KeyboardEvent) {
  if (
    event instanceof KeyboardEvent &&
    event.key !== 'ContextMenu' &&
    !(event.key === 'F10' && event.shiftKey)
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  const header = event.currentTarget as HTMLElement,
    box = header.getBoundingClientRect();
  void windowMenu.value?.showAt(
    event instanceof MouseEvent ? event.clientX : box.right - 8,
    event instanceof MouseEvent ? event.clientY : box.bottom,
    header,
  );
}
watch(feedback.open, () => {
  feedbackNavigationError.value = '';
});
async function feedbackMain() {
  try {
    await client.returnMain(true);
  } catch (e) {
    feedbackNavigationError.value = message(e);
  }
}
const modeTrigger = ref<HTMLButtonElement>();
let cancelFocus: { epoch: string; control: Element | null } | undefined;
watch(modes.busy, async (active) => {
  if (active || !cancelFocus) return;
  const saved = cancelFocus;
  cancelFocus = undefined;
  await nextTick();
  if (
    disposed ||
    media.value?.epoch !== saved.epoch ||
    (document.activeElement !== saved.control && document.activeElement !== document.body)
  )
    return;
  (
    document.querySelector<HTMLButtonElement>('.mini-intelligence button:not(:disabled)') ??
    modeTrigger.value
  )?.focus({ preventScroll: true });
});
function cancelRecommendation() {
  if (media.value) cancelFocus = { epoch: media.value.epoch, control: document.activeElement };
  void modes.cancel();
}
function dismissModes() {
  modeTrigger.value?.focus({ preventScroll: true });
  modes.dismiss();
}
function retryMode() {
  modeTrigger.value?.focus({ preventScroll: true });
  modes.retry();
}
const modeName = computed(() =>
  media.value?.mode && media.value.modeFamily
    ? modeLabel(modes.mode.value.value, modes.mode.displayFamily.value)
    : '播放模式不可用',
);
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
async function refreshQueue(offset = queue.value?.offset || 0, retry = 0) {
  if (!view.value || !expanded.value) return;
  const ticket = view.value.ticket;
  const owner = view.value.media.epoch;
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
    if (
      !disposed &&
      expanded.value &&
      request === queueRequest &&
      view.value?.media.epoch === owner
    ) {
      if (
        ['stale', 'snapshot_changed'].includes((e as { code?: string })?.code ?? '') &&
        retry < 2
      ) {
        await refresh(true).catch(() => {});
        if (
          !disposed &&
          expanded.value &&
          request === queueRequest &&
          view.value?.media.epoch === owner
        )
          await refreshQueue(offset, retry + 1);
      } else queueError.value = message(e);
    }
  }
}
async function refresh(fresh = false): Promise<MiniView | undefined> {
  if (disposed) return;
  if (reading) {
    if (!fresh) return reading;
    await reading.catch(() => {});
    return refresh();
  }
  reading = readOnce();
  return reading;
}
async function readOnce(): Promise<MiniView | undefined> {
  try {
    const value = await client.read();
    if (disposed) return;
    const changed = view.value?.ticket !== value.ticket;
    view.value = value;
    shortcuts.account(value.media.epoch);
    if (changed && expanded.value) {
      if (value.media.item?.kind === 'music' || value.media.modeFamily === 'fm')
        void refreshQueue();
      else {
        expanded.value = false;
        queue.value = null;
        ++queueRequest;
        void client.panel(false).catch(() => {});
      }
    }
    return value;
  } finally {
    reading = undefined;
  }
}
async function poll() {
  try {
    await refresh();
  } catch (e) {
    if (!disposed && !['stale', 'snapshot_changed'].includes((e as { code?: string })?.code ?? ''))
      error.value = message(e);
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
  if (busy.value || !view.value) return;
  const epoch = view.value.media.epoch;
  const focused = document.activeElement === pinTrigger.value;
  const shell = pinTrigger.value?.closest<HTMLElement>('.mini-shell');
  if (focused) shell?.focus({ preventScroll: true });
  await pinState.apply();
  await nextTick();
  if (!disposed && focused && view.value?.media.epoch === epoch && document.activeElement === shell)
    pinTrigger.value?.focus({ preventScroll: true });
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
  if (event.repeat || event.isComposing || event.defaultPrevented) return;
  if (feedback.open.value) return;
  if (
    event.key === 'Escape' &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.metaKey &&
    !event.shiftKey
  ) {
    event.preventDefault();
    if (modes.issue.value) {
      dismissModes();
      return;
    }
    void windowAction('returnMain');
    return;
  }
  if (media.value)
    shortcuts.handle(event, busy.value || confirmingClear.value, media.value.selection);
}

onMounted(async () => {
  window.addEventListener('keydown', key);
  try {
    await refresh();
    await nextTick();
    await client.ready();
    if (view.value) await shortcuts.connect(view.value.media.epoch);
    void poll();
  } catch (e) {
    error.value = message(e);
  }
});
onUnmounted(() => {
  disposed = true;
  shortcuts.dispose();
  ++queueRequest;
  clearTimeout(timer);
  window.removeEventListener('keydown', key);
});
</script>

<template>
  <main
    class="mini-shell"
    tabindex="-1"
    :class="{ expanded }"
    :data-control-ticket="view?.ticket"
    :data-intelligence-phase="media?.intelligence"
    :data-intelligence-operation="media?.intelligenceOperation"
    aria-label="迷你播放器"
  >
    <header class="mini-header" tabindex="-1" @contextmenu="windowContext" @keydown="windowContext">
      <span class="mini-brand" @mousedown.left="windowAction('drag')"
        >Cumulus <span>迷你播放器</span></span
      >
      <MiniWindowMenu
        ref="windowMenu"
        :view="view"
        :modes="modes"
        :lyrics="menuModel"
        :pin-label="pinState.label.value"
        :pin-selected="pinState.selected.value"
        :disabled="
          busy ||
          menuModel.working.value ||
          reportOpening ||
          !view ||
          feedback.open.value ||
          modes.busy.value ||
          modes.mode.pending.value
        "
        :aria-busy="menuModel.working.value || reportOpening"
        @select="windowMenuSelect"
      />
      <button
        ref="pinTrigger"
        data-mini-pin
        :class="{ selected: pinState.selected.value }"
        :disabled="busy || !view"
        :title="pinState.label.value"
        :aria-label="pinState.label.value"
        :aria-pressed="pinState.selected.value"
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
        <h1 :title="media?.item?.title">
          {{
            media?.item?.title ||
            (media?.modeFamily === 'fm' ? '暂时没有漫游推荐' : '从主窗口选一首音乐')
          }}
        </h1>
        <p
          :title="
            artistLabel(
              media?.item,
              media?.modeFamily === 'fm' ? '返回主窗口重新获取推荐' : '让音乐陪着你',
            )
          "
        >
          {{
            artistLabel(
              media?.item,
              media?.modeFamily === 'fm' ? '返回主窗口重新获取推荐' : '让音乐陪着你',
            )
          }}
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
        ref="modeTrigger"
        data-mini-mode
        :data-mode="media?.mode"
        :data-family="media?.modeFamily"
        :data-target="modes.mode.target.value"
        :aria-busy="modes.mode.pending.value"
        :disabled="busy || !media?.mode || !media?.modeFamily"
        :title="modeName"
        :aria-label="modeName"
        @click="modes.mode.cycle"
      >
        <BoundedPanelTransition name="mini-mode-glyph">
          <span
            :key="modes.mode.displayFamily.value + ':' + modes.mode.value.value"
            class="mini-mode-glyph"
          >
            <Icon
              :name="
                media?.mode && media.modeFamily
                  ? modeIcon(modes.mode.value.value, modes.mode.displayFamily.value)
                  : 'repeat'
              "
            />
          </span>
        </BoundedPanelTransition>
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
        v-if="media?.modeFamily === 'fm'"
        ref="feedbackTrigger"
        :disabled="!feedback.canOpen.value"
        title="减少推荐"
        aria-label="减少推荐"
        @click="feedback.show"
      >
        <Icon name="trash" />
      </button>
      <button
        :class="{ selected: expanded }"
        :disabled="media?.item?.kind !== 'music' && media?.modeFamily !== 'fm'"
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
        v-if="modes.busy.value || modes.resumable.value"
        class="mini-intelligence"
        role="status"
      >
        <span>{{
          modes.busy.value
            ? modes.cancelling.value
              ? '正在取消…'
              : modes.preparing.value
                ? '准备心动模式'
                : '获取心动推荐'
            : '推荐尚未完成'
        }}</span>
        <button
          v-if="modes.busy.value"
          :disabled="modes.cancelling.value"
          @click="cancelRecommendation"
        >
          取消推荐
        </button>
        <button v-else @click="modes.resume">继续获取</button>
      </span>
      <span
        v-else
        class="mini-message"
        :title="
          menuProgress
            ? menuProgress
            : error || lyricMenuIssue || pinState.notice.value || media?.error || ''
        "
        :role="
          menuProgress
            ? 'status'
            : error || lyricMenuIssue || pinState.notice.value || media?.error
              ? 'alert'
              : undefined
        "
        >{{
          menuProgress ||
          error ||
          lyricMenuIssue ||
          pinState.notice.value ||
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
    <Transition
      name="mini-mode-feedback"
      @before-leave="(element) => ((element as HTMLElement).inert = true)"
    >
      <section v-if="modes.issue.value" class="mini-mode-feedback" role="alert">
        <span>{{ modes.issue.value.message }}</span>
        <div>
          <button v-if="modes.canRetry.value" @click="retryMode">重试</button
          ><button aria-label="关闭播放模式提示" @click="dismissModes">关闭</button>
        </div>
      </section>
    </Transition>
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
      <div v-else-if="!queue.total" class="mini-queue-empty">
        {{ media?.modeFamily === 'fm' ? '暂无漫游推荐，可返回主窗口重新获取' : '播放队列空空如也' }}
      </div>
      <ol v-else class="mini-queue-rows" :class="{ readonly: !queue.editable }">
        <li
          v-for="entry in queue.tracks"
          :key="entry.entryId"
          :class="{ current: queue.current === entry.entryId }"
        >
          <button
            class="mini-queue-track"
            :disabled="busy || !queue.editable"
            :title="`${entry.track.name} · ${artistLabel(entry.track)}`"
            @click="perform({ kind: 'select', entry_id: entry.entryId }, queueTicket)"
          >
            <Icon :name="queue.current === entry.entryId ? 'volume' : 'music'" /><span
              >{{ entry.track.name }}<small>{{ artistLabel(entry.track) }}</small></span
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
    <ModalTransition
      :return-focus="() => feedbackTrigger ?? null"
      :fallback-focus="() => modeTrigger ?? null"
    >
      <MiniFeedbackPanel
        v-if="feedback.open.value"
        :model="feedback"
        :navigation-issue="feedbackNavigationError"
        @main="feedbackMain"
      />
    </ModalTransition>
  </main>
</template>
