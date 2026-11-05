<script setup lang="ts">
import type {
  TrackCurrentToggle,
  TrackMenuRequest,
  TrackPendingToggle,
  TrackPlayGesture,
} from '../services/trackMenu';
import { computed, nextTick, shallowRef, watch } from 'vue';
import {
  emptyPlaylistDragSelection,
  playlistRowDown,
  playlistRowRelease,
  playlistSelectionNeighbor,
} from '../services/playlistDragSelection';
import { useVirtualRows } from '../composables/useVirtualRows';
import type { TrackView } from '../services/contracts';
import type { AlbumDurationOrder } from '../services/albumDurationSort';
import TrackRow from './TrackRow.vue';
const props = defineProps<{
  tracks: TrackView[];
  matchKeyword?: string;
  durationOrder?: AlbumDurationOrder;
  originalRecordings?: ReadonlySet<string>;
  liked: Set<string>;
  busy: Set<string>;
  playable?: boolean;
  playableIds?: ReadonlySet<string>;
  currentId?: string;
  currentToggle?: TrackCurrentToggle;
  pendingToggle?: TrackPendingToggle;
  toggleBusy?: boolean;
  navigable?: boolean;
  preserveScrollOnRefresh?: boolean;
  queueDropEnabled?: boolean;
  playlistSelectionScope?: string;
  selectionLabel?: string;
  locateIndex?: number;
  scrollScope?: string;
}>();
const emit = defineEmits<{
  sortDuration: [];
  like: [track: TrackView];
  play: [index: number, gesture?: TrackPlayGesture];
  toggleCurrent: [target: TrackCurrentToggle, anchor: HTMLButtonElement];
  togglePending: [target: TrackPendingToggle, anchor: HTMLButtonElement];
  more: [request: TrackMenuRequest];
  artist: [id: string];
  album: [id: string];
  scrollEdge: [scope: string, scrolled: boolean, overflow: number];
  queueDrag: [
    event: PointerEvent,
    rows: { track: TrackView; index: number }[],
    current: () => boolean,
  ];
}>();
const rowHeight = 56;
const {
  container: scroller,
  start,
  end,
  onScroll,
  reset,
  restore: restoreScroll,
  center,
} = useVirtualRows(() => props.tracks.length, rowHeight);
const rows = computed(() => props.tracks.slice(start.value, end.value));
const selection = shallowRef(emptyPlaylistDragSelection());
const selected = computed(() => selection.value.ids);
const displayed = computed(() => props.tracks.map((track) => track.id));
const displayedIndexes = computed(() => {
  const indexes = new Map<string, number>();
  displayed.value.forEach((id, index) => {
    if (!indexes.has(id)) indexes.set(id, index);
  });
  return indexes;
});
let collapse: string | undefined;
watch(
  () => props.playlistSelectionScope,
  () => {
    selection.value = emptyPlaylistDragSelection();
    collapse = undefined;
  },
  { flush: 'sync' },
);
watch(displayed, (ids) => {
  if (!props.playlistSelectionScope) return;
  const available = new Set(ids);
  const old = selection.value;
  const next = [...old.ids].filter((id) => available.has(id));
  const anchor = old.anchor && available.has(old.anchor) ? old.anchor : undefined;
  if (next.length !== old.ids.size || anchor !== old.anchor) {
    selection.value = { ids: new Set(next), anchor };
  }
});
function targetRow(event: MouseEvent) {
  if (!(event.target instanceof Element) || event.target.closest('button, a, input, select'))
    return;
  const element = event.target.closest<HTMLElement>('.track-row[data-track-id]');
  if (!element) return;
  const index = Number(element.getAttribute('aria-rowindex')) - 2;
  const track = props.tracks[index];
  if (track?.id === element.dataset.trackId) return { element, track, index };
}
function queueDrag(event: PointerEvent) {
  if (!props.playlistSelectionScope || event.pointerType === 'touch') return;
  collapse = undefined;
  const hit = targetRow(event);
  if (!hit) {
    if (event.button === 0 && event.target === scroller.value)
      selection.value = emptyPlaylistDragSelection();
    return;
  }
  if (event.button !== 0 && event.button !== 2) return;
  const { element, track, index } = hit;
  const modifier = event.ctrlKey || event.metaKey;
  if (event.button === 0 && !modifier && !event.shiftKey && selected.value.has(track.id)) {
    collapse = track.id;
  }
  selection.value = playlistRowDown(
    selection.value,
    displayed.value,
    track.id,
    modifier,
    event.shiftKey,
    event.button === 2,
  );
  if (event.button === 0) element.focus({ preventScroll: true });
  if (event.button !== 0 || !props.queueDropEnabled) return;
  const captured = selection.value;
  const indexes =
    selected.value.size > 1
      ? [...selected.value].map((id) => displayedIndexes.value.get(id) ?? -1)
      : [index];
  const rows = indexes.map((index) => ({ track: props.tracks[index], index }));
  if (rows.some(({ track }) => !track || track.missing || track.local) || rows.length > 10000)
    return;
  emit('queueDrag', event, rows, () => captured === selection.value);
}
function plainRelease(event: MouseEvent) {
  if (!props.playlistSelectionScope || !collapse) return;
  const hit = targetRow(event);
  const id = collapse;
  collapse = undefined;
  if (hit?.track.id === id && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
    selection.value = playlistRowRelease(id);
  }
}
async function focusRow(index: number) {
  const node = scroller.value;
  if (!node) return;
  const top = index * rowHeight;
  if (top < node.scrollTop) node.scrollTop = top;
  else if (top + rowHeight > node.scrollTop + node.clientHeight)
    node.scrollTop = top + rowHeight - node.clientHeight;
  const scope = props.playlistSelectionScope;
  const row = props.tracks[index]?.id;
  onScroll();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await nextTick();
  if (
    node !== scroller.value ||
    scope !== props.playlistSelectionScope ||
    props.tracks[index]?.id !== row ||
    !row ||
    !selected.value.has(row)
  )
    return;
  node.querySelector<HTMLElement>(`[aria-rowindex="${index + 2}"]`)?.focus({ preventScroll: true });
}
function focusSelection(event: FocusEvent) {
  if (!props.playlistSelectionScope || !event.relatedTarget || !displayed.value.length) return;
  const first = displayed.value.findIndex((id) => selected.value.has(id));
  if (first < 0) selection.value = playlistRowRelease(displayed.value[0]);
  void focusRow(Math.max(0, first));
}
function selectionKey(event: KeyboardEvent) {
  if (
    !props.playlistSelectionScope ||
    !(event.target instanceof Element) ||
    event.target.closest('button, a, input, select')
  )
    return;
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
    event.preventDefault();
    event.stopPropagation();
    selection.value = { ids: new Set(displayed.value), anchor: selection.value.anchor };
  } else if (
    !event.shiftKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    (event.key === 'ArrowUp' || event.key === 'ArrowDown')
  ) {
    event.preventDefault();
    event.stopPropagation();
    const index = playlistSelectionNeighbor(
      displayed.value,
      selected.value,
      event.key === 'ArrowUp' ? -1 : 1,
    );
    if (index !== undefined) {
      selection.value = playlistRowRelease(displayed.value[index]);
      void focusRow(index);
    }
  }
}
function preventNativeDrag(event: DragEvent) {
  if (
    props.queueDropEnabled &&
    event.target instanceof Element &&
    event.target.closest('.track-row')
  )
    event.preventDefault();
}
let wasScrolled = false;
function scroll() {
  onScroll();
  const node = scroller.value;
  const scrolled = (node?.scrollTop ?? 0) > 0;
  if (node && props.scrollScope && scrolled !== wasScrolled)
    emit('scrollEdge', props.scrollScope, scrolled, node.scrollHeight - node.clientHeight);
  wasScrolled = scrolled;
}
watch(
  () => props.tracks,
  (_now, previous) => {
    if (
      !props.preserveScrollOnRefresh &&
      (props.tracks[0]?.id !== previous[0]?.id || props.tracks.length < previous.length)
    ) {
      reset();
    }
  },
);
async function locate() {
  const index = props.locateIndex;
  if (index === undefined || index < 0) return;
  const target = props.tracks[index];
  if (!target || !center(index)) return;
  await nextTick();
  if (index !== props.locateIndex || props.tracks[index] !== target) return;
  scroller.value
    ?.querySelector<HTMLElement>(`[aria-rowindex="${index + 2}"]`)
    ?.focus({ preventScroll: true });
}
defineExpose({ locate, restoreScroll });
</script>
<template>
  <div
    class="track-table"
    :role="playlistSelectionScope ? 'grid' : 'table'"
    :aria-label="playlistSelectionScope ? (selectionLabel ?? '歌单歌曲') : undefined"
    :aria-multiselectable="playlistSelectionScope ? true : undefined"
    :aria-rowcount="tracks.length + 1"
  >
    <div class="track-columns track-header" role="row">
      <span role="columnheader">#</span><span role="columnheader">歌曲 / 歌手</span
      ><span role="columnheader">专辑</span><span role="columnheader">喜欢</span
      ><span role="columnheader" :aria-sort="durationOrder === 'original' ? 'none' : durationOrder">
        <button
          v-if="durationOrder"
          type="button"
          class="track-duration-sort"
          aria-label="按时长排序"
          data-catalog-key="duration-sort"
          @click="emit('sortDuration')"
        >
          时长<span v-if="durationOrder !== 'original'" aria-hidden="true">
            {{ durationOrder === 'ascending' ? '↑' : '↓' }}
          </span>
        </button>
        <template v-else>时长</template>
      </span>
    </div>
    <div
      ref="scroller"
      class="track-scroll"
      @scroll.passive="scroll"
      :tabindex="playlistSelectionScope ? 0 : undefined"
      @focus.self="focusSelection"
      @keydown="selectionKey"
      @pointerdown="queueDrag"
      @click="plainRelease"
      @dragstart="preventNativeDrag"
    >
      <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
      <TrackRow
        v-for="(track, index) in rows"
        :key="`${start + index}:${track.id}`"
        v-memo="[
          track,
          start + index,
          currentId === track.id,
          currentToggle?.trackId === track.id ? currentToggle : undefined,
          toggleBusy,
          pendingToggle?.trackId === track.id ? pendingToggle : undefined,
          liked.has(track.id),
          busy.has(track.id),
          playable,
          playableIds?.has(track.id),
          navigable,
          matchKeyword,
          originalRecordings?.has(track.id),
          locateIndex !== undefined,
          !!playlistSelectionScope,
          selected.has(track.id),
        ]"
        :track="track"
        :tabindex="playlistSelectionScope || locateIndex !== undefined ? -1 : undefined"
        :selection="playlistSelectionScope ? selected.has(track.id) : undefined"
        :match-keyword="matchKeyword"
        :original-recording="originalRecordings?.has(track.id)"
        :index="start + index"
        :current-id="currentId"
        :current-toggle="
          !track.missing &&
          !track.local &&
          (playableIds?.has(track.id) ?? true) &&
          currentToggle?.trackId === track.id
            ? currentToggle
            : undefined
        "
        :pending-toggle="
          !track.missing &&
          !track.local &&
          (playableIds?.has(track.id) ?? true) &&
          pendingToggle?.trackId === track.id
            ? pendingToggle
            : undefined
        "
        :toggle-busy="toggleBusy"
        :liked="liked.has(track.id)"
        :busy="busy.has(track.id)"
        :playable="playable && (playableIds?.has(track.id) ?? true)"
        :navigable="navigable"
        @play="(index, gesture) => emit('play', index, gesture)"
        @toggle-current="(target, anchor) => emit('toggleCurrent', target, anchor)"
        @toggle-pending="(target, anchor) => emit('togglePending', target, anchor)"
        @like="emit('like', $event)"
        @more="emit('more', $event)"
        @artist="emit('artist', $event)"
        @album="emit('album', $event)"
      />
      <div :style="{ height: `${(tracks.length - end) * rowHeight}px` }" aria-hidden="true" />
    </div>
  </div>
</template>

<style scoped>
.track-duration-sort {
  display: inline-flex;
  gap: 3px;
  align-items: center;
  border: 0;
  background: transparent;
  color: inherit;
  padding: 4px 0;
  font: inherit;
  white-space: nowrap;
  cursor: pointer;
}
.track-duration-sort:hover {
  color: var(--text-primary, currentColor);
}
</style>
