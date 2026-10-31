<script setup lang="ts">
import { computed, watch, onMounted, onUnmounted, ref, shallowRef, nextTick } from 'vue';
import { useVirtualRows } from '../composables/useVirtualRows';
import { useControlFocus } from '../composables/useControlFocus';
import type { PlayerModel } from '../composables/usePlayer';
import { time, type QueueView, type QueueEdit, type QueueCollectionView } from '../services/player';
import { failure } from '../services/contracts';
import { useCore } from '../services/core';
import { artistLabel } from '../services/artistLabel';
import Icon from './Icon.vue';
import ModalTransition from './ModalTransition.vue';
import QueueClearDialog from './queue/QueueClearDialog.vue';
const props = defineProps<{ player: PlayerModel; authenticated: boolean }>();
const emit = defineEmits<{
  collect: [value: QueueCollectionView, trigger: HTMLElement | undefined];
  login: [];
  discover: [];
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
const panel = ref<HTMLElement>(),
  clearButton = ref<HTMLButtonElement>(),
  closeButton = ref<HTMLButtonElement>();
const clearOpen = ref(false),
  clearTicket = shallowRef<QueueView>();
const clearChanged = computed(
  () =>
    !!clearTicket.value &&
    (!queueCurrent.value ||
      !!queueError.value ||
      clearTicket.value.epoch !== view.value?.epoch ||
      clearTicket.value.revision !== view.value.queueRevision),
);
// Keep the last dialog body during its exit animation, including error text.
const clearLeaving = shallowRef({
  count: 0,
  busy: false,
  changed: false,
  blocked: false,
  issue: '',
});
const clearPresentation = computed(() =>
  clearOpen.value
    ? {
        count: clearTicket.value?.tracks.length ?? 0,
        busy: queueEditing.value,
        changed: !queueEditing.value && clearChanged.value,
        blocked: locked.value,
        issue: queueEditError.value,
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
    clearLeaving.value = clearPresentation.value;
    clearOpen.value = false;
  },
);
onUnmounted(() => {
  collectionRead++;
  if (focus?.isConnected && !focus.matches(':disabled')) focus.focus({ preventScroll: true });
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
async function edit(input: QueueEdit, index?: number) {
  if (locked.value) return;
  const active = document.activeElement;
  const owner = queue.value?.epoch;
  if (await props.player.editQueue(input)) {
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
  }
}
function keydown(event: KeyboardEvent) {
  const target = event.target as HTMLElement;
  if (!target.matches('.queue-song') || locked.value) return;
  const id = target.closest<HTMLElement>('.queue-row')?.dataset.entryId;
  const index = queue.value?.tracks.findIndex((i) => i.entryId === id) ?? -1;
  if (index < 0) return;
  const next = {
    ArrowDown: index + 1,
    ArrowUp: index - 1,
    Home: 0,
    End: (queue.value?.tracks.length ?? 1) - 1,
  }[event.key];
  if (next !== undefined) {
    event.preventDefault();
    void focusRow(next);
  } else if (event.key === 'Delete' || event.key === 'Backspace') {
    event.preventDefault();
    if (!event.repeat) void edit({ kind: 'remove', entry_ids: [id!] }, index);
  }
}
function openClear() {
  if (locked.value || !queue.value?.tracks.length) return;
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
  if (!clearOpen.value || clearChanged.value || locked.value) return;
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
        };
        clearOpen.value = false;
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
        :disabled="locked || !queue?.tracks.length"
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
        ]"
        class="queue-row"
        :data-entry-id="item.entryId"
        :class="{ current: item.entryId === currentEntry }"
        :aria-current="item.entryId === currentEntry ? 'true' : undefined"
      >
        <button
          class="queue-song"
          :disabled="locked || item.track.missing"
          @click="edit({ kind: 'select', entry_id: item.entryId })"
        >
          <Icon :name="item.entryId === currentEntry ? 'volume' : 'music'" /><span
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
