import { nextTick, shallowRef, watch, type Ref } from 'vue';
import { playlistSelectionNeighbor } from '../services/playlistDragSelection';
import {
  emptyLyricSelection,
  lyricResultKey,
  lyricSelectionAll,
  lyricSelectionDown,
  lyricSelectionOnly,
  lyricSelectionUp,
  reconcileLyricSelection,
  type LyricSelection,
} from '../services/searchLyricSelection';

const interactive = 'button, a, input, select, textarea, [contenteditable]';

type LyricDragStart = (
  event: PointerEvent,
  row: { row: HTMLElement; index: number; id: string },
  selected: LyricSelection,
  current: () => boolean,
  dragStarted: () => void,
) => boolean;

export function useSearchLyricSelection(
  enabled: Readonly<Ref<boolean>>,
  scope: () => readonly unknown[],
  ids: Readonly<Ref<readonly string[]>>,
  panel: Readonly<Ref<HTMLElement | undefined>>,
  body: Readonly<Ref<HTMLElement | undefined>>,
  pin: (index: number) => void,
  queueDrag?: { enabled: () => boolean; start: LyricDragStart },
) {
  const selection = shallowRef(emptyLyricSelection());
  let collapse = false;
  let pointer: PointerEvent | undefined;
  let nativeDragOwner: Element | undefined;
  let pressRevision = 0;
  function reset() {
    selection.value = emptyLyricSelection();
    collapse = false;
    pointer = undefined;
    nativeDragOwner = undefined;
    pressRevision++;
  }
  watch(scope, reset, { flush: 'sync' });
  watch(
    ids,
    (current) => {
      selection.value = reconcileLyricSelection(selection.value, current);
    },
    { flush: 'sync' },
  );

  function hit(target: EventTarget | null) {
    if (!(target instanceof Element)) return;
    const row = target.closest<HTMLElement>('.search-lyric-result[data-lyric-index]');
    if (!row || row.parentElement !== body.value) return;
    const index = Number(row.dataset.lyricIndex);
    const id = ids.value[index];
    if (id && row.dataset.resultKey === lyricResultKey(index, id)) return { row, index, id };
  }
  function gesture(event: MouseEvent) {
    return { button: event.button, ctrl: event.ctrlKey, shift: event.shiftKey };
  }
  function pointerDown(event: PointerEvent) {
    // The compatibility mousedown below still owns M106's selection step.
    pointer = undefined;
    nativeDragOwner = undefined;
    if (
      !enabled.value ||
      !queueDrag?.enabled() ||
      !event.isPrimary ||
      event.button !== 0 ||
      event.pointerType === 'touch' ||
      event.altKey ||
      !(event.target instanceof Element) ||
      event.target.closest(interactive)
    )
      return;
    const track = event.target.closest('.track-row');
    if (!track) return;
    const text = document.getSelection();
    if (text && !text.isCollapsed && text.rangeCount && text.getRangeAt(0).intersectsNode(track))
      return;
    pointer = event;
  }
  function pointerCancel(event: PointerEvent) {
    if (pointer?.pointerId === event.pointerId) pointer = undefined;
  }
  function nativeDrag(event: DragEvent) {
    if (event.target instanceof Element && nativeDragOwner?.contains(event.target))
      event.preventDefault();
  }
  function mouseDown(event: MouseEvent) {
    if (!enabled.value) return;
    const press = ++pressRevision;
    const start = pointer;
    pointer = undefined;
    nativeDragOwner = undefined;
    collapse = false;
    const row = hit(event.target);
    if (!row) return;
    const result = lyricSelectionDown(selection.value, ids.value, row, gesture(event));
    selection.value = result.selection;
    collapse = result.collapse;
    if (event.button === 0 && event.target instanceof Element && !event.target.closest(interactive))
      row.row.querySelector<HTMLElement>('.track-row')?.focus({ preventScroll: true });
    if (
      !queueDrag ||
      !start ||
      start.target !== event.target ||
      event.button !== 0 ||
      event.altKey ||
      start.ctrlKey !== event.ctrlKey ||
      start.shiftKey !== event.shiftKey ||
      event.timeStamp < start.timeStamp ||
      event.timeStamp - start.timeStamp > 120 ||
      Math.abs(start.clientX - event.clientX) > 1 ||
      Math.abs(start.clientY - event.clientY) > 1 ||
      !(event.target instanceof Element) ||
      event.target.closest(interactive)
    )
      return;
    const track = event.target.closest('.track-row');
    if (!track || track.parentElement !== row.row) return;
    const captured = selection.value;
    const current = () => press === pressRevision && selection.value === captured;
    if (
      queueDrag.start(start, row, captured, current, () => {
        if (current()) collapse = false;
      })
    )
      nativeDragOwner = track;
  }
  function mouseUp(event: MouseEvent) {
    if (!enabled.value) return;
    const pending = collapse;
    collapse = false;
    pointer = undefined;
    nativeDragOwner = undefined;
    const row = hit(event.target);
    if (row) selection.value = lyricSelectionUp(selection.value, row, gesture(event), pending);
  }
  function clearBackground(event: MouseEvent) {
    if (enabled.value && event.target === panel.value && event.button === 0) {
      selection.value = { ids: new Set(), anchor: selection.value.anchor };
      collapse = false;
    }
  }
  async function focusRow(index: number) {
    const owner = body.value;
    const shown = ids.value;
    const captured = selection.value;
    if (!owner || !shown[index]) return;
    pin(index);
    await nextTick();
    if (
      !enabled.value ||
      owner !== body.value ||
      shown !== ids.value ||
      captured !== selection.value ||
      !owner.contains(document.activeElement)
    )
      return;
    const row = owner.querySelector<HTMLElement>(`[data-lyric-index="${index}"] .track-row`);
    row?.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    row?.focus({ preventScroll: true });
  }
  function tableFocus(event: FocusEvent) {
    if (!enabled.value || !event.relatedTarget || !ids.value.length) return;
    const first = ids.value.findIndex((id) => selection.value.ids.has(id));
    if (first < 0)
      selection.value = lyricSelectionOnly(selection.value, { index: 0, id: ids.value[0]! }, false);
    void focusRow(Math.max(0, first));
  }
  function selectionKey(event: KeyboardEvent) {
    if (
      !enabled.value ||
      !(event.target instanceof Element) ||
      event.target.closest('input, select, textarea, [contenteditable]')
    )
      return;
    if (event.ctrlKey && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      event.stopPropagation();
      selection.value = lyricSelectionAll(selection.value, ids.value);
    } else if (
      !event.ctrlKey &&
      !event.shiftKey &&
      !event.altKey &&
      !event.metaKey &&
      (event.key === 'ArrowUp' || event.key === 'ArrowDown')
    ) {
      event.preventDefault();
      event.stopPropagation();
      const index = playlistSelectionNeighbor(
        ids.value,
        selection.value.ids,
        event.key === 'ArrowUp' ? -1 : 1,
      );
      if (index === undefined) return;
      selection.value = lyricSelectionOnly(
        selection.value,
        { index, id: ids.value[index]! },
        false,
      );
      void focusRow(index);
    }
  }
  return {
    selection,
    pointerDown,
    pointerCancel,
    nativeDrag,
    mouseDown,
    mouseUp,
    clearBackground,
    tableFocus,
    selectionKey,
  };
}
