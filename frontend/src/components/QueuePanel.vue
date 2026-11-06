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
import { useQueueDrop } from '../composables/useQueueDrop';
import { useControlFocus } from '../composables/useControlFocus';
import type { PlayerModel } from '../composables/usePlayer';
import type { TogetherModel } from '../composables/useTogether';
import {
  queueMoreRecommendations,
  time,
  type QueueView,
  type QueueEdit,
  type QueueTrack,
  type QueueOrigin,
  type QueueCollectionView,
} from '../services/player';
import { rowTop, rowExtent } from '../services/virtualWindow';
import { displayedQueueMove } from '../services/queueSelection';
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
  playlistSourceRefresh?: { pending: boolean };
}>();
const emit = defineEmits<{
  collect: [value: QueueCollectionView, trigger: HTMLElement | undefined];
  login: [];
  discover: [];
  origin: [target: QueueOrigin];
  together: [];
  cleared: [];
  refreshPlaylistSource: [event: MouseEvent];
}>();
const client = useCore();
const collecting = ref(false),
  collectionIssue = ref('');
let collectionRead = 0;
const {
  queue,
  view,
  busy,
  queueError,
  queueLoading,
  queueEditing,
  queueEditError,
  queueCurrent,
  queueMoreChanging,
  queueMoreError,
} = props.player;
const moreRecommendations = computed(() =>
  props.authenticated &&
  !view.value?.fm &&
  view.value?.queueMoreAvailable !== false &&
  queue.value?.epoch === view.value?.epoch
    ? queueMoreRecommendations(queue.value)
    : undefined,
);
const showMoreMarks = computed(() => moreRecommendations.value?.enabled === true);
const separatorIndex = computed(() => {
  const id = moreRecommendations.value?.separatorBefore;
  return showMoreMarks.value && id
    ? (queue.value?.tracks.findIndex((entry) => entry.entryId === id) ?? -1)
    : -1;
});
const row = 64;
const expansion = computed(() =>
  separatorIndex.value > 0 ? { index: separatorIndex.value, extra: 10 } : undefined,
);
const offset = (index: number) => rowTop(index, row, expansion.value);
const { container, start, end, onScroll, center } = useVirtualRows(
  () => queue.value?.tracks.length ?? 0,
  row,
  8,
  () => expansion.value,
);
const visible = computed(() => queue.value?.tracks.slice(start.value, end.value) ?? []);
const currentEntry = computed(() => view.value?.entryId);
const currentIndex = computed(() => {
  if (!queueCurrent.value || !currentEntry.value) return -1;
  return queue.value?.tracks.findIndex((item) => item.entryId === currentEntry.value) ?? -1;
});
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
const moreToggleLocked = computed(
  () => locked.value || queueMoreChanging.value || !moreRecommendations.value?.canToggle,
);
const mounted = ref(false);
let morePrefetched = false;
watch(
  [mounted, props.player.ready, queueCurrent, moreRecommendations],
  () => {
    const displayed = queue.value;
    if (
      !mounted.value ||
      morePrefetched ||
      !props.player.ready.value ||
      !queueCurrent.value ||
      !displayed ||
      !moreRecommendations.value?.enabled ||
      !moreRecommendations.value.prefetchOnOpen
    )
      return;
    morePrefetched = true;
    void props.player.queueMoreOpened(displayed);
  },
  { flush: 'post' },
);
function toggleMore() {
  const displayed = queue.value;
  const state = moreRecommendations.value;
  if (moreToggleLocked.value || !displayed || !state) return;
  // The explicit on gesture already requests recommendations. Do not also
  // turn the same opening into a passive prefetch as the projection updates.
  if (!state.enabled) morePrefetched = true;
  void props.player.toggleQueueMore(displayed, !state.enabled);
}

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
  rowHeight: row,
  expansion: () => expansion.value,
  reorder: (movement, displayed, focusId) => {
    void edit(movement, movement.entry_ids.indexOf(focusId), displayed);
  },
});
const dragging = drag.dragging;
const canDrop = computed(
  () =>
    !locked.value &&
    !clearOpen.value &&
    !!queue.value?.tracks.length &&
    !queue.value.clearContext.roomId &&
    !view.value?.intelligence &&
    view.value?.mode !== 'random' &&
    !['preparing', 'loading'].includes(view.value?.state ?? 'idle'),
);
const externalDrop = useQueueDrop({
  queue: () => queue.value,
  playId: () => view.value?.playId ?? null,
  container: () => container.value,
  disabled: () => !canDrop.value || dragging.value.size > 0,
  rowHeight: row,
  expansion: () => expansion.value,
  scroll: onScroll,
  commit: (source, displayed, playId, before) => {
    void props.player.dropIntoQueue(source, displayed, playId, before);
  },
});
defineExpose({
  beginDrop: externalDrop.begin,
  canDrop,
  focusAfterSourceRefresh: () => closeButton.value?.focus({ preventScroll: true }),
});
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
  props.player.clearQueueMoreError();
  mounted.value = true;
  focus = document.activeElement as HTMLElement;
  panel.value?.focus({ preventScroll: true });
});
let located: { element: HTMLElement; epoch: string; entry: string } | undefined;
watch(
  [container, () => view.value?.epoch, currentEntry, currentIndex],
  ([element, epoch, entry, index]) => {
    if (!element || !epoch || !entry) located = undefined;
    if (!element) return;
    // The player stream can publish the new entry before its queue rows.
    // Center only once on the matching snapshot, not on later list edits.
    if (
      epoch &&
      entry &&
      index >= 0 &&
      (located?.element !== element || located.epoch !== epoch || located.entry !== entry) &&
      center(index)
    )
      located = { element, epoch, entry };
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
  mounted.value = false;
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
  const displayed = queue.value;
  const id = displayed?.tracks[index]?.entryId;
  if (!displayed || !id || !displayed.tracks[index + direction]) return;
  const movement = displayedQueueMove(
    displayed.tracks,
    new Set([id]),
    direction < 0 ? index - 1 : index + 2,
    queueMoreRecommendations(displayed)?.separatorBefore,
  );
  if (movement) void edit(movement, index + direction, displayed);
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
  const top = offset(index);
  const height = rowExtent(index, row, expansion.value);
  if (top < element.scrollTop) element.scrollTop = top;
  else if (top + height > element.scrollTop + element.clientHeight)
    element.scrollTop = top + height - element.clientHeight;
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
  // Park keyboard focus on a stable enabled container BEFORE disabling rows.
  // WebKit may deliver the focused button's disabled blur after Vue nextTick;
  // refocusing that same button too early otherwise strands focus on body.
  const parking = panel.value?.contains(active) ? container.value : undefined;
  if (parking && active !== parking) parking.focus({ preventScroll: true });
  const parked = parking && document.activeElement === parking ? parking : undefined;
  const ownsFocus = () =>
    document.activeElement === active ||
    document.activeElement === document.body ||
    (!!parked && document.activeElement === parked);
  if (await props.player.editQueue(input, displayed)) {
    if (owner !== view.value?.epoch || !panel.value) return;
    const revision = view.value?.queueRevision;
    await props.player.loadQueue();
    if (owner !== view.value?.epoch || revision !== view.value?.queueRevision || !panel.value)
      return;
    // Preserve newer user focus while native/list replies were pending.
    if (index !== undefined && ownsFocus()) await focusRow(index);
  } else {
    // Disabling a pending control may move WebKit focus to body. A rejected
    // edit keeps the selection; restore its live control only after observing
    // the same entries, never after a replacement queue or a new user focus.
    if (owner !== view.value?.epoch || !panel.value) return;
    // A same-revision cache can still have a read/enable transition in flight.
    // Reconcile explicitly before restoring the failed edit's keyboard target.
    await props.player.loadQueue();
    await nextTick();
    // Let WebKit apply the re-enabled controls and its deferred native blur.
    // Vue's microtask flush alone can precede that blur on a slower renderer.
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
    );
    if (
      owner === view.value?.epoch &&
      panel.value &&
      displayed?.tracks.length === queue.value?.tracks.length &&
      displayed?.tracks.every((item, i) => item.entryId === queue.value?.tracks[i]?.entryId) &&
      ownsFocus() &&
      active instanceof HTMLElement &&
      active.isConnected &&
      !active.matches(':disabled')
    )
      active.focus({ preventScroll: true });
  }
}
const currentQueueTransport = computed(() =>
  props.player.loaded.value ? view.value?.entryId : undefined,
);
const queueToggleFocus = useControlFocus(
  () => `${view.value?.epoch}:${view.value?.queueRevision}:${view.value?.playId}`,
  () => container.value,
);
function queuePlayLabel(entryId: string) {
  if (entryId !== currentQueueTransport.value) return '播放';
  return props.player.playing.value ? '暂停' : '继续播放';
}
async function activateRow(trigger: EventTarget | null, entryId: string, missing: boolean) {
  if (locked.value || missing) return;
  if (entryId === currentQueueTransport.value) {
    // The loaded entry already owns the decoder. Use its guarded transport
    // command instead of selecting and opening the same source again.
    const restore = queueToggleFocus.capture(trigger);
    try {
      await props.player.toggleSource();
    } finally {
      await nextTick();
      restore();
    }
  } else await edit({ kind: 'select', entry_id: entryId });
}
function openOrigin(row: QueueTrack) {
  const displayed = queue.value;
  if (!displayed || !queueCurrent.value || queueError.value || queueEditing.value) return;
  const current = displayed.tracks.find((entry) => entry.entryId === row.entryId);
  if (current !== row || !row.origin) return;
  emit('origin', row.origin);
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
    const item = queue.value?.tracks[index];
    if (!event.repeat && item) void activateRow(target, item.entryId, item.track.missing);
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
    :class="{ 'queue-panel-with-more': moreRecommendations }"
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
    <div v-if="moreRecommendations" class="queue-more">
      <span class="queue-more-copy">
        <span class="queue-more-badge" aria-hidden="true">荐</span>
        <span id="queue-more-label">在列表下方推荐你喜欢的相似歌曲</span>
      </span>
      <button
        class="queue-more-switch"
        type="button"
        role="switch"
        data-queue-more-toggle
        aria-labelledby="queue-more-label"
        :aria-checked="moreRecommendations.enabled"
        :aria-disabled="moreToggleLocked"
        :aria-busy="queueMoreChanging"
        @click="toggleMore"
      />
    </div>
    <p
      v-if="moreRecommendations && queueMoreError"
      class="queue-notice"
      role="alert"
      data-queue-more-error
    >
      {{ queueMoreError }}
    </p>
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
    <p
      v-if="playlistSourceRefresh && !clearTicket"
      class="queue-notice"
      role="alert"
      data-queue-playlist-source-changed
      :aria-busy="playlistSourceRefresh.pending"
    >
      歌单歌曲已更新，播放列表未改动。
      <button
        class="text-button"
        :aria-disabled="playlistSourceRefresh.pending"
        @click="emit('refreshPlaylistSource', $event)"
      >
        {{ playlistSourceRefresh.pending ? '正在读取…' : '重新读取歌单' }}
      </button>
    </p>
    <p v-if="queue?.fm" class="queue-notice" role="status">
      漫游队列由推荐更新，请使用漫游页面控制播放。
    </p>
    <p
      v-else-if="view?.intelligence === 'seeded'"
      class="queue-notice queue-intelligence"
      role="status"
    >
      心动推荐尚未完成，当前保留种子歌曲；可从播放模式切回列表循环。
    </p>
    <p
      v-else-if="view?.intelligence === 'fallback'"
      class="queue-notice queue-intelligence"
      role="status"
    >
      心动模式开启失败，当前使用列表循环。
    </p>
    <p
      v-else-if="view?.intelligence === 'active'"
      class="queue-notice queue-intelligence"
      role="status"
    >
      心动模式
    </p>
    <p
      v-if="
        queueCurrent &&
        view?.entryId &&
        !queue?.tracks.some((item) => item.entryId === view?.entryId)
      "
      class="queue-notice queue-current-outside"
      role="status"
    >
      当前歌曲继续保留，未列入此播放列表。
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
        <div :style="{ height: `${offset(start)}px` }" aria-hidden="true" />
        <div
          v-for="(item, index) in visible"
          :key="item.entryId"
          v-memo="[
            item,
            start + index,
            currentEntry === item.entryId,
            currentQueueTransport === item.entryId,
            locked,
            queueCurrent,
            !!queueError,
            queueEditing,
            queue.fm,
            queue.tracks.length,
            selected.has(item.entryId),
            dragging.has(item.entryId),
            showMoreMarks && item.moreRecommendation,
            expansion?.index === start + index,
          ]"
          class="queue-row"
          :data-entry-id="item.entryId"
          :class="{
            current: item.entryId === currentEntry,
            selected: selected.has(item.entryId),
            dragging: dragging.has(item.entryId),
            'queue-more-separated': expansion?.index === start + index,
          }"
          :aria-current="item.entryId === currentEntry ? 'true' : undefined"
        >
          <span
            v-if="expansion?.index === start + index"
            class="queue-more-divider"
            data-queue-more-divider
            aria-hidden="true"
          />
          <button
            class="icon-button queue-play"
            :disabled="locked || item.track.missing"
            :aria-label="`${queuePlayLabel(item.entryId)} ${item.track.name}`"
            @click="activateRow($event.currentTarget, item.entryId, item.track.missing)"
          >
            <Icon :name="queuePlayLabel(item.entryId) === '暂停' ? 'pause' : 'play'" />
          </button>
          <button
            class="queue-song"
            :disabled="locked"
            :aria-pressed="selected.has(item.entryId)"
            aria-keyshortcuts="Enter"
            :title="
              currentQueueTransport === item.entryId
                ? '单击选择，双击或按 Enter 暂停或继续；Ctrl 多选，Shift 连选'
                : '单击选择，双击或按 Enter 播放；Ctrl 多选，Shift 连选'
            "
            @pointerdown="drag.start($event, item.entryId)"
            @dragstart.prevent
            @click="selectRow($event, item.entryId)"
            @dblclick="activateRow($event.currentTarget, item.entryId, item.track.missing)"
          >
            <span>
              <span class="queue-song-title">
                <strong class="ellipsis" :title="item.track.name">{{ item.track.name }}</strong>
                <span
                  v-if="showMoreMarks && item.moreRecommendation"
                  class="queue-more-badge"
                  data-queue-more-badge
                  title="更多推荐"
                  ><span aria-hidden="true">荐</span><span class="sr-only">更多推荐</span></span
                >
              </span>
              <small class="ellipsis" :title="artistLabel(item.track)">{{
                artistLabel(item.track)
              }}</small>
            </span>
          </button>
          <button
            v-if="item.origin"
            type="button"
            class="icon-button queue-origin"
            :disabled="!queueCurrent || !!queueError || queueEditing"
            title="来源"
            :aria-label="`来源：${item.track.name}`"
            @click="openOrigin(item)"
          >
            <Icon name="source" />
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
        <div
          :style="{ height: `${offset(queue.tracks.length) - offset(end)}px` }"
          aria-hidden="true"
        />
        <div
          v-if="drag.marker.value !== undefined || externalDrop.marker.value !== undefined"
          class="queue-drop-marker"
          :style="{ top: `${externalDrop.marker.value ?? drag.marker.value}px` }"
          aria-hidden="true"
        />
      </div>
      <div v-if="dragging.size" class="queue-drag-hint" role="status">
        正在移动 {{ dragging.size }} 项 · Esc 取消
      </div>
      <div v-else-if="externalDrop.dragging.value" class="queue-drag-hint" role="status">
        {{
          externalDrop.itemCount.value > 1
            ? `正在添加 ${externalDrop.itemCount.value} 首歌曲`
            : '正在添加歌曲'
        }}
        · Esc 取消
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
