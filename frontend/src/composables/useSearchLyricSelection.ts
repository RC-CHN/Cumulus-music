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
} from '../services/searchLyricSelection';

export function useSearchLyricSelection(
  enabled: Readonly<Ref<boolean>>,
  scope: () => readonly unknown[],
  ids: Readonly<Ref<readonly string[]>>,
  panel: Readonly<Ref<HTMLElement | undefined>>,
  body: Readonly<Ref<HTMLElement | undefined>>,
  pin: (index: number) => void,
) {
  const selection = shallowRef(emptyLyricSelection());
  let collapse = false;
  function reset() {
    selection.value = emptyLyricSelection();
    collapse = false;
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
  function mouseDown(event: MouseEvent) {
    if (!enabled.value) return;
    collapse = false;
    const row = hit(event.target);
    if (!row) return;
    const result = lyricSelectionDown(selection.value, ids.value, row, gesture(event));
    selection.value = result.selection;
    collapse = result.collapse;
    if (
      event.button === 0 &&
      event.target instanceof Element &&
      !event.target.closest('button, a, input, select, textarea, [contenteditable]')
    )
      row.row.querySelector<HTMLElement>('.track-row')?.focus({ preventScroll: true });
  }
  function mouseUp(event: MouseEvent) {
    if (!enabled.value) return;
    const pending = collapse;
    collapse = false;
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
  return { selection, mouseDown, mouseUp, clearBackground, tableFocus, selectionKey };
}
