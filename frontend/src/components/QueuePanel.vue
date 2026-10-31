<script setup lang="ts">
import {
  computed,
  watch,
  onMounted,
  onBeforeUnmount,
  onUnmounted,
  ref,
  shallowRef,
  nextTick,
} from 'vue';
import { useVirtualRows } from '../composables/useVirtualRows';
import { useQueueSelection } from '../composables/useQueueSelection';
import { useQueueDrag } from '../composables/useQueueDrag';
import { useControlFocus } from '../composables/useControlFocus';
import type { PlayerModel } from '../composables/usePlayer';
import type { TogetherModel } from '../composables/useTogether';
import { time, type QueueView, type QueueEdit, type QueueCollectionView } from '../services/player';
import { failure } from '../services/contracts';
import { useCore } from '../services/core';
import { artistLabel } from '../services/artistLabel';
import Icon from './Icon.vue';
import ModalTransition from './ModalTransition.vue';
import QueueClearDialog from './queue/QueueClearDialog.vue';
const props = defineProps<{
  player: PlayerModel;
  authenticated: boolean;
  together: TogetherModel;
}>();
const emit = defineEmits<{
  collect: [value: QueueCollectionView, trigger: HTMLElement | undefined];
  login: [];
  discover: [];
  together: [];
  cleared: [];
}>();
const client = useCore();
const collecting = ref(false),
  collectionIssue = ref('');
let collectionRead = 0;
const { queue, view, busy, queueError, queueLoading, queueEditing, queueEditError, queueCurrent } =
  props.player;
const row = 64;
const { container, start, end, onScroll } = useVirtualRows(
  () => queue.value?.tracks.length ?? 0,
  row,
);
const visible = computed(() => queue.value?.tracks.slice(start.value, end.value) ?? []);
const currentEntry = computed(() => view.value?.entryId);
const queueCount = computed(() =>
  queueCurrent.value ? queue.value!.tracks.length : (view.value?.queueLen ?? 0),
);
const locked = computed(
  () =>
    busy.value ||
    props.player.queueBusy.value ||
    queueLoading.value ||
    queueEditing.value ||
    !queueCurrent.value ||
    !!queueError.value ||
    !!queue.value?.fm,
);
const collectionLocked = computed(
  () =>
    collecting.value ||
    busy.value ||
    props.player.queueBusy.value ||
    queueLoading.value ||
    queueEditing.value ||
    !queueCurrent.value ||
    !!queueError.value ||
    !queue.value?.tracks.length,
);
const clearLocked = computed(() => locked.value || !!queue.value?.clearContext.issue);
const selection = useQueueSelection(() => queue.value);
const selected = selection.selected;
const completedRoom = ref<string>();
let completedObservation: TogetherModel['view']['value'];
const clearCompletion = computed(() => {
  if (!completedRoom.value || props.together.epoch.value !== view.value?.epoch) return '';
  const state = props.together.view.value;
  if (!state || state === completedObservation || props.together.loading.value)
    return '播放列表已清空，正在核对一起听退出状态。';
  if (!state.room && !state.attached && !state.localRecovery && !state.pending.length)
    return state.lifecycleNotice ?? '播放列表已清空，一起听已结束。';
  return '播放列表已清空，一起听退出或本机恢复尚未完成，请查看房间状态。';
});
const panel = ref<HTMLElement>(),
  clearButton = ref<HTMLButtonElement>(),
  closeButton = ref<HTMLButtonElement>();
const clearOpen = ref(false),
  clearTicket = shallowRef<QueueView>();
const drag = useQueueDrag({
  queue: () => queue.value,
  container: () => container.value,
  disabled: () => locked.value || clearOpen.value,
  selected: () => selected.value,
  select: selection.select,
  scroll: onScroll,
  reorder: (ids, displayed, focusId) => {
    void edit({ kind: 'reorder', entry_ids: ids }, ids.indexOf(focusId), displayed);
  },
});
const dragging = drag.dragging;
const clearChanged = computed(
  () =>
    !!clearTicket.value &&
    (!queueCurrent.value ||
      !!queueError.value ||
      clearTicket.value.epoch !== view.value?.epoch ||
      clearTicket.value.revision !== view.value.queueRevision ||
      clearTicket.value.clearContext.roomId !== queue.value?.clearContext.roomId),
);
// Keep the last dialog body during its exit animation, including error text.
const clearLeaving = shallowRef({
  count: 0,
  busy: false,
  changed: false,
  blocked: false,
  issue: '',
  endTogether: false,
});
const clearPresentation = computed(() =>
  clearOpen.value
    ? {
        count: clearTicket.value?.tracks.length ?? 0,
        busy: queueEditing.value,
        changed: !queueEditing.value && clearChanged.value,
        blocked: clearLocked.value,
        issue: queueEditError.value,
        endTogether: !!clearTicket.value?.clearContext.roomId,
      }
    : clearLeaving.value,
);
let focus: HTMLElement | null = null;
onMounted(() => {
  focus = document.activeElement as HTMLElement;
  panel.value?.focus({ preventScroll: true });
});
watch(
  [container, currentEntry],
  ([element, current]) => {
    if (!element) return;
    const index = queue.value?.tracks.findIndex((i) => i.entryId === current) ?? -1;
    if (index >= 0) element.scrollTop = Math.max(0, index * row - (element.clientHeight - row) / 2);
    onScroll();
    if (document.activeElement === panel.value) element.focus({ preventScroll: true });
  },
  { flush: 'post' },
);
watch(
  () => view.value?.epoch,
  () => {
    collectionRead++;
    collecting.value = false;
    collectionIssue.value = '';
    completedRoom.value = undefined;
    clearLeaving.value = clearPresentation.value;
    clearOpen.value = false;
  },
);
let leavingPanel: HTMLElement | undefined;
onBeforeUnmount(() => {
  leavingPanel = panel.value;
});
onUnmounted(() => {
  collectionRead++;
  const active = document.activeElement;
  if (
    (active === document.body || leavingPanel?.contains(active)) &&
    focus?.isConnected &&
    !focus.matches(':disabled')
  )
    focus.focus({ preventScroll: true });
});
async function collectAll(event: Event) {
  if (collectionLocked.value) return;
  if (!props.authenticated) {
    emit('login');
    return;
  }
  const displayed = queue.value!;
  const current = ++collectionRead;
  const trigger = event.currentTarget instanceof HTMLElement ? event.currentTarget : undefined;
  collecting.value = true;
  collectionIssue.value = '';
  try {
    const value = await client.playerQueueCollection(displayed.epoch, displayed.revision);
    if (
      current !== collectionRead ||
      !props.player.queueOpen.value ||
      value.epoch !== view.value?.epoch
    )
      return;
    if (value.revision !== view.value.queueRevision) {
      collectionIssue.value = '播放列表已更新，请核对后重新收藏';
      return;
    }
    if (value.tracks.length) emit('collect', value, trigger);
    else collectionIssue.value = '声音或未匹配的本地音乐无法收藏到歌单';
  } catch (error) {
    if (current === collectionRead) {
      const issue = failure(error);
      collectionIssue.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) emit('login');
    }
  } finally {
    if (current === collectionRead) {
      collecting.value = false;
      await nextTick();
      if (
        props.player.queueOpen.value &&
        collectionIssue.value &&
        trigger?.isConnected &&
        !trigger.matches(':disabled') &&
        (document.activeElement === document.body || document.activeElement === panel.value)
      )
        trigger.focus({ preventScroll: true });
    }
  }
}
const queueReadFocus = useControlFocus(
  () => view.value?.epoch,
  () =>
    panel.value?.querySelector<HTMLButtonElement>('.error-state button') ??
    container.value ??
    panel.value?.querySelector<HTMLButtonElement>('.queue-empty button') ??
    closeButton.value,
);
function retryQueue(event: Event) {
  return queueReadFocus.run(event, () => props.player.loadQueue());
}
function move(index: number, direction: number) {
  const ids = queue.value?.tracks.map((i) => i.entryId);
  if (!ids || !ids[index + direction]) return;
  [ids[index], ids[index + direction]] = [ids[index + direction]!, ids[index]!];
  void edit({ kind: 'reorder', entry_ids: ids }, index + direction);
}
async function focusRow(index: number) {
  const element = container.value;
  const displayed = queue.value;
  const active = document.activeElement;
  if (!element || !queue.value?.tracks.length) {
    panel.value?.focus({ preventScroll: true });
    return;
  }
  index = Math.max(0, Math.min(index, queue.value.tracks.length - 1));
  const top = index * row;
  if (top < element.scrollTop) element.scrollTop = top;
  else if (top + row > element.scrollTop + element.clientHeight)
    element.scrollTop = top + row - element.clientHeight;
  onScroll();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await nextTick();
  if (
    element !== container.value ||
    displayed !== queue.value ||
    (document.activeElement !== active && document.activeElement !== document.body)
  )
    return;
  const entry = queue.value?.tracks[index]?.entryId;
  if (entry)
    [...element.querySelectorAll<HTMLElement>('.queue-row')]
      .find((n) => n.dataset.entryId === entry)
      ?.querySelector<HTMLButtonElement>('.queue-song')
      ?.focus({ preventScroll: true });
}
async function edit(input: QueueEdit, index?: number, displayed = queue.value) {
  if (locked.value) return;
  const active = document.activeElement;
  const owner = queue.value?.epoch;
  if (await props.player.editQueue(input, displayed)) {
    if (owner !== view.value?.epoch || !panel.value) return;
    const revision = view.value?.queueRevision;
    await props.player.loadQueue();
    if (owner !== view.value?.epoch || revision !== view.value?.queueRevision || !panel.value)
      return;
    // Preserve newer user focus while native/list replies were pending.
    if (
      index !== undefined &&
      (document.activeElement === active || document.activeElement === document.body)
    )
      await focusRow(index);
  } else {
    // Disabling a pending control may move WebKit focus to body. A rejected
    // edit keeps the selection; restore its live control only after observing
    // the same entries, never after a replacement queue or a new user focus.
    if (owner !== view.value?.epoch || !panel.value) return;
    if (!queueCurrent.value) await props.player.loadQueue();
    await nextTick();
    if (
      owner === view.value?.epoch &&
      panel.value &&
      displayed?.tracks.length === queue.value?.tracks.length &&
      displayed?.tracks.every((item, i) => item.entryId === queue.value?.tracks[i]?.entryId) &&
      (document.activeElement === active || document.activeElement === document.body) &&
      active instanceof HTMLElement &&
      active.isConnected &&
      !active.matches(':disabled')
    )
      active.focus({ preventScroll: true });
  }
}
function selectRow(event: MouseEvent, id: string) {
  // Pointer selection runs on down/up to preserve a group while dragging.
  // Keyboard activation and accessibility tools produce a zero-detail click.
  if (!locked.value && event.detail === 0)
    selection.select(id, event.ctrlKey || event.metaKey, event.shiftKey);
}
function removeSelection() {
  if (locked.value || !selected.value.size) return;
  const ids = selection.ids.value.filter((id) => selected.value.has(id));
  void edit({ kind: 'remove', entry_ids: ids }, selection.ids.value.indexOf(ids[0]!));
}
function clearSelection() {
  const first = selection.ids.value.findIndex((id) => selected.value.has(id));
  selection.clear();
  void focusRow(Math.max(0, first));
}
function keydown(event: KeyboardEvent) {
  const target = event.target as HTMLElement;
  if (locked.value || event.altKey) return;
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
    event.preventDefault();
    event.stopPropagation();
    selection.all();
    return;
  }
  if (event.key === 'Delete' || event.key === 'Backspace') {
    event.preventDefault();
    if (!event.repeat && selected.value.size) removeSelection();
    return;
  }
  if (!target.matches('.queue-song')) return;
  const id = target.closest<HTMLElement>('.queue-row')?.dataset.entryId;
  const index = queue.value?.tracks.findIndex((i) => i.entryId === id) ?? -1;
  if (index < 0) return;
  let next = {
    ArrowDown: index + 1,
    ArrowUp: index - 1,
    Home: 0,
    End: (queue.value?.tracks.length ?? 1) - 1,
  }[event.key];
  if (
    (event.key === 'ArrowUp' || event.key === 'ArrowDown') &&
    !event.shiftKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    selected.value.size
  ) {
    const selectedIndices = selection.ids.value.flatMap((id, i) =>
      selected.value.has(id) ? [i] : [],
    );
    next = event.key === 'ArrowUp' ? selectedIndices[0]! - 1 : selectedIndices.at(-1)! + 1;
  }
  if (next !== undefined) {
    event.preventDefault();
    if (!event.ctrlKey && !event.metaKey)
      selection.select(
        selection.ids.value[Math.max(0, Math.min(next, selection.ids.value.length - 1))]!,
        false,
        event.shiftKey,
      );
    void focusRow(next);
  } else if (event.key === 'Enter') {
    event.preventDefault();
    if (!event.repeat && !queue.value?.tracks[index]?.track.missing)
      void edit({ kind: 'select', entry_id: id! });
  } else if (event.key === ' ') {
    event.preventDefault();
    if (!event.repeat) selection.select(id!, true, event.shiftKey);
  }
}
function openClear() {
  if (clearLocked.value || !queue.value?.tracks.length) return;
  queueEditError.value = '';
  clearTicket.value = queue.value;
  clearOpen.value = true;
}
function closeClear() {
  if (queueEditing.value) return;
  clearLeaving.value = clearPresentation.value;
  clearOpen.value = false;
}
function clearLeft() {
  if (clearOpen.value) return;
  const owner = clearTicket.value?.epoch;
  clearTicket.value = undefined;
  if (owner === view.value?.epoch) queueEditError.value = '';
}
const clearFocus = useControlFocus(
  () => view.value?.epoch,
  () => panel.value?.querySelector<HTMLButtonElement>('.queue-clear-dialog .secondary'),
);
async function confirmClear(event: Event) {
  if (!clearOpen.value || clearChanged.value || clearLocked.value) return;
  await clearFocus.run(event, async () => {
    const ticket = clearTicket.value;
    if (await props.player.editQueue({ kind: 'clear' }, ticket)) {
      if (clearTicket.value === ticket) {
        clearLeaving.value = {
          count: ticket?.tracks.length ?? 0,
          busy: true,
          changed: false,
          blocked: true,
          issue: '',
          endTogether: !!ticket?.clearContext.roomId,
        };
        completedObservation = props.together.view.value;
        completedRoom.value = ticket?.clearContext.roomId ?? undefined;
        clearOpen.value = false;
        emit('cleared');
      }
    }
  });
}
</script>
<template>
  <aside
    ref="panel"
    class="queue-panel"
    tabindex="-1"
    aria-label="当前播放队列"
    :aria-busy="queueLoading || queueEditing"
    @keydown.esc.stop="player.queueOpen.value = false"
  >
    <header>
      <div>
        <h2>
          播放列表 <small>{{ queueCount }}</small>
        </h2>
      </div>
      <button
        class="text-button queue-collect"
        :disabled="collectionLocked"
        :aria-busy="collecting"
        @click="collectAll"
      >
        <Icon name="star" />{{ collecting ? '正在准备…' : '收藏全部' }}
      </button>
      <button
        ref="clearButton"
        class="text-button"
        :disabled="clearLocked || !queue?.tracks.length"
        @click="openClear"
      >
        <Icon name="trash" />清空</button
      ><button
        ref="closeButton"
        class="icon-button"
        aria-label="关闭播放队列"
        @click="player.queueOpen.value = false"
      >
        <Icon name="close" />
      </button>
    </header>
    <div v-if="queueError" class="error-state" role="alert">
      {{ queueError }}<button @click="retryQueue">重试</button>
    </div>
    <p v-if="collectionIssue" class="queue-notice" role="alert">{{ collectionIssue }}</p>
    <p v-if="queue?.tracks.length && queue.clearContext.issue" class="queue-notice" role="status">
      {{ queue.clearContext.issue }}
      <button class="text-button" @click="emit('together')">查看一起听状态</button>
    </p>
    <p v-if="clearCompletion" class="queue-notice queue-clear-completion" role="status">
      {{ clearCompletion }}
      <button class="text-button" @click="emit('together')">查看一起听状态</button>
    </p>
    <p
      v-if="queueEditError && !clearTicket && !queueLoading && !queueError"
      class="queue-notice"
      role="alert"
    >
      {{ queueEditError }}
    </p>
    <p v-if="queue?.fm" class="queue-notice" role="status">
      漫游队列由推荐更新，请使用漫游页面控制播放。
    </p>
    <div
      v-if="queue?.tracks.length && !queue.fm"
      class="queue-selection"
      role="group"
      aria-label="已选歌曲操作"
    >
      <span role="status">{{
        selected.size ? `已选择 ${selected.size} 项` : '单击选择，双击播放'
      }}</span>
      <template v-if="selected.size">
        <button class="text-button" :disabled="locked" @click="removeSelection">移出队列</button>
        <button class="text-button" :disabled="locked" @click="clearSelection">取消选择</button>
      </template>
    </div>
    <p
      v-if="queue && (queueLoading || !queueCurrent) && !queueError"
      class="queue-notice"
      role="status"
    >
      正在更新播放列表…
    </p>
    <div v-if="!queue && queueLoading" class="empty-state" role="status">正在加载队列…</div>
    <div
      v-else-if="queueCurrent && !queueLoading && !queueError && queue && !queue.tracks.length"
      class="empty-state queue-empty"
    >
      <p>播放列表空空如也</p>
      <button class="secondary" @click="emit('discover')">发现音乐</button>
    </div>
    <div
      v-else-if="queue?.tracks.length"
      ref="container"
      class="queue-scroll"
      tabindex="0"
      aria-label="播放列表歌曲"
      @keydown="keydown"
      @scroll.passive="onScroll"
    >
      <div class="queue-items">
        <div :style="{ height: `${start * row}px` }" aria-hidden="true" />
        <div
          v-for="(item, index) in visible"
          :key="item.entryId"
          v-memo="[
            item,
            start + index,
            currentEntry === item.entryId,
            locked,
            queue.fm,
            queue.tracks.length,
            selected.has(item.entryId),
            dragging.has(item.entryId),
          ]"
          class="queue-row"
          :data-entry-id="item.entryId"
          :class="{
            current: item.entryId === currentEntry,
            selected: selected.has(item.entryId),
            dragging: dragging.has(item.entryId),
          }"
          :aria-current="item.entryId === currentEntry ? 'true' : undefined"
        >
          <button
            class="icon-button queue-play"
            :disabled="locked || item.track.missing"
            :aria-label="`播放 ${item.track.name}`"
            @click="edit({ kind: 'select', entry_id: item.entryId })"
          >
            <Icon :name="item.entryId === currentEntry ? 'volume' : 'play'" />
          </button>
          <button
            class="queue-song"
            :disabled="locked"
            :aria-pressed="selected.has(item.entryId)"
            title="单击选择，双击播放；Ctrl 多选，Shift 连选"
            @pointerdown="drag.start($event, item.entryId)"
            @dragstart.prevent
            @click="selectRow($event, item.entryId)"
            @dblclick="!item.track.missing && edit({ kind: 'select', entry_id: item.entryId })"
          >
            <span
              ><strong class="ellipsis" :title="item.track.name">{{ item.track.name }}</strong
              ><small class="ellipsis" :title="artistLabel(item.track)">{{
                artistLabel(item.track)
              }}</small></span
            >
          </button>
          <time :title="item.track.durationMs > 0 ? undefined : '时长未知'">{{
            item.track.durationMs > 0 ? time(item.track.durationMs / 1000) : '—'
          }}</time>
          <button
            class="icon-button"
            aria-label="上移歌曲"
            :disabled="locked || start + index === 0"
            @click="move(start + index, -1)"
          >
            <Icon name="up" /></button
          ><button
            class="icon-button"
            aria-label="下移歌曲"
            :disabled="locked || start + index + 1 === queue.tracks.length"
            @click="move(start + index, 1)"
          >
            <Icon name="down" /></button
          ><button
            class="icon-button"
            aria-label="移出队列"
            :disabled="locked"
            @click="edit({ kind: 'remove', entry_ids: [item.entryId] }, start + index)"
          >
            <Icon name="close" />
          </button>
        </div>
        <div :style="{ height: `${(queue.tracks.length - end) * row}px` }" aria-hidden="true" />
        <div
          v-if="drag.marker.value !== undefined"
          class="queue-drop-marker"
          :style="{ top: `${drag.marker.value}px` }"
          aria-hidden="true"
        />
      </div>
      <div v-if="dragging.size" class="queue-drag-hint" role="status">
        正在移动 {{ dragging.size }} 项 · Esc 取消
      </div>
    </div>
    <ModalTransition
      :restore-focus="clearTicket?.epoch === view?.epoch"
      :return-focus="() => clearButton ?? null"
      :fallback-focus="() => closeButton ?? null"
      @left="clearLeft"
    >
      <QueueClearDialog
        v-if="clearOpen && clearTicket"
        v-bind="clearPresentation"
        @close="closeClear"
        @confirm="confirmClear"
      />
    </ModalTransition>
  </aside>
</template>
